//! Mermaid 看板（kanban）。

use super::*;

pub(crate) struct KanbanLane {
    pub(crate) name: String,
    pub(crate) cards: Vec<String>,
}

/// 解析看板：无方括号的行是泳道名，`[卡片]`（或 `id[卡片]`）是卡片。
pub(crate) fn parse_kanban(code: &str) -> Vec<KanbanLane> {
    let mut lanes: Vec<KanbanLane> = Vec::new();
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.eq_ignore_ascii_case("kanban") {
            continue;
        }
        if let Some(open) = l.find('[')
            && let Some(close) = l[open + 1..].find(']')
        {
            let text = l[open + 1..open + 1 + close].trim().trim_matches('"');
            if let Some(lane) = lanes.last_mut() {
                lane.cards.push(text.to_string());
                continue;
            }
        }
        lanes.push(KanbanLane {
            name: l.to_string(),
            cards: Vec::new(),
        });
    }
    lanes
}

pub(crate) fn draw_kanban(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let lanes = parse_kanban(code);
    if lanes.is_empty() {
        return raw_code(ui, code, theme);
    }
    let avail = ui.available_width().max(320.0).min(900.0);
    let gap = 10.0;
    let n = lanes.len() as f32;
    let col_w = ((avail - gap * (n - 1.0)) / n).max(60.0);
    let card_wrap = (col_w - 22.0).max(28.0);
    let card_font = FontId::proportional(11.0);
    let card_galleys: Vec<Vec<_>> = lanes
        .iter()
        .map(|l| {
            l.cards
                .iter()
                .map(|c| {
                    ui.fonts_mut(|f| f.layout(c.clone(), card_font.clone(), theme.text, card_wrap))
                })
                .collect()
        })
        .collect();
    let header_h = 26.0;
    let card_pad = 7.0;
    let card_gap = 6.0;
    let lane_h = card_galleys
        .iter()
        .map(|gs| {
            gs.iter()
                .map(|g| g.size().y + card_pad * 2.0)
                .sum::<f32>()
                + card_gap * gs.len().saturating_sub(1) as f32
        })
        .fold(0.0, f32::max);
    let h = 10.0 + header_h + 8.0 + lane_h + 10.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail, h), Sense::click());
    let p = ui.painter();
    for (li, lane) in lanes.iter().enumerate() {
        let x = rect.left() + li as f32 * (col_w + gap);
        let lane_rect = Rect::from_min_size(Pos2::new(x, rect.top() + 6.0), Vec2::new(col_w, h - 12.0));
        p.rect_filled(lane_rect, 6.0, theme.code_header);
        p.text(
            Pos2::new(x + col_w / 2.0, lane_rect.top() + 5.0),
            Align2::CENTER_TOP,
            &lane.name,
            FontId::proportional(12.0),
            theme.heading,
        );
        let mut cy = lane_rect.top() + header_h + 8.0;
        for (ci, g) in card_galleys[li].iter().enumerate() {
            let ch = g.size().y + card_pad * 2.0;
            let cr = Rect::from_min_size(
                Pos2::new(x + 6.0, cy),
                Vec2::new(col_w - 12.0, ch),
            );
            p.rect_filled(cr, 4.0, palette_color(theme, ci));
            p.rect_stroke(
                cr,
                4.0,
                Stroke::new(1.0, theme.border),
                egui::StrokeKind::Inside,
            );
            p.galley(
                Pos2::new(cr.left() + 6.0, cr.top() + card_pad),
                g.clone(),
                theme.text,
            );
            cy += ch + card_gap;
        }
    }
    resp
}
