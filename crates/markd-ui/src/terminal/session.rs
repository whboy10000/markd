//! 单个 PTY 会话：屏幕模型渲染、键盘输入与 PTY 读写。
//!
//! 一个 `Session` 就是一个独立的 shell（一个 PTY + 一块字符网格）。
//! 上层 `Terminal` 负责把多个 Session 组织成分屏树与页签，这里只管「一个格子
//! 怎么画、一个按键怎么转发」，不关心自己被摆在窗口的哪个位置。

use super::*;

/// 终端自己的配色。可独立于应用主题切到固定深色（月亮按钮）。
#[derive(Clone, Copy)]
pub(super) struct TermColors {
    /// 默认前景色。
    pub text: Color32,
    /// 次要文字（cwd、未聚焦提示）。
    pub dim: Color32,
    /// 强调色（光标、聚焦边框）。
    pub accent: Color32,
    /// 终端背景色。
    pub bg: Color32,
    /// 报错文字。
    pub error: Color32,
    /// 等宽字体字号（用户设置里的 terminal 字号）。
    pub mono: f32,
}

impl TermColors {
    /// `dark = true` 时用一套固定的深色终端配色，不随应用浅色主题变白。
    pub(super) fn new(theme: &Theme, dark: bool) -> Self {
        let base = Self {
            text: theme.text,
            dim: theme.dim,
            accent: theme.accent,
            bg: theme.panel,
            error: Color32::from_rgb(220, 95, 90),
            mono: theme.mono,
        };
        if dark {
            Self {
                text: Color32::from_rgb(212, 216, 224),
                dim: Color32::from_rgb(120, 130, 146),
                accent: Color32::from_rgb(86, 156, 214),
                bg: Color32::from_rgb(24, 26, 31),
                error: Color32::from_rgb(240, 110, 104),
                ..base
            }
        } else {
            base
        }
    }
}

pub(super) struct Session {
    /// 稳定唯一 id：既做 egui widget id（焦点），也做页签/窗格的标识。
    pub(super) id: u64,
    /// 页签标题，通常是 shell 名。
    pub(super) title: String,
    cwd: PathBuf,
    screen: Screen,
    parser: vte::Parser,
    rx: Option<Receiver<TermEvent>>,
    pub(super) master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    size: (u16, u16),
    pub(super) exited: bool,
    pub(super) error: Option<String>,
    pub(super) focus_req: bool,
    pub(super) focused: bool,
    body_id: egui::Id,
}

