//! 跨块选区剪贴板与轻提示。

use super::*;

impl MarkdApp {
    /// 复制渲染态拖选出的跨块选区；没有选区时返回 false（交给 TextEdit 原生复制）。
    /// 同时写入带格式的 HTML 与纯文本，粘贴到富文本编辑器时保留块/行内格式。
    pub(super) fn copy_cross_sel(&mut self, ctx: &egui::Context) -> bool {
        // 表格单元格选区：只复制选中的那几格（列用制表符分隔，粘到表格软件正好一格一个）。
        if self.editor.cross_sel.is_none()
            && let Some(text) = editor::table_sel_text(&self.doc, &self.editor)
        {
            let html = editor::table_sel_html(&self.doc, &self.editor).unwrap_or_default();
            crate::clipboard::copy_rich(ctx, &text, &html);
            self.notify(ctx, "已复制".to_string());
            return true;
        }
        let Some(sel) = self.editor.cross_sel else {
            return false;
        };
        let Some(text) = editor::cross_sel_text(&self.doc, Some(sel)) else {
            return false;
        };
        let html = crate::export::cross_sel_html(&self.doc.blocks, sel);
        crate::clipboard::copy_rich(ctx, &text, &html);
        self.notify(ctx, "已复制".to_string());
        true
    }

    /// 剪切跨块选区：先复制（带格式）到剪贴板，再删除选中的内容。
    /// 没有选区时返回 false（交给 TextEdit 原生剪切）。
    pub(super) fn cut_cross_sel(&mut self, ctx: &egui::Context) -> bool {
        // 表格单元格选区：删格子里的文字容易连带破坏表格结构，只复制不删。
        if self.editor.cross_sel.is_none()
            && let Some(text) = editor::table_sel_text(&self.doc, &self.editor)
        {
            let html = editor::table_sel_html(&self.doc, &self.editor).unwrap_or_default();
            crate::clipboard::copy_rich(ctx, &text, &html);
            self.notify(ctx, "已复制（表格单元格不支持剪切）".to_string());
            return true;
        }
        let Some(sel) = self.editor.cross_sel else {
            return false;
        };
        let html = crate::export::cross_sel_html(&self.doc.blocks, sel);
        let Some(text) = editor::cut_cross_sel(&mut self.doc, &mut self.editor) else {
            return false;
        };
        crate::clipboard::copy_rich(ctx, &text, &html);
        self.notify(ctx, "已剪切".to_string());
        self.on_changed();
        ctx.request_repaint();
        true
    }

