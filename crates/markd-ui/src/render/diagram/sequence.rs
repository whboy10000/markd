//! Mermaid 时序图（sequenceDiagram）。

use super::*;

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum SeqItem {
    /// from/to 是参与者下标；dashed = `-->`，cross = `x` 结尾箭头（-x / --x）。
    Message {
        from: usize,
        to: usize,
        label: String,
        dashed: bool,
        cross: bool,
    },
    /// over 是参与者下标区间（单元素 = 单个参与者上方）。
    Note { over: (usize, usize), label: String },
}

pub(crate) struct SeqParticipant {
    pub(crate) id: String,
    pub(crate) label: String,
}

/// 解析 sequenceDiagram：participant/actor 声明、四类箭头消息、Note、title。
/// 识别不了的控制行（activate/loop/alt/autonumber 等）直接跳过，不影响其余内容。
pub(crate) fn parse_sequence(code: &str) -> (Vec<SeqParticipant>, Vec<SeqItem>, String) {
    let mut parts: Vec<SeqParticipant> = Vec::new();
    let mut items: Vec<SeqItem> = Vec::new();
    let mut title = String::new();

    fn ensure_part(parts: &mut Vec<SeqParticipant>, id: &str) -> usize {
        if let Some(i) = parts.iter().position(|p| p.id == id) {
            return i;
        }
        parts.push(SeqParticipant {
            id: id.to_string(),
            label: id.to_string(),
        });
        parts.len() - 1
    }

    for raw in code.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_lowercase();
        if lower == "sequencediagram" || lower.starts_with("sequencediagram ") {
            continue;
        }
        if lower.starts_with("title") {
            title = line[5..].trim().to_string();
            continue;
        }
        if lower.starts_with("participant ") || lower.starts_with("actor ") {
            let rest = &line[lower.find(' ').unwrap_or(0) + 1..];
            let (pid, alias) = match rest.find(" as ") {
                Some(i) => (rest[..i].trim(), rest[i + 4..].trim()),
                None => (rest.trim(), ""),
            };
            if pid.is_empty() {
                continue;
            }
            let idx = ensure_part(&mut parts, pid);
            if !alias.is_empty() {
                parts[idx].label = alias.to_string();
            }
            continue;
        }
        if lower.starts_with("note ") {
            let rest = &line[5..];
            let (scope, text) = match rest.find(':') {
                Some(i) => (&rest[..i], rest[i + 1..].trim()),
                None => (rest, ""),
            };
            let st = scope.trim();
            // 参与者名区分大小写，只对前缀关键字做小写匹配，截取时用原文
            let sl = st.to_lowercase();
            let ids: Vec<&str> = if sl.strip_prefix("over ").is_some() {
                st[5..]
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else if sl.strip_prefix("left of ").is_some() {
                vec![st[8..].trim()]
            } else if sl.strip_prefix("right of ").is_some() {
                vec![st[9..].trim()]
            } else {
                vec![]
            };
            if ids.is_empty() || text.is_empty() {
                continue;
            }
            let idxs: Vec<usize> = ids.iter().map(|id| ensure_part(&mut parts, id)).collect();
            let lo = idxs.iter().copied().min().unwrap_or(0);
            let hi = idxs.iter().copied().max().unwrap_or(0);
            items.push(SeqItem::Note {
                over: (lo, hi),
                label: text.to_string(),
            });
            continue;
        }
        // 控制行：跳过（activate/autonumber/loop/alt/else/end/opt/par/rect 等）
        const SKIP: [&str; 11] = [
            "autonumber",
            "activate ",
            "deactivate ",
            "loop ",
            "alt ",
            "else",
            "end",
            "opt ",
            "par ",
            "rect ",
            "critical ",
        ];
        if SKIP.iter().any(|k| lower == *k || lower.starts_with(k)) {
            continue;
        }
        // 消息行：`A->>B: 文本` / `A-->>B` / `A-x B` / `A--xB`
        if let Some(ci) = line.find(':') {
            let (left, label) = (&line[..ci], line[ci + 1..].trim());
            // 依次找箭头标记。必须先匹配更长的 "-->>"/"--x"：如果把 "->>"
            // 排在前面，"D-->>S" 会命中第二个 '-' 开始的 "->>"，把发送方
            // 切成 "D-"、接收方切成 "S-"，凭空多出两个参与者。
            const ARROWS: [&str; 6] = ["-->>", "->>", "--x", "-x", "--", "->"];
            if let Some(arrow) = ARROWS.iter().find(|a| left.contains(**a)) {
                let ai = left.find(arrow).unwrap();
                let a = left[..ai].trim();
                let b = left[ai + arrow.len()..].trim();
                if a.is_empty() || b.is_empty() {
                    continue;
                }
                let dashed = arrow.starts_with("--");
                let cross = arrow.ends_with('x');
                let from = ensure_part(&mut parts, a);
                let to = ensure_part(&mut parts, b);
                items.push(SeqItem::Message {
                    from,
                    to,
                    label: label.to_string(),
                    dashed,
                    cross,
                });
            }
        }
    }
    (parts, items, title)
}

