//! Author: aurelianus <aurelianus@126.com>

//! 设置弹窗：左侧分类 + 右侧选项，风格对齐 Typora 的偏好设置。

use crate::theme::{Theme, ThemeKind};
use crate::workspace::Config;
use egui::RichText;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Appearance,
    Editor,
    Files,
    Images,
    Markdown,
    Export,
}

const TABS: [(Tab, &str); 7] = [
    (Tab::General, "通用"),
    (Tab::Appearance, "外观"),
    (Tab::Editor, "编辑器"),
    (Tab::Files, "文件"),
    (Tab::Images, "图像"),
    (Tab::Markdown, "Markdown"),
    (Tab::Export, "导出"),
];

#[derive(Default)]
pub struct Settings {
    pub open: bool,
    tab: Option<Tab>,
    /// 每次打开递增，用作窗口 Id：让窗口重新使用默认尺寸与居中位置，
    /// 而不是沿用上次拖动/拉伸后持久化下来的旧几何。
    open_seq: u64,
}

impl Settings {
    pub fn open(&mut self) {
        self.open = true;
        if self.tab.is_none() {
            self.tab = Some(Tab::General);
        }
        // 每次打开都重新居中并恢复默认大小。
        self.open_seq = self.open_seq.wrapping_add(1);
    }

