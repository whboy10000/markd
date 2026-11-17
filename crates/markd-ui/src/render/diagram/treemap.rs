//! Mermaid 矩形树图（treemap-beta）。

use super::*;

pub(crate) struct TreeItem {
    pub(crate) label: String,
    pub(crate) value: f32,
    pub(crate) parent: Option<usize>,
    pub(crate) children: Vec<usize>,
    pub(crate) root: usize,
}

pub(crate) fn parse_treemap(code: &str) -> (String, Vec<TreeItem>) {
    let mut title = String::new();
    let mut items: Vec<TreeItem> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.to_lowercase().starts_with("treemap-beta") {
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            title = t.trim().to_string();
            continue;
        }
        if let Some((name, value)) = l.split_once(':') {
            let label = name.trim().trim_matches('"').to_string();
            let value: f32 = value.trim().parse().unwrap_or(0.0);
            if label.is_empty() || value <= 0.0 {
                continue;
            }
            let indent: usize = raw
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum();
            while stack.last().is_some_and(|(i, _)| *i >= indent) {
                stack.pop();
            }
            let parent = stack.last().map(|&(_, idx)| idx);
            let root = parent.map_or(items.len(), |p| items[p].root);
            items.push(TreeItem {
                label,
                value,
                parent,
                children: Vec::new(),
                root,
            });
            let idx = items.len() - 1;
            if let Some(p) = parent {
                items[p].children.push(idx);
            }
            stack.push((indent, idx));
        }
    }
    (title, items)
}

fn node_value(idx: usize, items: &[TreeItem]) -> f32 {
    if items[idx].children.is_empty() {
        items[idx].value.max(0.0)
    } else {
        items[idx]
            .children
            .iter()
            .map(|&c| node_value(c, items))
            .sum::<f32>()
            .max(0.01)
    }
}

fn layout_items(
    list: &[usize],
    rect: Rect,
    items: &[TreeItem],
    values: &[f32],
    root: usize,
    out: &mut Vec<(usize, Rect, bool, usize)>,
) {
    if list.is_empty() {
        return;
    }
    if list.len() == 1 {
        let idx = list[0];
        let has_children = !items[idx].children.is_empty();
        out.push((idx, rect, has_children, root));
        if has_children {
            let header = 18.0_f32.min(rect.height() * 0.4);
            let inner = Rect::from_min_max(
                Pos2::new(rect.left() + 3.0, rect.top() + header),
                Pos2::new(rect.right() - 3.0, rect.bottom() - 3.0),
            );
            layout_items(&items[idx].children, inner, items, values, root, out);
        }
        return;
    }
    let total: f32 = list.iter().map(|&i| values[i]).sum();
    let total = if total <= 0.0 { 1.0 } else { total };
    let mut acc = 0.0;
    let mut cut = list.len() / 2;
    for (i, &idx) in list.iter().enumerate() {
        acc += values[idx];
        if acc >= total / 2.0 {
            cut = (i + 1).min(list.len() - 1).max(1);
            break;
        }
    }
    let (a, b) = list.split_at(cut);
    let a_total: f32 = a.iter().map(|&i| values[i]).sum();
    let frac = (a_total / total).clamp(0.05, 0.95);
    if rect.width() >= rect.height() {
        let x = rect.left() + rect.width() * frac;
        layout_items(
            a,
            Rect::from_min_max(rect.min, Pos2::new(x, rect.bottom())),
            items,
            values,
            root,
            out,
        );
        layout_items(
            b,
            Rect::from_min_max(Pos2::new(x, rect.top()), rect.max),
            items,
            values,
            root,
            out,
        );
    } else {
        let y = rect.top() + rect.height() * frac;
        layout_items(
            a,
            Rect::from_min_max(rect.min, Pos2::new(rect.right(), y)),
            items,
            values,
            root,
            out,
        );
        layout_items(
            b,
            Rect::from_min_max(Pos2::new(rect.left(), y), rect.max),
            items,
            values,
            root,
            out,
        );
    }
}

