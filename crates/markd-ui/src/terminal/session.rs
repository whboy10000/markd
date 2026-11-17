//! PTY 会话与终端面板绘制。

use super::*;

pub struct Terminal {
    pub open: bool,
    pub height: f32,
    cwd: PathBuf,
    screen: Screen,
    parser: vte::Parser,
    rx: Option<Receiver<TermEvent>>,
    pub(super) master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    size: (u16, u16),
    exited: bool,
    pub(super) error: Option<String>,
    pub(super) focus_req: bool,
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

    pub(super) fn write_bytes(&mut self, data: &[u8]) {
        if let Some(w) = self.writer.as_mut() {
            let _ = w.write_all(data);
            let _ = w.flush();
        }
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

    pub(super) fn plain_text(&self) -> String {
        let mut s = String::new();
        for row in self.screen.scrollback.iter().chain(self.screen.grid.iter()) {
            for cell in row {
                s.push(cell.ch);
            }
            s.push('\n');
        }
        s
    }

    fn cell_format(
        &self,
        cell: &Cell,
        theme: &Theme,
        font_id: &FontId,
        line_height: f32,
    ) -> TextFormat {
        let (mut fg, bg) = if cell.inverse {
            (
                cell.bg.unwrap_or(theme.panel),
                cell.fg.unwrap_or(theme.text),
            )
        } else {
            (
                cell.fg.unwrap_or(theme.text),
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
            ui.label(
                RichText::new(self.cwd.to_string_lossy())
                    .small()
                    .color(theme.dim),
            );
            if self.master.is_some() && !self.focused {
                ui.label(RichText::new("· 点击下方区域输入").small().color(theme.dim));
            }
            if let Some(e) = &self.error {
                ui.label(
                    RichText::new(e)
                        .small()
                        .color(Color32::from_rgb(220, 95, 90)),
                );
            } else if self.exited {
                ui.label(
                    RichText::new("shell 已退出")
                        .small()
                        .color(Color32::from_rgb(220, 95, 90)),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("关闭")
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .clicked()
                {
                    self.open = false;
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
        for (li, row) in self
            .screen
            .scrollback
            .iter()
            .chain(self.screen.grid.iter())
            .enumerate()
        {
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
                let size = egui::vec2(body_w.max(galley.size().x), avail.y.max(galley.size().y));
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
