//! Mermaid Git 图（gitGraph）。

use super::*;

/// 解析并绘制 mermaid gitGraph：commit/branch/checkout/merge 状态机。
pub(crate) fn draw_git(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    struct Lane {
        name: String,
        // (序号, 合并来源 lane 下标；usize::MAX = 普通提交, 提交文字)
        dots: Vec<(usize, usize, String)>,
    }
    fn quoted_after(line: &str, key: &str) -> Option<String> {
        let i = line.find(key)? + key.len();
        let rest = &line[i..];
        let q1 = rest.find('"')?;
        let q2 = rest[q1 + 1..].find('"')?;
        Some(rest[q1 + 1..q1 + 1 + q2].to_string())
    }
    let mut lanes: Vec<Lane> = vec![Lane {
        name: "main".to_string(),
        dots: Vec::new(),
    }];
    let mut cur = 0usize;
    let mut seq = 0usize;
    for raw in code.lines() {
        let l = raw.trim();
        let lower = l.to_lowercase();
        if l.is_empty() || lower == "gitgraph" || l.starts_with("%%") {
            continue;
        }
        if let Some(rest) = lower.strip_prefix("branch ") {
            let name = rest.trim();
            if !lanes.iter().any(|ln| ln.name == name) {
                lanes.push(Lane {
                    name: name.to_string(),
                    dots: Vec::new(),
                });
                cur = lanes.len() - 1;
            } else if let Some(i) = lanes.iter().position(|ln| ln.name == name) {
                cur = i;
            }
            continue;
        }
        if let Some(rest) = lower.strip_prefix("checkout ") {
            if let Some(i) = lanes.iter().position(|ln| ln.name == rest.trim()) {
                cur = i;
            }
            continue;
        }
        if lower.starts_with("merge ") {
            let src = lower[6..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim();
            if let Some(i) = lanes.iter().position(|ln| ln.name == src) {
                seq += 1;
                let label = quoted_after(l, "tag:").unwrap_or_default();
                lanes[cur].dots.push((seq, i, label));
            }
            continue;
        }
        if lower.starts_with("commit") {
            seq += 1;
            let label = quoted_after(l, "id:").unwrap_or_default();
            lanes[cur].dots.push((seq, usize::MAX, label));
        }
    }
    let total = seq;
    if total == 0 {
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
    let lane_h = 44.0;
    let pad_l = 64.0;
    let w = ui.available_width().max(320.0).min(900.0);
    let h = lanes.len() as f32 * lane_h + 30.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let p = ui.painter();
    let seg = (w - pad_l - 20.0) / (total as f32 + 0.5);
    let x_of = |s: usize| rect.min.x + pad_l + s as f32 * seg;
    let y_of = |i: usize| rect.min.y + 22.0 + i as f32 * lane_h;
    // 分支名标签
    for (i, lane) in lanes.iter().enumerate() {
        p.text(
            Pos2::new(rect.min.x + 8.0, y_of(i)),
            Align2::LEFT_CENTER,
            &lane.name,
            FontId::monospace(11.0),
            palette_color(theme, i),
        );
    }
    // 车道横线 + 提交点
    for (i, lane) in lanes.iter().enumerate() {
        if let (Some(first), Some(last)) = (lane.dots.first(), lane.dots.last()) {
            p.line_segment(
                [
                    Pos2::new(x_of(first.0), y_of(i)),
                    Pos2::new(x_of(last.0), y_of(i)),
                ],
                Stroke::new(2.0, palette_color(theme, i).gamma_multiply(0.9)),
            );
        }
        for (s, merge_from, label) in &lane.dots {
            let c = Pos2::new(x_of(*s), y_of(i));
            if *merge_from != usize::MAX {
                // 合并提交：从来源车道画一条折线过来
                let src_y = y_of(*merge_from);
                p.line_segment(
                    [Pos2::new(x_of(*s), src_y), Pos2::new(c.x, c.y)],
                    Stroke::new(1.5, theme.dim),
                );
                p.circle_filled(c, 6.0, palette_color(theme, *merge_from));
            } else {
                p.circle_filled(c, 6.0, palette_color(theme, i));
            }
            p.circle_stroke(c, 6.0, Stroke::new(1.4, theme.text));
            if !label.is_empty() {
                p.text(
                    Pos2::new(c.x, c.y + 9.0),
                    Align2::CENTER_TOP,
                    label,
                    FontId::proportional(9.5),
                    theme.dim,
                );
            }
        }
    }
    resp
}
