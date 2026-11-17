//! Mermaid 数据包图（packet-beta）。

use super::*;

pub(crate) struct PacketField {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) label: String,
}

pub(crate) fn parse_packet(code: &str) -> (String, Vec<PacketField>) {
    let mut title = String::new();
    let mut fields = Vec::new();
    let mut next_bit = 0usize;
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.to_lowercase().starts_with("packet-beta") {
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            title = t.trim().to_string();
            continue;
        }
        // `0-15: "Source Port"` 或 `+16: "..."`（从上一字段末尾续排）。
        let Some(ci) = l.find(':') else { continue };
        let range = l[..ci].trim();
        let label = l[ci + 1..].trim().trim_matches('"').to_string();
        let (start, end) = if let Some(rest) = range.strip_prefix('+') {
            let n: usize = rest.trim().parse().unwrap_or(1);
            (next_bit, next_bit + n.saturating_sub(1))
        } else if let Some((a, b)) = range.split_once('-') {
            match (a.trim().parse(), b.trim().parse()) {
                (Ok(a), Ok(b)) => (a, b),
                _ => continue,
            }
        } else {
            continue;
        };
        if end < start {
            continue;
        }
        next_bit = end + 1;
        fields.push(PacketField { start, end, label });
    }
    (title, fields)
}

pub(crate) fn draw_packet(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (title, fields) = parse_packet(code);
    if fields.is_empty() {
        return raw_code(ui, code, theme);
    }
    let total_bits = fields.iter().map(|f| f.end + 1).max().unwrap_or(32).max(1);
    let rows = total_bits.div_ceil(32);
    let avail = ui.available_width().max(320.0).min(900.0);
    let title_h = if title.is_empty() { 0.0 } else { 24.0 };
    let row_h = 42.0;
    let h = title_h + rows as f32 * row_h + 8.0;
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
    let body_top = rect.top() + title_h;
    let row_w = avail - 8.0;
    let bit_w = row_w / 32.0;
    for (fi, f) in fields.iter().enumerate() {
        for row in 0..rows {
            let row_start = row * 32;
            let row_end = row_start + 32;
            let s = f.start.max(row_start);
            let e = (f.end + 1).min(row_end);
            if s >= e {
                continue;
            }
            let x0 = rect.left() + 4.0 + (s - row_start) as f32 * bit_w;
            let x1 = rect.left() + 4.0 + (e - row_start) as f32 * bit_w;
            let fr = Rect::from_min_max(
                Pos2::new(x0, body_top + row as f32 * row_h + 4.0),
                Pos2::new(x1 - 1.0, body_top + (row as f32 + 1.0) * row_h - 4.0),
            );
            p.rect_filled(fr, 3.0, palette_color(theme, fi));
            p.rect_stroke(
                fr,
                3.0,
                Stroke::new(1.0, theme.border),
                egui::StrokeKind::Inside,
            );
            p.text(
                Pos2::new(fr.center().x, fr.top() + 2.0),
                Align2::CENTER_TOP,
                format!("{}-{}", f.start, f.end),
                FontId::proportional(9.0),
                theme.dim,
            );
            p.text(
                Pos2::new(fr.center().x, fr.bottom() - 4.0),
                Align2::CENTER_BOTTOM,
                &f.label,
                FontId::proportional(10.5),
                theme.text,
            );
        }
    }
    resp
}
