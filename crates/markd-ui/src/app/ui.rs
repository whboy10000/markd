//! 顶层界面：菜单栏、工具条、标签栏、侧栏与状态栏。

use super::*;

impl MarkdApp {
    /// 固定在窗口最顶部的单行菜单栏（VS Code 风格）。
    /// macOS 下这一行同时充当标题栏：系统红绿灯浮在左端，快捷按钮紧跟其右侧。
    pub(super) fn menubar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use egui::{Align2, FontId};
        ui.style_mut().interaction.selectable_labels = false;
        let chosen: Option<Command> = None;
        // 标题栏比默认高（约 1 cm）：用 horizontal_centered 让文档名、图标和拖动区
        // 在整行里垂直居中，跟 macOS 红绿灯的中心对齐；普通 horizontal 只按内容高度排，
        // 会把它们全部顶到上边缘。
        ui.horizontal_centered(|ui| {
            // 给 macOS 系统红绿灯按钮留出空间；其它平台只用常规边距。
            ui.add_space(if cfg!(target_os = "macos") { 78.0 } else { 4.0 });
            // 设置入口已移到「Markd」菜单与 Cmd/Ctrl+,，标题栏左侧不再放齿轮按钮。
            // 「首页」和「+ 新建」入口已收进这一行，编辑区上方不再重复放。
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
            // 标题栏只留一个入口：点它回到主界面（欢迎页 + 最近打开）。
            let home_txt = if self.home {
                RichText::new("主界面").strong().color(self.theme.accent)
            } else {
                RichText::new("主界面").color(self.theme.dim)
            };
            if ui
                .add_sized(
                    egui::vec2(HOME_BUTTON_WIDTH, 24.0),
                    egui::Button::selectable(self.home, home_txt),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("欢迎页与最近打开")
                .clicked()
            {
                self.home = true;
            }
            ui.add_space(4.0);
            // 已打开的文档排成一行圆角标签：小圆点 + 名字，激活标签右侧带 ×。
            if !self.tabs.is_empty() {
                ui.add_space(6.0);
                let mut to_switch: Option<usize> = None;
                let mut to_close: Option<usize> = None;
                // 给右侧的「+ 新建」和终端图标留够位置，放不下的标签用省略号收口。
                let budget = (ui.available_width() - NEW_TAB_BUTTON_WIDTH - 60.0).max(0.0);
                let mut used = 0.0;
                for i in 0..self.tabs.len() {
                    let active = !self.home && i == self.active;
                    let label = self.tab_display_label(i);
                    let w = TAB_CHIP_WIDTH;
                    if used + w > budget {
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::hover());
                        ui.painter().text(
                            r.center(),
                            Align2::CENTER_CENTER,
                            "…",
                            FontId::proportional(14.0),
                            self.theme.dim,
                        );
                        break;
                    }
                    used += w + 6.0;
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(w, 24.0), egui::Sense::click());
                    ui.painter().rect_filled(
                        rect,
                        egui::CornerRadius::same(6),
                        if active {
                            self.theme.border.gamma_multiply(0.55)
                        } else {
                            self.theme.border.gamma_multiply(0.25)
                        },
                    );
                    ui.painter().circle_filled(
                        egui::pos2(rect.left() + 10.0, rect.center().y),
                        3.0,
                        if active {
                            self.theme.accent
                        } else {
                            self.theme.dim
                        },
                    );
                    let text_color = if active {
                        self.theme.text
                    } else {
                        self.theme.dim
                    };
                    let text_w = w - if active { 38.0 } else { 22.0 };
                    let mut shown: String = label.chars().take((text_w / 7.0) as usize).collect();
                    if shown.chars().count() < label.chars().count() {
                        shown.pop();
                        shown.push('…');
                    }
                    ui.painter().text(
                        egui::pos2(rect.left() + 18.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        shown,
                        FontId::proportional(12.0),
                        text_color,
                    );
                    if active {
                        let xr = egui::Rect::from_min_size(
                            egui::pos2(rect.right() - 18.0, rect.center().y - 7.0),
                            egui::vec2(14.0, 14.0),
                        );
                        let id = ui.scope_id().with(("tab_close", self.tabs[i].uid));
                        let xresp = ui.interact(xr, id, egui::Sense::click());
                        if xresp.hovered() {
                            ui.painter().rect_filled(
                                xr,
                                egui::CornerRadius::same(4),
                                self.theme.border.gamma_multiply(0.5),
                            );
                        }
                        ui.painter().text(
                            xr.center(),
                            Align2::CENTER_CENTER,
                            "×",
                            FontId::proportional(13.0),
                            if xresp.hovered() {
                                self.theme.text
                            } else {
                                self.theme.dim
                            },
                        );
                        if xresp.clicked() {
                            to_close = Some(i);
                        }
                    }
                    if resp.clicked() && to_close.is_none() {
                        to_switch = Some(i);
                    }
                    let hover = self.tab_label(i);
                    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(hover);
                }
                if let Some(i) = to_close {
                    self.close_tab(i);
                } else if let Some(i) = to_switch {
                    self.switch_tab(i);
                }
                ui.add_space(6.0);
            }
            // 新建标签：标签条末尾的小方块，点击后直接开空白文档。
            let (plus_rect, plus_resp) = ui
                .allocate_exact_size(egui::vec2(NEW_TAB_BUTTON_WIDTH, 24.0), egui::Sense::click());
            let painter = ui.painter().clone();
            painter.rect_filled(
                plus_rect,
                egui::CornerRadius::same(6),
                if plus_resp.hovered() {
                    self.theme.border.gamma_multiply(0.45)
                } else {
                    self.theme.border.gamma_multiply(0.22)
                },
            );
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
        let width = ui.available_width();
        let top_y = ui.cursor().min.y;

        // 上半 0.7 cm：功能区选项卡。选中的字加粗并在底部画一条强调线。
        ui.allocate_ui_with_layout(
            egui::vec2(width, TOOLBAR_RESERVED_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add_space(10.0);
                // 撤销 / 重做只留符号，固定在选项卡行最左边（不再占下方盒子）。
                let icons_start = 10.0;
                let icon_size = 24.0;
                let icon_gap = 4.0;
                for (icon, hover, cmd) in [
                    ("undo", "撤销 (Cmd/Ctrl+Z)", Command::Undo),
                    ("redo", "重做 (Cmd/Ctrl+Shift+Z)", Command::Redo),
                ] {
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(icon_size, icon_size),
                        egui::Sense::click(),
                    );
                    if resp.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(5),
                            self.theme.border.gamma_multiply(0.35),
                        );
                    }
                    toolbar_icon(ui.painter(), rect, self.theme.text, icon);
                    if resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(hover)
                        .clicked()
                    {
                        chosen = Some(cmd);
                    }
                }
                ui.add_space(6.0);
                // 选项卡作为一组在剩余宽度里居中；左边留给撤销/重做。
                let icons_width = icons_start + icon_size * 2.0 + icon_gap + 6.0;
                let tabs_total = 7.0 * 52.0;
                ui.add_space(((width - icons_width - tabs_total) * 0.5).max(0.0));
                for (label, tab) in [
                    ("开始", ToolbarTab::Start),
                    ("插入", ToolbarTab::Insert),
                    ("页面样式", ToolbarTab::PageStyle),
                    ("高级", ToolbarTab::Advanced),
                    ("视图", ToolbarTab::View),
                    ("工具", ToolbarTab::Tools),
                    ("AI", ToolbarTab::AI),
                ] {
                    let active = self.toolbar_tab == tab;
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(52.0, TOOLBAR_RESERVED_HEIGHT),
                        egui::Sense::click(),
                    );
                    let color = if active {
                        self.theme.accent
                    } else {
                        self.theme.dim
                    };
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(if active { 13.0 } else { 12.0 }),
                        color,
                    );
                    if active {
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.left() + 4.0, rect.bottom() - 2.0),
                                egui::pos2(rect.right() - 4.0, rect.bottom() - 2.0),
                            ],
                            egui::Stroke::new(2.0, self.theme.accent),
                        );
                    }
                    if resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.toolbar_tab = tab;
                    }
                }

                // 选项卡行最右侧的折叠按钮：收起 / 展开下半工具区。
                let rest = ui.available_width();
                if rest > 34.0 {
                    ui.add_space(rest - 34.0);
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(5),
                            self.theme.border.gamma_multiply(0.35),
                        );
                    }
                    toolbar_icon(
                        ui.painter(),
                        rect,
                        self.theme.text,
                        if self.toolbar_collapsed {
                            "chevron_down"
                        } else {
                            "chevron_up"
                        },
                    );
                    if resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(if self.toolbar_collapsed {
                            "展开工具区"
                        } else {
                            "折叠工具区"
                        })
                        .clicked()
                    {
                        self.toolbar_collapsed = !self.toolbar_collapsed;
                    }
                }
            },
        );

        // 选项卡与工具区之间的横线，横跨整条工具条。
        let line_y = top_y + TOOLBAR_RESERVED_HEIGHT;
        let line_rect = ui.clip_rect();
        ui.painter().line_segment(
            [
                egui::pos2(line_rect.left(), line_y),
                egui::pos2(line_rect.right(), line_y),
            ],
            egui::Stroke::new(1.0, self.theme.border),
        );

        // 折叠时只剩选项卡行与这条底部分隔线；撤销/重做在折叠态仍要点得动，
        // 所以先执行上半区收集到的命令再返回，不画下半按钮区。
        if self.toolbar_collapsed {
            if let Some(c) = chosen {
                self.execute(c, ctx);
            }
            return;
        }

        // 下半 2.5 cm：每个功能占一个 1.5 cm × 1.7 cm 的独立盒子，上图标下文字。
        // 空白选项卡只占高度，不画按钮。
        // 盒子较多时可能超过窗口宽度：25 个 1.5 cm 盒子在 1280 宽窗口放不下。
        // 用横向滚动而不是换行——下半区 2.5 cm 只够放一行 1.7 cm 的盒子。
        ui.allocate_ui_with_layout(
            egui::vec2(width, TOOLBAR_HEIGHT - TOOLBAR_RESERVED_HEIGHT),
            egui::Layout::left_to_right(egui::Align::LEFT).with_cross_justify(true),
            |ui| {
                egui::ScrollArea::horizontal()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.with_layout(
                            egui::Layout::left_to_right(egui::Align::LEFT).with_cross_justify(true),
                            |ui| {
                                let start = self.toolbar_tab == ToolbarTab::Start;
                                let insert = self.toolbar_tab == ToolbarTab::Insert;
                                let advanced = self.toolbar_tab == ToolbarTab::Advanced;
                                let tools = self.toolbar_tab == ToolbarTab::Tools;
                                // 盒子作为一组居中：横向布局不会替我们做主轴整体居中，
                                // 所以按当前页的盒子数量算好总宽，先在左侧补空；放不下时补空为 0。
                                let count = if start {
                                    17
                                } else if insert {
                                    15
                                } else if advanced {
                                    3
                                } else if tools {
                                    2
                                } else {
                                    0
                                };
                                if count > 0 {
                                    let spacing = ui.spacing().item_spacing.x;
                                    let total = count as f32 * TOOLBOX_WIDTH
                                        + (count as f32 - 1.0) * spacing;
                                    ui.add_space(((width - total) * 0.5).max(0.0));
                                }

                                if start {
                                    for (icon, label, hover, cmd) in [
                                        (
                                            "paragraph",
                                            "正文段落",
                                            "正文段落 (Cmd/Ctrl+0)",
                                            Command::Paragraph,
                                        ),
                                        ("h1", "一级标题", "一级标题 (Cmd/Ctrl+1)", Command::H1),
                                        ("h2", "二级标题", "二级标题 (Cmd/Ctrl+2)", Command::H2),
                                        ("h3", "三级标题", "三级标题 (Cmd/Ctrl+3)", Command::H3),
                                        ("h4", "四级标题", "四级标题 (Cmd/Ctrl+4)", Command::H4),
                                        ("h5", "五级标题", "五级标题 (Cmd/Ctrl+5)", Command::H5),
                                        ("h6", "六级标题", "六级标题 (Cmd/Ctrl+6)", Command::H6),
                                    ] {
                                        let r = toolbar_box(ui, icon, label, hover);
                                        if r.clicked() {
                                            chosen = Some(cmd);
                                        }
                                    }

                                    for (icon, label, hover, cmd) in [
                                        (
                                            "bold",
                                            "加粗",
                                            "加粗：作用于选中文字；无选区时作用于整行",
                                            Command::Bold,
                                        ),
                                        ("underline", "下划线", "下划线", Command::Underline),
                                        ("italic", "斜体", "斜体 (Cmd/Ctrl+I)", Command::Italic),
                                        (
                                            "strike",
                                            "删除线",
                                            "删除线 (Ctrl+Shift+`)",
                                            Command::Strike,
                                        ),
                                        (
                                            "line_break",
                                            "换行",
                                            "换行：在光标处插入 <br>，段落、列表、表格单元格都生效",
                                            Command::LineBreak,
                                        ),
                                    ] {
                                        let r = toolbar_box(ui, icon, label, hover);
                                        if r.clicked() {
                                            chosen = Some(cmd);
                                        }
                                    }

                                    for (icon, label, hover, cmd) in [
                                        (
                                            "list",
                                            "无序列表",
                                            "无序列表 (Cmd/Ctrl+Option+U)",
                                            Command::Bullet,
                                        ),
                                        (
                                            "ordered_list",
                                            "有序列表",
                                            "有序列表 (Cmd/Ctrl+Option+O)",
                                            Command::Ordered,
                                        ),
                                        (
                                            "task_list",
                                            "任务列表",
                                            "任务列表 (Cmd/Ctrl+Option+X)",
                                            Command::Task,
                                        ),
                                    ] {
                                        let r = toolbar_box(ui, icon, label, hover);
                                        if r.clicked() {
                                            chosen = Some(cmd);
                                        }
                                    }

                                    // 引用是开关：双击第二击会抵消，直接忽略。
                                    let r = toolbar_box(
                                        ui,
                                        "quote",
                                        "引用",
                                        "引用块 (Cmd/Ctrl+Option+Q)",
                                    );
                                    if r.clicked() && !r.double_clicked() {
                                        chosen = Some(Command::Quote);
                                    }
                                }

                                if insert {
                                    let r = toolbar_box(
                                        ui,
                                        "image",
                                        "图片",
                                        "插入图片（也可 Cmd/Ctrl+V 粘贴）",
                                    );
                                    if r.clicked() {
                                        chosen = Some(Command::Image);
                                    }
                                    let r = toolbar_box(
                                        ui,
                                        "link",
                                        "超链接",
                                        "插入超链接 (Cmd/Ctrl+K)",
                                    );
                                    if r.clicked() {
                                        chosen = Some(Command::Link);
                                    }
                                    let r = toolbar_box(
                                        ui,
                                        "video",
                                        "视频",
                                        "插入 B 站视频：预览是播放器卡片，导出 HTML 后可直接播放",
                                    );
                                    if r.clicked() {
                                        chosen = Some(Command::Video);
                                    }
                                    let r = toolbar_box(
                                        ui,
                                        "file",
                                        "文件",
                                        "插入文件（PDF/Word/Excel…）：可选本地文件或粘贴远程链接，预览是附件卡片",
                                    );
                                    if r.clicked() {
                                        chosen = Some(Command::Attachment);
                                    }

                                    let r = toolbar_box(
                                        ui,
                                        "formula",
                                        "公式",
                                        "行内 / 公式块 / 代码块公式",
                                    );
                                    egui::Popup::menu(&r).show(|ui| {
                                        for (label, cmd) in [
                                            ("行内公式", Command::InlineMath),
                                            ("公式块", Command::Math),
                                            ("代码块公式", Command::MathCode),
                                        ] {
                                            if ui.button(label).clicked() {
                                                chosen = Some(cmd);
                                                ui.close();
                                            }
                                        }
                                    });

                                    let r = toolbar_box(
                                        ui,
                                        "code",
                                        "代码",
                                        "行内代码 / 代码块 / 代码组",
                                    );
                                    egui::Popup::menu(&r).show(|ui| {
                                        for (label, cmd) in [
                                            ("行内代码", Command::Code),
                                            ("代码块", Command::CodeBlock),
                                            ("代码组", Command::CodeGroup),
                                        ] {
                                            if ui.button(label).clicked() {
                                                chosen = Some(cmd);
                                                ui.close();
                                            }
                                        }
                                    });

                                    let r = toolbar_box(
                                        ui,
                                        "mermaid",
                                        "Mermaid",
                                        "插入 Mermaid 图表模板",
                                    );
                                    egui::Popup::menu(&r).show(|ui| {
                                        egui::ScrollArea::vertical().max_height(460.0).show(
                                            ui,
                                            |ui| {
                                                for (label, template) in MERMAID_TEMPLATES {
                                                    if ui.button(*label).clicked() {
                                                        chosen_tpl = Some(*template);
                                                        ui.close();
                                                    }
                                                }
                                            },
                                        );
                                    });
                                }

                                // 原「插入」下拉里的七项全部展开，只放在「插入」选项卡里。
                                if insert {
                                    for (icon, label, hover, cmd) in [
                                        ("table", "表格", "插入表格", Command::Table),
                                        ("rule", "分隔线", "插入分隔线", Command::Rule),
                                        (
                                            "chart",
                                            "图表",
                                            "插入 Mermaid 图表模板",
                                            Command::Mermaid,
                                        ),
                                        (
                                            "flowchart",
                                            "流程图",
                                            "插入流程图模板",
                                            Command::Flowchart,
                                        ),
                                        ("footnote", "脚注", "插入脚注", Command::Footnote),
                                        ("toc", "目录", "插入目录 [TOC]", Command::Toc),
                                        (
                                            "upload",
                                            "上传图片",
                                            "上传本地图片并插入",
                                            Command::UploadImages,
                                        ),
                                        (
                                            "html",
                                            "HTML 内嵌",
                                            "插入 HTML 内嵌：span 改颜色字号、div 做背景提示框、hr 分割线、br 换行",
                                            Command::HtmlEmbed,
                                        ),
                                    ] {
                                        let r = toolbar_box(ui, icon, label, hover);
                                        if r.clicked() {
                                            chosen = Some(cmd);
                                        }
                                    }
                                }

                                // 「工具」页：PDF 导出（浏览器无头打印）与 Pandoc 格式选择弹窗。
                                if tools {
                                    let r = toolbar_box(
                                        ui,
                                        "pdf",
                                        "PDF 导出",
                                        "把当前文档打印成 PDF（自动检测 Chrome / Edge）",
                                    );
                                    if r.clicked() {
                                        chosen = Some(Command::ExportPdf);
                                    }

                                    let hover = "选择导出格式与保存位置（Word / PPT / HTML / PDF 等）";
                                    let r = toolbar_box(ui, "pandoc", "Pandoc 导出", hover);
                                    if r.clicked() {
                                        chosen = Some(Command::ExportWithPandoc);
                                    }
                                }

                                // 「查找和替换」留在开始页；快照 / 关系图 / 统计迁到「高级」选项卡。
                                if start {
                                    let r = toolbar_box(ui, "search", "查找和替换", "查找和替换");
                                    if r.clicked() {
                                        chosen = Some(Command::Find);
                                    }
                                }

                                // 原「开始」页里的快照 / 关系图 / 统计，只放在「高级」选项卡里。
                                if advanced {
                                    for (icon, label, cmd) in [
                                        ("snapshot", "快照", Command::Snapshot),
                                        ("graph", "关系图", Command::Graph),
                                        ("stats", "统计", Command::Stats),
                                    ] {
                                        let r = toolbar_box(ui, icon, label, label);
                                        if r.clicked() {
                                            chosen = Some(cmd);
                                        }
                                    }
                                }
                            },
                        );
                    });
            },
        );
        if let Some(c) = chosen {
            self.execute(c, ctx);
        }
        if let Some(tpl) = chosen_tpl {
            editor::insert_template(&mut self.doc, &mut self.editor, tpl);
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
        // 大纲面板自带滚动（虚拟化只排版视口内的行），不能再套一层 ScrollArea：
        // 外层会把可用高度放开成无限，内层就拿不到视口、算不出该排版哪几行。
        match self.side_tab {
            SideTab::Outline if self.config.show_outline => {
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
            }
            // 文件树，以及大纲被关闭时的回退（避免侧栏出现空白或提示文字）。
            _ => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    sidebar::file_tree(
                        ui,
                        &mut self.tree,
                        self.doc.path.as_deref(),
                        &self.theme,
                        &mut self.icons,
                        &mut actions,
                    )
                });
            }
        }
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
                if self.pandoc.is_busy() || self.pdf.is_busy() {
                    ui.spinner();
                }
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
