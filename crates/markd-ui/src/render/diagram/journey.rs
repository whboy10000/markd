//! Mermaid 旅程图（journey）。

use super::*;

/// 解析 mermaid 旅程图：section 分组 + `任务: 分数: 演员`。
pub(crate) fn draw_journey(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let mut title = String::new();
    struct JSection {
        name: String,
        tasks: Vec<(String, i64, String)>,
    }
    let mut sections: Vec<JSection> = Vec::new();
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") {
            continue;
        }
        let lower = l.to_lowercase();
        if lower == "journey" {
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            if lower.starts_with("title") {
                title = t.trim().to_string();
                continue;
            }
        }
        if let Some(sec) = l.strip_prefix("section") {
            if lower.starts_with("section") {
                sections.push(JSection {
                    name: sec.trim().to_string(),
                    tasks: Vec::new(),
                });
                continue;
            }
        }
        let parts: Vec<&str> = l.split(':').map(|s| s.trim()).collect();
        if parts.len() >= 2 && !parts[0].is_empty() {
            let score = parts
                .get(1)
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(3);
            let actors = parts.get(2).unwrap_or(&"").to_string();
            if let Some(sec) = sections.last_mut() {
                sec.tasks
                    .push((parts[0].to_string(), score.clamp(1, 5), actors));
            }
        }
    }
    let total_tasks: usize = sections.iter().map(|s| s.tasks.len()).sum();
    if total_tasks == 0 {
        let r = panel_frame(theme).show(ui, |ui| {
            for line in code.lines() {
                ui.label(
                    egui::RichText::new(line)
                        .monospace()
                        .small()
                        .color(theme.dim),
                );
            }
        });
        return r.response.interact(Sense::click());
    }
    let title_h = if title.is_empty() { 0.0 } else { 22.0 };
    let sec_h = 96.0;
    let w = ui.available_width().max(320.0).min(900.0);
    let h = title_h + sections.len() as f32 * sec_h + 10.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let p = ui.painter();
    if !title.is_empty() {
        p.text(
            Pos2::new(rect.center().x, rect.min.y + 2.0),
            Align2::CENTER_TOP,
            &title,
            FontId::proportional(13.0),
            theme.heading,
        );
    }
    let mut y = rect.min.y + title_h + 4.0;
    for (si, sec) in sections.iter().enumerate() {
        p.text(
            Pos2::new(rect.min.x + 12.0, y + 8.0),
            Align2::LEFT_CENTER,
            &sec.name,
            FontId::proportional(12.0),
            theme.accent,
        );
        let n = sec.tasks.len().max(1);
        let seg = (w - 40.0) / n as f32;
        let line_y = y + 34.0;
        if sec.tasks.len() > 1 {
            p.line_segment(
                [
                    Pos2::new(rect.min.x + 20.0 + seg / 2.0, line_y),
                    Pos2::new(rect.min.x + 20.0 + seg * (n as f32 - 0.5), line_y),
                ],
                Stroke::new(1.5, theme.border),
            );
        }
        for (ti, (name, score, actors)) in sec.tasks.iter().enumerate() {
            let cx = rect.min.x + 20.0 + seg * (ti as f32 + 0.5);
            let radius = 7.0 + *score as f32 * 1.2;
            let fill = palette_color(theme, si + ti);
            p.circle_filled(Pos2::new(cx, line_y), radius, fill);
            p.circle_stroke(
                Pos2::new(cx, line_y),
                radius,
                Stroke::new(1.2, theme.border),
            );
            p.text(
                Pos2::new(cx, line_y),
                Align2::CENTER_CENTER,
                &score.to_string(),
                FontId::proportional(10.0),
                theme.text,
            );
            p.text(
                Pos2::new(cx, line_y + radius + 6.0),
                Align2::CENTER_TOP,
                name,
                FontId::proportional(11.0),
                theme.text,
            );
            if !actors.is_empty() {
                p.text(
                    Pos2::new(cx, line_y + radius + 22.0),
                    Align2::CENTER_TOP,
                    actors,
                    FontId::proportional(10.0),
                    theme.dim,
                );
            }
        }
        y += sec_h;
    }
    resp
}
