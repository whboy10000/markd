//! Mermaid 时间线（timeline）。
//!
//! 语法对齐官方：`title` 标题、`section` 分段、`时间 : 事件 : 事件`，
//! 以 `:` 开头的续行把事件补到上一个时间段；`<br>` 视为换行。
//! 画法：横向时间轴，每列是一个时间段——事件块自上而下堆叠在轴上，
//! 时间段名放在轴下方，section 以整列浅色底带区分。

use super::*;

pub(crate) struct TimelinePeriod {
    pub(crate) label: String,
    pub(crate) events: Vec<String>,
}

pub(crate) struct TimelineSection {
    pub(crate) name: String,
    pub(crate) periods: Vec<TimelinePeriod>,
}

/// 把 Mermaid 的 `<br>` 统一成换行，并去掉首尾空白。
fn timeline_text(s: &str) -> String {
    s.replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .trim()
        .to_string()
}

pub(crate) fn parse_timeline(code: &str) -> (String, Vec<TimelineSection>) {
    let mut title = String::new();
    let mut sections: Vec<TimelineSection> = Vec::new();
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") {
            continue;
        }
        let lower = l.to_lowercase();
        if lower.starts_with("timeline") {
            continue;
        }
        if lower.starts_with("title") {
            title = timeline_text(&l[5..]);
            continue;
        }
        if lower.starts_with("section") {
            sections.push(TimelineSection {
                name: timeline_text(&l[7..]),
                periods: Vec::new(),
            });
            continue;
        }
        // 没有 section 时归入默认段（段名为空，不画底带标题）。
        if sections.is_empty() {
            sections.push(TimelineSection {
                name: String::new(),
                periods: Vec::new(),
            });
        }
        let sec = sections.last_mut().expect("刚插入过默认段");
        let mut parts = l.split(':');
        let head = parts.next().unwrap_or_default().trim();
        let events: Vec<String> = parts
            .map(timeline_text)
            .filter(|e| !e.is_empty())
            .collect();
        if head.is_empty() {
            // 续行 `: 事件`：追加到上一个时间段。
            if let Some(period) = sec.periods.last_mut() {
                period.events.extend(events);
            }
        } else {
            sec.periods.push(TimelinePeriod {
                label: timeline_text(head),
                events,
            });
        }
    }
    (title, sections)
}

