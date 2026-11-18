//! Mermaid 泳道图（swimlane-beta）。

use super::*;

pub(crate) struct SwimLane {
    pub(crate) name: String,
    pub(crate) steps: Vec<String>,
}

pub(crate) fn parse_swimlane(code: &str) -> (String, Vec<SwimLane>) {
    let mut title = String::new();
    let mut lanes: Vec<SwimLane> = Vec::new();
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.to_lowercase().starts_with("swimlane-beta") {
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            title = t.trim().to_string();
            continue;
        }
        if let Some(rest) = l.strip_prefix("swimlane") {
            let name = rest.trim().trim_matches('"').to_string();
            lanes.push(SwimLane {
                name,
                steps: Vec::new(),
            });
            continue;
        }
        if let Some(rest) = l.strip_prefix("step") {
            if let Some(lane) = lanes.last_mut() {
                lane.steps.push(rest.trim().trim_matches('"').to_string());
            }
        }
    }
    (title, lanes)
}

pub(crate) fn draw_swimlane(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (title, lanes) = parse_swimlane(code);
    if lanes.is_empty() {
        return raw_code(ui, code, theme);
    }
    let label_w = 86.0;
    let step_w = 120.0;
    let step_h = 30.0;
    let gap = 26.0;
    let lane_gap = 8.0;
    let max_steps = lanes
        .iter()
        .map(|l| l.steps.len())
        .max()
        .unwrap_or(1)
        .max(1);
    let content_w = max_steps as f32 * (step_w + gap);
    let avail = ui.available_width().max(320.0);
    let w = (label_w + content_w + 16.0).min(avail.max(320.0));
    let title_h = if title.is_empty() { 0.0 } else { 24.0 };
    let lane_h = step_h + lane_gap * 2.0;
    let h = title_h + lanes.len() as f32 * lane_h + 8.0;
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
    let body_top = rect.top() + title_h;
    for (li, lane) in lanes.iter().enumerate() {
        let y = body_top + li as f32 * lane_h;
        let lane_rect = Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(w, lane_h));
        p.rect_filled(
            lane_rect,
            4.0,
            if li % 2 == 0 {
                theme.code_header
            } else {
                theme.code_bg
            },
        );
        p.text(
            Pos2::new(rect.left() + label_w / 2.0, lane_rect.center().y),
            Align2::CENTER_CENTER,
            &lane.name,
            FontId::proportional(12.0),
            theme.heading,
        );
        p.line_segment(
            [
                Pos2::new(rect.left() + label_w, lane_rect.top() + 4.0),
                Pos2::new(rect.left() + label_w, lane_rect.bottom() - 4.0),
            ],
            Stroke::new(1.0, theme.border),
        );
        for (si, step) in lane.steps.iter().enumerate() {
            let cx = rect.left() + label_w + 12.0 + si as f32 * (step_w + gap);
            let sr = Rect::from_center_size(
                Pos2::new(cx + step_w / 2.0, lane_rect.center().y),
                Vec2::new(step_w, step_h),
            );
            p.rect_filled(sr, 5.0, palette_color(theme, li));
            p.rect_stroke(
                sr,
                5.0,
                Stroke::new(1.0, theme.border),
                egui::StrokeKind::Inside,
            );
            p.text(
                sr.center(),
                Align2::CENTER_CENTER,
                step,
                FontId::proportional(11.0),
                theme.text,
            );
            if si + 1 < lane.steps.len() {
                let a = Pos2::new(sr.right(), sr.center().y);
                let b = Pos2::new(sr.right() + gap, sr.center().y);
                p.line_segment([a, b], Stroke::new(1.2, theme.dim));
                paint_arrow_head(p, b, (1.0, 0.0), theme.dim, ArrowHead::Triangle);
            }
        }
    }
    resp
}
