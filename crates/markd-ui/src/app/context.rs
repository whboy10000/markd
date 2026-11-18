//! 编辑区空白处的右键上下文菜单。
//!
//! 菜单本身由 app 层绘制（需要图标缓存与命令分发），编辑器只负责在
//! 空白处右键时把弹出位置写进 `EditorState.context_menu`。

use super::*;
use crate::ast::Block;
use crate::editor;
use egui::{Align2, Color32, FontId, Id, Pos2, Rect, Sense, Stroke, Vec2};

#[derive(Clone, Copy)]
enum MenuIcon {
    Cut,
    Copy,
    Paste,
    PasteAs,
    Paragraph,
    Format,
    Insert,
}

enum ContextAction {
    Cut,
    Copy,
    Paste,
    CopyMarkdown,
    CopyPlain,
    PastePlain,
    ParagraphAbove,
    ParagraphBelow,
}

impl MarkdApp {
    /// 绘制右键菜单；位置来自编辑器本帧记录的空白右键。
    pub(super) fn editor_context_menu(&mut self, ctx: &egui::Context) {
        if self.home {
            self.editor.context_menu = None;
            return;
        }
        let Some(pos) = self.editor.context_menu else {
            return;
        };
        let tab_key = self.tabs.get(self.active).map(|t| t.uid).unwrap_or(0);
        let theme = self.theme.clone();
        let mut close = false;

        let area = egui::Area::new(Id::new(("editor_context_menu", tab_key)))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style())
                    .fill(theme.panel)
                    .stroke(Stroke::new(1.0, theme.border))
                    .corner_radius(8.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        // 固定菜单宽度。Area 的可用宽度默认是「屏幕剩余宽度」，
                        // 只设 min_width 会让每一行被撑到 600px 以上。
                        ui.set_width(212.0);
                        ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);

                        let enabled = self.has_selection();
                        let cut = menu_row(
                            ui,
                            Some(MenuIcon::Cut),
                            "剪切",
                            Some(shortcut("⌘X", "Ctrl+X")),
                            false,
                            enabled,
                            false,
                            &theme,
                        );
                        if cut.clicked() {
                            self.context_action(ContextAction::Cut, ctx);
                            close = true;
                        }

                        let copy = menu_row(
                            ui,
                            Some(MenuIcon::Copy),
                            "拷贝",
                            Some(shortcut("⌘C", "Ctrl+C")),
                            false,
                            enabled,
                            false,
                            &theme,
                        );
                        if copy.clicked() {
                            self.context_action(ContextAction::Copy, ctx);
                            close = true;
                        }

                        let paste = menu_row(
                            ui,
                            Some(MenuIcon::Paste),
                            "粘贴",
                            Some(shortcut("⌘V", "Ctrl+V")),
                            false,
                            true,
                            false,
                            &theme,
                        );
                        if paste.clicked() {
                            self.context_action(ContextAction::Paste, ctx);
                            close = true;
                        }

                        let paste_as = menu_row(
                            ui,
                            Some(MenuIcon::PasteAs),
                            "复制 / 粘贴为…",
                            None,
                            false,
                            true,
                            true,
                            &theme,
                        );
                        egui::Popup::menu(&paste_as)
                            .style(menu_style(&theme))
                            .show(|ui| {
                                for (label, action) in [
                                    ("复制为 Markdown", ContextAction::CopyMarkdown),
                                    ("复制为纯文本", ContextAction::CopyPlain),
                                    ("粘贴为纯文本", ContextAction::PastePlain),
                                ] {
                                    if ui.button(label).clicked() {
                                        self.context_action(action, ctx);
                                        close = true;
                                        ui.close();
                                    }
                                }
                            });

                        menu_separator(ui, &theme);

