//! Mermaid 饼图。

use super::*;

/// 解析 mermaid 饼图：`pie title 标题` 加若干行 `"名称" : 数值`。
pub(crate) fn parse_pie(code: &str) -> (String, Vec<(String, f64)>) {
    let mut title = String::new();
    let mut data = Vec::new();
    for raw in code.lines() {
        let l = raw.trim();
        if let Some(t) = l.strip_prefix("pie title ") {
            title = t.trim().to_string();
            continue;
        }
        if l.is_empty() || l.to_lowercase().starts_with("pie") || l.starts_with("%%") {
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            title = t.trim().to_string();
            continue;
        }
        if l.starts_with('"') {
            if let Some(e) = l[1..].find('"') {
                let label = l[1..1 + e].to_string();
                let rest = l[1 + e..].trim_start_matches('"').trim();
                let val = rest
                    .split(':')
                    .next_back()
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .unwrap_or(0.0);
                if !label.is_empty() {
                    data.push((label, val));
                }
            }
        }
    }
    (title, data)
}

pub(crate) const PIE_PALETTE: [Color32; 6] = [
    Color32::from_rgb(76, 141, 255),
    Color32::from_rgb(255, 159, 67),
    Color32::from_rgb(46, 204, 113),
    Color32::from_rgb(231, 76, 60),
    Color32::from_rgb(155, 89, 182),
    Color32::from_rgb(26, 188, 156),
];

/// 圆扇形：用「圆心 + 弧上采样点」的三角扇逼近，兼容当前 egui 版本
/// （它没有 Shape::circle_sector）。
pub(crate) fn pie_sector(
    center: Pos2,
    radius: f32,
    start: f64,
    end: f64,
    color: Color32,
) -> egui::Shape {
    let span = (end - start).max(0.0);
    let steps = ((span / 0.08).ceil() as usize).max(2);
    let mut pts = vec![center];
    for k in 0..=steps {
        let a = start + span * (k as f64) / (steps as f64);
        pts.push(Pos2::new(
            center.x + (a as f32).cos() * radius,
            center.y + (a as f32).sin() * radius,
        ));
    }
    let mut mesh = egui::Mesh::default();
    for pt in &pts {
        mesh.colored_vertex(*pt, color);
    }
    let mut idx = Vec::with_capacity(steps * 3);
    for k in 0..steps {
        let b = (k + 1) as u32;
        idx.extend_from_slice(&[0, b, b + 1]);
    }
    mesh.indices = idx;
    egui::Shape::mesh(mesh)
}

// ---------------- Mermaid 时序图（sequenceDiagram） ----------------

pub(crate) fn draw_pie(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (title, data) = parse_pie(code);
    if data.is_empty() {
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
    let total: f64 = data.iter().map(|(_, v)| v.abs().max(0.0)).sum();
    let total = if total <= 0.0 { 1.0 } else { total };
    let r0 = 62.0_f32;
    let legend_w = 160.0_f32;
    let title_h = if title.is_empty() { 0.0 } else { 22.0 };
    let avail = ui.available_width().max(200.0);
    let w = (r0 * 2.0 + legend_w + 48.0).min(avail).max(180.0);
    let h = title_h + r0 * 2.0 + 16.0;
    // Sense::click 让图形本身可点击：点一下就能重新进入编辑态改代码，
    // 不然插入流程图后就再也改不了了（此前是 Sense::hover，点击永远无效）。
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let p = ui.painter();
    let cx = rect.min.x + 16.0 + r0;
    let cy = rect.min.y + title_h + 8.0 + r0;
    if !title.is_empty() {
        p.text(
            Pos2::new(rect.min.x + 16.0, rect.min.y + 4.0),
            Align2::LEFT_TOP,
            &title,
            FontId::new(12.0, FontFamily::Proportional),
            theme.text,
        );
    }
    // 从正上方开始、顺时针按数值占比分扇区；图例在右侧列出名称和百分比。
    let mut a0 = -std::f64::consts::FRAC_PI_2;
    for (k, (label, val)) in data.iter().enumerate() {
        let frac = (val.abs().max(0.0) / total) as f32;
        let a1 = a0 + (frac as f64) * std::f64::consts::TAU;
        let color = PIE_PALETTE[k % PIE_PALETTE.len()];
        p.add(pie_sector(Pos2::new(cx, cy), r0, a0, a1, color));
        let ly = cy + r0 - 16.0 - (k.min(7) as f32) * 22.0;
        let swatch = Rect::from_center_size(Pos2::new(cx + r0 + 24.0, ly), Vec2::new(12.0, 12.0));
        p.rect_filled(swatch, 3.0, color);
        p.text(
            Pos2::new(swatch.right() + 6.0, ly),
            Align2::LEFT_CENTER,
            format!("{} {:.0}%", label, frac * 100.0),
            FontId::new(12.0, FontFamily::Proportional),
            theme.text,
        );
        a0 = a1;
    }
    resp
}