    /// 画设置窗口。返回 (是否有改动, 是否点了「恢复默认」)。
    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme, cfg: &mut Config) -> (bool, bool) {
        if !self.open {
            return (false, false);
        }
        let mut changed = false;
        let mut reset = false;
        let mut open = self.open;
        let mut close = false;
        let mut tab = self.tab.unwrap_or(Tab::General);

        // 尺寸随可用窗口自适应：默认 720×520，小窗口时留出边距后收缩，
        // 保证弹窗完整可见且比例合适。
        let screen = ctx.content_rect();
        let size = egui::vec2(
            720.0_f32.min((screen.width() - 48.0).max(420.0)),
            520.0_f32.min((screen.height() - 48.0).max(300.0)),
        );
        // 居中显示；窗口 Id 每次打开都是新的，所以默认位置总会生效。
        let pos = screen.center() - size * 0.5;

        egui::Window::new("设置")
            .id(egui::Id::unique(("settings_window", self.open_seq)))
            .open(&mut open)
            .collapsible(false)
            // 固定为合适的外框尺寸：内容按这个尺寸排版，避免内容把窗口越撑越大。
            .fixed_size(size)
            // 标题栏拖动移动窗口，窗口保持在其他内容之上。
            .movable(true)
            .drag_area(egui::WindowDrag::TitleBar)
            .default_pos(pos)
            .show(ctx, |ui| {
                // 预留底部按钮行，剩余高度给右侧滚动区。
                let content_h = (ui.available_height() - 48.0).max(120.0);
                let right_w = (ui.available_width() - 156.0).max(280.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(132.0);
                        for (t, label) in TABS {
                            let txt = if tab == t {
                                RichText::new(label).strong().color(theme.accent)
                            } else {
                                RichText::new(label)
                            };
                            if ui.selectable_label(tab == t, txt).clicked() {
                                tab = t;
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(right_w);
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .max_height(content_h)
                            .show(ui, |ui| {
                                changed |= Self::body(ui, tab, cfg, theme);
                            });
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("恢复默认").clicked() {
                        reset = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("完成").clicked() {
                            close = true;
                        }
                    });
                });
            });

        self.open = open && !close;
        self.tab = Some(tab);
        (changed, reset)
    }

    fn body(ui: &mut egui::Ui, tab: Tab, cfg: &mut Config, _theme: &Theme) -> bool {
        let mut changed = false;
        match tab {
            Tab::General => {
                ui.label(RichText::new("通用").strong());
                ui.separator();
                changed |= ui
                    .checkbox(&mut cfg.restore_last_file, "启动时恢复上次打开的文件")
                    .changed();
                changed |= ui
                    .checkbox(&mut cfg.confirm_on_exit, "退出前确认")
                    .changed();
                ui.add_space(8.0);
                ui.label("界面缩放");
                changed |= ui
                    .add(egui::Slider::new(&mut cfg.ui_scale, 0.8..=1.6).text("倍"))
                    .changed();
                ui.add_space(8.0);
                ui.label(format!(
                    "Markd {} · 纯 Rust + egui 实现的 Markdown 编辑器",
                    env!("CARGO_PKG_VERSION")
                ));
            }
            Tab::Appearance => {
                ui.label(RichText::new("外观").strong());
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("主题");
                    let mut kind = cfg.theme;
                    egui::ComboBox::from_id_salt("settings_theme")
                        .selected_text(kind.name())
                        .show_ui(ui, |ui| {
                            for k in ThemeKind::ALL {
                                if ui.selectable_value(&mut kind, k, k.name()).clicked() {
                                    cfg.theme = k;
                                    changed = true;
                                }
                            }
                        });
                });
                ui.add_space(6.0);
                ui.label("正文字号");
                changed |= ui
                    .add(egui::Slider::new(&mut cfg.font_size, 12.0..=26.0).text("pt"))
                    .changed();
                ui.label("侧栏宽度");
                changed |= ui
                    .add(egui::Slider::new(&mut cfg.sidebar_width, 160.0..=520.0).text("pt"))
                    .changed();
                ui.add_space(6.0);
                changed |= ui
                    .checkbox(&mut cfg.show_sidebar_header, "显示侧栏标签栏")
                    .changed();
                changed |= ui
                    .checkbox(&mut cfg.show_workspace_bar, "显示工作区栏")
                    .changed();
            }
            Tab::Editor => {
                ui.label(RichText::new("编辑器").strong());
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Tab 宽度");
                    changed |= ui
                        .add(egui::DragValue::new(&mut cfg.tab_size).range(1..=8))
                        .changed();
                });
                changed |= ui
                    .checkbox(&mut cfg.auto_pair, "输入括号/引号时自动补全")
                    .changed();
                ui.add_space(6.0);
                changed |= ui.checkbox(&mut cfg.auto_save, "自动保存").changed();
                ui.horizontal(|ui| {
                    ui.label("自动保存间隔");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut cfg.auto_save_interval)
                                .range(5..=600)
                                .suffix(" 秒"),
                        )
                        .changed();
                });
                ui.add_space(8.0);
                ui.label(RichText::new("写作模式（互斥）").strong());
                changed |= ui.checkbox(&mut cfg.source_mode, "源码模式").changed();
                changed |= ui.checkbox(&mut cfg.focus_mode, "专注模式").changed();
                changed |= ui.checkbox(&mut cfg.typewriter, "打字机模式").changed();
            }
            Tab::Files => {
                ui.label(RichText::new("文件").strong());
                ui.separator();
                let mut ws = cfg.workspace.clone().unwrap_or_default();
                ui.label("默认工作区");
                ui.horizontal(|ui| {
                    if ui.text_edit_singleline(&mut ws).changed() {
                        cfg.workspace = if ws.trim().is_empty() {
                            None
                        } else {
                            Some(ws.clone())
                        };
                        changed = true;
                    }
                    if ui.button("选择…").clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            ws = p.to_string_lossy().to_string();
                            cfg.workspace = Some(ws.clone());
                            changed = true;
                        }
                    }
                });
                ui.add_space(6.0);
                let last = cfg
                    .last_file
                    .clone()
                    .unwrap_or_else(|| "（无）".to_string());
                ui.label(format!("上次打开：{last}"));
            }
            Tab::Images => {
                ui.label(RichText::new("图像").strong());
                ui.separator();
                changed |= ui
                    .checkbox(&mut cfg.insert_image_to_assets, "插入图片时复制到资源目录")
                    .changed();
                ui.horizontal(|ui| {
                    ui.label("资源目录名");
                    changed |= ui.text_edit_singleline(&mut cfg.image_assets_dir).changed();
                });
                changed |= ui
                    .checkbox(&mut cfg.image_relative, "使用相对路径引用图片")
                    .changed();
            }
            Tab::Markdown => {
                ui.label(RichText::new("Markdown").strong());
                ui.separator();
                changed |= ui
                    .checkbox(&mut cfg.math_enabled, "数学公式 $…$ / $$…$$")
                    .changed();
                changed |= ui
                    .checkbox(&mut cfg.mermaid_enabled, "Mermaid / 流程图")
                    .changed();
                changed |= ui.checkbox(&mut cfg.toc_enabled, "目录 [TOC]").changed();
                changed |= ui.checkbox(&mut cfg.footnote_enabled, "脚注").changed();
                ui.add_space(6.0);
                changed |= ui
                    .checkbox(&mut cfg.highlight_code, "代码块语法高亮")
                    .changed();
                changed |= ui
                    .checkbox(&mut cfg.auto_number_heading, "标题自动编号（预览）")
                    .changed();
            }
            Tab::Export => {
                ui.label(RichText::new("导出").strong());
                ui.separator();
                changed |= ui
                    .checkbox(&mut cfg.export_embed_images, "导出 HTML 时内嵌图片")
                    .changed();
                changed |= ui
                    .checkbox(&mut cfg.export_open_after, "导出完成后打开文件")
                    .changed();
                ui.add_space(6.0);
                ui.label(RichText::new("PDF 由浏览器打印生成：先在 Markd 里导出 HTML，再用浏览器「打印 → 存储为 PDF」。").small());
            }
        }
        changed
    }
}
