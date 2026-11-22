//! Mermaid Git 图（gitGraph），按 Mermaid 官方样式绘制。

use super::*;

/// Git 图专用饱和配色：跟随 Mermaid 官方的蓝 / 黄 / 绿顺序，
/// 亮色与暗色主题下都能保持足够对比度（不复用 diagram_palette 的淡色）。
fn git_color(i: usize) -> Color32 {
    const COLORS: [Color32; 6] = [
        Color32::from_rgb(37, 99, 235),   // main 蓝
        Color32::from_rgb(214, 158, 0),   // develop 黄（略深，白字可读）
        Color32::from_rgb(30, 170, 78),   // feature 绿
        Color32::from_rgb(168, 85, 247),  // 紫
        Color32::from_rgb(234, 100, 40),  // 橙
        Color32::from_rgb(20, 160, 170),  // 青
    ];
    COLORS[i % COLORS.len()]
}

/// 一个提交 / 合并点。
struct Dot {
    seq: usize,
    /// Some((来源车道, 合并时来源车道的最后 seq)) 表示合并点；None 为普通提交。
    from: Option<(usize, usize)>,
    label: String,
}

struct Lane {
    name: String,
    dots: Vec<Dot>,
    /// (父车道, 分出时父车道的最后 seq) —— 用于画 branch 折线。
    born: Option<(usize, usize)>,
}

