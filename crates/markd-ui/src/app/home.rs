//! 首页：欢迎页与最近打开。

use super::*;

impl MarkdApp {
    /// 首页：欢迎信息 + 三个入口 + 最近打开列表。
    pub(super) fn home_view(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<HomeAction> {
        let mut act = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let w = ui.available_width().min(620.0);
                let pad = ((ui.available_width() - w) / 2.0).max(0.0);
                ui.add_space(56.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(pad);
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        ui.label(
                            RichText::new("Markd")
                                .size(36.0)
                                .strong()
                                .color(self.theme.text),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("纯 Rust 实现的 Markdown 编辑器")
                                .size(13.0)
                                .color(self.theme.dim),
                        );
                        ui.add_space(22.0);
                        ui.horizontal(|ui| {
                            if ui.button("新建文档").clicked() {
                                act = Some(HomeAction::New);
                            }
                            if ui.button("打开文件…").clicked() {
                                act = Some(HomeAction::OpenFile);
                            }
                            if ui.button("打开工作区…").clicked() {
                                act = Some(HomeAction::OpenWorkspace);
                            }
                        });
                        ui.add_space(30.0);
                        ui.label(RichText::new("最近打开").color(self.theme.dim));
                        ui.add_space(8.0);
                        if self.config.recent.is_empty() {
                            ui.label(RichText::new("还没有打开过文件").color(self.theme.dim));
                        }
                        for p in self.config.recent.clone() {
                            let path = PathBuf::from(&p);
                            let exists = path.is_file();
                            let name = path
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| p.clone());
                            let dir = path
                                .parent()
                                .map(|d| d.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let color = if exists {
                                self.theme.text
                            } else {
                                self.theme.dim
                            };
                            let row = ui
                                .horizontal(|ui| {
                                    let r = ui.add(
                                        egui::Label::new(RichText::new(name).color(color))
                                            .sense(egui::Sense::click()),
                                    );
                                    ui.label(RichText::new(dir).small().color(self.theme.dim));
                                    r
                                })
                                .inner;
                            if row.hovered() && exists {
                                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if row.clicked() && exists {
                                act = Some(HomeAction::Recent(path.clone()));
                            }
                            ui.add_space(3.0);
                        }
                    });
                });
            });
        act
    }
}
