//! 主界面：左侧新建/打开 + 搜索 + 最近文档。

use super::*;

/// 左侧导航栏宽度，参考图里约 130pt。
const RAIL_WIDTH: f32 = 132.0;
/// 参考图里的主按钮绿色。
const PRIMARY_GREEN: egui::Color32 = egui::Color32::from_rgb(16, 174, 96);

fn format_size(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.2} KB", n as f64 / 1024.0)
    } else {
        format!("{:.2} MB", n as f64 / (1024.0 * 1024.0))
    }
}

struct RecentRow {
    path: PathBuf,
    name: String,
    date: String,
    size: String,
    bytes: u64,
}

impl MarkdApp {
    /// 主界面：只保留新建、打开、搜索与最近文档。
    pub(super) fn home_view(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<HomeAction> {
        let mut act = None;
        let mut sort = self.home_sort;
        let mut search = std::mem::take(&mut self.home_search);
        let recent = self.config.recent.clone();
        let theme = &self.theme;

        let rail_height = ui.available_height();
        ui.horizontal_top(|ui| {
            // ---------------- 左侧：新建 / 打开 ----------------
            ui.allocate_ui_with_layout(
                egui::vec2(RAIL_WIDTH, rail_height),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.add_space(14.0);
                    if ui
                        .add_sized(
                            egui::vec2(112.0, 32.0),
                            egui::Button::new(
                                RichText::new("＋   新建")
                                    .size(17.0)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(PRIMARY_GREEN),
                        )
                        .clicked()
                    {
                        act = Some(HomeAction::New);
                    }
                    ui.add_space(6.0);
                    if ui
                        .add_sized(
                            egui::vec2(112.0, 30.0),
                            egui::Button::new(RichText::new("打开").size(16.0)),
                        )
                        .clicked()
                    {
                        act = Some(HomeAction::OpenFile);
                    }
                },
            );
            ui.separator();

            // ---------------- 右侧：搜索 + 最近文档 ----------------
            // 钉死宽度：ScrollArea 在横向布局里会按内容无限撑开。
            let content_w = ui.available_width();
            ui.allocate_ui_with_layout(
                egui::vec2(content_w, rail_height),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut search)
                                        .hint_text(RichText::new("搜索文件、模板…").size(15.0))
                                        .desired_width(420.0),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new("⌄  筛选").size(15.0).color(theme.dim),
                                        );
                                    },
                                );
                            });
                            ui.add_space(28.0);

                            ui.label(
                                RichText::new("最近文档")
                                    .size(17.0)
                                    .strong()
                                    .color(theme.text),
                            );
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.add_space(430.0);
                                for (label, kind) in
                                    [("来源", 3u8), ("打开时间排序", 0), ("文件大小", 2)]
                                {
                                    if ui
                                        .add_sized(
                                            egui::vec2(90.0, 20.0),
                                            egui::Button::new(
                                                RichText::new(label).size(15.0).color(theme.dim),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        sort = kind;
                                    }
                                }
                            });
                            ui.add_space(4.0);
                            ui.separator();

                            let mut rows: Vec<RecentRow> = Vec::new();
                            let needle = search.trim().to_lowercase();
                            for p in &recent {
                                let path = PathBuf::from(p);
                                let name = path
                                    .file_name()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_else(|| p.clone());
                                if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
                                    continue;
                                }
                                let meta = std::fs::metadata(&path).ok();
                                let bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                                let date = meta
                                    .as_ref()
                                    .and_then(|m| m.modified().ok())
                                    .map(|t| {
                                        chrono::DateTime::<chrono::Local>::from(t)
                                            .format("%Y/%m/%d")
                                            .to_string()
                                    })
                                    .unwrap_or_else(|| "—".to_string());
                                rows.push(RecentRow {
                                    path,
                                    name,
                                    date,
                                    size: format_size(bytes),
                                    bytes,
                                });
                            }
                            match sort {
                                1 => rows.sort_by(|a, b| a.name.cmp(&b.name)),
                                2 => rows.sort_by(|a, b| b.bytes.cmp(&a.bytes)),
                                // 日期是 YYYY/MM/DD，字符串倒序即时间倒序。
                                _ => rows.sort_by(|a, b| b.date.cmp(&a.date)),
                            }
                            if rows.is_empty() {
                                ui.add_space(18.0);
                                ui.label(
                                    RichText::new(if needle.is_empty() {
                                        "还没有打开过文件"
                                    } else {
                                        "没有匹配的文件"
                                    })
                                    .color(theme.dim),
                                );
                            }
                            for row in &rows {
                                let exists = row.path.is_file();
                                ui.horizontal(|ui| {
                                    ui.add_space(4.0);
                                    let icon_at = ui.cursor().min;
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_size(icon_at, egui::vec2(18.0, 18.0)),
                                        egui::CornerRadius::same(3),
                                        PRIMARY_GREEN,
                                    );
                                    ui.painter().text(
                                        icon_at + egui::vec2(9.0, 9.0),
                                        egui::Align2::CENTER_CENTER,
                                        "M",
                                        egui::FontId::proportional(11.0),
                                        egui::Color32::WHITE,
                                    );
                                    ui.add_space(22.0);
                                    let color = if exists { theme.text } else { theme.dim };
                                    let name = ui.add(
                                        egui::Label::new(
                                            RichText::new(&row.name).size(16.0).color(color),
                                        )
                                        .sense(egui::Sense::click()),
                                    );
                                    if name.hovered() && exists {
                                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    if name.clicked() && exists {
                                        act = Some(HomeAction::Recent(row.path.clone()));
                                    }
                                    ui.add_space(
                                        (420.0 - row.name.chars().count() as f32 * 7.0).max(0.0),
                                    );
                                    ui.label(RichText::new("本地文件").size(14.0).color(theme.dim));
                                    ui.add_space(90.0);
                                    ui.label(RichText::new(&row.date).size(14.0).color(theme.dim));
                                    ui.add_space(70.0);
                                    ui.label(RichText::new(&row.size).size(14.0).color(theme.dim));
                                    ui.add_space(24.0);
                                    ui.label(RichText::new("⋯").size(16.0).color(theme.dim));
                                });
                                ui.separator();
                            }
                        });
                },
            );
        });

        self.home_search = search;
        self.home_sort = sort;
        act
    }
}
