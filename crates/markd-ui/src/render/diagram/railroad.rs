//! Mermaid 铁路图（railroad-beta）。

use super::*;

pub(crate) enum RailItem {
    Terminal(String),
    NonTerminal(String),
}

/// 解析铁路图：`sequence` 下的 `terminal "x"` / `non-terminal "x"` 顺序绘制。
pub(crate) fn parse_railroad(code: &str) -> (String, Vec<RailItem>) {
    let mut title = String::new();
    let mut items = Vec::new();
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.to_lowercase().starts_with("railroad-beta") {
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            title = t.trim().to_string();
            continue;
        }
        let lower = l.to_lowercase();
        if lower == "sequence" {
            continue;
        }
        let parse_quoted = |rest: &str| -> String { rest.trim().trim_matches('"').to_string() };
        if let Some(rest) = l.strip_prefix("terminal") {
            items.push(RailItem::Terminal(parse_quoted(rest)));
        } else if let Some(rest) = l.strip_prefix("non-terminal") {
            items.push(RailItem::NonTerminal(parse_quoted(rest)));
        } else if let Some(rest) = l.strip_prefix("optional") {
            items.push(RailItem::NonTerminal(parse_quoted(rest)));
        } else if let Some(rest) = l.strip_prefix("choice") {
            items.push(RailItem::Terminal(parse_quoted(rest)));
        }
    }
    (title, items)
}

pub(crate) fn draw_railroad(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (title, items) = parse_railroad(code);
    if items.is_empty() {
        return raw_code(ui, code, theme);
    }
    let item_h = 30.0;
    let pad_x = 14.0;
    let gap = 24.0;
    let text_galleys: Vec<_> = items
        .iter()
        .map(|it| {
            let (text, size) = match it {
                RailItem::Terminal(t) => (t, 11.0),
                RailItem::NonTerminal(t) => (t, 11.0),
            };
            ui.fonts_mut(|f| {
                f.layout(
                    text.clone(),
                    FontId::monospace(size),
                    theme.text,
                    f32::INFINITY,
                )
            })
        })
        .collect();
    let widths: Vec<f32> = text_galleys
        .iter()
        .map(|g| (g.size().x + pad_x * 2.0).max(52.0))
        .collect();
    let content_w: f32 = widths.iter().sum::<f32>() + gap * (items.len() - 1) as f32;
    let avail = ui.available_width().max(320.0);
    let title_h = if title.is_empty() { 0.0 } else { 24.0 };
    let h = title_h + item_h + 40.0;
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
    let cy = rect.top() + title_h + 10.0 + item_h / 2.0;
    let start_x = rect.center().x - content_w / 2.0;
    // 起始小圆点。
    p.circle_filled(Pos2::new(start_x - 12.0, cy), 3.5, theme.accent);
    let mut x = start_x;
    for (i, item) in items.iter().enumerate() {
        let w = widths[i];
        p.line_segment(
            [Pos2::new(x - gap, cy), Pos2::new(x, cy)],
            Stroke::new(1.2, theme.dim),
        );
        let r = Rect::from_min_size(Pos2::new(x, cy - item_h / 2.0), Vec2::new(w, item_h));
        let (radius, fill) = match item {
            RailItem::Terminal(_) => (item_h / 2.0, palette_color(theme, 1)),
            RailItem::NonTerminal(_) => (4.0, palette_color(theme, 0)),
        };
        p.rect_filled(r, radius, fill);
        p.rect_stroke(
            r,
            radius,
            Stroke::new(1.0, theme.border),
            egui::StrokeKind::Inside,
        );
        let g = &text_galleys[i];
        p.galley(
            Pos2::new(
                r.center().x - g.size().x / 2.0,
                r.center().y - g.size().y / 2.0,
            ),
            g.clone(),
            theme.text,
        );
        x += w;
    }
    // 收尾线与箭头。
    let end_x = x + 12.0;
    p.line_segment(
        [Pos2::new(x, cy), Pos2::new(end_x, cy)],
        Stroke::new(1.2, theme.dim),
    );
    paint_arrow_head(
        p,
        Pos2::new(end_x + 8.0, cy),
        (1.0, 0.0),
        theme.dim,
        ArrowHead::Triangle,
    );
    p.line_segment(
        [Pos2::new(end_x, cy), Pos2::new(end_x + 8.0, cy)],
        Stroke::new(1.2, theme.dim),
    );
    resp
}
