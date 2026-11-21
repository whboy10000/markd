//! 各功能弹窗：查找、命令面板、设置、关于、文件操作确认等。

use super::*;

impl MarkdApp {
    pub(super) fn windows(&mut self, ctx: &egui::Context) {
        // 只有快速打开面板真的打开时才扫盘。之前每帧都算一次，
        // 工作区在桌面上时要遍历整个目录，CPU 直接被这一个调用吃满。
        let files = if self.palette.open {
            self.quick_open_files()
        } else {
            Vec::new()
        };
        if let Some(cmd) = self.palette.show(ctx, &files) {
            self.execute(cmd, ctx);
        }
        if let Some((path, line)) = self.search.show(ctx, self.root.as_deref()) {
            self.open_file(path);
            self.editor.active = line.min(self.doc.blocks.len().saturating_sub(1));
        }
        if let Some(name) = self.graph.show(ctx, &self.link_docs, &self.theme) {
            if let Some(p) = self.files().into_iter().find(|p| {
                p.file_stem()
                    .map(|s| s.to_string_lossy() == name)
                    .unwrap_or(false)
            }) {
                self.open_file(p);
            }
        }
        if let Some(content) = self.snapshots.show(ctx) {
            self.doc.record();
            self.doc.set_source(&content);
            self.on_changed();
            self.status = "已恢复到所选版本".to_string();
        }
        self.stats.show(ctx, &self.stats_cache, &self.theme);

        if let Some(kind) = self.menu {
            let title = match kind {
                MenuKind::File => "文件",
                MenuKind::Edit => "编辑",
                MenuKind::Paragraph => "段落",
                MenuKind::Format => "格式",
                MenuKind::View => "视图",
                MenuKind::Theme => "主题",
                MenuKind::Window => "窗口",
                MenuKind::Help => "帮助",
            };
            let guard = self.menu_guard;
            self.menu_guard = false;
            let mut chosen: Option<Command> = None;
            let mut dismiss = false;
            let mut open = true;
            let window_resp = egui::Window::new(title)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_pos(egui::pos2(16.0, 64.0))
                .show(ctx, |ui| {
                    if matches!(kind, MenuKind::File | MenuKind::Paragraph) {
                        // 文件 / 段落条目较多，超出高度时内部滚动。
                        egui::ScrollArea::vertical()
                            .max_height(480.0)
                            .show(ui, |ui| {
                                for c in menu_commands(kind) {
                                    if ui.button(c.label()).clicked() {
                                        chosen = Some(c);
                                    }
                                }
                            });
                    } else {
                        for c in menu_commands(kind) {
                            if ui.button(c.label()).clicked() {
                                chosen = Some(c);
                            }
                        }
                    }
                    if kind == MenuKind::Theme {
                        ui.separator();
                        for t in ThemeKind::ALL {
                            if ui.button(t.name()).clicked() {
                                self.config.set_theme(t);
                                self.rebuild_theme();
                                self.config.save();
                                dismiss = true;
                            }
                        }
                    }
                });
            if !open {
                dismiss = true;
            }
            // 打开或切换菜单的那一帧跳过，避免标题按钮的点击被当成「点击外部」。
            if !guard {
                if let Some(inner) = window_resp {
                    if inner.response.clicked_elsewhere() {
                        dismiss = true;
                    }
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                dismiss = true;
            }
            if let Some(c) = chosen {
                self.execute(c, ctx);
                dismiss = true;
            }
            if dismiss {
                self.menu = None;
            }
        }

        if self.about {
            egui::Window::new("关于 Markd")
                .open(&mut self.about)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(RichText::new("Markd").heading());
                    ui.label("纯 Rust + egui 从零构建的 Markdown 编辑器");
                    ui.label("自研 AST 解析、块级实时渲染、代码高亮与流程图绘制");
                    ui.separator();
                    ui.label("特色：版本快照、命令面板、全文搜索、双向链接、写作统计");
                    ui.label(RichText::new("本作品为从零实现的独立 Markdown 编辑器").small());
                });
        }

        let (settings_changed, settings_reset) =
            self.settings.show(ctx, &self.theme, &mut self.config);
        if settings_reset {
            self.config = Config::default();
            self.apply_config(ctx);
            self.config.save();
        } else if settings_changed {
            self.apply_config(ctx);
            self.config.save();
        }

