//! Author: aurelianus <aurelianus@126.com>

//! 侧边栏：文件树与大纲面板。

use crate::document::OutlineItem;
use crate::theme::Theme;
use crate::workspace::FileNode;
use egui::{self, RichText, Sense};
use std::collections::HashSet;
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
    icons: &mut crate::icons::IconCache,
    actions: &mut Vec<SideAction>,
) {
    fn walk(
        ui: &mut egui::Ui,
        nodes: &mut [FileNode],
        current: Option<&Path>,
        theme: &Theme,
        icons: &mut crate::icons::IconCache,
        actions: &mut Vec<SideAction>,
        depth: usize,
    ) {
        let full_w = ui.available_width();
        for n in nodes.iter_mut() {
            let selected = current.map(|c| c == n.path.as_path()).unwrap_or(false);
            if n.is_dir {
                let row = ui
                    .horizontal(|ui| {
                        ui.set_min_width(full_w.max(60.0));
                        ui.add_space(depth as f32 * 14.0);
                        // 展开箭头：自绘三角形，固定占一条与图标同高的窄带，
                        // 保证箭头始终垂直居中于图标（文字箭头基线不稳、且偏小）。
                        let (arrow_rect, _) = ui.allocate_exact_size(
                            egui::vec2(12.0, crate::icons::SIZE),
                            Sense::hover(),
                        );
                        let c = arrow_rect.center();
                        let (dx, dy) = (5.0, 4.0);
                        let pts: Vec<egui::Pos2> = if n.expanded {
                            // 向下：上边两个角，下尖一个角
                            vec![
                                egui::pos2(c.x - dx, c.y - dy),
                                egui::pos2(c.x + dx, c.y - dy),
                                egui::pos2(c.x, c.y + dy),
                            ]
                        } else {
                            // 向右：左边两个角，右尖一个角
                            vec![
                                egui::pos2(c.x - dy, c.y - dx),
                                egui::pos2(c.x - dy, c.y + dx),
                                egui::pos2(c.x + dy, c.y),
                            ]
                        };
                        ui.painter().add(egui::Shape::convex_polygon(
                            pts,
                            theme.dim,
                            egui::Stroke::NONE,
                        ));
                        let icon = crate::icons::folder_icon(&n.name, n.expanded);
                        icons.show(ui, &icon, crate::icons::SIZE);
                        ui.add(
                            egui::Label::new(RichText::new(&n.name).color(theme.text).strong())
                                .selectable(false),
                        );
                    })
                    .response
                    .interact(Sense::click())
                    .on_hover_cursor(egui::CursorIcon::Default);
                if row.clicked() {
                    actions.push(SideAction::ToggleDir(n.path.clone()));
                }
                if n.expanded {
                    walk(
                        ui,
                        &mut n.children,
                        current,
                        theme,
                        icons,
                        actions,
                        depth + 1,
                    );
                }
            } else {
                let icon = crate::icons::file_icon(&n.name);
                let row = ui
                    .horizontal(|ui| {
                        ui.set_min_width(full_w.max(60.0));
                        ui.add_space(depth as f32 * 14.0 + 14.0);
                        icons.show(ui, icon, crate::icons::SIZE);
                        let mut r = RichText::new(&n.name);
                        if selected {
                            r = r.strong().color(theme.accent);
                        } else {
                            r = r.color(theme.text);
                        }
                        ui.add(egui::Label::new(r).selectable(false));
                    })
                    .response
                    .interact(Sense::click())
                    .on_hover_cursor(egui::CursorIcon::Default);
                if row.clicked() {
                    actions.push(SideAction::Open(n.path.clone()));
                }
            }
        }
    }
    walk(ui, nodes, current, theme, icons, actions, 0);
}

/// 计算大纲每个条目的可见性：被折叠标题的所有下级条目（level 更大的连续段）隐藏。
fn outline_visibility(outline: &[OutlineItem], folded: &HashSet<usize>) -> Vec<bool> {
    let mut vis = Vec::with_capacity(outline.len());
    let mut hidden_under: Option<u8> = None;
    for (i, item) in outline.iter().enumerate() {
        if let Some(lv) = hidden_under {
            if item.level > lv {
                vis.push(false);
                continue;
            }
            hidden_under = None;
        }
        vis.push(true);
        if folded.contains(&i) {
            hidden_under = Some(item.level);
        }
    }
    vis
}

pub fn outline_panel(
    ui: &mut egui::Ui,
    outline: &[OutlineItem],
    theme: &Theme,
    actions: &mut Vec<SideAction>,
    folded: &std::collections::HashSet<usize>,
    fold_toggles: &mut Vec<usize>,
) {
    if outline.is_empty() {
        ui.label(
            RichText::new("（暂无标题）")
                .size(theme.body - 1.0)
                .color(theme.dim),
        );
        return;
    }
    // 大纲和文件树共用侧栏宽度：显式撑满可用宽度，避免条目短时面板看起来更窄。
    let full_w = ui.available_width();
    let vis = outline_visibility(outline, folded);
    for (i, item) in outline.iter().enumerate() {
        if !vis[i] {
            continue;
        }
        let has_child = outline
            .get(i + 1)
            .map(|n| n.level > item.level)
            .unwrap_or(false);
        let is_folded = folded.contains(&i);
        let indent = ((item.level.saturating_sub(1)) as f32) * 13.0;
        let mut label_resp: Option<egui::Response> = None;
        ui.horizontal(|ui| {
            ui.set_min_width(full_w.max(60.0));
            ui.add_space(indent);
            if has_child {
                let arrow = if is_folded { "▶" } else { "▼" };
                let a = ui
                    .add(
                        egui::Label::new(RichText::new(arrow).small().color(theme.dim))
                            .sense(Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if is_folded {
                        "展开小节"
                    } else {
                        "折叠小节"
                    });
                if a.clicked() {
                    fold_toggles.push(i);
                }
            } else {
                ui.add_space(12.0);
            }
            label_resp = Some(
                ui.add(
                    egui::Label::new(
                        RichText::new(item.title.clone())
                            .size(theme.body - 1.0)
                            .color(theme.text),
                    )
                    .selectable(false)
                    .sense(Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::Default),
            );
        });
        if let Some(lr) = label_resp {
            if lr.clicked() {
                actions.push(SideAction::Jump(item.block));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // OutlineItem / HashSet 由 use super::* 带入

    fn item(level: u8, title: &str, block: usize) -> OutlineItem {
        OutlineItem {
            level,
            title: title.to_string(),
            block,
        }
    }

    /// 折叠某个标题后，它的所有下级条目都隐藏，其余条目照常显示。
    #[test]
    fn outline_fold_hides_children_only() {
        let outline = vec![
            item(1, "一章", 0),
            item(2, "1.1", 1),
            item(2, "1.2", 2),
            item(1, "二章", 3),
            item(2, "2.1", 4),
        ];
        let mut folded = HashSet::new();
        folded.insert(0);
        let vis = outline_visibility(&outline, &folded);
        assert_eq!(vis, vec![true, false, false, true, true]);
        folded.insert(3);
        let vis = outline_visibility(&outline, &folded);
        assert_eq!(vis, vec![true, false, false, true, false]);
        folded.clear();
        assert!(outline_visibility(&outline, &folded).iter().all(|v| *v));
    }
}