pub(crate) fn draw_timeline(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (title, sections) = parse_timeline(code);
    let flat: Vec<(usize, &TimelinePeriod)> = sections
        .iter()
        .enumerate()
        .flat_map(|(si, s)| s.periods.iter().map(move |p| (si, p)))
        .collect();
    if flat.is_empty() {
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

    let n = flat.len();
    let dark = theme.kind.is_dark();
    let avail = ui.available_width().max(320.0).min(980.0);
    let margin = 12.0;
    let col_w = ((avail - margin * 2.0) / n as f32).max(48.0);
    let canvas_w = margin * 2.0 + col_w * n as f32;

    // 先量出所有文字尺寸，再决定画布高度（长事件换行后会撑高整列）。
    let wrap = (col_w - 18.0).max(24.0);
    let event_font = FontId::proportional(11.0);
    let period_font = FontId::proportional(11.5);
    let period_color = if dark { theme.text } else { theme.heading };
    let ev_galleys: Vec<Vec<_>> = flat
        .iter()
        .map(|(_, p)| {
            p.events
                .iter()
                .map(|e| {
                    ui.fonts_mut(|f| f.layout(e.clone(), event_font.clone(), theme.text, wrap))
                })
                .collect()
        })
        .collect();
    let period_galleys: Vec<_> = flat
        .iter()
        .map(|(_, p)| {
            ui.fonts_mut(|f| f.layout(p.label.clone(), period_font.clone(), period_color, wrap))
        })
        .collect();
    let title_galley = if title.is_empty() {
        None
    } else {
        Some(
            ui.fonts_mut(|f| {
                f.layout(
                    title.clone(),
                    FontId::proportional(13.5),
                    theme.heading,
                    (canvas_w - 24.0).max(80.0),
                )
            }),
        )
    };

    let has_sections = sections.iter().any(|s| !s.name.is_empty());
    let section_galleys: Vec<Option<_>> = sections
        .iter()
        .map(|s| {
            if !has_sections || s.name.is_empty() {
                None
            } else {
                let band_w = (col_w * s.periods.len().max(1) as f32 - 16.0).max(40.0);
                Some(ui.fonts_mut(|f| {
                    f.layout(
                        s.name.clone(),
                        FontId::proportional(12.0),
                        theme.heading,
                        band_w,
                    )
                }))
            }
        })
        .collect();

    let ev_pad = 5.0;
    let ev_gap = 5.0;
    let col_h: Vec<f32> = ev_galleys
        .iter()
        .map(|gs| {
            let inner: f32 = gs.iter().map(|g| g.size().y + ev_pad * 2.0).sum();
            inner + ev_gap * gs.len().saturating_sub(1) as f32
        })
        .collect();
    let max_ev_h = col_h.iter().copied().fold(0.0, f32::max);
    let period_box_h = period_galleys
        .iter()
        .map(|g| g.size().y + 12.0)
        .fold(24.0, f32::max);
    let sec_h = if has_sections {
        section_galleys
            .iter()
            .flatten()
            .map(|g| g.size().y)
            .fold(0.0, f32::max)
            .max(16.0)
            + 10.0
    } else {
        0.0
    };
    let title_h = title_galley
        .as_ref()
        .map(|g| g.size().y + 8.0)
        .unwrap_or(0.0);
    let axis_gap = 12.0;
    let h = title_h + sec_h + max_ev_h + axis_gap + period_box_h + 12.0;

    let (rect, resp) = ui.allocate_exact_size(Vec2::new(canvas_w, h), Sense::click());
    let p = ui.painter();

    let x0 = rect.left() + margin;
    let mut y = rect.top() + 4.0;
    if let Some(g) = &title_galley {
        p.galley(
            Pos2::new(rect.center().x - g.size().x / 2.0, y),
            g.clone(),
            theme.heading,
        );
        y += title_h;
    }
    let body_top = y;
    let axis_y = body_top + sec_h + max_ev_h + axis_gap;
    let period_top = axis_y + 5.0;

    // section 底带与段名。
    let mut idx0 = 0usize;
    for (si, sec) in sections.iter().enumerate() {
        if sec.periods.is_empty() {
            continue;
        }
        let count = sec.periods.len();
        let bx0 = x0 + col_w * idx0 as f32;
        let bx1 = bx0 + col_w * count as f32;
        let base = palette_color(theme, si);
        if has_sections {
            let alpha = if dark { 60 } else { 110 };
            let band = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha);
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(bx0 + 2.0, body_top),
                    Pos2::new(bx1 - 2.0, period_top + period_box_h + 4.0),
                ),
                6.0,
                band,
            );
            if si > 0 {
                p.line_segment(
                    [
                        Pos2::new(bx0, body_top),
                        Pos2::new(bx0, period_top + period_box_h + 4.0),
                    ],
                    Stroke::new(1.0, theme.border),
                );
            }
            if let Some(g) = &section_galleys[si] {
                p.galley(
                    Pos2::new((bx0 + bx1) / 2.0 - g.size().x / 2.0, body_top + 5.0),
                    g.clone(),
                    theme.heading,
                );
            }
        }
        idx0 += count;
    }

    // 横向时间轴。
    p.line_segment(
        [
            Pos2::new(x0, axis_y),
            Pos2::new(x0 + col_w * n as f32, axis_y),
        ],
        Stroke::new(1.5, theme.border),
    );

    // 每一列：事件自上而下（第一个事件在最上），时间段名在轴下方。
    let mut ci = 0usize;
    for (si, sec) in sections.iter().enumerate() {
        let base = palette_color(theme, si);
        let fill = if dark { base.gamma_multiply(0.75) } else { base };
        let text_color = if dark { theme.text } else { theme.heading };
        for _ in &sec.periods {
            let cx = x0 + col_w * (ci as f32 + 0.5);
            let width = (col_w - 10.0).max(20.0);
            let left = cx - width / 2.0;
            let mut ey = body_top + sec_h + max_ev_h - col_h[ci];
            for g in &ev_galleys[ci] {
                let bh = g.size().y + ev_pad * 2.0;
                let box_rect = Rect::from_min_size(Pos2::new(left, ey), Vec2::new(width, bh));
                p.rect_filled(box_rect, 4.0, fill);
                p.rect_stroke(
                    box_rect,
                    4.0,
                    Stroke::new(1.0, theme.border),
                    egui::StrokeKind::Inside,
                );
                p.galley(
                    Pos2::new(box_rect.left() + 6.0, box_rect.top() + ev_pad),
                    g.clone(),
                    text_color,
                );
                ey += bh + ev_gap;
            }
            // 时间段名。
            let pr = Rect::from_min_size(
                Pos2::new(left, period_top),
                Vec2::new(width, period_box_h),
            );
            p.rect_filled(pr, 5.0, base);
            p.rect_stroke(
                pr,
                5.0,
                Stroke::new(1.2, theme.border),
                egui::StrokeKind::Inside,
            );
            let g = &period_galleys[ci];
            p.galley(
                Pos2::new(
                    pr.center().x - g.size().x / 2.0,
                    pr.center().y - g.size().y / 2.0,
                ),
                g.clone(),
                period_color,
            );
            // 轴上的时间点。
            p.circle_filled(Pos2::new(cx, axis_y), 3.0, base);
            p.circle_stroke(
                Pos2::new(cx, axis_y),
                3.0,
                Stroke::new(1.0, theme.border),
            );
            ci += 1;
        }
    }
    resp
}