                        let paragraph = menu_row(
                            ui,
                            Some(MenuIcon::Paragraph),
                            "段落",
                            None,
                            false,
                            true,
                            true,
                            &theme,
                        );
                        egui::Popup::menu(&paragraph)
                            .style(menu_style(&theme))
                            .show(|ui| {
                                for (label, key, cmd) in [
                                    ("一级标题", "⌘1", Command::H1),
                                    ("二级标题", "⌘2", Command::H2),
                                    ("三级标题", "⌘3", Command::H3),
                                    ("四级标题", "⌘4", Command::H4),
                                    ("五级标题", "⌘5", Command::H5),
                                    ("六级标题", "⌘6", Command::H6),
                                ] {
                                    if menu_button_row(ui, label, Some(key), false, &theme) {
                                        self.execute(cmd, ctx);
                                        close = true;
                                        ui.close();
                                    }
                                }
                                menu_separator(ui, &theme);
                                for (label, key, cmd) in [
                                    ("引用", "⌥⌘Q", Command::Quote),
                                    ("有序列表", "⌥⌘O", Command::Ordered),
                                    ("无序列表", "⌥⌘U", Command::Bullet),
                                    ("任务列表", "⌥⌘X", Command::Task),
                                ] {
                                    if menu_button_row(ui, label, Some(key), false, &theme) {
                                        self.execute(cmd, ctx);
                                        close = true;
                                        ui.close();
                                    }
                                }
                                menu_separator(ui, &theme);
                                let is_paragraph = matches!(
                                    self.doc.blocks.get(self.editor.active),
                                    Some(Block::Paragraph { .. }) | Some(Block::Blank { .. })
                                );
                                if menu_button_row(ui, "段落", Some("⌘0"), is_paragraph, &theme)
                                {
                                    self.execute(Command::Paragraph, ctx);
                                    close = true;
                                    ui.close();
                                }
                            });

                        let format = menu_row(
                            ui,
                            Some(MenuIcon::Format),
                            "格式",
                            None,
                            false,
                            true,
                            true,
                            &theme,
                        );
                        egui::Popup::menu(&format)
                            .style(menu_style(&theme))
                            .show(|ui| {
                                for (label, key, cmd) in [
                                    ("加粗", "⌘B", Command::Bold),
                                    ("斜体", "⌘I", Command::Italic),
                                    ("下划线", "⌘U", Command::Underline),
                                    ("代码", "⌃`", Command::Code),
                                    ("删除线", "⌃⇧`", Command::Strike),
                                    ("高亮", "⇧⌘H", Command::Highlight),
                                ] {
                                    if menu_button_row(ui, label, Some(key), false, &theme) {
                                        self.execute(cmd, ctx);
                                        close = true;
                                        ui.close();
                                    }
                                }
                                menu_separator(ui, &theme);
                                if menu_button_row(ui, "超链接", Some("⌘K"), false, &theme) {
                                    self.execute(Command::Link, ctx);
                                    close = true;
                                    ui.close();
                                }
                                menu_separator(ui, &theme);
                                if menu_button_row(ui, "清除样式", Some("⌘\\"), false, &theme)
                                {
                                    self.execute(Command::ClearFormat, ctx);
                                    close = true;
                                    ui.close();
                                }
                            });

