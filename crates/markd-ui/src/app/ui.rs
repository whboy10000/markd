//! 顶层界面：菜单栏、工具条、标签栏、侧栏与状态栏。

use super::*;

impl MarkdApp {
    /// 固定在窗口最顶部的单行菜单栏（VS Code 风格）。
    /// macOS 下这一行同时充当标题栏：系统红绿灯浮在左端，快捷按钮紧跟其右侧。
    pub(super) fn menubar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.style_mut().interaction.selectable_labels = false;
        let mut chosen: Option<Command> = None;
        ui.horizontal(|ui| {
            // 给 macOS 系统红绿灯按钮留出空间；其它平台只用常规边距。
            ui.add_space(if cfg!(target_os = "macos") { 78.0 } else { 4.0 });
            // 设置入口已移到「Markd」菜单与 Cmd/Ctrl+,，标题栏左侧不再放齿轮按钮。
            // 「首页」入口已移到编辑区上方的标签栏，标题栏这里不再重复放。
            // macOS 的菜单已移到屏幕左上角的系统菜单栏，窗口内不再重复显示。
            if !cfg!(target_os = "macos") {
                for (label, kind) in [
                    ("文件", MenuKind::File),
                    ("编辑", MenuKind::Edit),
                    ("段落", MenuKind::Paragraph),
                    ("格式", MenuKind::Format),
                    ("视图", MenuKind::View),
                    ("主题", MenuKind::Theme),
                    ("窗口", MenuKind::Window),
                    ("帮助", MenuKind::Help),
                ] {
                    if ui
                        .add(egui::Button::new(label).frame(false))
                        .on_hover_cursor(egui::CursorIcon::Default)
                        .clicked()
                    {
                        let active = self.menu == Some(kind);
                        self.menu = if active { None } else { Some(kind) };
                        self.menu_guard = self.menu.is_some();
                    }
                }
                ui.separator();
            }
            // 左侧只放当前文档名（首页时显示「首页」）
            let title = if self.home {
                "首页".to_string()
            } else {
                self.doc.title()
            };
            ui.label(RichText::new(title).strong().color(self.theme.dim));

            // 右侧：终端图标
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                // 保存按钮已移除，保存仍可通过菜单 / Cmd+S 触发。
                // 终端开关：Codicon 线性图标。面板收起时显示 panel（底部停靠），
                // 展开时显示 panel-close（面板 + ×），16px、无背景无边框，
                // 悬停只把前景色调亮。
                let (icon_rect, icon_resp) =
                    ui.allocate_exact_size(egui::vec2(22.0, 18.0), egui::Sense::click());
                let icon_tint = if icon_resp.hovered() {
                    self.theme.text.lerp_to_gamma(egui::Color32::WHITE, 0.3)
                } else {
                    self.theme.text
                };
                let icon_key = if self.terminal.open {
                    "ui:panel-close"
                } else {
                    "ui:panel"
                };
                self.icons
                    .paint_ui(ui, icon_rect, icon_key, 16.0, icon_tint);
                if icon_resp.on_hover_text("终端 (Cmd/Ctrl+J)").clicked() {
                    self.terminal.toggle();
                }
                // 剩余空白 = 拖动区（双击 = 最大化）
                let r = ui.available_rect_before_wrap();
                let drag = ui.allocate_response(
                    egui::vec2(r.width().max(1.0), r.height().max(1.0)),
                    egui::Sense::click_and_drag(),
                );
                if drag.double_clicked() {
                    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else if drag.dragged() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            });
        });
        if let Some(c) = chosen {
            self.execute(c, ctx);
        }
    }

    /// 菜单栏下方的格式工具条。
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.style_mut().interaction.selectable_labels = false;
        let mut chosen: Option<Command> = None;
        let mut chosen_tpl: Option<&'static str> = None;
        // 按钮较多（13 个）：窗口变窄时换行，而不是把最右边的「搜索」挤到看不见。
        ui.horizontal_wrapped(|ui| {
            ui.add_space(6.0);
            // 「命令面板 / 快速打开」按钮已移除：命令面板的全部功能收进菜单栏「文件」，
            // 打开面板仍走 Cmd/Ctrl+P（命令面板）与 Cmd/Ctrl+Shift+O（快速打开）。
            // 撤销 / 重做放在「标题」左边：改错了能立刻回退一步。
            if icon_button(ui, "撤销 (Cmd/Ctrl+Z)", painter_undo_icon) {
                chosen = Some(Command::Undo);
            }
            if icon_button(ui, "重做 (Cmd/Ctrl+Shift+Z)", painter_redo_icon) {
                chosen = Some(Command::Redo);
            }
            // 标题级别收进一个下拉：先点选目标段落让它成为活动块，再选 H1–H6，
            // 整段会替换掉原有的 #/>/- 前缀变成对应标题（H1→H6 之间也能直接互转）。
            ui.menu_button("标题", |ui| {
                for (label, cmd) in [
                    // 「段落」把已选中的标题/引用/列表还原成普通段落。
                    ("段落 正文", Command::Paragraph),
                    ("H1 一级标题", Command::H1),
                    ("H2 二级标题", Command::H2),
                    ("H3 三级标题", Command::H3),
                    ("H4 四级标题", Command::H4),
                    ("H5 五级标题", Command::H5),
                    ("H6 六级标题", Command::H6),
                ] {
                    if ui.button(label).clicked() {
                        chosen = Some(cmd);
                        ui.close();
                    }
                }
            });
            if ui
                .add(egui::Button::new(RichText::new("B").strong()))
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("加粗：作用于选中文字；无选区时作用于整行")
                .clicked()
            {
                chosen = Some(Command::Bold);
            }
            if ui
                .add(egui::Button::new(RichText::new("U").underline()))
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("下划线")
                .clicked()
            {
                chosen = Some(Command::Underline);
            }
            if ui
                .add(egui::Button::new(RichText::new("I").italics()))
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("斜体 (Cmd/Ctrl+I)")
                .clicked()
            {
                chosen = Some(Command::Italic);
            }
            // 删除线：图标直接画「ABC 中间一条横线」，比写「删除线」三个字更直观。
            if ui
                .add(egui::Button::new(RichText::new("ABC").strikethrough()))
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("删除线 (Ctrl+Shift+`)")
                .clicked()
            {
                chosen = Some(Command::Strike);
            }
            // 三种列表收进一个下拉，节省工具条空间。
            ui.menu_button("列表", |ui| {
                for (label, cmd) in [
                    ("无序列表", Command::Bullet),
                    ("有序列表", Command::Ordered),
                    ("任务列表", Command::Task),
                ] {
                    if ui.button(label).clicked() {
                        chosen = Some(cmd);
                        ui.close();
                    }
                }
            });
            // 引用从「插入」下拉里提出来，高频使用，放在「插入」左侧。
            // 引用是开关（再点一次取消），双击会让「加引用 → 取消」互相抵消，
            // 看起来像第一次点击没生效；双击的第二击直接忽略。
            let quote = ui
                .button("引用")
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("引用块 (Cmd/Ctrl+Option+Q)");
            if quote.clicked() && !quote.double_clicked() {
                chosen = Some(Command::Quote);
            }
            // 图片从「插入」下拉里提出来：高频使用，放在「插入」左侧。
            // 点它从磁盘选图；Cmd/Ctrl+V 则直接插入剪贴板里的图片。
            if ui
                .button("图片")
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("插入图片（也可 Cmd/Ctrl+V 粘贴剪贴板图片）")
                .clicked()
            {
                chosen = Some(Command::Image);
            }
            // 超链接从「插入」下拉里提出来：高频使用，放在「插入」左侧。
            if ui
                .button("超链接")
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("插入超链接 (Cmd/Ctrl+K)")
                .clicked()
            {
                chosen = Some(Command::Link);
            }
            // 行内公式 / 公式块也从「插入」下拉里提出来，放在「插入」左侧。
            if ui
                .button("行内公式")
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("插入行内公式 ($...$)")
                .clicked()
            {
                chosen = Some(Command::InlineMath);
            }
            if ui
                .button("公式块")
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("插入独立公式块 (Cmd/Ctrl+Option+B)")
                .clicked()
            {
                chosen = Some(Command::Math);
            }
            if ui
                .button("代码块公式")
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("插入公式代码块 (```math ... ```)")
                .clicked()
            {
                chosen = Some(Command::MathCode);
            }
            // 插入类动作收进一个下拉：表格、代码、分隔线、图表…
            // 全塞在工具条上会挤成一排小按钮，下拉更好找。
            ui.menu_button("插入", |ui| {
                for (label, cmd) in [
                    ("表格", Command::Table),
                    ("代码块", Command::CodeBlock),
                    ("代码组", Command::CodeGroup),
                    ("行内代码", Command::Code),
                    ("分隔线", Command::Rule),
                    ("图表", Command::Mermaid),
                    ("流程图", Command::Flowchart),
                    ("脚注", Command::Footnote),
                    ("目录 [TOC]", Command::Toc),
                    ("上传本地图片", Command::UploadImages),
                ] {
                    if ui.button(label).clicked() {
                        chosen = Some(cmd);
                        ui.close();
                    }
                }
            });
            // Mermaid 下拉：插入各类图表的标准模板，与「插入」按钮并排。
            // 图类型较多（20+），放进可滚动列表，避免菜单超出窗口被截断。
            ui.menu_button("Mermaid", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(460.0)
                    .show(ui, |ui| {
                        for (label, template) in MERMAID_TEMPLATES {
                            if ui.button(*label).clicked() {
                                chosen_tpl = Some(*template);
                                ui.close();
                            }
                        }
                    });
            });
            ui.separator();
            for (label, cmd) in [
                ("快照", Command::Snapshot),
                ("关系图", Command::Graph),
                ("统计", Command::Stats),
            ] {
                if ui
                    .button(label)
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .clicked()
                {
                    chosen = Some(cmd.clone());
                }
            }
            // 放大镜 = 文档内查找/替换（和 Cmd/Ctrl+F、Ctrl+H 同一个面板）。
            // 工作区全文搜索仍在「视图」菜单与命令面板里（Command::Search）。
            if icon_button(ui, "查找/替换 (Cmd/Ctrl+F、Ctrl+H)", painter_search_icon) {
                self.find.open_for(false);
            }
        });
        if let Some(c) = chosen {
            self.execute(c, ctx);
        }
        if let Some(tpl) = chosen_tpl {
            editor::insert_template(&mut self.doc, &mut self.editor, tpl);
        }
    }

    /// 编辑区顶部的标签栏：首页页签 + 每个打开的文件一个页签。
    /// 点页签切换，页签末尾的 × 关闭；它声明在 left 面板之后，所以只占编辑区宽度。
    pub(super) fn tabbar(&mut self, ui: &mut egui::Ui) {
        use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke};
        // 页签之间的分隔线：1px 低对比灰，上下内缩 5px，不顶到容器边缘。
        fn divider(ui: &mut egui::Ui) {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 14.0), egui::Sense::hover());
            let x = rect.center().x;
            let y = rect.center().y;
            ui.painter().line_segment(
                [Pos2::new(x, y - 7.0), Pos2::new(x, y + 7.0)],
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(128, 128, 128, 89)),
            );
        }
        ui.style_mut().interaction.selectable_labels = false;
        ui.style_mut().spacing.item_spacing.x = 4.0;
        let mut switch: Option<usize> = None;
        let mut close: Option<usize> = None;
        ui.horizontal(|ui| {
            // 左侧留 10pt：侧栏右缘的拖动条占着靠里 8pt，避免抢它的点击。
            ui.add_space(10.0);
            let home_txt = if self.home {
                RichText::new("首页").strong().color(self.theme.accent)
            } else {
                RichText::new("首页").color(self.theme.dim)
            };
            if ui
                .selectable_label(self.home, home_txt)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("欢迎页与最近打开")
                .clicked()
            {
                self.home = true;
            }
            // 首页之后直接排文件页签，每个页签前加一条细竖线。
            for i in 0..self.tabs.len() {
                divider(ui);
                let active = !self.home && i == self.active;
                let mut text = self.tab_label(i);
                if self.tab_dirty(i) {
                    text.push_str(" •");
                }
                let color = if active {
                    self.theme.text
                } else {
                    self.theme.dim
                };
                let tip = self.tabs[i]
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "未保存的文档".to_string());
                // 整个页签是一块可点击的「胶囊」：文字与关闭按钮共用同一个
                // 背景和 hover 反馈，不再像并排的两个独立控件。
                let font = FontId::proportional(12.5);
                let galley = ui.fonts_mut(|f| f.layout(text.clone(), font, color, f32::INFINITY));
                let pad_l = 10.0;
                let pad_r = 5.0;
                let close_w = 18.0;
                let gap = 4.0;
                let tab_w = pad_l + galley.size().x + gap + close_w + pad_r;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(tab_w, 24.0), egui::Sense::click());
                let painter = ui.painter().clone();
                // 页签 hover 用指针位置判断：关闭按钮叠在页签内部，
                // 两者不能互相抢 hover，否则 × 会一边淡入一边闪。
                let pointer = ui.input(|i| i.pointer.hover_pos());
                let hovered = pointer.is_some_and(|p| rect.contains(p));
                if active {
                    painter.rect_filled(rect, 5.0, self.theme.bg);
                    painter.rect_stroke(
                        rect,
                        5.0,
                        Stroke::new(1.0, self.theme.border),
                        egui::StrokeKind::Inside,
                    );
                    // 底部一条主题色短线，强化「当前页签」。
                    painter.line_segment(
                        [
                            Pos2::new(rect.left() + 6.0, rect.bottom() - 1.0),
                            Pos2::new(rect.right() - 6.0, rect.bottom() - 1.0),
                        ],
                        Stroke::new(2.0, self.theme.accent),
                    );
                } else if hovered {
                    painter.rect_filled(rect, 5.0, self.theme.border.gamma_multiply(0.35));
                }
                painter.galley(
                    Pos2::new(rect.left() + pad_l, rect.center().y - galley.size().y / 2.0),
                    galley,
                    color,
                );
                // 关闭按钮默认完全隐藏，鼠标悬停页签时淡入；位置固定在文字右侧
                // 4pt 处、与文字垂直居中，宽度始终占位，避免 hover 时页签跳动。
                let close_alpha = ui.ctx().animate_bool_with_time(
                    ui.scope_id().with(("tab_close_fade", i)),
                    hovered,
                    0.15,
                );
                let fade = |c: Color32, a: f32| {
                    Color32::from_rgba_unmultiplied(
                        c.r(),
                        c.g(),
                        c.b(),
                        (a.clamp(0.0, 1.0) * 255.0).round() as u8,
                    )
                };
                let close_rect = Rect::from_center_size(
                    Pos2::new(rect.right() - pad_r - close_w / 2.0, rect.center().y),
                    egui::vec2(close_w, 18.0),
                );
                let close_shown = close_alpha > 0.01;
                if close_shown {
                    let close_resp = ui.interact(
                        close_rect,
                        ui.scope_id().with(("tab_close", i)),
                        egui::Sense::click(),
                    );
                    let close_hovered = close_resp.hovered();
                    if close_hovered {
                        painter.circle_filled(
                            close_rect.center(),
                            8.0,
                            fade(self.theme.dim, 0.18 * close_alpha),
                        );
                    }
                    painter.text(
                        close_rect.center(),
                        Align2::CENTER_CENTER,
                        "×",
                        FontId::proportional(14.0),
                        fade(
                            if close_hovered {
                                self.theme.text
                            } else {
                                self.theme.dim
                            },
                            close_alpha,
                        ),
                    );
                    if close_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("关闭标签")
                        .clicked()
                    {
                        close = Some(i);
                    }
                }
                // 指针落在已显示的 × 上时不要顺带切页签。
                let on_close = close_shown && pointer.is_some_and(|p| close_rect.contains(p));
                if resp.clicked() && !on_close {
                    switch = Some(i);
                }
                if resp.middle_clicked() {
                    close = Some(i);
                }
                resp.on_hover_text(tip);
            }
            // 最后一个页签与「+」之间同样加竖线；「+」右侧不再加。
            divider(ui);
            // 新建页签按钮紧跟在最后一个页签后面（浏览器风格），不固定到行尾。
            // 尺寸、圆角和 hover 底色与页签保持一致。
            let (plus_rect, plus_resp) =
                ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
            let painter = ui.painter().clone();
            if plus_resp.hovered() {
                painter.rect_filled(plus_rect, 5.0, self.theme.border.gamma_multiply(0.35));
            }
            painter.text(
                plus_rect.center(),
                Align2::CENTER_CENTER,
                "+",
                FontId::proportional(15.0),
                if plus_resp.hovered() {
                    self.theme.text
                } else {
                    self.theme.dim
                },
            );
            if plus_resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("新建 Markdown 文档 (Cmd/Ctrl+N)")
                .clicked()
            {
                self.new_document();
            }
        });
        // 一帧内关闭优先：关掉标签后下标会变，不再处理切换。
        if let Some(i) = close {
            self.close_tab(i);
        } else if let Some(i) = switch {
            self.switch_tab(i);
        }
    }

    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.style_mut().interaction.selectable_labels = false;
        if self.config.show_sidebar_header {
            ui.horizontal(|ui| {
                // 只显示当前页签名（文件 / 大纲），切换入口在底部状态栏。
                let name = match self.side_tab {
                    SideTab::Files => "文件",
                    SideTab::Outline => "大纲",
                };
                ui.label(RichText::new(name).small().strong().color(self.theme.text));
                // 大纲页签：一键展开 / 折叠全部
                if self.side_tab == SideTab::Outline {
                    if ui.small_button("展开").clicked() {
                        self.outline_folded.clear();
                    }
                    if ui
                        .small_button("折叠")
                        .on_hover_text("折叠所有含小节的标题")
                        .clicked()
                    {
                        for (i, item) in self.outline.iter().enumerate() {
                            if self
                                .outline
                                .get(i + 1)
                                .is_some_and(|n| n.level > item.level)
                            {
                                self.outline_folded.insert(i);
                            }
                        }
                    }
                }
            });
            ui.separator();
        }
        let mut set_ws: Option<PathBuf> = None;
        if self.config.show_workspace_bar {
            ui.horizontal(|ui| {
                ui.label(RichText::new("工作区").small().color(self.theme.dim));
                let out = ui.text_edit_singleline(&mut self.workspace_input);
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui
                    .button("打开")
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .clicked()
                    || (enter && out.lost_focus())
                {
                    set_ws = Some(PathBuf::from(self.workspace_input.trim()));
                }
                if ui
                    .button("+")
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .clicked()
                {
                    self.new_document();
                }
            });
            ui.separator();
        }
        if let Some(p) = set_ws {
            self.set_workspace(p);
        }
        let mut actions = Vec::new();
        egui::ScrollArea::vertical().show(ui, |ui| match self.side_tab {
            SideTab::Files => sidebar::file_tree(
                ui,
                &mut self.tree,
                self.doc.path.as_deref(),
                &self.theme,
                &mut self.icons,
                &mut actions,
            ),
            SideTab::Outline => {
                if self.config.show_outline {
                    let mut fold_toggles: Vec<usize> = Vec::new();
                    sidebar::outline_panel(
                        ui,
                        &self.outline,
                        &self.theme,
                        &mut actions,
                        &self.outline_folded,
                        &mut fold_toggles,
                    );
                    for i in fold_toggles {
                        if !self.outline_folded.insert(i) {
                            self.outline_folded.remove(&i);
                        }
                    }
                } else {
                    // 隐藏大纲时回退到文件树，避免侧栏出现空白或提示文字。
                    sidebar::file_tree(
                        ui,
                        &mut self.tree,
                        self.doc.path.as_deref(),
                        &self.theme,
                        &mut self.icons,
                        &mut actions,
                    );
                }
            }
        });
        for a in actions {
            match a {
                SideAction::Open(p) => self.open_file(p),
                SideAction::ToggleDir(p) => {
                    if !self.expanded.remove(&p) {
                        self.expanded.insert(p);
                    }
                    self.refresh_tree();
                }
                SideAction::Jump(i) => {
                    self.editor.active = i.min(self.doc.blocks.len().saturating_sub(1));
                    self.editor.scroll_req = Some(self.editor.active);
                    self.editor.scroll_top = true;
                }
                SideAction::Context(p) => {
                    // 右键：确保侧栏处于文件列表状态，并在指针处弹出菜单。
                    self.side_tab = SideTab::Files;
                    self.config.show_sidebar = true;
                    let pos = ui
                        .ctx()
                        .input(|i| i.pointer.interact_pos())
                        .unwrap_or(egui::Pos2::ZERO);
                    self.ctx_menu = Some((p, pos));
                    ui.ctx().request_repaint();
                }
            }
        }
    }

    pub(super) fn statusbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.style_mut().interaction.selectable_labels = false;
        ui.style_mut().spacing.item_spacing.x = 4.0;
        let st = self.stats_cache.clone();
        let mut chosen: Option<Command> = None;
        ui.horizontal(|ui| {
            // 状态栏入口：点「文件」显示文件树，点「大纲」显示大纲。
            let files_on = self.config.show_sidebar && self.side_tab == SideTab::Files;
            let files_txt = if files_on {
                RichText::new("文件").strong().color(self.theme.accent)
            } else {
                RichText::new("文件").color(self.theme.dim)
            };
            if ui
                .selectable_label(files_on, files_txt)
                .on_hover_cursor(egui::CursorIcon::Default)
                .clicked()
            {
                let (show, tab, show_outline) =
                    sidebar_click(self.config.show_sidebar, self.side_tab, SideTab::Files);
                self.config.show_sidebar = show;
                self.side_tab = tab;
                self.config.show_outline = show_outline;
                self.config.save();
            }

            let outline_on = self.config.show_sidebar && self.side_tab == SideTab::Outline;
            let outline_txt = if outline_on {
                RichText::new("大纲").strong().color(self.theme.accent)
            } else {
                RichText::new("大纲").color(self.theme.dim)
            };
            if ui
                .selectable_label(outline_on, outline_txt)
                .on_hover_cursor(egui::CursorIcon::Default)
                .clicked()
            {
                let (show, tab, show_outline) =
                    sidebar_click(self.config.show_sidebar, self.side_tab, SideTab::Outline);
                self.config.show_sidebar = show;
                self.side_tab = tab;
                self.config.show_outline = show_outline;
                self.config.save();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} 字 · {} 字符 · {} 行 · 约 {} 分钟",
                        st.words, st.chars, st.lines, st.reading_min
                    ))
                    .color(self.theme.dim),
                );
                if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).color(self.theme.accent));
                }
                if self.doc.dirty {
                    ui.label(RichText::new("未保存").color(Color32::from_rgb(200, 120, 60)));
                }
                ui.label(
                    RichText::new(format!(
                        "块 {}/{}",
                        self.editor.active + 1,
                        self.doc.blocks.len(),
                    ))
                    .color(self.theme.dim),
                );
                // 主题按钮：显示当前主题，点击在浅色 / 深色之间切换。
                let theme_txt = RichText::new(self.theme.kind.name()).color(self.theme.accent);
                if ui
                    .selectable_label(false, theme_txt)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("点击切换浅色 / 深色主题")
                    .clicked()
                {
                    chosen = Some(Command::ThemeNext);
                }
                ui.separator();
                for (label, active, cmd) in [
                    ("打字机", self.config.typewriter, Command::ToggleTypewriter),
                    ("专注", self.config.focus_mode, Command::ToggleFocus),
                    ("源码", self.config.source_mode, Command::ToggleSource),
                ] {
                    let txt = if active {
                        RichText::new(label).strong().color(self.theme.accent)
                    } else {
                        RichText::new(label).color(self.theme.dim)
                    };
                    if ui
                        .selectable_label(active, txt)
                        .on_hover_cursor(egui::CursorIcon::Default)
                        .clicked()
                    {
                        chosen = Some(cmd);
                    }
                }
            });
        });
        if let Some(c) = chosen {
            self.execute(c, ctx);
        }
    }
}