/// 虚线：egui 的 Stroke 不支持 dash，手动画短段。
pub(crate) fn paint_dashed_line(
    p: &egui::Painter,
    from: Pos2,
    to: Pos2,
    color: Color32,
    width: f32,
) {
    let dash = 5.0;
    let gap = 3.0;
    let len = (to - from).length();
    if len <= 0.5 {
        return;
    }
    let dir = (to - from) / len;
    let mut t = 0.0;
    while t < len {
        let end = (t + dash).min(len);
        p.line_segment(
            [from + dir * t, from + dir * end],
            Stroke::new(width, color),
        );
        t = end + gap;
    }
}

pub(crate) fn draw_sequence(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (parts, items, title) = parse_sequence(code);
    if parts.is_empty() {
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
    let row_h = 34.0;
    let self_h = 44.0;
    let note_h = 30.0;
    let box_h = 26.0;
    let pad = 14.0;
    let title_h = if title.is_empty() { 0.0 } else { 22.0 };
    let n = parts.len();
    let w = ui.available_width().max(260.0);
    let slots = (w - 2.0 * pad) / n as f32;
    let total_h = title_h + box_h + 10.0 + items.len() as f32 * row_h + 14.0;
    // Sense::click 让图形可点击进入编辑态改代码（与流程图/饼图一致）。
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, total_h), Sense::click());
    let p = ui.painter();

    // 参与者水平均分；标签过长的盒子以文字宽度为准，但不超过槽宽
    let x_of = |i: usize| rect.min.x + pad + slots * (i as f32 + 0.5);
    if !title.is_empty() {
        p.text(
            Pos2::new(rect.center().x, rect.min.y + 2.0),
            Align2::CENTER_TOP,
            &title,
            FontId::proportional(13.0),
            theme.heading,
        );
    }
    let top_y = rect.min.y + title_h + 2.0;
    let bottom_y = rect.max.y - 10.0;
    // 生命线（虚线）先画，箭头和盒子盖在上面
    for i in 0..n {
        let x = x_of(i);
        paint_dashed_line(
            p,
            Pos2::new(x, top_y + box_h),
            Pos2::new(x, bottom_y),
            theme.dim.gamma_multiply(0.6),
            1.0,
        );
    }
    // 参与者盒子
    for (i, part) in parts.iter().enumerate() {
        let cx = x_of(i);
        let bw = ((part.label.chars().count() as f32) * 12.0 + 20.0).min((slots - 6.0).max(40.0));
        let bx = Rect::from_center_size(
            Pos2::new(cx, top_y + box_h / 2.0),
            Vec2::new(bw, box_h - 6.0),
        );
        p.rect_filled(bx, 4.0, theme.table_head);
        p.rect_stroke(
            bx,
            4.0,
            Stroke::new(1.0, theme.border),
            egui::StrokeKind::Outside,
        );
        p.text(
            bx.center(),
            Align2::CENTER_CENTER,
            &part.label,
            FontId::proportional(12.0),
            theme.text,
        );
    }
    // 消息 / Note 按源码顺序排布
    let mut y = top_y + box_h + 12.0;
    for it in &items {
        match it {
            SeqItem::Message {
                from,
                to,
                label,
                dashed,
                cross,
            } => {
                let x0 = x_of(*from);
                let x1 = x_of(*to);
                if from == to {
                    // 自环消息：向右绕一个小圈，标签写在右侧
                    let x = x0;
                    let loop_w = 26.0;
                    let y1 = y + 6.0;
                    let y2 = y + 26.0;
                    p.line_segment(
                        [Pos2::new(x, y1), Pos2::new(x + loop_w, y1)],
                        Stroke::new(1.0, theme.text),
                    );
                    p.line_segment(
                        [Pos2::new(x + loop_w, y1), Pos2::new(x + loop_w, y2)],
                        Stroke::new(1.0, theme.text),
                    );
                    p.line_segment(
                        [Pos2::new(x + loop_w, y2), Pos2::new(x + 2.0, y2)],
                        Stroke::new(1.0, theme.text),
                    );
                    // 箭头尖（指向左，回到生命线）
                    p.line_segment(
                        [Pos2::new(x + 2.0, y2), Pos2::new(x + 9.0, y2 - 4.0)],
                        Stroke::new(1.0, theme.text),
                    );
                    p.line_segment(
                        [Pos2::new(x + 2.0, y2), Pos2::new(x + 9.0, y2 + 4.0)],
                        Stroke::new(1.0, theme.text),
                    );
                    if !label.is_empty() {
                        p.text(
                            Pos2::new(x + loop_w + 8.0, (y1 + y2) / 2.0),
                            Align2::LEFT_CENTER,
                            label,
                            FontId::proportional(12.0),
                            theme.text,
                        );
                    }
                    y += self_h;
                } else {
                    let (xa, xb) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
                    if *dashed {
                        paint_dashed_line(p, Pos2::new(x0, y), Pos2::new(x1, y), theme.text, 1.0);
                    } else {
                        p.line_segment(
                            [Pos2::new(x0, y), Pos2::new(x1, y)],
                            Stroke::new(1.2, theme.text),
                        );
                    }
                    // 箭头尖（指向 to 端）
                    let dir = ((x1 - x0) as f32).signum();
                    let tip = Pos2::new(x1, y);
                    if *cross {
                        // -x：叉形箭头
                        p.line_segment(
                            [
                                tip - egui::vec2(7.0 * dir, 6.0),
                                tip + egui::vec2(1.0 * dir, 0.0),
                            ],
                            Stroke::new(1.2, theme.text),
                        );
                        p.line_segment(
                            [
                                tip - egui::vec2(7.0 * dir, -6.0),
                                tip + egui::vec2(1.0 * dir, 0.0),
                            ],
                            Stroke::new(1.2, theme.text),
                        );
                    } else {
                        // 三角箭头
                        p.line_segment(
                            [tip - egui::vec2(9.0 * dir, 5.0), tip],
                            Stroke::new(1.2, theme.text),
                        );
                        p.line_segment(
                            [tip - egui::vec2(9.0 * dir, -5.0), tip],
                            Stroke::new(1.2, theme.text),
                        );
                    }
                    if !label.is_empty() {
                        p.text(
                            Pos2::new((xa + xb) / 2.0, y - 6.0),
                            Align2::CENTER_BOTTOM,
                            label,
                            FontId::proportional(12.0),
                            theme.text,
                        );
                    }
                    y += row_h;
                }
            }
            SeqItem::Note { over, label } => {
                let xa = x_of(over.0);
                let xb = x_of(over.1);
                let cx = (xa + xb) / 2.0;
                let bw = ((label.chars().count() as f32) * 12.0 + 24.0)
                    .max(60.0)
                    .min((xb - xa + slots * 0.9).max(60.0));
                let nb = Rect::from_center_size(
                    Pos2::new(cx, y + note_h / 2.0 - 4.0),
                    Vec2::new(bw, note_h - 10.0),
                );
                p.rect_filled(nb, 2.0, theme.code_bg);
                p.rect_stroke(
                    nb,
                    2.0,
                    Stroke::new(1.0, theme.border),
                    egui::StrokeKind::Outside,
                );
                p.text(
                    nb.center(),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(11.5),
                    theme.dim,
                );
                y += note_h;
            }
        }
    }
    resp
}
