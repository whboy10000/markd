//! 侧边栏：文件树与大纲面板。

use crate::document::OutlineItem;
use crate::theme::Theme;
use crate::workspace::FileNode;
use egui::{self, RichText, Sense};
use std::path::{Path, PathBuf};

pub enum SideAction {
    Open(PathBuf),
    ToggleDir(PathBuf),
    Jump(usize),
}

pub fn file_tree(
    ui: &mut egui::Ui,
    nodes: &mut [FileNode],
    current: Option<&Path>,
    theme: &Theme,
    actions: &mut Vec<SideAction>,
) {
    fn walk(
        ui: &mut egui::Ui,
        nodes: &mut [FileNode],
        current: Option<&Path>,
        theme: &Theme,
        actions: &mut Vec<SideAction>,
        depth: usize,
    ) {
        for n in nodes.iter_mut() {
            let selected = current.map(|c| c == n.path.as_path()).unwrap_or(false);
            if n.is_dir {
                let resp = ui
                    .horizontal(|ui| {
                        ui.add_space(depth as f32 * 11.0);
                        let icon = if n.expanded { "▾" } else { "▸" };
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{}  {}", icon, n.name))
                                    .color(theme.text)
                                    .strong(),
                            )
                            .selectable(false)
                            .sense(Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::Default)
                    })
                    .inner;
                if resp.clicked() {
                    actions.push(SideAction::ToggleDir(n.path.clone()));
                }
                if n.expanded {
                    walk(ui, &mut n.children, current, theme, actions, depth + 1);
                }
            } else {
                let resp = ui
                    .horizontal(|ui| {
                        ui.add_space(depth as f32 * 11.0 + 12.0);
                        let mut r = RichText::new(format!("·  {}", n.name));
                        if selected {
                            r = r.strong().color(theme.accent);
                        } else {
                            r = r.color(theme.text);
                        }
                        ui.add(egui::Label::new(r).selectable(false).sense(Sense::click()))
                            .on_hover_cursor(egui::CursorIcon::Default)
                    })
                    .inner;
                if resp.clicked() {
                    actions.push(SideAction::Open(n.path.clone()));
                }
            }
        }
    }
    walk(ui, nodes, current, theme, actions, 0);
}

pub fn outline_panel(
    ui: &mut egui::Ui,
    outline: &[OutlineItem],
    theme: &Theme,
    actions: &mut Vec<SideAction>,
) {
    if outline.is_empty() {
        ui.label(
            RichText::new("（暂无标题）")
                .size(theme.body + 2.0)
                .color(theme.dim),
        );
        return;
    }
    // 大纲和文件树共用侧栏宽度：显式撑满可用宽度，避免条目短时面板看起来更窄。
    let full_w = ui.available_width();
    for item in outline {
        let indent = (item.level.saturating_sub(1) as f32) * 13.0;
        let resp = ui
            .horizontal(|ui| {
                ui.set_min_width(full_w.max(60.0));
                ui.add_space(indent);
                ui.add(
                    egui::Label::new(
                        RichText::new(item.title.clone())
                            .size(theme.body + 2.0)
                            .color(theme.text),
                    )
                    .selectable(false)
                    .sense(Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::Default)
            })
            .inner;
        if resp.clicked() {
            actions.push(SideAction::Jump(item.block));
        }
    }
}
