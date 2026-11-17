//! Mermaid C4 上下文图（C4Context）。

use super::*;

pub(crate) struct C4Node {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) kind: String,
}

pub(crate) struct C4Rel {
    pub(crate) a: String,
    pub(crate) b: String,
    pub(crate) label: String,
}

fn call_args(line: &str) -> Option<(String, Vec<String>)> {
    let open = line.find('(')?;
    let close = line[open..].rfind(')')? + open;
    let name = line[..open].trim().to_string();
    let args: Vec<String> = line[open + 1..close]
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .collect();
    Some((name, args))
}

pub(crate) fn parse_c4(code: &str) -> (String, Vec<C4Node>, Vec<C4Rel>) {
    let mut title = String::new();
    let mut nodes: Vec<C4Node> = Vec::new();
    let mut rels = Vec::new();
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.eq_ignore_ascii_case("C4Context") {
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            title = t.trim().to_string();
            continue;
        }
        let Some((name, args)) = call_args(l) else {
            continue;
        };
        if name.eq_ignore_ascii_case("Rel")
            || name.eq_ignore_ascii_case("Rel_Back")
            || name.eq_ignore_ascii_case("BiRel")
        {
            if args.len() >= 2 {
                rels.push(C4Rel {
                    a: args[0].clone(),
                    b: args[1].clone(),
                    label: args.get(2).cloned().unwrap_or_default(),
                });
            }
            continue;
        }
        if !matches!(
            name.as_str(),
            "Person" | "System" | "System_Ext" | "SystemDb" | "Container" | "ContainerDb"
        ) {
            continue;
        }
        if args.len() >= 2 {
            nodes.push(C4Node {
                id: args[0].clone(),
                label: args[1].clone(),
                kind: name,
            });
        }
    }
    (title, nodes, rels)
}

fn c4_color(theme: &Theme, kind: &str) -> Color32 {
    match kind {
        "Person" => palette_color(theme, 1),
        "System" => palette_color(theme, 0),
        "System_Ext" => palette_color(theme, 3),
        _ => palette_color(theme, 2),
    }
}

pub(crate) fn draw_c4(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (title, nodes, rels) = parse_c4(code);
    if nodes.is_empty() {
        return raw_code(ui, code, theme);
    }
    let bw = 150.0_f32;
    let bh = 54.0_f32;
    let gap_x = 54.0_f32;
    let gap_y = 22.0_f32;
    let title_h = if title.is_empty() { 0.0 } else { 24.0 };
    // 按类型分三列：用户 / 系统 / 外部系统。
    let col_of = |kind: &str| -> usize {
        match kind {
            "Person" => 0,
            "System_Ext" => 2,
            _ => 1,
        }
    };
    let mut cols: [Vec<usize>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (i, n) in nodes.iter().enumerate() {
        cols[col_of(&n.kind).min(2)].push(i);
    }
    let max_rows = cols.iter().map(|c| c.len()).max().unwrap_or(1);
    let h = title_h + max_rows as f32 * (bh + gap_y) + 8.0;
    let w: f32 = (bw * 3.0 + gap_x * 2.0 + 24.0).min(ui.available_width().max(bw * 3.0 + 24.0));
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
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
    let mut rects: HashMap<String, Rect> = HashMap::new();
    for (ci, col) in cols.iter().enumerate() {
        for (ri, &ni) in col.iter().enumerate() {
            let n = &nodes[ni];
            let x = rect.left() + 12.0 + ci as f32 * (bw + gap_x);
            let y = rect.top() + title_h + 4.0 + ri as f32 * (bh + gap_y);
            let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(bw, bh));
            p.rect_filled(r, 6.0, c4_color(theme, &n.kind));
            p.rect_stroke(
                r,
                6.0,
                Stroke::new(1.2, theme.border),
                egui::StrokeKind::Inside,
            );
            p.text(
                Pos2::new(r.center().x, r.center().y - 6.0),
                Align2::CENTER_CENTER,
                &n.label,
                FontId::proportional(12.0),
                theme.heading,
            );
            p.text(
                Pos2::new(r.center().x, r.center().y + 11.0),
                Align2::CENTER_CENTER,
                format!("[{}]", n.kind),
                FontId::proportional(9.5),
                theme.dim,
            );
            rects.insert(n.id.clone(), r);
        }
    }
    for rel in &rels {
        let (Some(ra), Some(rb)) = (rects.get(&rel.a), rects.get(&rel.b)) else {
            continue;
        };
        let a = ra.center();
        let b = rb.center();
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dist = (dx * dx + dy * dy).sqrt().max(1.0);
        let (ux, uy) = (dx / dist, dy / dist);
        let trim = |r: &Rect, dx: f32, dy: f32| -> f32 {
            let tx = if dx.abs() > 1e-4 {
                r.width() / 2.0 / dx.abs()
            } else {
                f32::INFINITY
            };
            let ty = if dy.abs() > 1e-4 {
                r.height() / 2.0 / dy.abs()
            } else {
                f32::INFINITY
            };
            tx.min(ty)
        };
        let t0 = trim(ra, ux, uy);
        let t1 = dist - trim(rb, -ux, -uy);
        if t1 <= t0 + 2.0 {
            continue;
        }
        let s = Pos2::new(a.x + ux * t0, a.y + uy * t0);
        let e = Pos2::new(a.x + ux * t1, a.y + uy * t1);
        p.line_segment([s, e], Stroke::new(1.3, theme.dim));
        paint_arrow_head(p, e, (ux, uy), theme.dim, ArrowHead::Triangle);
        if !rel.label.is_empty() {
            p.text(
                Pos2::new((s.x + e.x) / 2.0, (s.y + e.y) / 2.0 - 7.0),
                Align2::CENTER_CENTER,
                &rel.label,
                FontId::proportional(10.0),
                theme.dim,
            );
        }
    }
    resp
}