    /// 复制活动块内的单块文本选区（无跨块选区时）。写入带格式的 HTML。
    /// 没有单块选区时返回 false（例如只有光标、或焦点在终端）。
    /// 处理成功后会从 egui 输入里移除 `Event::Copy`，阻止 TextEdit 随后用
    /// 纯文本再次复制并覆盖剪贴板——这是让单块复制也带格式的关键。
    pub(super) fn copy_single_sel_rich(&mut self, ctx: &egui::Context) -> bool {
        let Some((idx, _, _, text)) = editor::selection_text(&self.doc, &self.editor) else {
            return false;
        };
        let html = self
            .doc
            .blocks
            .get(idx)
            .map(|b| crate::export::selected_text_html(b, &text))
            .unwrap_or_default();
        crate::clipboard::copy_rich(ctx, &text, &html);
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Copy)));
        self.notify(ctx, "已复制".to_string());
        true
    }

    /// 剪切活动块内的单块文本选区：带格式复制后删除选中文字。
    /// 处理成功后移除 `Event::Cut`，避免 TextEdit 再走原生纯文本剪切。
    pub(super) fn cut_single_sel_rich(&mut self, ctx: &egui::Context) -> bool {
        let Some((idx, _, _, text)) = editor::selection_text(&self.doc, &self.editor) else {
            return false;
        };
        let html = self
            .doc
            .blocks
            .get(idx)
            .map(|b| crate::export::selected_text_html(b, &text))
            .unwrap_or_default();
        let Some(cut_text) = editor::cut_selection(&mut self.doc, &mut self.editor) else {
            return false;
        };
        crate::clipboard::copy_rich(ctx, &cut_text, &html);
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Cut)));
        self.notify(ctx, "已剪切".to_string());
        self.on_changed();
        ctx.request_repaint();
        let _ = text;
        true
    }

    /// 为当前选区（跨块优先，其次单块）生成带格式的 HTML；供右键「复制/剪切」复用。
    pub(super) fn selection_html(&self) -> Option<String> {
        if let Some(sel) = self.editor.cross_sel {
            return Some(crate::export::cross_sel_html(&self.doc.blocks, sel));
        }
        if self.editor.table_sel.is_some() {
            return editor::table_sel_html(&self.doc, &self.editor);
        }
        let (idx, _, _, text) = editor::selection_text(&self.doc, &self.editor)?;
        self.doc
            .blocks
            .get(idx)
            .map(|b| crate::export::selected_text_html(b, &text))
    }

    /// 更新状态栏文字并弹出轻提示（底部居中浮层，自动淡出），同时播放一声轻音效。
    pub(super) fn notify(&mut self, ctx: &egui::Context, msg: String) {
        self.status = msg.clone();
        self.toast = Some((msg, ctx.input(|i| i.time)));
        Self::play_toast_sound();
    }

    /// 轻提示音效：不引音频库，直接调系统自带播放器放一声短促的提示音，
    /// 后台线程 wait 防僵尸进程；系统/音效文件不存在时静默跳过，不影响任何功能。
    pub(super) fn play_toast_sound() {
        #[cfg(target_os = "macos")]
        let mut cmd = {
            let mut c = std::process::Command::new("afplay");
            // Tink 是系统音效里最轻脆短促的一声，-v 0.4 压低音量做轻提示
            c.args(["-v", "0.4", "/System/Library/Sounds/Tink.aiff"]);
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut c = std::process::Command::new("powershell");
            c.args([
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "[System.Media.SystemSounds]::Asterisk.Play()",
            ]);
            c
        };
        #[cfg(target_os = "linux")]
        let mut cmd = {
            let mut c = std::process::Command::new("paplay");
            c.arg("/usr/share/sounds/freedesktop/stereo/complete.oga");
            c
        };
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        if let Ok(mut child) = cmd.spawn() {
            // 后台线程回收子进程，避免反复复制后留下僵尸进程
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }

    /// 轻提示渲染：底部居中浮层（白底浅绿描边 + 绿色对勾图标），停留 1.8s 后
    /// 用 0.4s 淡出。只读不改 UI 状态，不挡鼠标（interactable(false)）。
    pub(super) fn show_toast(&mut self, ctx: &egui::Context) {
        const HOLD: f32 = 1.8;
        const FADE: f32 = 0.4;
        let Some((msg, at)) = self.toast.clone() else {
            return;
        };
        let now = ctx.input(|i| i.time);
        let elapsed = (now - at) as f32;
        if elapsed > HOLD + FADE {
            self.toast = None;
            return;
        }
        // 淡出期间持续重绘，否则动画会卡住
        ctx.request_repaint();
        let alpha = ((HOLD + FADE - elapsed) / FADE).clamp(0.0, 1.0);
        let mix = |c: egui::Color32| -> egui::Color32 {
            let a = (c.a() as f32 * alpha) as u8;
            egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
        };
        // 配色固定为参考样式：白底 / 浅绿描边 / 绿色对勾 / 深色文字，不随主题变化
        const BG: egui::Color32 = egui::Color32::WHITE;
        const BORDER: egui::Color32 = egui::Color32::from_rgb(0x9B, 0xD8, 0x76);
        const ICON: egui::Color32 = egui::Color32::from_rgb(0x52, 0xC4, 0x1A);
        const TEXT: egui::Color32 = egui::Color32::from_rgb(0x33, 0x33, 0x33);
        // 不用 anchor：Area 会记住上次位置，anchor 在状态残留时可能被顶到窗口中部；
        // 直接每帧按屏幕矩形算出定位点，用 fixed_pos + pivot 强制定位。
        // 气泡下缘落在窗口高度的 40% 处（中上部）：醒目且不与顶部工具条打架。
        let screen = ctx.viewport_rect();
        let pos = egui::pos2(screen.center().x, screen.top() + screen.height() * 0.40);
        #[allow(deprecated)]
        egui::Area::new(egui::Id::new("toast"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(mix(BG))
                    .stroke(egui::Stroke::new(1.0, mix(BORDER)))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .shadow(egui::Shadow {
                        offset: [0, 2],
                        blur: 10,
                        spread: 0,
                        color: mix(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 45)),
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // 自绘绿色圆 + 白色对勾图标，不依赖 emoji 字体
                            let (ir, _) = ui
                                .allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                            let p = ui.painter();
                            let c = ir.center();
                            p.circle_filled(c, 8.0, mix(ICON));
                            p.add(egui::Shape::line(
                                vec![
                                    egui::pos2(c.x - 3.5, c.y + 0.5),
                                    egui::pos2(c.x - 1.0, c.y + 3.0),
                                    egui::pos2(c.x + 3.5, c.y - 2.5),
                                ],
                                egui::Stroke::new(1.8, mix(egui::Color32::WHITE)),
                            ));
                            ui.label(egui::RichText::new(&msg).size(13.0).color(mix(TEXT)));
                        });
                    });
            });
    }
}
