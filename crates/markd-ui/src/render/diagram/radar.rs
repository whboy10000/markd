//! Mermaid 雷达图（radar-beta）。

use super::*;

pub(crate) struct RadarCurve {
    pub(crate) name: String,
    pub(crate) values: Vec<f32>,
}

pub(crate) struct RadarData {
    pub(crate) title: String,
    pub(crate) axes: Vec<String>,
    pub(crate) curves: Vec<RadarCurve>,
    pub(crate) max: f32,
}

pub(crate) fn parse_radar(code: &str) -> RadarData {
    let mut d = RadarData {
        title: String::new(),
        axes: Vec::new(),
        curves: Vec::new(),
        max: 100.0,
    };
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.to_lowercase().starts_with("radar-beta") {
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            d.title = t.trim().to_string();
        } else if let Some(rest) = l.strip_prefix("axis") {
            d.axes = rest
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .collect();
        } else if let Some(rest) = l.strip_prefix("max") {
            if let Ok(v) = rest.trim().parse::<f32>() {
                d.max = v;
            }
        } else if let Some((name, values)) = l.strip_prefix("curve").and_then(|r| {
            let (name, rest) = r.trim().split_once('[')?;
            let close = rest.find(']')?;
            let values: Vec<f32> = rest[..close]
                .split(',')
                .filter_map(|v| v.trim().parse().ok())
                .collect();
            Some((name.trim().to_string(), values))
        }) {
            d.curves.push(RadarCurve { name, values });
        }
    }
    d
}

fn radar_color(theme: &Theme, i: usize) -> Color32 {
    match i % 4 {
        0 => theme.accent,
        1 => theme.link,
        2 => Color32::from_rgb(216, 122, 178),
        _ => Color32::from_rgb(120, 178, 120),
    }
}

pub(crate) fn draw_radar(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let d = parse_radar(code);
    if d.axes.len() < 3 {
        return raw_code(ui, code, theme);
    }
    let avail = ui.available_width().max(320.0).min(760.0);
    let title_h = if d.title.is_empty() { 0.0 } else { 22.0 };
    let radius = 104.0;
    let legend_w = 150.0;
    let h = title_h + radius * 2.0 + 40.0;
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
    let center = Pos2::new(
        rect.left() + (avail - legend_w) / 2.0,
        rect.top() + title_h + radius + 18.0,
    );
    let n = d.axes.len();
    let angle = |i: usize| -> f32 {
        -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32
    };
    let point_at = |i: usize, t: f32| -> Pos2 {
        let a = angle(i);
        Pos2::new(
            center.x + a.cos() * radius * t,
            center.y + a.sin() * radius * t,
        )
    };
    // 同心多边形。
    for ring in 1..=4 {
        let t = ring as f32 / 4.0;
        let pts: Vec<Pos2> = (0..n).map(|i| point_at(i, t)).collect();
        p.add(egui::Shape::closed_line(
            pts,
            Stroke::new(1.0, theme.border.gamma_multiply(0.7)),
        ));
    }
    for i in 0..n {
        p.line_segment(
            [center, point_at(i, 1.0)],
            Stroke::new(1.0, theme.border.gamma_multiply(0.7)),
        );
        let a = angle(i);
        let lp = Pos2::new(
            center.x + a.cos() * (radius + 14.0),
            center.y + a.sin() * (radius + 14.0),
        );
        let align = match (a.cos(), a.sin()) {
            (c, s) if c.abs() < 0.25 && s < 0.0 => Align2::CENTER_BOTTOM,
            (c, _) if c.abs() < 0.25 => Align2::CENTER_TOP,
            (c, s) if c > 0.0 && s.abs() < 0.4 => Align2::LEFT_CENTER,
            (c, s) if c < 0.0 && s.abs() < 0.4 => Align2::RIGHT_CENTER,
            (c, _) if c > 0.0 => Align2::LEFT_CENTER,
            _ => Align2::RIGHT_CENTER,
        };
        p.text(
            lp,
            align,
            &d.axes[i],
            FontId::proportional(11.0),
            theme.text,
        );
    }
    // 每条曲线一个多边形。
    for (ci, curve) in d.curves.iter().enumerate() {
        let pts: Vec<Pos2> = (0..n)
            .map(|i| {
                let v = curve.values.get(i).copied().unwrap_or(0.0);
                let t = (v / d.max.max(1e-3)).clamp(0.0, 1.0);
                point_at(i, t)
            })
            .collect();
        let color = radar_color(theme, ci);
        // 雷达多边形通常不是凸多边形，用「圆心三角形扇」填充更稳。
        for i in 0..n {
            p.add(egui::Shape::convex_polygon(
                vec![center, pts[i], pts[(i + 1) % n]],
                color.gamma_multiply(0.18),
                Stroke::NONE,
            ));
        }
        p.add(egui::Shape::closed_line(pts, Stroke::new(1.8, color)));
    }
    // 图例。
    let lx = rect.right() - legend_w + 8.0;
    let mut ly = center.y - (d.curves.len().min(6) as f32) * 10.0;
    for (ci, curve) in d.curves.iter().enumerate().take(6) {
        let swatch = Rect::from_center_size(Pos2::new(lx + 6.0, ly), Vec2::splat(10.0));
        p.rect_filled(swatch, 2.0, radar_color(theme, ci));
        p.text(
            Pos2::new(lx + 16.0, ly),
            Align2::LEFT_CENTER,
            &curve.name,
            FontId::proportional(11.0),
            theme.text,
        );
        ly += 20.0;
    }
    resp
}
