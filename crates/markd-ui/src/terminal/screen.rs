//! VT100/xterm 屏幕模型与 ANSI 序列处理。

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Cell {
    pub(super) ch: char,
    pub(super) fg: Option<Color32>,
    pub(super) bg: Option<Color32>,
    pub(super) bold: bool,
    pub(super) inverse: bool,
}

impl Cell {
    fn blank() -> Self {
        Self {
            ch: ' ',
            fg: None,
            bg: None,
            bold: false,
            inverse: false,
        }
    }
}

pub(super) enum TermEvent {
    Data(Vec<u8>),
    Exited,
}

/// 一个极简 VT100/xterm 屏幕模型，实现 `vte::Perform`。
pub(super) struct Screen {
    pub(super) rows: usize,
    pub(super) cols: usize,
    pub(super) grid: Vec<Vec<Cell>>,
    pub(super) scrollback: VecDeque<Vec<Cell>>,
    pub(super) cur_r: usize,
    pub(super) cur_c: usize,
    pub(super) saved: (usize, usize),
    pub(super) fg: Option<Color32>,
    pub(super) bg: Option<Color32>,
    pub(super) bold: bool,
    pub(super) inverse: bool,
    pub(super) scroll_top: usize,
    pub(super) scroll_bottom: usize,
    pub(super) wrap_pending: bool,
    pub(super) auto_wrap: bool,
    pub(super) cursor_visible: bool,
    pub(super) bracketed_paste: bool,
    pub(super) app_cursor: bool,
    pub(super) alt: Option<(Vec<Vec<Cell>>, (usize, usize))>,
    pub(super) replies: Vec<Vec<u8>>,
}

impl Screen {
    pub(super) fn new(rows: usize, cols: usize) -> Self {
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
        Cell {
            ch: ' ',
            fg: None,
            bg: self.bg,
            bold: false,
            inverse: false,
        }
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

    pub(super) fn resize(&mut self, rows: usize, cols: usize) {
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
pub(super) fn extended_color(v: &[u16], i: usize) -> Option<(Color32, usize)> {
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

pub(super) fn ansi_color(i: u8) -> Color32 {
    const BASE: [(u8, u8, u8); 16] = [
        (40, 42, 46),
        (220, 80, 80),
        (90, 190, 110),
        (220, 190, 90),
        (95, 150, 225),
        (195, 120, 205),
        (90, 195, 205),
        (215, 218, 222),
        (110, 112, 118),
        (250, 110, 110),
        (120, 220, 140),
        (245, 215, 115),
        (130, 180, 245),
        (225, 150, 235),
        (120, 225, 235),
        (245, 246, 248),
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

pub(super) fn flat_params(p: &vte::Params) -> Vec<u16> {
    p.iter().flat_map(|s| s.iter().copied()).collect()
}

pub(super) fn pv(v: &[u16], i: usize, def: u16) -> u16 {
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
        self.grid[self.cur_r][self.cur_c] = Cell {
            ch: c,
            fg: self.fg,
            bg: self.bg,
            bold: self.bold,
            inverse: self.inverse,
        };
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

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
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
    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
    }
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
}