/// 解析并绘制 mermaid gitGraph：commit/branch/checkout/merge 状态机。
pub(crate) fn draw_git(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
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
        born: None,
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
            if let Some(i) = lanes.iter().position(|ln| ln.name == name) {
                cur = i;
            } else {
                // 分出锚点 = 父车道此刻的最后一个提交，折线从它垂下来。
                let anchor = lanes[cur].dots.last().map(|d| d.seq).unwrap_or(0);
                lanes.push(Lane {
                    name: name.to_string(),
                    dots: Vec::new(),
                    born: Some((cur, anchor)),
                });
                cur = lanes.len() - 1;
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
            let src = lower[6..].split_whitespace().next().unwrap_or("").trim();
            if let Some(from) = lanes.iter().position(|ln| ln.name == src) {
                // 合并锚点 = 来源车道此刻最后一个提交，折线从它水平延伸过来。
                let anchor = lanes[from].dots.last().map(|d| d.seq).unwrap_or(0);
                seq += 1;
                // Mermaid 的 merge 既可用 tag: 也可用 id: 标注，两者都要显示。
                let label = quoted_after(l, "tag:")
                    .or_else(|| quoted_after(l, "id:"))
                    .unwrap_or_default();
                lanes[cur].dots.push(Dot {
                    seq,
                    from: Some((from, anchor)),
                    label,
                });
            }
            continue;
        }
        if lower.starts_with("commit") {
            seq += 1;
            let label = quoted_after(l, "id:").unwrap_or_default();
            lanes[cur].dots.push(Dot {
                seq,
                from: None,
                label,
            });
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

    let lane_h = 64.0;
    let pad_l = 96.0;
    let w = ui.available_width().max(320.0).min(960.0);
    let h = lanes.len() as f32 * lane_h + 38.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let p = ui.painter();
    let seg = (w - pad_l - 48.0) / (total as f32 + 1.0);
    let x_of = |s: usize| rect.min.x + pad_l + s as f32 * seg;
    let y_of = |i: usize| rect.min.y + 26.0 + i as f32 * lane_h;

    // 1. 每条车道的灰色基准虚线（贯穿整幅图）
    let guide = theme.dim.gamma_multiply(0.55);
    for i in 0..lanes.len() {
        let y = y_of(i);
        let x_end = rect.min.x + w - 12.0;
        let mut x = rect.min.x + pad_l - 14.0;
        while x < x_end {
            let x2 = (x + 3.0).min(x_end);
            p.line_segment(
                [Pos2::new(x, y), Pos2::new(x2, y)],
                Stroke::new(1.0, guide),
            );
            x += 8.0;
        }
    }

    // 2. 分支折线：从父车道分出点垂直下来，再水平接到新车道首个提交
    for (i, lane) in lanes.iter().enumerate() {
        if let (Some((parent, anchor)), Some(first)) = (lane.born, lane.dots.first()) {
            let x = x_of(anchor);
            let y0 = y_of(parent);
            let y1 = y_of(i);
            let stroke = Stroke::new(4.0, git_color(i));
            p.line_segment([Pos2::new(x, y0), Pos2::new(x, y1)], stroke);
            p.line_segment([Pos2::new(x, y1), Pos2::new(x_of(first.seq), y1)], stroke);
        }
    }

    // 3. 每条车道自己的提交实线
    for (i, lane) in lanes.iter().enumerate() {
        let stroke = Stroke::new(4.0, git_color(i));
        for pair in lane.dots.windows(2) {
            p.line_segment(
                [
                    Pos2::new(x_of(pair[0].seq), y_of(i)),
                    Pos2::new(x_of(pair[1].seq), y_of(i)),
                ],
                stroke,
            );
        }
    }

    // 4. 合并折线：来源车道最后提交 → 水平到合并点 x → 垂直到目标车道
    for (i, lane) in lanes.iter().enumerate() {
        for dot in &lane.dots {
            if let Some((from, anchor)) = dot.from {
                let stroke = Stroke::new(4.0, git_color(from));
                let x0 = x_of(anchor);
                let y0 = y_of(from);
                let x1 = x_of(dot.seq);
                let y1 = y_of(i);
                p.line_segment([Pos2::new(x0, y0), Pos2::new(x1, y0)], stroke);
                p.line_segment([Pos2::new(x1, y0), Pos2::new(x1, y1)], stroke);
            }
        }
    }

    // 5. 节点与斜向哈希标签
    let label_font = FontId::proportional(12.0);
    for (i, lane) in lanes.iter().enumerate() {
        let color = git_color(i);
        for dot in &lane.dots {
            let c = Pos2::new(x_of(dot.seq), y_of(i));
            if dot.from.is_some() {
                // 合并点：空心圆（代码块底色填充 + 本车道色粗边），与实心提交点区分。
                p.circle_filled(c, 7.0, theme.code_bg);
                p.circle_stroke(c, 7.0, Stroke::new(3.0, color));
            } else {
                p.circle_filled(c, 7.0, color);
                p.circle_stroke(c, 7.0, Stroke::new(2.0, theme.code_bg));
            }
            if !dot.label.is_empty() {
                let galley = p.ctx().fonts_mut(|fonts| {
                    fonts.layout_job(egui::text::LayoutJob::simple_singleline(
                        dot.label.clone(),
                        label_font.clone(),
                        theme.text,
                    ))
                });
                // angle 顺时针为正，-45° 让文字沿左下→右上的 / 方向，
                // pivot 在文字左上角，故把起点放在节点左下方。
                p.add(egui::epaint::TextShape {
                    pos: Pos2::new(c.x - 30.0, c.y + 26.0),
                    galley,
                    underline: Stroke::NONE,
                    fallback_color: theme.text,
                    override_text_color: None,
                    opacity_factor: 1.0,
                    angle: -std::f32::consts::FRAC_PI_4,
                });
            }
        }
    }

    // 6. 分支名徽章（彩色圆角底 + 白字），画在最后以盖住线头
    let badge_font = FontId::proportional(13.0);
    for (i, lane) in lanes.iter().enumerate() {
        let y = y_of(i);
        let f = badge_font.clone();
        let galley = p.ctx().fonts_mut(|fonts| {
            fonts.layout_job(egui::text::LayoutJob::simple_singleline(
                lane.name.clone(),
                f,
                Color32::WHITE,
            ))
        });
        let badge = Rect::from_min_size(
            Pos2::new(rect.min.x + 6.0, y - (galley.rect.height() + 10.0) / 2.0),
            Vec2::new(galley.rect.width() + 16.0, galley.rect.height() + 10.0),
        );
        p.rect_filled(badge, 5.0, git_color(i));
        p.text(badge.center(), Align2::CENTER_CENTER, &lane.name, badge_font.clone(), Color32::WHITE);
    }

    resp
}
