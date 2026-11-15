//! 集成终端：基于 PTY 的交互式 shell（VS Code 风格）。
//!
//! `portable-pty` 启动一个持久 shell，`vte` 把它的 ANSI 字节流解析成字符网格，
//! 键盘输入直接写回 PTY。输出经 `mpsc` 回传，UI 帧 `poll()` 时更新，线程用
//! `ctx.request_repaint()` 唤醒界面。

use crate::theme::Theme;
use egui::text::{LayoutJob, TextFormat};
use egui::{self, Color32, FontFamily, FontId, RichText, ScrollArea, Sense};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtyPair, PtySize};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

const MAX_SCROLLBACK: usize = 400;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    ch: char,
    fg: Option<Color32>,
    bg: Option<Color32>,
    bold: bool,
    inverse: bool,
}

impl Cell {
    fn blank() -> Self {
        Self { ch: ' ', fg: None, bg: None, bold: false, inverse: false }
    }
}

enum TermEvent {
    Data(Vec<u8>),
    Exited,
}

/// 一个极简 VT100/xterm 屏幕模型，实现 `vte::Perform`。
struct Screen {
    rows: usize,
    cols: usize,
    grid: Vec<Vec<Cell>>,
    scrollback: VecDeque<Vec<Cell>>,
    cur_r: usize,
    cur_c: usize,
    saved: (usize, usize),
    fg: Option<Color32>,
    bg: Option<Color32>,
    bold: bool,
    inverse: bool,
    scroll_top: usize,
    scroll_bottom: usize,
    wrap_pending: bool,
    auto_wrap: bool,
    cursor_visible: bool,
    bracketed_paste: bool,
    app_cursor: bool,
    alt: Option<(Vec<Vec<Cell>>, (usize, usize))>,
    replies: Vec<Vec<u8>>,
}

