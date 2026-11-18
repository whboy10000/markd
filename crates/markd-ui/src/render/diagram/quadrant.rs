//! Mermaid 象限图（quadrantChart）。

use super::*;

pub(crate) struct QuadrantPoint {
    pub(crate) label: String,
    pub(crate) x: f32,
    pub(crate) y: f32,
}

pub(crate) struct QuadrantData {
    pub(crate) title: String,
    pub(crate) x_low: String,
    pub(crate) x_high: String,
    pub(crate) y_low: String,
    pub(crate) y_high: String,
    pub(crate) quadrants: [String; 4],
    pub(crate) points: Vec<QuadrantPoint>,
}

fn quoted(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

pub(crate) fn parse_quadrant(code: &str) -> QuadrantData {
    let mut d = QuadrantData {
        title: String::new(),
        x_low: String::new(),
        x_high: String::new(),
        y_low: String::new(),
        y_high: String::new(),
        quadrants: Default::default(),
        points: Vec::new(),
    };
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.eq_ignore_ascii_case("quadrantChart") {
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            d.title = t.trim().to_string();
            continue;
        }
        if let Some(rest) = l.strip_prefix("x-axis") {
            let parts: Vec<&str> = rest.split("-->").collect();
            if parts.len() == 2 {
                d.x_low = quoted(parts[0]);
                d.x_high = quoted(parts[1]);
            }
            continue;
        }
        if let Some(rest) = l.strip_prefix("y-axis") {
            let parts: Vec<&str> = rest.split("-->").collect();
            if parts.len() == 2 {
                d.y_low = quoted(parts[0]);
                d.y_high = quoted(parts[1]);
            }
            continue;
        }
        if let Some(rest) = l.strip_prefix("quadrant-")
            && let Some((n, label)) = rest.split_once(char::is_whitespace)
            && let Ok(idx) = n.trim().parse::<usize>()
            && (1..=4).contains(&idx)
        {
            d.quadrants[idx - 1] = quoted(label);
            continue;
        }
        // `"名称": [0.8, 0.9]`
        if let Some((name, rest)) = l.split_once(':')
            && let Some((x, y)) = rest.split_once(',')
        {
            let x = x
                .trim()
                .trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-');
            let y = y
                .trim()
                .trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-');
            if let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>()) {
                d.points.push(QuadrantPoint {
                    label: quoted(name),
                    x: x.clamp(0.0, 1.0),
                    y: y.clamp(0.0, 1.0),
                });
            }
        }
    }
    d
}

pub(crate) fn draw_quadrant(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let d = parse_quadrant(code);
    if d.points.is_empty() && d.quadrants.iter().all(|q| q.is_empty()) {
        return raw_code(ui, code, theme);
    }
    let avail = ui.available_width().max(320.0);
    let title_h = if d.title.is_empty() { 0.0 } else { 24.0 };
    let plot = (avail.min(420.0) - 40.0).max(200.0);
    let h = title_h + plot + 56.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail, h), Sense::click());
    let p = ui.painter();
    if !d.title.is_empty() {
        p.text(
            Pos2::new(rect.center().x, rect.top() + 2.0),
            Align2::CENTER_TOP,
            &d.title,
            FontId::proportional(13.0),
            theme.heading,
        );
    }
    let g = Rect::from_min_size(
        Pos2::new(rect.center().x - plot / 2.0, rect.top() + title_h + 14.0),
        Vec2::splat(plot),
    );
    // 象限底色：1 右上、2 左上、3 左下、4 右下。
    let half = plot / 2.0;
    let cells = [
        Rect::from_min_size(Pos2::new(g.left() + half, g.top()), Vec2::splat(half)),
        Rect::from_min_size(g.min, Vec2::splat(half)),
        Rect::from_min_size(Pos2::new(g.left(), g.top() + half), Vec2::splat(half)),
        Rect::from_min_size(
            Pos2::new(g.left() + half, g.top() + half),
            Vec2::splat(half),
        ),
    ];
    for (i, cell) in cells.iter().enumerate() {
        p.rect_filled(*cell, 0.0, palette_color(theme, i).gamma_multiply(0.55));
    }
    p.line_segment(
        [
            Pos2::new(g.center().x, g.top()),
            Pos2::new(g.center().x, g.bottom()),
        ],
        Stroke::new(1.2, theme.border),
    );
    p.line_segment(
        [
            Pos2::new(g.left(), g.center().y),
            Pos2::new(g.right(), g.center().y),
        ],
        Stroke::new(1.2, theme.border),
    );
    p.rect_stroke(
        g,
        0.0,
        Stroke::new(1.2, theme.border),
        egui::StrokeKind::Inside,
    );
    let corners = [
        Align2::RIGHT_TOP,
        Align2::LEFT_TOP,
        Align2::LEFT_BOTTOM,
        Align2::RIGHT_BOTTOM,
    ];
    for i in 0..4 {
        if d.quadrants[i].is_empty() {
            continue;
        }
        let inset = 6.0;
        let pos = match i {
            0 => Pos2::new(cells[i].right() - inset, cells[i].top() + inset),
            1 => Pos2::new(cells[i].left() + inset, cells[i].top() + inset),
            2 => Pos2::new(cells[i].left() + inset, cells[i].bottom() - inset),
            _ => Pos2::new(cells[i].right() - inset, cells[i].bottom() - inset),
        };
        p.text(
            pos,
            corners[i],
            &d.quadrants[i],
            FontId::proportional(11.0),
            theme.dim,
        );
    }
    // 轴端标签。
    p.text(
        Pos2::new(g.left(), g.bottom() + 6.0),
        Align2::LEFT_TOP,
        &d.x_low,
        FontId::proportional(10.5),
        theme.dim,
    );
    p.text(
        Pos2::new(g.right(), g.bottom() + 6.0),
        Align2::RIGHT_TOP,
        &d.x_high,
        FontId::proportional(10.5),
        theme.dim,
    );
    p.text(
        Pos2::new(g.left() - 4.0, g.bottom() + 24.0),
        Align2::RIGHT_TOP,
        &d.y_low,
        FontId::proportional(10.5),
        theme.dim,
    );
    p.text(
        Pos2::new(g.left() - 4.0, g.top() - 2.0),
        Align2::RIGHT_BOTTOM,
        &d.y_high,
        FontId::proportional(10.5),
        theme.dim,
    );
    for pt in d.points.iter() {
        let pos = Pos2::new(g.left() + pt.x * plot, g.bottom() - pt.y * plot);
        p.circle_filled(pos, 5.0, theme.accent);
        p.circle_stroke(pos, 5.0, Stroke::new(1.0, theme.code_bg));
        p.text(
            Pos2::new(pos.x + 8.0, pos.y),
            Align2::LEFT_CENTER,
            &pt.label,
            FontId::proportional(10.5),
            theme.text,
        );
    }
    resp
}