                        let insert = menu_row(
                            ui,
                            Some(MenuIcon::Insert),
                            "插入",
                            None,
                            false,
                            true,
                            true,
                            &theme,
                        );
                        egui::Popup::menu(&insert)
                            .style(menu_style(&theme))
                            .show(|ui| {
                                if menu_button_row(ui, "图像", Some("⌃⌘I"), false, &theme) {
                                    self.execute(Command::Image, ctx);
                                    close = true;
                                    ui.close();
                                }
                                menu_separator(ui, &theme);
                                for (label, key, cmd) in [
                                    ("代码块", "⌥⌘C", Command::CodeBlock),
                                    ("公式块", "⌥⌘B", Command::Math),
                                    ("代码块公式", "", Command::MathCode),
                                    ("表格", "⌥⌘T", Command::Table),
                                ] {
                                    if menu_button_row(ui, label, Some(key), false, &theme) {
                                        self.execute(cmd, ctx);
                                        close = true;
                                        ui.close();
                                    }
                                }
                                menu_separator(ui, &theme);
                                if menu_button_row(ui, "水平分割线", Some("⌥⌘-"), false, &theme)
                                {
                                    self.execute(Command::Rule, ctx);
                                    close = true;
                                    ui.close();
                                }
                                if menu_button_row(ui, "在上方插入段落", None, false, &theme)
                                {
                                    self.context_action(ContextAction::ParagraphAbove, ctx);
                                    close = true;
                                    ui.close();
                                }
                                if menu_button_row(ui, "在下方插入段落", None, false, &theme)
                                {
                                    self.context_action(ContextAction::ParagraphBelow, ctx);
                                    close = true;
                                    ui.close();
                                }
                            });
                    });
            });

        // 点菜单外或按 Esc 关闭。子菜单在独立图层里，点中时也会关闭主菜单，
        // 但动作已经在同一帧执行完毕。
        let rect = area.response.rect;
        // 只用左键判断「点外部关闭」。右键本身也会产生 any_click，若把它算进来，
        // 菜单弹出的同一帧可能因为 Area 首次 sizing/约束而误判为点外面，
        // 表现就是「有时候右键能弹、有时候一闪而过」。
        let clicked_outside = ctx.input(|i| i.pointer.primary_clicked())
            && ctx
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| !rect.contains(p));
        if close || clicked_outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.editor.context_menu = None;
        }
    }

    fn has_selection(&self) -> bool {
        self.editor.cross_sel.is_some() || editor::selection_text(&self.doc, &self.editor).is_some()
    }

    fn context_action(&mut self, action: ContextAction, ctx: &egui::Context) {
        match action {
            ContextAction::Cut => {
                if let Some(text) = editor::cut_selection(&mut self.doc, &mut self.editor) {
                    ctx.copy_text(text);
                    self.notify(ctx, "已剪切".to_string());
                    self.on_changed();
                } else {
                    self.notify(ctx, "没有可剪切的内容".to_string());
                }
            }
            ContextAction::Copy => {
                if let Some(text) = editor::copy_selection(&self.doc, &self.editor) {
                    ctx.copy_text(text);
                    self.notify(ctx, "已复制".to_string());
                } else {
                    self.notify(ctx, "没有可复制的内容".to_string());
                }
            }
            ContextAction::Paste => {
                if let Some(text) = clipboard_text() {
                    if editor::paste_text(&mut self.doc, &mut self.editor, &text) {
                        self.notify(ctx, "已粘贴".to_string());
                        self.on_changed();
                    }
                } else {
                    self.notify(ctx, "剪贴板里没有文本".to_string());
                }
            }
            ContextAction::CopyMarkdown => {
                if let Some(text) = editor::copy_selection_markdown(&self.doc, &self.editor) {
                    ctx.copy_text(text);
                    self.notify(ctx, "已复制为 Markdown".to_string());
                } else {
                    self.notify(ctx, "没有可复制的内容".to_string());
                }
            }
            ContextAction::CopyPlain => {
                if let Some(text) = editor::copy_selection(&self.doc, &self.editor) {
                    ctx.copy_text(text);
                    self.notify(ctx, "已复制为纯文本".to_string());
                } else {
                    self.notify(ctx, "没有可复制的内容".to_string());
                }
            }
            ContextAction::PastePlain => {
                if let Some(text) = clipboard_text() {
                    if editor::paste_text(&mut self.doc, &mut self.editor, &text) {
                        self.notify(ctx, "已粘贴为纯文本".to_string());
                        self.on_changed();
                    }
                } else {
                    self.notify(ctx, "剪贴板里没有文本".to_string());
                }
            }
            ContextAction::ParagraphAbove => {
                editor::insert_paragraph(&mut self.doc, &mut self.editor, true);
                self.on_changed();
            }
            ContextAction::ParagraphBelow => {
                editor::insert_paragraph(&mut self.doc, &mut self.editor, false);
                self.on_changed();
            }
        }
    }
}

fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

fn shortcut(mac: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

fn menu_style(theme: &Theme) -> egui::style::StyleModifier {
    let panel = theme.panel;
    let hover = theme.border.gamma_multiply(0.45);
    let text = theme.text;
    egui::style::StyleModifier::new(move |s| {
        s.visuals.panel_fill = panel;
        s.visuals.window_fill = panel;
        s.visuals.override_text_color = Some(text);
        s.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        s.visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        s.visuals.widgets.hovered.bg_fill = hover;
        s.visuals.widgets.hovered.weak_bg_fill = hover;
        s.visuals.widgets.active.bg_fill = hover;
        s.visuals.widgets.active.weak_bg_fill = hover;
        s.spacing.item_spacing = egui::vec2(6.0, 2.0);
    })
}

#[allow(clippy::too_many_arguments)]
fn menu_row(
    ui: &mut egui::Ui,
    icon: Option<MenuIcon>,
    label: &str,
    shortcut: Option<&str>,
    checked: bool,
    enabled: bool,
    arrow: bool,
    theme: &Theme,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 27.0), Sense::click());
    if resp.hovered() && enabled {
        ui.painter()
            .rect_filled(rect, 4.0, theme.border.gamma_multiply(0.45));
    }
    let color = if enabled {
        theme.text
    } else {
        theme.dim.gamma_multiply(0.55)
    };
    let mut x = rect.left() + 8.0;
    if let Some(icon) = icon {
        let icon_rect =
            Rect::from_center_size(Pos2::new(x + 7.0, rect.center().y), Vec2::splat(14.0));
        paint_icon_kind(ui.painter(), icon_rect, icon, color);
        x += 24.0;
    } else if checked {
        ui.painter().text(
            Pos2::new(x + 7.0, rect.center().y),
            Align2::CENTER_CENTER,
            "✓",
            FontId::proportional(12.0),
            theme.accent,
        );
        x += 24.0;
    }
    ui.painter().text(
        Pos2::new(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        color,
    );
    if let Some(s) = shortcut {
        ui.painter().text(
            Pos2::new(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            s,
            FontId::proportional(11.0),
            theme.dim,
        );
    } else if arrow {
        ui.painter().text(
            Pos2::new(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            "›",
            FontId::proportional(14.0),
            theme.dim,
        );
    }
    resp
}

fn menu_button_row(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: Option<&str>,
    checked: bool,
    theme: &Theme,
) -> bool {
    menu_row(ui, None, label, shortcut, checked, true, false, theme).clicked()
}

fn menu_separator(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 7.0), Sense::hover());
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + 8.0, rect.center().y),
            Pos2::new(rect.right() - 8.0, rect.center().y),
        ],
        Stroke::new(1.0, theme.border),
    );
}