impl Screen {
    fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            grid: vec![vec![Cell::blank(); cols]; rows],
            scrollback: VecDeque::new(),
            cur_r: 0,
            cur_c: 0,
            saved: (0, 0),
            fg: None,
            bg: None,
            bold: false,
            inverse: false,
            scroll_top: 0,
            scroll_bottom: rows,
            wrap_pending: false,
            auto_wrap: true,
            cursor_visible: true,
            bracketed_paste: false,
            app_cursor: false,
            alt: None,
            replies: Vec::new(),
        }
    }

    fn blank_row(&self) -> Vec<Cell> {
        vec![Cell::blank(); self.cols]
    }

    fn erase_blank(&self) -> Cell {
        Cell { ch: ' ', fg: None, bg: self.bg, bold: false, inverse: false }
    }

    fn trim_scrollback(&mut self) {
        while self.scrollback.len() > MAX_SCROLLBACK {
            self.scrollback.pop_front();
        }
    }

    fn clear_all(&mut self) {
        self.grid = vec![self.blank_row(); self.rows];
        self.scrollback.clear();
        self.cur_r = 0;
        self.cur_c = 0;
        self.wrap_pending = false;
    }

    fn resize(&mut self, rows: usize, cols: usize) {
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        if rows < self.grid.len() {
            let extra = self.grid.len() - rows;
            if self.alt.is_none() {
                for row in self.grid.drain(..extra) {
                    self.scrollback.push_back(row);
                }
                self.trim_scrollback();
            } else {
                self.grid.drain(..extra);
            }
            self.cur_r = self.cur_r.saturating_sub(extra);
        }
        while self.grid.len() < rows {
            self.grid.push(vec![Cell::blank(); cols]);
        }
        for row in self.grid.iter_mut() {
            row.truncate(cols);
            while row.len() < cols {
                row.push(Cell::blank());
            }
        }
        self.scroll_top = 0;
        self.scroll_bottom = rows;
        self.cur_r = self.cur_r.min(rows - 1);
        self.cur_c = self.cur_c.min(cols - 1);
        self.wrap_pending = false;
    }

    fn linefeed(&mut self) {
        self.wrap_pending = false;
        if self.cur_r + 1 >= self.scroll_bottom {
            if self.grid.is_empty() {
                return;
            }
            let removed = self.grid.remove(self.scroll_top.min(self.grid.len() - 1));
            if self.scroll_top == 0 && self.alt.is_none() {
                self.scrollback.push_back(removed);
                self.trim_scrollback();
            }
            let blank = self.blank_row();
            let at = self.scroll_bottom.saturating_sub(1).min(self.grid.len());
            self.grid.insert(at, blank);
        } else {
            self.cur_r += 1;
        }
    }

    fn reverse_index(&mut self) {
        if self.cur_r == self.scroll_top {
            if self.scroll_bottom > self.scroll_top && self.scroll_bottom <= self.grid.len() {
                self.grid.remove(self.scroll_bottom - 1);
                let blank = self.blank_row();
                self.grid.insert(self.scroll_top, blank);
            }
        } else {
            self.cur_r -= 1;
        }
    }

    fn enter_alt(&mut self) {
        if self.alt.is_some() {
            return;
        }
        let saved = std::mem::replace(
            &mut self.grid,
            vec![vec![Cell::blank(); self.cols]; self.rows],
        );
        self.alt = Some((saved, (self.cur_r, self.cur_c)));
        self.cur_r = 0;
        self.cur_c = 0;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows;
        self.wrap_pending = false;
    }

    fn leave_alt(&mut self) {
        if let Some((grid, cur)) = self.alt.take() {
            self.grid = grid;
            self.cur_r = cur.0.min(self.rows - 1);
            self.cur_c = cur.1.min(self.cols - 1);
            self.scroll_top = 0;
            self.scroll_bottom = self.rows;
            self.wrap_pending = false;
        }
    }

    fn reset_pen(&mut self) {
        self.fg = None;
        self.bg = None;
        self.bold = false;
        self.inverse = false;
    }

    fn reset(&mut self) {
        self.reset_pen();
        self.scroll_top = 0;
        self.scroll_bottom = self.rows;
        self.auto_wrap = true;
        self.cursor_visible = true;
        self.bracketed_paste = false;
        self.app_cursor = false;
        self.alt = None;
        self.clear_all();
    }

    fn erase_display(&mut self, mode: u16) {
        let blank = self.erase_blank();
        match mode {
            0 => {
                for c in self.cur_c..self.cols {
                    self.grid[self.cur_r][c] = blank;
                }
                for r in self.cur_r + 1..self.rows {
                    self.grid[r] = vec![blank; self.cols];
                }
            }
            1 => {
                for r in 0..self.cur_r {
                    self.grid[r] = vec![blank; self.cols];
                }
                for c in 0..=self.cur_c.min(self.cols - 1) {
                    self.grid[self.cur_r][c] = blank;
                }
            }
            2 | 3 => {
                for r in 0..self.rows {
                    self.grid[r] = vec![blank; self.cols];
                }
                if mode == 3 {
                    self.scrollback.clear();
                }
            }
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let blank = self.erase_blank();
        let r = self.cur_r;
        match mode {
            0 => {
                for c in self.cur_c..self.cols {
                    self.grid[r][c] = blank;
                }
            }
            1 => {
                for c in 0..=self.cur_c.min(self.cols - 1) {
                    self.grid[r][c] = blank;
                }
            }
            2 => self.grid[r] = vec![blank; self.cols],
            _ => {}
        }
    }

    fn insert_chars(&mut self, n: usize) {
        let blank = self.erase_blank();
        let row = &mut self.grid[self.cur_r];
        for _ in 0..n.min(self.cols) {
            let at = self.cur_c.min(row.len());
            row.insert(at, blank);
            row.pop();
        }
    }

    fn delete_chars(&mut self, n: usize) {
        let blank = self.erase_blank();
        let row = &mut self.grid[self.cur_r];
        for _ in 0..n.min(self.cols) {
            let at = self.cur_c.min(row.len() - 1);
            row.remove(at);
            row.push(blank);
        }
    }

    fn erase_chars(&mut self, n: usize) {
        let blank = self.erase_blank();
        for c in self.cur_c..(self.cur_c + n).min(self.cols) {
            self.grid[self.cur_r][c] = blank;
        }
    }

    fn insert_lines(&mut self, n: usize) {
        for _ in 0..n.min(self.rows) {
            if self.cur_r >= self.scroll_bottom {
                break;
            }
            self.grid.remove(self.scroll_bottom - 1);
            let blank = self.blank_row();
            self.grid.insert(self.cur_r, blank);
        }
    }

    fn delete_lines(&mut self, n: usize) {
        for _ in 0..n.min(self.rows) {
            if self.cur_r >= self.scroll_bottom {
                break;
            }
            self.grid.remove(self.cur_r);
            let blank = self.blank_row();
            self.grid.insert(self.scroll_bottom - 1, blank);
        }
    }

    fn sgr(&mut self, v: &[u16]) {
        if v.is_empty() {
            self.reset_pen();
            return;
        }
        let mut i = 0;
        while i < v.len() {
            let p = v[i];
            match p {
                0 => self.reset_pen(),
                1 => self.bold = true,
                7 => self.inverse = true,
                21 | 22 => self.bold = false,
                27 => self.inverse = false,
                30..=37 => self.fg = Some(ansi_color((p - 30) as u8)),
                38 => {
                    if let Some(c) = extended_color(v, i) {
                        self.fg = Some(c.0);
                        i += c.1;
                    }
                }
                39 => self.fg = None,
                40..=47 => self.bg = Some(ansi_color((p - 40) as u8)),
                48 => {
                    if let Some(c) = extended_color(v, i) {
                        self.bg = Some(c.0);
                        i += c.1;
                    }
                }
                49 => self.bg = None,
                90..=97 => self.fg = Some(ansi_color((p - 90 + 8) as u8)),
                100..=107 => self.bg = Some(ansi_color((p - 100 + 8) as u8)),
                _ => {}
            }
            i += 1;
        }
    }
}

