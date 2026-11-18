//! Mermaid 思维导图（mindmap）。

use super::*;

pub(crate) struct MindNode {
    pub(crate) label: String,
    pub(crate) depth: usize,
    pub(crate) parent: Option<usize>,
    pub(crate) branch: usize,
}

fn mindmap_label(line: &str) -> String {
    let l = line.trim();
    if let Some(i) = l.find("((")
        && let Some(j) = l[i + 2..].find("))")
    {
        return l[i + 2..i + 2 + j].trim().to_string();
    }
    for (open, close) in [('[', ']'), ('(', ')'), ('{', '}')] {
        if let Some(i) = l.find(open) {
            let rest = &l[i + 1..];
            if let Some(j) = rest.rfind(close) {
                return rest[..j].trim().trim_matches('"').to_string();
            }
        }
    }
    l.trim_matches('"').to_string()
}

/// 按缩进建树：缩进更深的行是上一行的子节点。
pub(crate) fn parse_mindmap(code: &str) -> Vec<MindNode> {
    let mut nodes: Vec<MindNode> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for raw in code.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("%%")
            || trimmed.eq_ignore_ascii_case("mindmap")
        {
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
        let branch = match parent {
            None => nodes.len(),
            Some(p) if nodes[p].parent.is_none() => nodes.len(),
            Some(p) => nodes[p].branch,
        };
        nodes.push(MindNode {
            label: mindmap_label(trimmed),
            depth: stack.len(),
            parent,
            branch,
        });
        stack.push((indent, nodes.len() - 1));
    }
    nodes
}

fn assign_y(idx: usize, children: &[Vec<usize>], y: &mut [f32], cursor: &mut f32, row_h: f32) {
    if children[idx].is_empty() {
        *cursor += row_h;
        y[idx] = *cursor - row_h / 2.0;
    } else {
        for &c in &children[idx] {
            assign_y(c, children, y, cursor, row_h);
        }
        let first = y[children[idx][0]];
        let last = y[*children[idx].last().expect("children 非空")];
        y[idx] = (first + last) / 2.0;
    }
}

pub(crate) fn draw_mindmap(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let nodes = parse_mindmap(code);
    if nodes.is_empty() {
        return raw_code(ui, code, theme);
    }
    let font = FontId::proportional(11.5);
    let galleys: Vec<_> = nodes
        .iter()
        .map(|n| ui.fonts_mut(|f| f.layout(n.label.clone(), font.clone(), theme.text, 160.0)))
        .collect();
    let sizes: Vec<Vec2> = galleys
        .iter()
        .map(|g| Vec2::new((g.size().x + 22.0).max(44.0), g.size().y + 12.0))
        .collect();
    let max_depth = nodes.iter().map(|n| n.depth).max().unwrap_or(0);
    let mut col_w = vec![0.0_f32; max_depth + 1];
    for (i, n) in nodes.iter().enumerate() {
        col_w[n.depth] = col_w[n.depth].max(sizes[i].x);
    }
    let gap_x = 26.0;
    let mut xs = vec![0.0_f32; max_depth + 1];
    for d in 1..=max_depth {
        xs[d] = xs[d - 1] + col_w[d - 1] + gap_x;
    }
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        match n.parent {
            Some(p) => children[p].push(i),
            None => roots.push(i),
        }
    }
    let row_h = 34.0;
    let mut ys = vec![0.0_f32; nodes.len()];
    let mut cursor = 0.0_f32;
    for &r in &roots {
        assign_y(r, &children, &mut ys, &mut cursor, row_h);
    }
    let content_w = xs[max_depth] + col_w[max_depth];
    let w = (content_w + 20.0)
        .max(280.0)
        .min(ui.available_width().max(280.0));
    let h = cursor + 16.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let p = ui.painter();
    let origin = Pos2::new(rect.left() + 10.0, rect.top() + 8.0);
    let center_of = |i: usize| -> Pos2 {
        Pos2::new(
            origin.x + xs[nodes[i].depth] + sizes[i].x / 2.0,
            origin.y + ys[i],
        )
    };
    // 连线先画。
    for (i, n) in nodes.iter().enumerate() {
        let Some(parent) = n.parent else { continue };
        let a = center_of(parent);
        let b = center_of(i);
        let start = Pos2::new(a.x + sizes[parent].x / 2.0, a.y);
        let end = Pos2::new(b.x - sizes[i].x / 2.0, b.y);
        let mid = (start.x + end.x) / 2.0;
        let color = palette_color(theme, nodes[i].branch);
        p.add(egui::epaint::CubicBezierShape::from_points_stroke(
            [start, Pos2::new(mid, start.y), Pos2::new(mid, end.y), end],
            false,
            Color32::TRANSPARENT,
            Stroke::new(1.4, color),
        ));
    }
    // 节点。
    for (i, n) in nodes.iter().enumerate() {
        let c = center_of(i);
        let r = Rect::from_center_size(c, sizes[i]);
        let fill = palette_color(theme, n.branch);
        let radius = if n.parent.is_none() {
            r.height() / 2.0
        } else {
            6.0
        };
        p.rect_filled(r, radius, fill);
        p.rect_stroke(
            r,
            radius,
            Stroke::new(1.2, theme.border),
            egui::StrokeKind::Inside,
        );
        let g = &galleys[i];
        p.galley(
            Pos2::new(
                r.center().x - g.size().x / 2.0,
                r.center().y - g.size().y / 2.0,
            ),
            g.clone(),
            theme.heading,
        );
    }
    resp
}
