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
    /// 后台「测试上传」的结果通道：网络请求不能阻塞 UI 线程。
    host_test: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    host_test_status: String,
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
                                changed |= self.body(ui, tab, cfg, theme);
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

    fn body(&mut self, ui: &mut egui::Ui, tab: Tab, cfg: &mut Config, theme: &Theme) -> bool {
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
                changed |= ui
                    .checkbox(&mut cfg.show_line_numbers, "显示行号栏")
                    .on_hover_text("源码模式与混合模式都会显示行号；关闭后正文占满整个编辑区")
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

                ui.add_space(10.0);
                ui.separator();
                ui.label(RichText::new("图床（上传后文档可直接在线阅读）").strong());
                ui.horizontal(|ui| {
                    ui.label("平台");
                    let mut kind = cfg.image_host;
                    egui::ComboBox::from_id_salt("settings_image_host")
                        .selected_text(kind.label())
                        .show_ui(ui, |ui| {
                            for k in crate::image_host::ImageHostKind::ALL {
                                if ui.selectable_value(&mut kind, k, k.label()).clicked() {
                                    cfg.image_host = k;
                                    changed = true;
                                }
                            }
                        });
                });
                if cfg.image_host.is_enabled() {
                    // 每个字段单独一行：标签右对齐 + 输入框统一宽度，
                    // 避免两列长短不一挤在一起。
                    let w = 300.0;
                    egui::Grid::new("settings_image_host_grid")
                        .num_columns(2)
                        .min_col_width(52.0)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            let label = |ui: &mut egui::Ui, text: &str| {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(text);
                                    },
                                );
                            };

                            label(ui, "用户名");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut cfg.image_host_owner)
                                        .desired_width(w),
                                )
                                .changed();
                            ui.end_row();

                            label(ui, "仓库");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut cfg.image_host_repo)
                                        .desired_width(w),
                                )
                                .changed();
                            ui.end_row();

                            label(ui, "分支");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut cfg.image_host_branch)
                                        .desired_width(w),
                                )
                                .changed();
                            ui.end_row();

                            label(ui, "目录");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut cfg.image_host_dir)
                                        .desired_width(w),
                                )
                                .changed();
                            ui.end_row();

                            label(ui, "保存路径");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut cfg.image_host_path)
                                        .desired_width(w)
                                        .hint_text("{year}/{month}/{day}"),
                                )
                                .changed();
                            ui.end_row();

                            label(ui, "令牌");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut cfg.image_host_token)
                                        .password(true)
                                        .desired_width(w)
                                        .hint_text("Gitee 私人令牌"),
                                )
                                .changed();
                            ui.end_row();
                        });
                    let host = crate::image_host::ImageHostConfig::from_config(cfg);
                    ui.label(
                        RichText::new(
                            "保存路径支持 {year} {month} {day} {hour} {minute}，\
                             默认按年月日分文件夹；留空表示直接放在目录下。",
                        )
                        .small(),
                    );
                    ui.label(
                        RichText::new(format!("示例：{}", crate::image_host::path_preview(&host)))
                            .small()
                            .color(theme.dim),
                    );
                    ui.label(
                        RichText::new(
                            "令牌只保存在本地 ~/.markd/config.json，请勿提交到仓库。\
                             Gitee 令牌在「设置 → 私人令牌」生成，需要 projects 权限。",
                        )
                        .small(),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("测试上传").clicked() {
                            let host = crate::image_host::ImageHostConfig::from_config(cfg);
                            let (tx, rx) = std::sync::mpsc::channel();
                            self.host_test = Some(rx);
                            self.host_test_status = "正在上传测试图片…".to_string();
                            std::thread::spawn(move || {
                                let _ = tx.send(crate::image_host::test_upload(&host));
                            });
                        }
                        let polled = self.host_test.as_ref().map(|rx| rx.try_recv());
                        match polled {
                            Some(Ok(Ok(url))) => {
                                self.host_test = None;
                                self.host_test_status = format!("上传成功：{url}");
                            }
                            Some(Ok(Err(e))) => {
                                self.host_test = None;
                                self.host_test_status = format!("上传失败：{e}");
                            }
                            Some(Err(std::sync::mpsc::TryRecvError::Empty)) => {
                                ui.ctx()
                                    .request_repaint_after(std::time::Duration::from_millis(150));
                            }
                            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                                self.host_test = None;
                            }
                            None => {}
                        }
                        if !self.host_test_status.is_empty() {
                            ui.label(RichText::new(&self.host_test_status).small());
                        }
                    });
                    ui.label(
                        RichText::new(
                            "插入图片会先显示本地副本，上传完成后自动替换为公网 URL；\
                             已有文档可用「插入 → 上传本地图片」批量上传。",
                        )
                        .small(),
                    );
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Id, Pos2, Rect, UiBuilder};

    /// 渲染设置窗口若干帧（让 Window/ScrollArea 布局稳定），返回最后一帧的形状。
    fn render(tab: Tab, cfg: &mut Config) -> Vec<egui::epaint::ClippedShape> {
        let mut s = Settings::default();
        s.open = true;
        s.tab = Some(tab);
        let theme = Theme::get(ThemeKind::Light);
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for i in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 800.0))),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |root| {
                let mut ui = egui::Ui::new(
                    root.clone(),
                    Id::unique(("settings_test", i)),
                    UiBuilder::new(),
                );
                let ctx = ui.ctx().clone();
                let _ = s.show(&ctx, &theme, cfg);
            });
            shapes = std::mem::take(&mut out.shapes);
            out.textures_delta.clear();
        }
        shapes
    }

    fn find_y(shapes: &[egui::epaint::ClippedShape], name: &str) -> Option<f32> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos.y),
            _ => None,
        })
    }

    /// 图床各字段（用户名/仓库/分支/目录/令牌）应各占一行。
    #[test]
    fn image_host_fields_are_on_separate_rows() {
        let mut cfg = Config::default();
        cfg.image_host = crate::image_host::ImageHostKind::Gitee;
        let shapes = render(Tab::Images, &mut cfg);
        let ys: Vec<f32> = ["用户名", "仓库", "分支", "目录", "保存路径", "令牌"]
            .iter()
            .filter_map(|n| find_y(&shapes, n))
            .collect();
        assert_eq!(ys.len(), 6, "未找到全部图床字段标签：{ys:?}");
        for w in ys.windows(2) {
            assert!(w[1] - w[0] > 10.0, "字段应各占一行：{ys:?}");
        }
    }

    /// 编辑器设置里应提供「显示行号栏」开关（用户可自行关闭行号）。
    #[test]
    fn editor_tab_has_line_number_toggle() {
        let mut cfg = Config::default();
        let shapes = render(Tab::Editor, &mut cfg);
        assert!(
            find_y(&shapes, "显示行号栏").is_some(),
            "编辑器设置应包含行号栏开关"
        );
    }
}
