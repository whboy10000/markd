//! Author: aurelianus <aurelianus@126.com>

//! 侧边栏：文件树与大纲面板。

use crate::document::OutlineItem;
use crate::theme::Theme;
use crate::workspace::FileNode;
use egui::{self, RichText, Sense};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
pub enum SideAction {
    Open(PathBuf),
    ToggleDir(PathBuf),
    Jump(usize),
    /// 右键某行：请求显示上下文菜单（同时把侧栏切回文件列表页签）。
    Context(PathBuf),
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
                if row.secondary_clicked() {
                    actions.push(SideAction::Context(n.path.clone()));
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
                if row.secondary_clicked() {
                    actions.push(SideAction::Context(n.path.clone()));
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

/// 大纲面板：只排版视口内的行。
///
/// 1000 万字的文档有 6000+ 个标题，逐行全量排版实测一帧 23.5ms
/// （`MARKD_STRESS=1 cargo test stress_sidebar`），开着大纲页签就会掉帧。
/// 行高固定 + 标题单行截断，才能用 `ScrollArea::show_rows` 按视口取区间。
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
    // 被折叠隐藏的行不参与排版，所以先列出「行号 → 条目下标」。
    // 几千个布尔值的扫描是微秒级，比排版一行都便宜。
    let vis = outline_visibility(outline, folded);
    let rows: Vec<usize> = (0..outline.len()).filter(|&i| vis[i]).collect();
    let arrow_box = (theme.body + 4.0).max(20.0);
    // 行高取「正文字体行高」「箭头盒」「正文字号」里最大的那个：标题不换行，行里
    // 再没别的东西，这个值就是各行自然排版的高度，等步长定位因此不会累积错位。
    let row_h = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(arrow_box)
        .max(theme.body);
    egui::ScrollArea::vertical().show_rows(ui, row_h, rows.len(), |ui, range| {
        for &i in &rows[range] {
            let item = &outline[i];
            let has_child = outline
                .get(i + 1)
                .map(|n| n.level > item.level)
                .unwrap_or(false);
            let is_folded = folded.contains(&i);
            let indent = ((item.level.saturating_sub(1)) as f32) * 13.0;
            // 折叠箭头做成固定 20px 的点击盒：图标放大到正文字号，命中区域也从
            // 原来只有字形那么大扩到一整块，方便一眼看到、随手点中。
            let mut label_resp: Option<egui::Response> = None;
            ui.horizontal(|ui| {
                ui.set_min_width(full_w.max(60.0));
                ui.add_space(indent);
                if has_child {
                    let arrow = if is_folded { "▶" } else { "▼" };
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(arrow_box, arrow_box), Sense::click());
                    if ui.is_rect_visible(rect) {
                        if resp.hovered() {
                            ui.painter()
                                .rect_filled(rect.shrink(1.0), 3.0, theme.table_head);
                        }
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            arrow,
                            egui::FontId::proportional(theme.body),
                            if resp.hovered() {
                                theme.accent
                            } else {
                                theme.dim
                            },
                        );
                    }
                    let a = resp
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
                    ui.add_space(arrow_box);
                }
                label_resp = Some(
                    ui.add(
                        egui::Label::new(
                            RichText::new(item.title.clone())
                                .size(theme.body - 1.0)
                                .color(theme.text),
                        )
                        // 单行截断：换行会让行高不统一，视口定位就不准了。
                        // 被截断时 egui 自动在悬停提示里给出完整标题。
                        .truncate()
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
    });
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

    /// 抽出一帧里画出来的文本，按纵坐标排序：`(文本, 文本占据的矩形)`。
    /// Label 以 `rect.left_top()` 为基准绘制，所以 `pos + galley 尺寸` 就是它的命中区。
    fn painted_texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
        let mut out: Vec<(String, egui::Rect)> = shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::Text(t) => Some((
                    t.galley.text().to_string(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                )),
                _ => None,
            })
            .collect();
        out.sort_by(|a, b| a.1.center().y.total_cmp(&b.1.center().y));
        out
    }

    fn title_num(label: &str) -> usize {
        label["标题 ".len()..].parse().expect("标题序号")
    }

    /// 无头跑大纲面板：一帧一次 `run_ui`，返回画出的文本矩形与面板产生的动作。
    /// 时间必须逐帧推进：egui 的滚轮是跨帧平滑的，`time` 不动画面就永远停不稳。
    struct Harness {
        ctx: egui::Context,
        theme: crate::theme::Theme,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                ctx: egui::Context::default(),
                theme: crate::theme::Theme::get(crate::theme::ThemeKind::Light),
                time: 0.0,
            }
        }

        fn frame(
            &mut self,
            outline: &[OutlineItem],
            folded: &HashSet<usize>,
            events: Vec<egui::Event>,
        ) -> (Vec<(String, egui::Rect)>, Vec<SideAction>) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(220.0, 260.0),
                )),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let mut actions = Vec::new();
            let mut toggles = Vec::new();
            let Self { ctx, theme, .. } = self;
            let mut out = ctx.run_ui(input, |ui| {
                egui::Panel::left("sidebar")
                    .exact_size(220.0)
                    .show(ui, |ui| {
                        outline_panel(ui, outline, theme, &mut actions, folded, &mut toggles)
                    });
            });
            let texts = painted_texts(&out.shapes);
            out.textures_delta.clear();
            (texts, actions)
        }

        /// 滚 `dy` 像素并等画面停稳，返回稳定后的画面（`dy = 0` 表示只等不动）。
        fn scroll_and_settle(
            &mut self,
            outline: &[OutlineItem],
            folded: &HashSet<usize>,
            dy: f32,
        ) -> Vec<(String, egui::Rect)> {
            if dy != 0.0 {
                self.frame(
                    outline,
                    folded,
                    vec![
                        egui::Event::PointerMoved(egui::pos2(100.0, 150.0)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, dy),
                            phase: egui::TouchPhase::Move,
                            source: egui::MouseWheelSource::Wheel,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
            let mut prev = self.frame(outline, folded, Vec::new()).0;
            for _ in 0..600 {
                let next = self.frame(outline, folded, Vec::new()).0;
                if next == prev && !self.ctx.input(|i| i.is_scrolling()) {
                    return next;
                }
                prev = next;
            }
            panic!("滚动 600 帧后画面还没稳定");
        }

        /// 点一下（移动 + 按下 + 松开，同一帧）。
        fn click(
            &mut self,
            outline: &[OutlineItem],
            folded: &HashSet<usize>,
            pos: egui::Pos2,
        ) -> (Vec<(String, egui::Rect)>, Vec<SideAction>) {
            self.frame(
                outline,
                folded,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    },
                ],
            )
        }
    }

    /// 行内可点候选：屏幕上完整可见的标题行。
    fn clickable(shown: &[(String, egui::Rect)]) -> Vec<(String, egui::Rect)> {
        shown
            .iter()
            .filter(|(t, r)| t.starts_with("标题 ") && r.top() > 40.0 && r.bottom() < 240.0)
            .cloned()
            .collect()
    }

    /// 虚拟化后「看到的行 = 点得到的行」：500 个标题只排版视口内那几行，
    /// 每滚一屏点中间那行，拿到的块下标必须就是那一行的标题。
    /// 视口区间算错（偏移一行、行高与实际排版不符）都会在这里露出来。
    #[test]
    fn virtualized_outline_clicks_the_row_you_see() {
        let outline: Vec<OutlineItem> = (0..500)
            .map(|i| OutlineItem {
                level: 1,
                title: format!("标题 {i}"),
                block: i * 3,
            })
            .collect();
        let folded = HashSet::new();
        let mut h = Harness::new();

        let shown = h.scroll_and_settle(&outline, &folded, 0.0);
        let nums: Vec<usize> = shown
            .iter()
            .filter(|(t, _)| t.starts_with("标题 "))
            .map(|(t, _)| title_num(t))
            .collect();
        assert!(
            nums.len() >= 2 && nums.len() < 30,
            "只该排版视口内的行，实际 {} 行",
            nums.len()
        );
        assert!(
            nums.windows(2).all(|w| w[1] == w[0] + 1),
            "屏幕上的行应当连续：{nums:?}"
        );

        for round in 0..6 {
            let dy = if round == 0 { 0.0 } else { -600.0 };
            let shown = h.scroll_and_settle(&outline, &folded, dy);
            let cands = clickable(&shown);
            assert!(!cands.is_empty(), "第 {round} 轮没有可点的行");
            let (label, rect) = cands[cands.len() / 2].clone();
            let num = title_num(&label);
            let (again, actions) = h.click(&outline, &folded, rect.center());
            assert_eq!(again, shown, "第 {round} 轮画面还在动，行位置读不稳");
            assert_eq!(
                actions,
                vec![SideAction::Jump(num * 3)],
                "点「{label}」(y={:.1}) 应跳到块 {}",
                rect.center().y,
                num * 3
            );
        }
    }

    /// 折叠后行号要跳过隐藏行：每 10 个二级标题挂在一个一级标题下，折叠全部一级后
    /// 屏幕上必须只剩一级标题，且点谁跳谁的块。
    #[test]
    fn folded_outline_rows_map_to_the_right_items() {
        let outline: Vec<OutlineItem> = (0..300)
            .map(|i| OutlineItem {
                level: if i % 10 == 0 { 1 } else { 2 },
                title: format!("标题 {i}"),
                block: i * 3,
            })
            .collect();
        let folded: HashSet<usize> = (0..300).step_by(10).collect();
        let mut h = Harness::new();

        for round in 0..3 {
            let dy = if round == 0 { 0.0 } else { -300.0 };
            let shown = h.scroll_and_settle(&outline, &folded, dy);
            let nums: Vec<usize> = clickable(&shown)
                .iter()
                .map(|(t, _)| title_num(t))
                .collect();
            assert!(
                nums.iter().all(|n| n % 10 == 0),
                "折叠后只剩一级标题：{nums:?}"
            );
            assert!(
                nums.windows(2).all(|w| w[1] == w[0] + 10),
                "可见行该按 0,10,20… 连续：{nums:?}"
            );
            let (label, rect) = clickable(&shown).pop().expect("有可点的行");
            let num = title_num(&label);
            let (_, actions) = h.click(&outline, &folded, rect.center());
            assert_eq!(
                actions,
                vec![SideAction::Jump(num * 3)],
                "点「{label}」应跳到块 {}",
                num * 3
            );
        }
    }
}