/// 用几何图形画 14×14 线性图标，避免为菜单再引入一套 SVG 资源。
fn paint_icon_kind(painter: &egui::Painter, rect: Rect, icon: MenuIcon, color: Color32) {
    let c = rect.center();
    let s = Stroke::new(1.3, color);
    match icon {
        MenuIcon::Cut => {
            painter.circle_stroke(Pos2::new(c.x - 3.0, c.y + 3.0), 2.2, s);
            painter.circle_stroke(Pos2::new(c.x + 3.0, c.y + 3.0), 2.2, s);
            painter.line_segment(
                [
                    Pos2::new(c.x - 1.6, c.y + 1.2),
                    Pos2::new(c.x + 4.0, c.y - 5.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 1.6, c.y + 1.2),
                    Pos2::new(c.x - 4.0, c.y - 5.0),
                ],
                s,
            );
        }
        MenuIcon::Copy => {
            let back = Rect::from_min_size(Pos2::new(c.x - 5.0, c.y - 5.0), Vec2::new(8.0, 9.0));
            let front = Rect::from_min_size(Pos2::new(c.x - 2.0, c.y - 2.5), Vec2::new(8.0, 9.0));
            painter.rect_stroke(back, 1.5, s, egui::StrokeKind::Inside);
            painter.rect_stroke(front, 1.5, s, egui::StrokeKind::Inside);
        }
        MenuIcon::Paste => {
            let body = Rect::from_min_size(Pos2::new(c.x - 5.0, c.y - 4.5), Vec2::new(10.0, 11.0));
            painter.rect_stroke(body, 2.0, s, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    Pos2::new(c.x - 2.5, c.y - 6.0),
                    Pos2::new(c.x + 2.5, c.y - 6.0),
                ],
                s,
            );
        }
        MenuIcon::PasteAs => {
            let body = Rect::from_min_size(Pos2::new(c.x - 5.5, c.y - 4.0), Vec2::new(9.0, 10.0));
            painter.rect_stroke(body, 2.0, s, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    Pos2::new(c.x + 1.0, c.y + 1.0),
                    Pos2::new(c.x + 6.0, c.y + 1.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 4.0, c.y - 1.5),
                    Pos2::new(c.x + 6.5, c.y + 1.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 4.0, c.y + 3.5),
                    Pos2::new(c.x + 6.5, c.y + 1.0),
                ],
                s,
            );
        }
        MenuIcon::Paragraph => {
            painter.line_segment(
                [
                    Pos2::new(c.x - 4.5, c.y - 5.0),
                    Pos2::new(c.x - 4.5, c.y + 5.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 4.5, c.y - 5.0),
                    Pos2::new(c.x + 2.5, c.y - 5.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 4.5, c.y - 1.0),
                    Pos2::new(c.x + 1.5, c.y - 1.0),
                ],
                s,
            );
            painter.circle_stroke(Pos2::new(c.x + 2.5, c.y - 1.0), 2.0, s);
        }
        MenuIcon::Format => {
            painter.text(
                Pos2::new(c.x, c.y - 1.0),
                Align2::CENTER_CENTER,
                "A",
                FontId::proportional(13.0),
                color,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 5.0, c.y + 5.0),
                    Pos2::new(c.x + 5.0, c.y + 5.0),
                ],
                s,
            );
        }
        MenuIcon::Insert => {
            painter.line_segment([Pos2::new(c.x - 5.0, c.y), Pos2::new(c.x + 5.0, c.y)], s);
            painter.line_segment([Pos2::new(c.x, c.y - 5.0), Pos2::new(c.x, c.y + 5.0)], s);
        }
    }
}