impl Session {
    pub(super) fn new(id: u64, cwd: PathBuf) -> Self {
        // shell 名做页签标题，取路径最后一段，跨平台。
        let title = std::env::var("SHELL")
            .ok()
            .and_then(|s| {
                PathBuf::from(s)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    "cmd".into()
                } else {
                    "shell".into()
                }
            });
        Self {
            id,
            title,
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
            body_id: egui::Id::new(("markd_terminal_body", id)),
        }
    }

    pub(super) fn body_id(&self) -> egui::Id {
        self.body_id
    }

    /// shell 启动后自己维护工作目录，只在启动前接受外部 cwd。
    pub(super) fn set_cwd(&mut self, path: PathBuf) {
        if self.master.is_none() {
            self.cwd = path;
        }
    }

    pub(super) fn write_bytes(&mut self, data: &[u8]) {
        if let Some(w) = self.writer.as_mut() {
            let _ = w.write_all(data);
            let _ = w.flush();
        }
    }

    pub(super) fn has_focus(&self) -> bool {
        self.focused
    }

    /// 主动让出键盘焦点（点回编辑区时调用）。
    pub(super) fn blur(&mut self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.surrender_focus(self.body_id));
        self.focused = false;
    }

    pub(super) fn start(&mut self, ctx: &egui::Context) {
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
        // 从 Dock/Finder 启动的 GUI 进程没有 LANG，shell 会落到 C locale，
        // ls 等命令把非 ASCII 文件名输出成 '?'。
        #[cfg(not(windows))]
        if std::env::var_os("LANG").is_none() {
            cmd.env("LANG", "en_US.UTF-8");
        }

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

    /// 每帧把后台线程收到的字节喂给 vte 解析器。
    pub(super) fn poll(&mut self) {
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

    pub(super) fn plain_text(&self) -> String {
        let mut s = String::new();
        for row in self.screen.scrollback.iter().chain(self.screen.grid.iter()) {
            for cell in row {
                if cell.ch != '\0' {
                    s.push(cell.ch);
                }
            }
            s.push('\n');
        }
        s
    }

    fn cell_colors(&self, cell: &Cell, colors: &TermColors) -> (Color32, Color32) {
        let (mut fg, bg) = if cell.inverse {
            (cell.bg.unwrap_or(colors.bg), cell.fg.unwrap_or(colors.text))
        } else {
            (
                cell.fg.unwrap_or(colors.text),
                cell.bg.unwrap_or(Color32::TRANSPARENT),
            )
        };
        if cell.bold {
            fg = Color32::from_rgb(
                fg.r().saturating_add(40),
                fg.g().saturating_add(40),
                fg.b().saturating_add(40),
            );
        }
        (fg, bg)
    }

    /// 在当前 Ui 里绘制这个会话（调用方已把它放进分屏对应的矩形）。
    /// `frame` 为 true 时在四周画一圈聚焦/普通边框。
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, colors: TermColors) {
        ui.style_mut().interaction.selectable_labels = false;
        self.ensure_started(ctx);

        // 背景铺满整个可用区域，深色终端才不会透出应用面板的浅色。
        let area = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(area, egui::CornerRadius::ZERO, colors.bg);

        let font_id = FontId::new(colors.mono, FontFamily::Monospace);
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
        let sb = self.screen.scrollback.len();
        let total_lines = sb + self.screen.grid.len();

        ScrollArea::vertical()
            .id_salt(("terminal_output", self.id))
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let body_w = (avail.x - 12.0).max(1.0);
                let size = egui::vec2(
                    body_w.max(cols as f32 * cw),
                    avail.y.max(total_lines as f32 * rh),
                );
                // 用稳定 id，才能可靠地在帧间保持/让出焦点。
                let id = self.body_id;
                // 整块可见区域做成可点击/可聚焦控件：只让文字本身可点的话，
                // 短输出时几乎点不中，表现就是「怎么点都没反应」。
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let response = ui.interact(rect, id, Sense::click() | Sense::FOCUSABLE);

                // 按固定网格逐格绘制：CJK 宽字符占两列，若用比例排版，
                // 字形宽度与格宽不一致，字会互相叠压，看起来就是乱码。
                let painter = ui.painter();
                let clip = ui.clip_rect();
                let first =
                    (((clip.top() - rect.min.y) / rh).floor().max(0.0) as usize).min(total_lines);
                let last =
                    (((clip.bottom() - rect.min.y) / rh).ceil().max(0.0) as usize).min(total_lines);
                let row_at = |li: usize| -> &Vec<Cell> {
                    if li < sb {
                        &self.screen.scrollback[li]
                    } else {
                        &self.screen.grid[li - sb]
                    }
                };
                for li in first..last {
                    let row = row_at(li);
                    let y = rect.min.y + li as f32 * rh;
                    let mut ci = 0;
                    while ci < row.len() {
                        let cell = &row[ci];
                        let (fg, bg) = self.cell_colors(cell, &colors);
                        let x = rect.min.x + ci as f32 * cw;
                        if cell.ch == '\0' {
                            if bg != Color32::TRANSPARENT {
                                painter.rect_filled(
                                    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cw, rh)),
                                    0.0,
                                    bg,
                                );
                            }
                            ci += 1;
                            continue;
                        }
                        let wide = ci + 1 < row.len() && row[ci + 1].ch == '\0';
                        if wide || !cell.ch.is_ascii() {
                            // 宽字符与非 ASCII 单独按列定位画，避免 fallback 字体宽度漂移。
                            let w = if wide { 2 } else { 1 };
                            if bg != Color32::TRANSPARENT {
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(x, y),
                                        egui::vec2(w as f32 * cw, rh),
                                    ),
                                    0.0,
                                    bg,
                                );
                            }
                            if cell.ch != ' ' {
                                painter.text(
                                    egui::pos2(x, y),
                                    egui::Align2::LEFT_TOP,
                                    cell.ch.to_string(),
                                    font_id.clone(),
                                    fg,
                                );
                            }
                            ci += w;
                        } else {
                            // 同样式连续 ASCII 合并成一次绘制，等宽字体步进恰为 cw。
                            let start = ci;
                            let mut text = String::new();
                            while ci < row.len() {
                                let c2 = &row[ci];
                                if c2.ch == '\0'
                                    || !c2.ch.is_ascii()
                                    || self.cell_colors(c2, &colors) != (fg, bg)
                                {
                                    break;
                                }
                                text.push(c2.ch);
                                ci += 1;
                            }
                            let sx = rect.min.x + start as f32 * cw;
                            if bg != Color32::TRANSPARENT {
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(sx, y),
                                        egui::vec2((ci - start) as f32 * cw, rh),
                                    ),
                                    0.0,
                                    bg,
                                );
                            }
                            painter.text(
                                egui::pos2(sx, y),
                                egui::Align2::LEFT_TOP,
                                text,
                                font_id.clone(),
                                fg,
                            );
                        }
                    }
                }
                if show_cursor && cursor_line >= first && cursor_line < last {
                    let x = rect.min.x + cursor_col as f32 * cw;
                    let y = rect.min.y + cursor_line as f32 * rh;
                    let crect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cw, rh));
                    painter.rect_filled(crect, 0.0, colors.accent);
                    // 光标下的字符反色重绘；宽字符只重绘落在光标格里的那一半。
                    let row = row_at(cursor_line);
                    if let Some(cell) = row.get(cursor_col) {
                        let (gx, ch) = if cell.ch == '\0' && cursor_col > 0 {
                            (x - cw, row[cursor_col - 1].ch)
                        } else {
                            (x, cell.ch)
                        };
                        if ch != ' ' && ch != '\0' {
                            painter.with_clip_rect(crect).text(
                                egui::pos2(gx, y),
                                egui::Align2::LEFT_TOP,
                                ch.to_string(),
                                font_id.clone(),
                                colors.bg,
                            );
                        }
                    }
                }
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
                        egui::Stroke::new(1.0, colors.accent),
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
                            egui::Event::Key {
                                key,
                                pressed: true,
                                modifiers,
                                ..
                            } => {
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