/// 解析 `38/48;5;n` 与 `38/48;2;r;g;b`，返回颜色与需要额外跳过的参数个数。
fn extended_color(v: &[u16], i: usize) -> Option<(Color32, usize)> {
    match v.get(i + 1).copied()? {
        5 => {
            let idx = *v.get(i + 2)?;
            Some((ansi_color(idx as u8), 2))
        }
        2 => {
            let r = *v.get(i + 2)?;
            let g = *v.get(i + 3)?;
            let b = *v.get(i + 4)?;
            Some((Color32::from_rgb(r as u8, g as u8, b as u8), 4))
        }
        _ => None,
    }
}

fn ansi_color(i: u8) -> Color32 {
    const BASE: [(u8, u8, u8); 16] = [
        (40, 42, 46), (220, 80, 80), (90, 190, 110), (220, 190, 90),
        (95, 150, 225), (195, 120, 205), (90, 195, 205), (215, 218, 222),
        (110, 112, 118), (250, 110, 110), (120, 220, 140), (245, 215, 115),
        (130, 180, 245), (225, 150, 235), (120, 225, 235), (245, 246, 248),
    ];
    if i < 16 {
        let (r, g, b) = BASE[i as usize];
        return Color32::from_rgb(r, g, b);
    }
    if i < 232 {
        let n = i - 16;
        let f = |x: u8| if x == 0 { 0 } else { 55 + 50 * x };
        return Color32::from_rgb(f(n / 36), f((n % 36) / 6), f(n % 6));
    }
    let g = 8 + (i - 232) * 10;
    Color32::from_rgb(g, g, g)
}

fn flat_params(p: &vte::Params) -> Vec<u16> {
    p.iter().flat_map(|s| s.iter().copied()).collect()
}

fn pv(v: &[u16], i: usize, def: u16) -> u16 {
    v.get(i).copied().unwrap_or(def)
}

impl vte::Perform for Screen {
    fn print(&mut self, c: char) {
        if self.wrap_pending {
            self.cur_c = 0;
            self.linefeed();
        }
        self.wrap_pending = false;
        if self.cur_r >= self.grid.len() || self.cur_c >= self.cols {
            return;
        }
        self.grid[self.cur_r][self.cur_c] =
            Cell { ch: c, fg: self.fg, bg: self.bg, bold: self.bold, inverse: self.inverse };
        if self.cur_c + 1 >= self.cols {
            if self.auto_wrap {
                self.wrap_pending = true;
            }
        } else {
            self.cur_c += 1;
        }
    }

