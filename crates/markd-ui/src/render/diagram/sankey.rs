//! Mermaid 桑基图（sankey-beta）。

use super::*;

pub(crate) struct SankeyLink {
    pub(crate) source: usize,
    pub(crate) target: usize,
    pub(crate) value: f32,
}

pub(crate) struct SankeyData {
    pub(crate) nodes: Vec<String>,
    pub(crate) links: Vec<SankeyLink>,
    pub(crate) depths: Vec<usize>,
}

pub(crate) fn parse_sankey(code: &str) -> SankeyData {
    let mut nodes: Vec<String> = Vec::new();
    let mut links: Vec<SankeyLink> = Vec::new();
    let find = |nodes: &mut Vec<String>, name: &str| -> usize {
        if let Some(i) = nodes.iter().position(|n| n == name) {
            i
        } else {
            nodes.push(name.to_string());
            nodes.len() - 1
        }
    };
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.to_lowercase().starts_with("sankey-beta") {
            continue;
        }
        let parts: Vec<&str> = l.split(',').map(str::trim).collect();
        if parts.len() < 3 {
            continue;
        }
        let Ok(value) = parts[2].parse::<f32>() else {
            continue;
        };
        if parts[0].is_empty() || parts[1].is_empty() {
            continue;
        }
        let s = find(&mut nodes, parts[0]);
        let t = find(&mut nodes, parts[1]);
        links.push(SankeyLink {
            source: s,
            target: t,
            value: value.max(0.0),
        });
    }
    let mut depths = vec![0usize; nodes.len()];
    for _ in 0..nodes.len().max(1) {
        let mut changed = false;
        for link in &links {
            let nd = (depths[link.source] + 1).min(nodes.len());
            if nd > depths[link.target] {
                depths[link.target] = nd;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    SankeyData {
        nodes,
        links,
        depths,
    }
}

pub(crate) fn draw_sankey(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let d = parse_sankey(code);
    if d.links.is_empty() {
        return raw_code(ui, code, theme);
    }
    let avail = ui.available_width().max(320.0).min(900.0);
    let h = 300.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail, h), Sense::click());
    let p = ui.painter();
    let ncols = d.depths.iter().copied().max().unwrap_or(0) + 1;
    let bar_w = 12.0;
    let margin = 18.0;
    let top = rect.top() + 20.0;
    let bottom = rect.bottom() - 24.0;
    let body_h = bottom - top;
    let mut totals = vec![0.0_f32; d.nodes.len()];
    let mut ins = vec![0.0_f32; d.nodes.len()];
    let mut outs = vec![0.0_f32; d.nodes.len()];
    for link in &d.links {
        outs[link.source] += link.value;
        ins[link.target] += link.value;
    }
    for i in 0..d.nodes.len() {
        totals[i] = ins[i].max(outs[i]).max(0.01);
    }
    let mut col_totals = vec![0.0_f32; ncols];
    for i in 0..d.nodes.len() {
        col_totals[d.depths[i].min(ncols - 1)] += totals[i];
    }
    let max_col = col_totals.iter().copied().fold(0.01, f32::max);
    let gap_y = 12.0;
    let unit = (body_h - gap_y * (d.nodes.len().min(ncols) as f32)) / max_col;
    let unit = unit.max(0.4);
    let col_gap = if ncols > 1 {
        (avail - margin * 2.0 - bar_w * ncols as f32) / (ncols - 1) as f32
    } else {
        0.0
    };
    // 每列内的节点纵向堆叠。
    let mut stack_y = vec![top; ncols];
    let mut node_rects: Vec<Rect> = vec![Rect::NOTHING; d.nodes.len()];
    for i in 0..d.nodes.len() {
        let col = d.depths[i].min(ncols - 1);
        let x = rect.left() + margin + col as f32 * (bar_w + col_gap);
        let nh = (totals[i] * unit).max(6.0);
        let y = stack_y[col];
        node_rects[i] = Rect::from_min_size(Pos2::new(x, y), Vec2::new(bar_w, nh));
        stack_y[col] = y + nh + gap_y;
    }
    // 先画流带（压在节点下层），再画节点与标签。
    let mut out_off = vec![0.0_f32; d.nodes.len()];
    let mut in_off = vec![0.0_f32; d.nodes.len()];
    for (li, link) in d.links.iter().enumerate() {
        let sr = node_rects[link.source];
        let tr = node_rects[link.target];
        let th = (link.value * unit * 0.85).max(2.0);
        let sy = sr.top() + out_off[link.source] + th / 2.0;
        let ty = tr.top() + in_off[link.target] + th / 2.0;
        out_off[link.source] += th;
        in_off[link.target] += th;
        let x1 = sr.right();
        let x2 = tr.left();
        let mx = (x1 + x2) / 2.0;
        let color = palette_color(theme, li);
        p.add(egui::epaint::CubicBezierShape::from_points_stroke(
            [
                Pos2::new(x1, sy),
                Pos2::new(mx, sy),
                Pos2::new(mx, ty),
                Pos2::new(x2, ty),
            ],
            false,
            Color32::TRANSPARENT,
            Stroke::new(th, color.gamma_multiply(0.6)),
        ));
    }
    for (i, r) in node_rects.iter().enumerate() {
        if r == &Rect::NOTHING {
            continue;
        }
        p.rect_filled(*r, 2.0, palette_color(theme, i));
        p.rect_stroke(
            *r,
            2.0,
            Stroke::new(1.0, theme.border),
            egui::StrokeKind::Inside,
        );
        let right_side = d.depths[i] < ncols.saturating_sub(1);
        let pos = if right_side {
            Pos2::new(r.right() + 5.0, r.center().y)
        } else {
            Pos2::new(r.left() - 5.0, r.center().y)
        };
        let align = if right_side {
            Align2::LEFT_CENTER
        } else {
            Align2::RIGHT_CENTER
        };
        p.text(
            pos,
            align,
            &d.nodes[i],
            FontId::proportional(10.5),
            theme.text,
        );
    }
    resp
}
