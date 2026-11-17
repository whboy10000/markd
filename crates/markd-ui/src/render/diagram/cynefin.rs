//! Mermaid 决策域图（cynefin-beta）。

use super::*;

pub(crate) fn parse_cynefin(code: &str) -> (String, Vec<(String, String)>) {
    let mut title = String::new();
    let mut domains = Vec::new();
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.to_lowercase().starts_with("cynefin-beta") {
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            title = t.trim().to_string();
            continue;
        }
        if let Some(rest) = l.strip_prefix("domain") {
            let mut parts = rest
                .split('"')
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let name = parts.next().unwrap_or("").to_string();
            let desc = parts.next().unwrap_or("").to_string();
            if !name.is_empty() {
                domains.push((name, desc));
            }
        }
    }
    (title, domains)
}

pub(crate) fn draw_cynefin(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (title, domains) = parse_cynefin(code);
    if domains.is_empty() {
        return raw_code(ui, code, theme);
    }
    let avail = ui.available_width().max(320.0);
    let side = avail.min(420.0);
    let title_h = if title.is_empty() { 0.0 } else { 24.0 };
    let h = title_h + side + 8.0;
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
    let grid = Rect::from_min_size(
        Pos2::new(rect.center().x - side / 2.0, rect.top() + title_h + 4.0),
        Vec2::splat(side),
    );
    let gap = 6.0;
    let cell = (side - gap) / 2.0;
    let quad = |r: usize, c: usize| {
        Rect::from_min_size(
            Pos2::new(grid.left() + c as f32 * (cell + gap), grid.top() + r as f32 * (cell + gap)),
            Vec2::splat(cell),
        )
    };
    // Cynefin 经典布局：左上 Complex、右上 Complicated、左下 Chaotic、右下 Clear。
    let slots = [quad(1, 1), quad(0, 1), quad(0, 0), quad(1, 0)];
    let mut confused: Option<&(String, String)> = None;
    let mut slot = 0usize;
    for (i, d) in domains.iter().enumerate() {
        let key = d.0.to_lowercase();
        let at = if key.contains("clear") {
            Some(0usize)
        } else if key.contains("complicated") {
            Some(1)
        } else if key.contains("complex") {
            Some(2)
        } else if key.contains("chaotic") {
            Some(3)
        } else if key.contains("confused") {
            None
        } else {
            Some(slot)
        };
        match at {
            Some(k) => {
                let r = slots[k.min(3)];
                p.rect_filled(r, 6.0, palette_color(theme, i));
                p.rect_stroke(
                    r,
                    6.0,
                    Stroke::new(1.0, theme.border),
                    egui::StrokeKind::Inside,
                );
                p.text(
                    Pos2::new(r.center().x, r.center().y - 9.0),
                    Align2::CENTER_CENTER,
                    &d.0,
                    FontId::proportional(13.0),
                    theme.heading,
                );
                if !d.1.is_empty() {
                    p.text(
                        Pos2::new(r.center().x, r.center().y + 10.0),
                        Align2::CENTER_CENTER,
                        &d.1,
                        FontId::proportional(11.0),
                        theme.dim,
                    );
                }
                slot = (slot + 1) % 4;
            }
            None => confused = Some(d),
        }
    }
    // 中央「混乱域」：圆心叠在四格交点上。
    if let Some(d) = confused {
        let c = grid.center();
        p.circle_filled(c, cell * 0.34, theme.code_header);
        p.circle_stroke(c, cell * 0.34, Stroke::new(1.2, theme.border));
        p.text(
            Pos2::new(c.x, c.y - 8.0),
            Align2::CENTER_CENTER,
            &d.0,
            FontId::proportional(12.5),
            theme.heading,
        );
        if !d.1.is_empty() {
            p.text(
                Pos2::new(c.x, c.y + 9.0),
                Align2::CENTER_CENTER,
                &d.1,
                FontId::proportional(10.5),
                theme.dim,
            );
        }
    }
    resp
}