pub(crate) fn draw_treemap(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (title, items) = parse_treemap(code);
    let roots: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, n)| n.parent.is_none())
        .map(|(i, _)| i)
        .collect();
    if roots.is_empty() {
        return raw_code(ui, code, theme);
    }
    let values: Vec<f32> = (0..items.len()).map(|i| node_value(i, &items)).collect();
    let avail = ui.available_width().max(320.0).min(900.0);
    let title_h = if title.is_empty() { 0.0 } else { 22.0 };
    let h = title_h + 240.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail, h), Sense::click());
    let p = ui.painter();
    if !title.is_empty() {
        p.text(
            Pos2::new(rect.center().x, rect.top() + 2.0),
            Align2::CENTER_TOP,
            &title,
            FontId::proportional(13.0),
            theme.heading,
        );
    }
    let area = Rect::from_min_max(
        Pos2::new(rect.left() + 2.0, rect.top() + title_h + 4.0),
        Pos2::new(rect.right() - 2.0, rect.bottom() - 4.0),
    );
    let mut placed: Vec<(usize, Rect, bool, usize)> = Vec::new();
    // 每个顶层节点用自己的配色：先按顶层分组再布局，保证同组同色。
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    for &r in &roots {
        groups.push((r, vec![r]));
    }
    // 顶层节点整体按比例排布。
    let group_vals: Vec<f32> = roots.iter().map(|&r| values[r]).collect();
    let order: Vec<usize> = (0..roots.len()).collect();
    let top_rects = split_rects(&order, &group_vals, area);
    for (gi, (root, _)) in groups.iter().enumerate() {
        if let Some((_, r)) = top_rects.iter().find(|(i, _)| *i == gi) {
            let idx = *root;
            let has_children = !items[idx].children.is_empty();
            placed.push((idx, *r, has_children, gi));
            if has_children {
                let header = 18.0_f32.min(r.height() * 0.4);
                let inner = Rect::from_min_max(
                    Pos2::new(r.left() + 3.0, r.top() + header),
                    Pos2::new(r.right() - 3.0, r.bottom() - 3.0),
                );
                layout_items(&items[idx].children, inner, &items, &values, gi, &mut placed);
            }
        }
    }
    for (idx, r, is_container, color) in &placed {
        let base = palette_color(theme, *color);
        let fill = if *is_container {
            base.gamma_multiply(0.75)
        } else {
            base
        };
        p.rect_filled(*r, 3.0, fill);
        p.rect_stroke(
            *r,
            3.0,
            Stroke::new(1.0, theme.border),
            egui::StrokeKind::Inside,
        );
        let g = ui.fonts_mut(|f| {
            f.layout(
                items[*idx].label.clone(),
                FontId::proportional(10.5),
                theme.text,
                (r.width() - 8.0).max(10.0),
            )
        });
        p.with_clip_rect(*r).galley(
            Pos2::new(r.left() + 4.0, r.top() + 3.0),
            g,
            theme.text,
        );
        if !is_container {
            p.with_clip_rect(*r).text(
                Pos2::new(r.left() + 4.0, r.top() + 17.0),
                Align2::LEFT_TOP,
                format!("{:.0}", values[*idx]),
                FontId::proportional(9.5),
                theme.dim,
            );
        }
    }
    resp
}

/// 把 [0, n) 按数值比例切成矩形（二分递归），与 `layout_items` 同一套逻辑，
/// 用于顶层分组，返回 (下标, 矩形)。
fn split_rects(order: &[usize], values: &[f32], rect: Rect) -> Vec<(usize, Rect)> {
    let mut out = Vec::new();
    fn rec(
        list: &[usize],
        values: &[f32],
        rect: Rect,
        out: &mut Vec<(usize, Rect)>,
    ) {
        if list.is_empty() {
            return;
        }
        if list.len() == 1 {
            out.push((list[0], rect));
            return;
        }
        let total: f32 = list.iter().map(|&i| values[i]).sum::<f32>().max(0.01);
        let mut acc = 0.0;
        let mut cut = list.len() / 2;
        for (i, &idx) in list.iter().enumerate() {
            acc += values[idx];
            if acc >= total / 2.0 {
                cut = (i + 1).min(list.len() - 1).max(1);
                break;
            }
        }
        let (a, b) = list.split_at(cut);
        let frac = (a.iter().map(|&i| values[i]).sum::<f32>() / total).clamp(0.05, 0.95);
        if rect.width() >= rect.height() {
            let x = rect.left() + rect.width() * frac;
            rec(a, values, Rect::from_min_max(rect.min, Pos2::new(x, rect.bottom())), out);
            rec(b, values, Rect::from_min_max(Pos2::new(x, rect.top()), rect.max), out);
        } else {
            let y = rect.top() + rect.height() * frac;
            rec(a, values, Rect::from_min_max(rect.min, Pos2::new(rect.right(), y)), out);
            rec(b, values, Rect::from_min_max(Pos2::new(rect.left(), y), rect.max), out);
        }
    }
    rec(order, values, rect, &mut out);
    out
}
