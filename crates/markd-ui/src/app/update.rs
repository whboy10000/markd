//! `eframe::App` 实现：每帧布局与窗口/终端/侧栏面板。

use super::*;

impl eframe::App for MarkdApp {
    /// 清屏色跟随主题：eframe 默认是近黑的 (12,12,12)，任何透传的区域都会显得发黑。
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme.bg.to_normalized_gamma_f32()
    }

    fn ui(&mut self, root_ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = root_ui.ctx().clone();
        // 关闭窗口（红绿灯、Cmd+Q、菜单退出）前先拦一道：有未保存的修改就弹确认框，
        // 而不是像以前那样直接写盘或悄悄丢掉。「不保存」会把 force_quit 置位，下一帧放行。
        if needs_exit_confirm(
            self.force_quit,
            ctx.input(|i| i.viewport().close_requested()),
            self.unsaved_tabs().len(),
        ) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.exit_dialog = true;
        }
        self.terminal.poll();
        // 消费 macOS 原生菜单栏产生的命令。
        let pending: Vec<Command> = match self.menu_queue.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => Vec::new(),
        };
        for c in pending {
            self.execute(c, &ctx);
        }
        // 「跟随系统」：系统深浅档一变就重算主题（配色表只认浅色/深色两档）。
        let dark = matches!(ctx.system_theme(), Some(egui::Theme::Dark));
        if dark != self.system_dark {
            self.system_dark = dark;
            if self.config.theme_follow_system {
                self.rebuild_theme();
            }
        }
        if self.theme_dirty {
            self.theme.apply(&ctx);
            self.theme_dirty = false;
        }
        // 后台算完的链接图回填（启动时不再同步扫盘）。
        if let Some(rx) = self.link_rx.take() {
            match rx.try_recv() {
                Ok(m) => self.link_docs = m,
                Err(std::sync::mpsc::TryRecvError::Empty) => self.link_rx = Some(rx),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
            }
        }
        // 图床上传完成：把本地图片链接替换成公网 URL。
        self.poll_image_uploads(&ctx);
        // 设置里的编辑器选项每帧同步到编辑器状态
        self.editor.tab_size = self.config.tab_size.max(1);
        self.editor.auto_pair = self.config.auto_pair;
        self.handle_shortcuts(&ctx);
        self.handle_dropped_files(&ctx);

        let ctx2 = ctx.clone();
        // 菜单栏固定在窗口最顶部（VS Code 风格的单行窄条），下方另起一条格式工具条。
        let menu_bg = self.theme.panel;
        let menubar = egui::Panel::top("menubar")
            .frame(egui::Frame::NONE.fill(menu_bg).inner_margin(4.0))
            .show(&mut *root_ui, |ui| self.menubar(ui, &ctx2));
        // 自定义标题栏：让系统红绿灯垂直居中于菜单栏这一行（非 macOS 为空操作）。
        let r = menubar.response.rect;
        frame.set_traffic_lights_position(&ctx, egui::Rangef::new(r.min.y, r.max.y), 12.0);
        if self.config.show_toolbar {
            egui::Panel::top("toolbar").show(&mut *root_ui, |ui| self.toolbar(ui, &ctx2));
        }
        // bottom 必须先于 left 声明：bottom 只收窄父 Ui 的 y 轴，此时 x 仍是整窗宽度，
        // 状态栏才能横跨整个窗口；若放在 left 之后，x 起点会被推到侧栏右缘而只剩中央区域。
        egui::Panel::bottom("statusbar").show(&mut *root_ui, |ui| self.statusbar(ui, &ctx2));
        // 终端面板：位于状态栏上方、横跨整窗；高度同样由自己维护并可拖动上边缘调整。
        if self.terminal.open {
            let h = self.terminal.height.clamp(120.0, 600.0);
            let panel = egui::Panel::bottom("terminal_panel")
                .resizable(false)
                .show_separator_line(true)
                .exact_size(h)
                .show(&mut *root_ui, |ui| self.terminal.ui(ui, &ctx2, &self.theme));
            let r = panel.response.rect;
            let handle = egui::Rect::from_min_max(
                egui::pos2(r.min.x, r.min.y - 6.0),
                egui::pos2(r.max.x, r.min.y + 6.0),
            );
            #[allow(deprecated)]
            let resize_id = egui::Id::new("terminal_resize");
            let drag = root_ui.interact(handle, resize_id, egui::Sense::drag());
            if drag.hovered() || drag.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if drag.dragged() {
                self.terminal.height = (h - drag.drag_delta().y).clamp(120.0, 600.0);
            }
        }
        if self.config.show_sidebar {
            // 宽度完全由 `config.sidebar_width` 控制。0.37 的 `PanelState` 不会持久化尺寸，
            // 每帧传 `default_size` 会把拖动结果弹回去，所以这里关掉内置 resizable，
            // 改用 `exact_size` + 在右缘自绘一条可拖动的分隔条。
            let width = self.config.sidebar_width.clamp(160.0, 900.0);
            let panel = egui::Panel::left("sidebar")
                .resizable(false)
                .show_separator_line(true)
                // 右边距为 0：滚动条紧贴侧栏分隔线，视觉上和那条线重合
                // 底色显式用 theme.bg：Frame::NONE 是透明的，会露出窗口清屏色（近黑），
                // 暗色主题下侧栏/大纲就会比编辑区黑一截。
                .frame(
                    egui::Frame::NONE
                        .fill(self.theme.bg)
                        .inner_margin(egui::Margin {
                            left: 8,
                            right: 0,
                            top: 6,
                            bottom: 4,
                        }),
                )
                .exact_size(width)
                .show(&mut *root_ui, |ui| self.sidebar(ui));

            let r = panel.response.rect;
            // 放在面板外侧，避免和侧栏滚动条抢同一块区域
            let handle = egui::Rect::from_min_max(
                egui::pos2(r.max.x, r.min.y),
                egui::pos2(r.max.x + 8.0, r.max.y),
            );
            #[allow(deprecated)]
            let resize_id = egui::Id::new("sidebar_resize");
            let drag = root_ui.interact(handle, resize_id, egui::Sense::drag());
            if drag.hovered() || drag.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if drag.dragged() {
                self.config.sidebar_width = (width + drag.drag_delta().x).clamp(160.0, 900.0);
            }
            if drag.drag_stopped() {
                self.config.save();
            }
        }

        // 标签栏：紧贴编辑区上方，可在偏好设置里关闭以节省纵向空间。
        // 声明在 left 之后，横向只占侧栏右侧的区域。
        if self.config.show_tabbar {
            egui::Panel::top("tabbar")
                .frame(
                    egui::Frame::NONE
                        .fill(self.theme.panel)
                        .inner_margin(egui::Margin {
                            left: 0,
                            right: 6,
                            top: 3,
                            bottom: 3,
                        }),
                )
                .show(&mut *root_ui, |ui| self.tabbar(ui));
        }

        // 终端持有焦点时编辑区进入只读，避免它每帧把焦点抢回去。
        self.editor.read_only = self.terminal.has_focus();
        let show_home = self.home;
        // CentralPanel 默认填充 panel_fill（浅色 #F3F3F3），编辑区应是正文色：
        // 复现 egui 0.37 Frame::central_panel（inner_margin 8），仅把底色换成 theme.bg（浅色 #FFFFFF）。
        let central_frame = egui::Frame::new().inner_margin(8).fill(self.theme.bg);
        let (event, actions, home_act) = egui::CentralPanel::default()
            .frame(central_frame)
            .show(&mut *root_ui, |ui| {
                if show_home {
                    let actions: Vec<Action> = Vec::new();
                    let act = self.home_view(ui, &ctx);
                    return (EditorEvent::None, actions, act);
                }
                let mut actions = Vec::new();
                let base: Option<PathBuf> = self
                    .doc
                    .path
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf());
                let tab_key = self.tabs.get(self.active).map(|t| t.uid).unwrap_or(0);
                let ev = editor::show_editor(
                    ui,
                    &mut self.doc,
                    &mut self.editor,
                    &self.theme,
                    &ctx,
                    &mut self.images,
                    base.as_deref(),
                    &self.outline,
                    self.config.source_mode,
                    self.config.focus_mode,
                    self.config.typewriter,
                    self.config.show_line_numbers,
                    tab_key,
                    &mut actions,
                );
                (ev, actions, None)
            })
            .inner;

        if let Some(a) = home_act {
            match a {
                HomeAction::New => self.new_document(),
                HomeAction::OpenFile => self.execute(Command::OpenFileDialog, &ctx),
                HomeAction::OpenWorkspace => self.execute(Command::OpenWorkspace, &ctx),
                HomeAction::Recent(p) => self.open_file(p),
            }
        }

        match event {
            EditorEvent::Changed => self.on_changed(),
            EditorEvent::None => {}
        }
        // 用户点回了编辑区：把键盘焦点从终端交还，下一帧编辑区恢复可编辑。
        if self.editor.clicked && self.terminal.has_focus() {
            self.terminal.blur(&ctx);
        }
        self.editor.clicked = false;
        for a in actions {
            match a {
                Action::ToggleTask(i) => {
                    editor::toggle_task(&mut self.doc, i);
                    self.on_changed();
                }
                Action::OpenLink(url) => self.open_link(&url),
                Action::Jump(i) => {
                    self.editor.active = i.min(self.doc.blocks.len().saturating_sub(1));
                    self.editor.scroll_req = Some(self.editor.active);
                    self.editor.scroll_top = true;
                }
                Action::Notify(msg) => self.notify(&ctx, msg),
                Action::Caret(i, off) => {
                    self.editor.active = i;
                    self.editor.focus_req = Some(i);
                    self.editor.pending_caret = Some(off);
                }
                Action::TableCellClick(bi, ri, ci, end) => {
                    // 渲染态点了表格某个单元格：进入编辑态时直接聚焦那一格，
                    // 光标按点击位置落到文字开头或末尾。
                    self.editor.table_focus = Some((ri, ci));
                    self.editor.table_caret_end = end;
                    // 记录最后命中的单元格：表格工具栏的行/列操作以此为基准。
                    ctx.memory_mut(|m| {
                        m.data
                            .insert_temp(egui::Id::new(("table_cell_ref", bi)), (ri, ci));
                    });
                }
                Action::TableOp(i, op) => {
                    editor::apply_table_op(&mut self.doc, i, op);
                    self.on_changed();
                }
                Action::AskDeleteBlock(i) => {
                    // 请求删除代码块：弹确认窗，确定才真正删除（取消不删）。
                    self.code_delete_confirm = Some(i);
                }
                Action::AskDeleteTable(i) => {
                    // 请求删除表格：同样先弹确认窗，避免误点丢数据。
                    self.table_delete_confirm = Some(i);
                }
                // 跨块拖选已在 show_editor 帧末聚合进 cross_sel，不会到达这里
                Action::CrossDrag(..) => {}
                // 表格单元格拖选同样在帧末聚合成 table_sel，不会到达这里
                Action::TableDrag(..) => {}
                // 代码组切换标签：只改视图状态，不动文档。
                Action::CodeGroupTab(i, t) => {
                    self.editor.code_group_tab.insert(i, t);
                    ctx.request_repaint();
                }
                // 代码组换当前标签的语言：改写这一组的围栏 info 并重建源码。
                Action::CodeGroupLang(i, lang) => {
                    let tab = self.editor.group_tab(i);
                    if let Some(crate::ast::Block::CodeGroup { items, .. }) =
                        self.doc.blocks.get(i).cloned()
                    {
                        let mut items = items;
                        let mut changed = false;
                        if let Some(it) = items.get_mut(tab)
                            && it.lang != lang
                        {
                            it.lang = lang;
                            changed = true;
                        }
                        if changed {
                            self.doc.record();
                            let src = crate::ast::code_group_source(&items);
                            self.doc.edit_block(i, &src);
                            self.on_changed();
                        }
                    }
                }
                // 代码组新增一个语言标签：写完源码后直接进入编辑，光标落在空代码里。
                Action::CodeGroupAddTab(i) => {
                    if let Some(crate::ast::Block::CodeGroup { items, .. }) =
                        self.doc.blocks.get(i).cloned()
                    {
                        let mut items = items;
                        items.push(crate::ast::CodeGroupItem {
                            lang: String::new(),
                            code: String::new(),
                        });
                        let next = items.len() - 1;
                        self.doc.record();
                        self.doc
                            .edit_block(i, &crate::ast::code_group_source(&items));
                        self.editor.code_group_tab.insert(i, next);
                        self.editor.active = i;
                        self.editor.focus_req = Some(i);
                        self.editor.caret_req = Some((i, 0));
                        self.editor.cross_sel = None;
                        self.editor.sel = None;
                        self.on_changed();
                    }
                }
                // 代码组删除当前语言标签：至少保留一个。
                Action::CodeGroupRemoveTab(i) => {
                    if let Some(crate::ast::Block::CodeGroup { items, .. }) =
                        self.doc.blocks.get(i).cloned()
                        && items.len() > 1
                    {
                        let tab = self.editor.group_tab(i).min(items.len() - 1);
                        let mut items = items;
                        items.remove(tab);
                        let next = tab.min(items.len() - 1);
                        self.doc.record();
                        self.doc
                            .edit_block(i, &crate::ast::code_group_source(&items));
                        self.editor.code_group_tab.insert(i, next);
                        self.editor.caret_req = Some((i, 0));
                        self.on_changed();
                    }
                }
                // 双击选词 / 三击选整段 / Shift+点击扩选：同样在 show_editor 聚合
                Action::WordSel(..) | Action::BlockSel(..) | Action::ShiftClick(..) => {}
                Action::FoldToggle(i) => {
                    // 折叠/展开代码块：只改视图状态，不动文档。
                    if !self.editor.folded.insert(i) {
                        self.editor.folded.remove(&i);
                    }
                    // 有路径的文档立即把折叠状态写盘，关闭/重开不丢。
                    if let Some(p) = &self.doc.path {
                        editor::save_folds(p, &self.doc, &self.editor.folded);
                    }
                }
                Action::SetLang(i, lang) => {
                    let rebuilt = match self.doc.blocks.get(i) {
                        Some(crate::ast::Block::Code {
                            code,
                            source,
                            quote,
                            ..
                        }) => {
                            let fence = if source.contains("~~~") { "~~~" } else { "```" };
                            if *quote == 0 {
                                Some(format!("{}{}\n{}\n{}", fence, lang, code, fence))
                            } else {
                                // 引用内的代码块：换语言时保留每行的 `>` 前缀。
                                let p = ">".repeat(*quote);
                                let body: Vec<String> = code
                                    .split('\n')
                                    .map(|l| {
                                        if l.is_empty() {
                                            p.clone()
                                        } else {
                                            format!("{p} {l}")
                                        }
                                    })
                                    .collect();
                                Some(format!(
                                    "{p} {fence}{lang}\n{}\n{p} {fence}",
                                    body.join("\n")
                                ))
                            }
                        }
                        // 图表块：选 mermaid 保持不变，选其它语言即转为普通代码块
                        Some(crate::ast::Block::Diagram { code, .. }) => {
                            if lang == "mermaid" {
                                None
                            } else {
                                Some(format!("```{}\n{}\n```", lang, code))
                            }
                        }
                        _ => None,
                    };
                    if let Some(src) = rebuilt {
                        self.doc.record();
                        self.doc.edit_block(i, &src);
                        self.on_changed();
                        self.status = format!(
                            "代码语言：{}",
                            if lang.is_empty() {
                                "纯文本"
                            } else {
                                lang.as_str()
                            }
                        );
                    }
                }
                Action::ResizeImage(bi, no, width) => {
                    // 拖动图片右下角缩放：把新宽度写成 `{width=N}` 属性。
                    // 拖动过程中每帧都会来，所以不记撤销栈（否则一次拖动几十步）。
                    if let Some(src) = resize_image_source(&self.doc, bi, no, width) {
                        self.doc.edit_block(bi, &src);
                        self.on_changed();
                    }
                }
            }
        }

        // 编辑区空白处的右键菜单：位置由编辑器在检测到空白右键时记录，
        // 这里统一绘制并分发菜单动作。
        self.editor_context_menu(&ctx);

        if std::env::var("MARKD_TRACE_FOCUS").is_ok() {
            eprintln!(
                "[state] home={} tabs={} active={} blocks={} focused={:?}",
                self.home,
                self.tabs.len(),
                self.active,
                self.doc.blocks.len(),
                ctx.memory(|m| m.focused())
            );
        }
        // 代码块删除确认弹窗：确定才删除，取消（或点 × 关闭）不删。
        if let Some(idx) = self.code_delete_confirm {
            let mut open = true;
            egui::Window::new("删除代码块")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(&ctx, |ui| {
                    ui.label("确定要删除这个代码块吗？删除后可以用撤销找回。");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("确定删除").clicked() {
                            self.doc.record();
                            // 只剩这一块时删成空段落，保证文档至少有一个块
                            if self.doc.blocks.len() <= 1 {
                                self.doc.edit_block(idx, "");
                            } else {
                                self.doc.remove_block(idx);
                            }
                            let n = self.doc.blocks.len();
                            self.editor.active = self.editor.active.min(n - 1);
                            self.on_changed();
                            self.code_delete_confirm = None;
                        }
                        if ui.button("取消").clicked() {
                            self.code_delete_confirm = None;
                        }
                    });
                });
            if !open {
                self.code_delete_confirm = None;
            }
        }
        // 表格删除确认弹窗：确定才删除，取消（或点 × 关闭）不删。
        if let Some(idx) = self.table_delete_confirm {
            let mut open = true;
            egui::Window::new("删除表格")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(&ctx, |ui| {
                    ui.label("确定要删除整张表格吗？删除后可以用撤销找回。");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("确定删除").clicked() {
                            // 保留原有「整表变成空段落」的删除语义，只是多了确认步骤。
                            editor::apply_table_op(&mut self.doc, idx, editor::TableOp::Delete);
                            self.editor.table_draft = None;
                            self.editor.table_focus = None;
                            self.on_changed();
                            self.table_delete_confirm = None;
                        }
                        if ui.button("取消").clicked() {
                            self.table_delete_confirm = None;
                        }
                    });
                });
            if !open {
                self.table_delete_confirm = None;
            }
        }
        self.show_toast(&ctx);
        self.windows(&ctx);
        self.auto_save();
        // TEMP 调试：模拟「在首页点新建文档」——CentralPanel 之后再建，下一帧才渲染编辑器
        if std::env::var("MARKD_AUTO_NEW").as_deref() == Ok("2") {
            self.frame_count += 1;
            if self.frame_count == 2 {
                self.new_document();
            }
        }
    }
}