        // ---------------- 文件树右键菜单 ----------------
        if let Some((target, pos)) = self.ctx_menu.clone() {
            #[derive(PartialEq)]
            enum FileMenuAction {
                Open,
                CopyPath,
                CopyRelative,
                CopyFile,
                CutFile,
                Paste,
                Rename,
                Delete,
                Reveal,
                NewFile,
                NewDir,
            }
            let mut chosen: Option<FileMenuAction> = None;
            let is_dir = target.is_dir();
            let reveal_label = if cfg!(target_os = "macos") {
                "在 Finder 中显示"
            } else {
                "在文件管理器中显示"
            };
            let area = egui::Area::new(egui::Id::new("file_ctx_menu"))
                .fixed_pos(pos)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(self.theme.panel)
                        .stroke(egui::Stroke::new(1.0, self.theme.border))
                        .corner_radius(6.0)
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.set_min_width(170.0);
                            ui.style_mut().interaction.selectable_labels = false;
                            ui.style_mut().spacing.item_spacing.y = 2.0;
                            if !is_dir
                                && ui
                                    .add(
                                        egui::Button::new("打开")
                                            .frame(false)
                                            .min_size(egui::vec2(158.0, 20.0)),
                                    )
                                    .clicked()
                            {
                                chosen = Some(FileMenuAction::Open);
                            }
                            if ui
                                .add(
                                    egui::Button::new("复制路径")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::CopyPath);
                            }
                            if ui
                                .add(
                                    egui::Button::new("复制相对路径")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::CopyRelative);
                            }
                            ui.separator();
                            if ui
                                .add(
                                    egui::Button::new("复制")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::CopyFile);
                            }
                            if ui
                                .add(
                                    egui::Button::new("剪切")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::CutFile);
                            }
                            let paste_enabled = self.file_clipboard.is_some();
                            if ui
                                .add_enabled(
                                    paste_enabled,
                                    egui::Button::new("粘贴")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::Paste);
                            }
                            ui.separator();
                            if ui
                                .add(
                                    egui::Button::new("重命名")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::Rename);
                            }
                            if ui
                                .add(
                                    egui::Button::new("删除")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::Delete);
                            }
                            ui.separator();
                            if ui
                                .add(
                                    egui::Button::new("新建文件")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::NewFile);
                            }
                            if ui
                                .add(
                                    egui::Button::new("新建文件夹")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::NewDir);
                            }
                            ui.separator();
                            if ui
                                .add(
                                    egui::Button::new(reveal_label)
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::Reveal);
                            }
                        });
                });
            // 点在菜单外面：收起菜单。
            let clicked_outside = ctx.input(|i| i.pointer.primary_clicked())
                && !area
                    .response
                    .rect
                    .contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or(egui::Pos2::ZERO)));
            if chosen.is_some() || clicked_outside {
                self.ctx_menu = None;
            }
            if let Some(act) = chosen {
                match act {
                    FileMenuAction::Open => {
                        if target.is_file() {
                            self.open_file(target.clone());
                        }
                    }
                    FileMenuAction::CopyPath => self.copy_file_path(&target, false, ctx),
                    FileMenuAction::CopyRelative => self.copy_file_path(&target, true, ctx),
                    FileMenuAction::CopyFile => {
                        self.file_clipboard = Some((target.clone(), false));
                        self.status = "已复制文件".to_string();
                    }
                    FileMenuAction::CutFile => {
                        self.file_clipboard = Some((target.clone(), true));
                        self.status = "已剪切文件".to_string();
                    }
                    FileMenuAction::Paste => {
                        let dest = if is_dir {
                            target.clone()
                        } else {
                            target
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| target.clone())
                        };
                        self.fs_paste(&dest);
                    }
                    FileMenuAction::Rename => {
                        let name = target
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        self.rename_dialog = Some((target.clone(), name));
                    }
                    FileMenuAction::Delete => {
                        self.delete_confirm = Some(target.clone());
                    }
                    FileMenuAction::Reveal => self.reveal_in_system(&target),
                    FileMenuAction::NewFile => {
                        let dir = if is_dir {
                            target.clone()
                        } else {
                            target
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| target.clone())
                        };
                        self.new_dialog = Some((dir, false, String::new()));
                    }
                    FileMenuAction::NewDir => {
                        let dir = if is_dir {
                            target.clone()
                        } else {
                            target
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| target.clone())
                        };
                        self.new_dialog = Some((dir, true, String::new()));
                    }
                }
            }
        }

        // ---------------- 重命名 / 删除确认 / 新建 对话框 ----------------
        if let Some((target, _)) = self.rename_dialog.clone() {
            let mut open = true;
            let mut confirm = false;
            let mut cancel = false;
            let mut input = std::mem::take(&mut self.rename_dialog)
                .map(|(_, b)| b)
                .unwrap_or_default();
            egui::Window::new("重命名")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(format!("原路径：{}", target.display()));
                    ui.text_edit_singleline(&mut input);
                    ui.horizontal(|ui| {
                        if ui.button("确定").clicked() {
                            confirm = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel {
                open = false;
            }
            if confirm {
                self.fs_rename(&target, &input);
                self.rename_dialog = None;
            } else if open {
                self.rename_dialog = Some((target, input));
            } else {
                self.rename_dialog = None;
            }
        }

        if let Some(target) = self.delete_confirm.clone() {
            let mut open = true;
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new("删除")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(format!("确定要删除 {} 吗？", target.display()));
                    ui.label(
                        RichText::new("此操作不可撤销。")
                            .small()
                            .color(self.theme.dim),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("删除").clicked() {
                            confirm = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel {
                open = false;
            }
            if confirm {
                self.fs_delete(&target);
                self.delete_confirm = None;
            } else if !open {
                self.delete_confirm = None;
            }
        }

        if let Some((dir, is_dir, _)) = self.new_dialog.clone() {
            let mut open = true;
            let mut confirm = false;
            let mut cancel = false;
            let mut input = std::mem::take(&mut self.new_dialog)
                .map(|(_, _, b)| b)
                .unwrap_or_default();
            let win_title = if is_dir {
                "新建文件夹"
            } else {
                "新建文件"
            };
            egui::Window::new(win_title)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(format!("位置：{}", dir.display()));
                    ui.text_edit_singleline(&mut input);
                    ui.horizontal(|ui| {
                        if ui.button("确定").clicked() {
                            confirm = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel {
                open = false;
            }
            if confirm {
                self.fs_new(&dir, is_dir, &input);
                self.new_dialog = None;
            } else if open {
                self.new_dialog = Some((dir, is_dir, input));
            } else {
                self.new_dialog = None;
            }
        }

        if self.workspace_dialog {
            let mut input = std::mem::take(&mut self.workspace_input);
            let mut open = self.workspace_dialog;
            let mut cancel = false;
            let mut confirm = false;
            let mut pick = false;
            egui::Window::new("打开工作区")
                .open(&mut open)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("工作区文件夹路径：");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut input);
                        if ui
                            .button("选择…")
                            .on_hover_cursor(egui::CursorIcon::Default)
                            .clicked()
                        {
                            pick = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("打开").clicked() {
                            confirm = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            // 原生对话框在 UI 闭包结束后调用，避免阻塞 egui 的构建过程。
            if pick {
                if let Some(p) = rfd::FileDialog::new()
                    .set_title("选择工作区文件夹")
                    .pick_folder()
                {
                    input = p.to_string_lossy().to_string();
                }
            }
            if cancel {
                open = false;
            }
            self.workspace_input = input.clone();
            self.workspace_dialog = open;
            if confirm {
                self.set_workspace(PathBuf::from(input.trim()));
                self.workspace_dialog = false;
            }
        }

        // 文档内查找/替换：命中的块滚到视野里，并把命中的文字选中（像 Typora 那样）。
        let fr = self.find.show(ctx, &mut self.doc, &self.theme);
        if let Some(h) = fr.jump {
            self.editor.active = h.block;
            self.editor.scroll_req = Some(h.block);
            self.editor.scroll_top = false;
            if let Some((a, b)) = self.find.hit_chars(&self.doc, h) {
                self.editor.select_req = Some((h.block, a, b));
            }
        }
        if fr.changed {
            self.on_changed();
        }

        if self.exit_dialog {
            self.exit_confirm(ctx);
        }

        if self.link_dialog.open {
            let mut open = true;
            let mut insert = false;
            let mut cancel = false;
            egui::Window::new("插入链接")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("链接地址");
                    let url_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.link_dialog.url)
                            .hint_text("粘贴链接，如 https://example.com")
                            .desired_width(380.0),
                    );
                    if self.link_dialog.focus_url {
                        url_resp.request_focus();
                        self.link_dialog.focus_url = false;
                    }
                    if url_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        insert = true;
                    }
                    ui.add_space(6.0);
                    ui.label("显示文字（留空则直接显示链接地址）");
                    let text_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.link_dialog.text)
                            .hint_text("默认使用选中的文字")
                            .desired_width(380.0),
                    );
                    if text_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        insert = true;
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("插入").clicked() {
                            insert = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            if insert {
                self.confirm_link_dialog();
            }
            if cancel || !open {
                self.link_dialog.open = false;
            }
        }

        if self.table_dialog {
            let mut open = true;
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new("插入表格")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("行数");
                        ui.add(egui::DragValue::new(&mut self.table_rows).range(1..=100));
                        ui.label("列数");
                        ui.add(egui::DragValue::new(&mut self.table_cols).range(1..=20));
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("插入").clicked() {
                            confirm = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            if confirm {
                let src = table_template(self.table_rows, self.table_cols);
                editor::insert_template(&mut self.doc, &mut self.editor, &src);
                self.on_changed();
            }
            if confirm || cancel || !open {
                self.table_dialog = false;
            }
        }
    }
}