    fn execute(&mut self, b: u8) {
        match b {
            0x08 => {
                self.cur_c = self.cur_c.saturating_sub(1);
                self.wrap_pending = false;
            }
            0x09 => {
                let next = (self.cur_c / 8 + 1) * 8;
                self.cur_c = next.min(self.cols.saturating_sub(1));
                self.wrap_pending = false;
            }
            0x0a | 0x0b | 0x0c => self.linefeed(),
            0x0d => {
                self.cur_c = 0;
                self.wrap_pending = false;
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, action: char) {
        let v = flat_params(params);
        let pos = |i: usize, d: u16| pv(&v, i, d).max(1) as usize - 1;
        let step = |i: usize| pv(&v, i, 1).max(1) as usize;
        match action {
            'm' => self.sgr(&v),
            'A' => self.cur_r = self.cur_r.saturating_sub(step(0)),
            'B' => self.cur_r = (self.cur_r + step(0)).min(self.scroll_bottom.saturating_sub(1)),
            'C' => self.cur_c = (self.cur_c + step(0)).min(self.cols - 1),
            'D' => self.cur_c = self.cur_c.saturating_sub(step(0)),
            'E' => {
                self.cur_r = (self.cur_r + step(0)).min(self.scroll_bottom.saturating_sub(1));
                self.cur_c = 0;
            }
            'F' => {
                self.cur_r = self.cur_r.saturating_sub(step(0));
                self.cur_c = 0;
            }
            'G' | '`' => self.cur_c = pos(0, 1).min(self.cols - 1),
            'd' => self.cur_r = pos(0, 1).min(self.rows - 1),
            'H' | 'f' => {
                self.cur_r = pos(0, 1).min(self.rows - 1);
                self.cur_c = pos(1, 1).min(self.cols - 1);
                self.wrap_pending = false;
            }
            'J' => self.erase_display(pv(&v, 0, 0)),
            'K' => self.erase_line(pv(&v, 0, 0)),
            'P' => self.delete_chars(step(0)),
            '@' => self.insert_chars(step(0)),
            'X' => self.erase_chars(step(0)),
            'L' => self.insert_lines(step(0)),
            'M' => self.delete_lines(step(0)),
            'r' => {
                let top = pos(0, 1).min(self.rows - 1);
                let bottom = (pv(&v, 1, self.rows as u16).max(1) as usize).min(self.rows);
                if top < bottom {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                }
                self.cur_r = self.scroll_top;
                self.cur_c = 0;
                self.wrap_pending = false;
            }
            's' => self.saved = (self.cur_r, self.cur_c),
            'u' => {
                self.cur_r = self.saved.0.min(self.rows - 1);
                self.cur_c = self.saved.1.min(self.cols - 1);
                self.wrap_pending = false;
            }
            'h' | 'l' if intermediates.contains(&b'?') => {
                let set = action == 'h';
                for &mode in &v {
                    match mode {
                        1 => self.app_cursor = set,
                        7 => self.auto_wrap = set,
                        25 => self.cursor_visible = set,
                        47 | 1047 | 1049 => {
                            if set {
                                self.enter_alt();
                            } else {
                                self.leave_alt();
                            }
                        }
                        2004 => self.bracketed_paste = set,
                        _ => {}
                    }
                }
            }
            'n' => match pv(&v, 0, 0) {
                6 => self
                    .replies
                    .push(format!("\x1b[{};{}R", self.cur_r + 1, self.cur_c + 1).into_bytes()),
                5 => self.replies.push(b"\x1b[0n".to_vec()),
                _ => {}
            },
            'c' => self.replies.push(b"\x1b[?1;2c".to_vec()),
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'7' => self.saved = (self.cur_r, self.cur_c),
            b'8' => {
                self.cur_r = self.saved.0.min(self.rows - 1);
                self.cur_c = self.saved.1.min(self.cols - 1);
                self.wrap_pending = false;
            }
            b'D' => self.linefeed(),
            b'E' => {
                self.cur_c = 0;
                self.linefeed();
            }
            b'M' => self.reverse_index(),
            b'c' => self.reset(),
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
}

pub struct Terminal {
    pub open: bool,
    pub height: f32,
    cwd: PathBuf,
    screen: Screen,
    parser: vte::Parser,
    rx: Option<Receiver<TermEvent>>,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    size: (u16, u16),
    exited: bool,
    error: Option<String>,
    focus_req: bool,
    focused: bool,
    body_id: egui::Id,
}

impl Terminal {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            open: false,
            height: 220.0,
            cwd,
            screen: Screen::new(24, 80),
            parser: vte::Parser::new(),
            rx: None,
            master: None,
            writer: None,
            child: None,
            size: (24, 80),
            exited: false,
            error: None,
            focus_req: false,
            focused: false,
            body_id: egui::Id::unique("markd_terminal_body"),
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.focus_req = self.open;
    }

    /// shell 启动后自己维护工作目录，只在启动前接受外部 cwd。
    pub fn set_cwd(&mut self, path: PathBuf) {
        if self.master.is_none() {
            self.cwd = path;
        }
    }

    pub fn has_focus(&self) -> bool {
        self.open && self.focused
    }

    /// 主动让出键盘焦点（点回编辑区时调用）。
    pub fn blur(&mut self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.surrender_focus(self.body_id));
        self.focused = false;
    }

    fn write_bytes(&mut self, data: &[u8]) {
        if let Some(w) = self.writer.as_mut() {
            let _ = w.write_all(data);
            let _ = w.flush();
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let (rows, cols) = self.size;
        let system = native_pty_system();
        let PtyPair { slave, master } = match system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(format!("无法创建 PTY: {e}"));
                return;
            }
        };

        let mut cmd = if cfg!(windows) {
            CommandBuilder::new_default_prog()
        } else {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
            CommandBuilder::new(shell)
        };
        cmd.cwd(&self.cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = match slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("无法启动 shell: {e}"));
                return;
            }
        };
        drop(slave);

        let reader = match master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(format!("无法读取 PTY: {e}"));
                return;
            }
        };
        let writer = master.take_writer().ok();

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.master = Some(master);
        self.writer = writer;
        self.child = Some(child);

        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut reader = reader;
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(TermEvent::Data(buf[..n].to_vec())).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                }
            }
            let _ = tx.send(TermEvent::Exited);
            ctx.request_repaint();
        });
    }

    fn ensure_started(&mut self, ctx: &egui::Context) {
        if self.master.is_none() && !self.exited && self.error.is_none() {
            self.start(ctx);
        }
    }

    fn restart(&mut self, ctx: &egui::Context) {
        self.write_bytes(b"exit\r");
        self.rx = None;
        self.master = None;
        self.writer = None;
        self.child = None;
        self.exited = false;
        self.error = None;
        self.screen.clear_all();
        self.focus_req = true;
        self.start(ctx);
    }

    /// 每帧把后台线程收到的字节喂给 vte 解析器。
    pub fn poll(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(ev) = rx.try_recv() {
                events.push(ev);
            }
        }
        for ev in events {
            match ev {
                TermEvent::Data(bytes) => {
                    self.parser.advance(&mut self.screen, &bytes);
                    let replies = std::mem::take(&mut self.screen.replies);
                    for reply in replies {
                        self.write_bytes(&reply);
                    }
                }
                TermEvent::Exited => {
                    self.exited = true;
                    self.rx = None;
                }
            }
        }
    }

    fn resize_pty(&mut self, rows: u16, cols: u16) {
        if self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        self.screen.resize(rows as usize, cols as usize);
        if let Some(master) = &self.master {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    fn plain_text(&self) -> String {
        let mut s = String::new();
        for row in self.screen.scrollback.iter().chain(self.screen.grid.iter()) {
            for cell in row {
                s.push(cell.ch);
            }
            s.push('\n');
        }
        s
    }

    fn cell_format(&self, cell: &Cell, theme: &Theme, font_id: &FontId, line_height: f32) -> TextFormat {
        let (mut fg, bg) = if cell.inverse {
            (cell.bg.unwrap_or(theme.panel), cell.fg.unwrap_or(theme.text))
        } else {
            (cell.fg.unwrap_or(theme.text), cell.bg.unwrap_or(Color32::TRANSPARENT))
        };
        if cell.bold {
            fg = Color32::from_rgb(
                fg.r().saturating_add(40),
                fg.g().saturating_add(40),
                fg.b().saturating_add(40),
            );
        }
        TextFormat {
            font_id: font_id.clone(),
            color: fg,
            background: bg,
            expand_bg: 0.0,
            line_height: Some(line_height),
            ..Default::default()
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, theme: &Theme) {
        ui.style_mut().interaction.selectable_labels = false;
        self.ensure_started(ctx);

        ui.horizontal(|ui| {
            ui.label(RichText::new("终端").strong().color(theme.accent));
            ui.label(RichText::new(self.cwd.to_string_lossy()).small().color(theme.dim));
            if self.master.is_some() && !self.focused {
                ui.label(RichText::new("· 点击下方区域输入").small().color(theme.dim));
            }
            if let Some(e) = &self.error {
                ui.label(RichText::new(e).small().color(Color32::from_rgb(220, 95, 90)));
            } else if self.exited {
                ui.label(RichText::new("shell 已退出").small().color(Color32::from_rgb(220, 95, 90)));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("关闭").on_hover_cursor(egui::CursorIcon::Default).clicked() {
                    self.open = false;
                }
                if ui.button("重启").on_hover_cursor(egui::CursorIcon::Default).clicked() {
                    self.restart(ctx);
                }
                if ui.button("复制").on_hover_cursor(egui::CursorIcon::Default).clicked() {
                    ctx.copy_text(self.plain_text());
                }
                if ui.button("清空").on_hover_cursor(egui::CursorIcon::Default).clicked() {
                    self.screen.clear_all();
                }
            });
        });
        ui.separator();

        let font_id = FontId::new(theme.mono, FontFamily::Monospace);
        let (cw, rh) = ctx.fonts_mut(|f| {
            let cw = f.glyph_width(&font_id, 'M');
            let rh = f.row_height(&font_id);
            (cw.max(1.0), rh.max(1.0))
        });
        let avail = ui.available_size();
        let cols = ((avail.x - 14.0) / cw).floor().max(20.0) as u16;
        let rows = (avail.y / rh).floor().max(6.0) as u16;
        self.resize_pty(rows, cols);

        let cursor_line = self.screen.scrollback.len() + self.screen.cur_r;
        let cursor_col = self.screen.cur_c;
        let show_cursor = self.screen.cursor_visible && self.focused;
        let mut job = LayoutJob::default();
        job.wrap.max_width = f32::INFINITY;
        let newline_fmt = TextFormat {
            font_id: font_id.clone(),
            color: theme.text,
            line_height: Some(rh),
            ..Default::default()
        };
        for (li, row) in self.screen.scrollback.iter().chain(self.screen.grid.iter()).enumerate() {
            if li > 0 {
                job.append("\n", 0.0, newline_fmt.clone());
            }
            let mut last = 0;
            for (ci, cell) in row.iter().enumerate() {
                if cell.ch != ' ' || cell.bg.is_some() || cell.inverse {
                    last = ci + 1;
                }
            }
            if li == cursor_line {
                last = last.max(cursor_col + 1);
            }
            for (ci, cell) in row.iter().take(last.min(row.len())).enumerate() {
                let mut fmt = self.cell_format(cell, theme, &font_id, rh);
                if show_cursor && li == cursor_line && ci == cursor_col {
                    fmt.background = theme.accent;
                    fmt.color = theme.panel;
                }
                job.append(&cell.ch.to_string(), 0.0, fmt);
            }
            if last == 0 {
                job.append(" ", 0.0, newline_fmt.clone());
            }
        }

        ScrollArea::vertical()
            .id_salt("terminal_output")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                // 先把文本排版好，再把整块可见区域做成可点击/可聚焦控件。
                // 只让文字本身可点的话，短输出时几乎点不中，表现就是「怎么点都没反应」。
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                let body_w = (avail.x - 12.0).max(1.0);
                let size = egui::vec2(
                    body_w.max(galley.size().x),
                    avail.y.max(galley.size().y),
                );
                // 用稳定 id，才能可靠地在帧间保持/让出焦点。
                let id = self.body_id;
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let response = ui.interact(rect, id, Sense::click() | Sense::FOCUSABLE);
                ui.painter().galley(response.rect.min, galley, theme.text);
                if response.clicked() {
                    response.request_focus();
                }
                if self.focus_req {
                    response.request_focus();
                    self.focus_req = false;
                }
                self.focused = ui.memory(|m| m.has_focus(id));
                if self.focused {
                    ui.painter().rect_stroke(
                        response.rect,
                        egui::CornerRadius::ZERO,
                        egui::Stroke::new(1.0, theme.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                if self.focused {
                    let events: Vec<egui::Event> = ui.input(|i| i.events.clone());
                    let mut out = Vec::new();
                    for ev in events {
                        match ev {
                            egui::Event::Text(s) => out.extend_from_slice(s.as_bytes()),
                            egui::Event::Paste(s) => {
                                let s = s.replace("\r\n", "\n").replace('\r', "\n");
                                let s = if self.screen.bracketed_paste {
                                    format!("\x1b[200~{s}\x1b[201~")
                                } else {
                                    s
                                };
                                out.extend_from_slice(s.as_bytes());
                            }
                            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                                if modifiers.command && !modifiers.ctrl && key == egui::Key::C {
                                    ctx.copy_text(self.plain_text());
                                } else if let Some(bytes) =
                                    key_bytes(key, modifiers, self.screen.app_cursor)
                                {
                                    out.extend_from_slice(&bytes);
                                }
                            }
                            _ => {}
                        }
                    }
                    if !out.is_empty() {
                        self.write_bytes(&out);
                    }
                }
            });
    }
}

/// 把 egui 按键转成写入 PTY 的字节序列。
fn key_bytes(key: egui::Key, m: egui::Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    use egui::Key;
    if m.ctrl {
        let b = match key {
            Key::A => 0x01, Key::B => 0x02, Key::C => 0x03, Key::D => 0x04,
            Key::E => 0x05, Key::F => 0x06, Key::G => 0x07, Key::H => 0x08,
            Key::J => 0x0a, Key::K => 0x0b, Key::L => 0x0c, Key::M => 0x0d,
            Key::N => 0x0e, Key::O => 0x0f, Key::P => 0x10, Key::Q => 0x11,
            Key::R => 0x12, Key::S => 0x13, Key::T => 0x14, Key::U => 0x15,
            Key::W => 0x17, Key::Y => 0x19, Key::Z => 0x1a,
            Key::OpenBracket => 0x1b,
            _ => return None,
        };
        return Some(vec![b]);
    }
    let seq: &[u8] = match key {
        Key::Enter => b"\r",
        Key::Backspace => b"\x7f",
        Key::Tab => {
            if m.shift {
                b"\x1b[Z"
            } else {
                b"\t"
            }
        }
        Key::Escape => b"\x1b",
        Key::ArrowUp => {
            if app_cursor { b"\x1bOA" } else { b"\x1b[A" }
        }
        Key::ArrowDown => {
            if app_cursor { b"\x1bOB" } else { b"\x1b[B" }
        }
        Key::ArrowRight => {
            if app_cursor { b"\x1bOC" } else { b"\x1b[C" }
        }
        Key::ArrowLeft => {
            if app_cursor { b"\x1bOD" } else { b"\x1b[D" }
        }
        Key::Home => b"\x1b[H",
        Key::End => b"\x1b[F",
        Key::Insert => b"\x1b[2~",
        Key::Delete => b"\x1b[3~",
        Key::PageUp => b"\x1b[5~",
        Key::PageDown => b"\x1b[6~",
        _ => return None,
    };
    Some(seq.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_text(screen: &Screen, r: usize) -> String {
        screen.grid[r]
            .iter()
            .map(|c| c.ch)
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn new_screen(rows: usize, cols: usize) -> (Screen, vte::Parser) {
        (Screen::new(rows, cols), vte::Parser::new())
    }

    #[test]
    fn prints_wraps_and_breaks_line() {
        let (mut s, mut p) = new_screen(3, 5);
        p.advance(&mut s, b"abcdefg");
        assert_eq!(row_text(&s, 0), "abcde");
        assert_eq!(row_text(&s, 1), "fg");
    }

    #[test]
    fn carriage_return_overwrites_line() {
        let (mut s, mut p) = new_screen(3, 10);
        p.advance(&mut s, b"hello\rHI");
        assert_eq!(row_text(&s, 0), "HIllo");
    }

    #[test]
    fn erase_line_and_display() {
        let (mut s, mut p) = new_screen(3, 10);
        p.advance(&mut s, b"abcdef\x1b[2K");
        assert_eq!(row_text(&s, 0), "");
        p.advance(&mut s, b"one\r\ntwo\r\nthree\x1b[2J");
        assert_eq!(row_text(&s, 0), "");
        assert_eq!(row_text(&s, 1), "");
    }

    #[test]
    fn cursor_positioning_and_sgr() {
        let (mut s, mut p) = new_screen(3, 10);
        p.advance(&mut s, b"\x1b[2;3HZ");
        assert_eq!(s.grid[1][2].ch, 'Z');
        p.advance(&mut s, b"\x1b[31mR");
        assert_eq!(s.grid[1][3].fg, Some(ansi_color(1)));
        p.advance(&mut s, b"\x1b[38;2;1;2;3mT");
        assert_eq!(s.grid[1][4].fg, Some(Color32::from_rgb(1, 2, 3)));
        p.advance(&mut s, b"\x1b[0mN");
        assert_eq!(s.grid[1][5].fg, None);
    }

    #[test]
    fn alt_screen_roundtrip() {
        let (mut s, mut p) = new_screen(3, 10);
        p.advance(&mut s, b"AAA");
        p.advance(&mut s, b"\x1b[?1049h");
        assert_eq!(row_text(&s, 0), "");
        p.advance(&mut s, b"BBB");
        assert_eq!(row_text(&s, 0), "BBB");
        p.advance(&mut s, b"\x1b[?1049l");
        assert_eq!(row_text(&s, 0), "AAA");
    }

    #[test]
    fn scrolling_pushes_into_scrollback() {
        let (mut s, mut p) = new_screen(2, 4);
        p.advance(&mut s, b"1\r\n2\r\n3");
        assert_eq!(s.scrollback.len(), 1);
        assert_eq!(row_text(&s, 0), "2");
        assert_eq!(row_text(&s, 1), "3");
    }

    #[test]
    fn bracketed_paste_mode_toggles() {
        let (mut s, mut p) = new_screen(2, 10);
        assert!(!s.bracketed_paste);
        p.advance(&mut s, b"\x1b[?2004h");
        assert!(s.bracketed_paste);
        p.advance(&mut s, b"\x1b[?2004l");
        assert!(!s.bracketed_paste);
    }

    #[test]
    fn maps_special_keys_to_pty_bytes() {
        let none = egui::Modifiers::NONE;
        assert_eq!(key_bytes(egui::Key::Enter, none, false).unwrap(), vec![b'\r']);
        assert_eq!(key_bytes(egui::Key::Backspace, none, false).unwrap(), vec![0x7f]);
        assert_eq!(key_bytes(egui::Key::Tab, none, false).unwrap(), vec![b'\t']);
        assert_eq!(key_bytes(egui::Key::ArrowUp, none, false).unwrap(), b"\x1b[A".to_vec());
        assert_eq!(key_bytes(egui::Key::ArrowUp, none, true).unwrap(), b"\x1bOA".to_vec());
        let ctrl = egui::Modifiers { ctrl: true, ..Default::default() };
        assert_eq!(key_bytes(egui::Key::C, ctrl, false).unwrap(), vec![0x03]);
        assert_eq!(key_bytes(egui::Key::D, ctrl, false).unwrap(), vec![0x04]);
        // Ctrl+V 留给粘贴事件，不能被当成控制字符写进 PTY。
        assert!(key_bytes(egui::Key::V, ctrl, false).is_none());
        assert!(key_bytes(egui::Key::Space, none, false).is_none());
    }

    fn run_term_frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        term: &mut Terminal,
        theme: &Theme,
        screen: egui::Rect,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let c = ctx.clone();
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| term.ui(ui, &c, theme));
        });
        out.textures_delta.clear();
    }

    /// 点击终端主体必须把键盘焦点从编辑区抢过来，否则输入仍然进入文档。
    #[test]
    fn clicking_body_takes_focus() {
        let ctx = egui::Context::default();
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 320.0));
        let mut term = Terminal::new(std::env::temp_dir());
        term.open = true;
        term.focus_req = false;

        run_term_frame(&ctx, vec![], &mut term, &theme, screen);
        assert!(!term.has_focus(), "初始不应有焦点");

        let p = egui::pos2(450.0, 200.0);
        run_term_frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            &mut term,
            &theme,
            screen,
        );
        run_term_frame(
            &ctx,
            vec![egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            &mut term,
            &theme,
            screen,
        );
        assert!(term.has_focus(), "点击终端主体后应获得键盘焦点");
    }

    /// 端到端：真的起一个 PTY shell，写入命令，再从解析结果里找输出。
    /// Windows 默认 shell 是 cmd，命令语法不同，这里只在类 Unix 上跑。
    #[cfg(not(windows))]
    #[test]
    fn pty_shell_executes_command() {
        let ctx = egui::Context::default();
        let mut term = Terminal::new(std::env::temp_dir());
        term.start(&ctx);
        assert!(term.master.is_some(), "PTY 未启动：{:?}", term.error);
        std::thread::sleep(std::time::Duration::from_millis(300));
        term.write_bytes(b"echo $((123456*2))\r");
        let mut text = String::new();
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            term.poll();
            text = term.plain_text();
            if text.contains("246912") {
                break;
            }
        }
        term.write_bytes(b"exit\r");
        assert!(text.contains("246912"), "PTY 没有回显命令结果：\n{text}");
    }
}

