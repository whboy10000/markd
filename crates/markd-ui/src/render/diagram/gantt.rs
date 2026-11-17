//! Mermaid 甘特图（gantt）。

use super::*;

/// 解析 mermaid 甘特图：section 分组 + 任务（日期/after 链 + 时长）。
/// 返回 (标题, [(分组或任务, 起始天, 时长天, 是否分组头, 颜色序号)])。
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

pub(crate) fn draw_gantt(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let mut title = String::new();
    enum Row {
        Section(String),
        Task { name: String, start: i64, dur: i64 },
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut task_ids: std::collections::HashMap<String, i64> = Default::default();
    let mut section_no = 0usize;
    let parse_date = |s: &str| -> Option<i64> {
        let p: Vec<i64> = s
            .split('-')
            .filter_map(|x| x.trim().parse::<i64>().ok())
            .collect();
        if p.len() == 3 {
            Some(days_from_civil(p[0], p[1], p[2]))
        } else {
            None
        }
    };
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") {
            continue;
        }
        let lower = l.to_lowercase();
        if lower == "gantt" {
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
                section_no += 1;
                rows.push(Row::Section(sec.trim().to_string()));
                continue;
            }
        }
        // 任务行：`名称 :规格`，规格逗号分隔：id / done|active|crit / 日期 / after id / 时长
        if let Some(ci) = l.find(':') {
            let name = l[..ci].trim().to_string();
            if name.is_empty() {
                continue;
            }
            let spec = l[ci + 1..].trim();
            let mut start: Option<i64> = None;
            let mut dur: i64 = 1;
            let mut id: Option<String> = None;
            for part in spec.split(',') {
                let p = part.trim();
                if p.is_empty() {
                    continue;
                }
                let pl = p.to_lowercase();
                if let Some(d) = parse_date(p) {
                    start = Some(d);
                } else if let Some(rest) = pl.strip_prefix("after ") {
                    start = task_ids.get(rest.trim()).copied();
                } else if let Some(num) = pl.strip_suffix('d') {
                    if let Ok(n) = num.trim().parse::<i64>() {
                        dur = n.max(1);
                    }
                } else if let Some(num) = pl.strip_suffix('w') {
                    if let Ok(n) = num.trim().parse::<i64>() {
                        dur = n.max(1) * 7;
                    }
                } else if id.is_none()
                    && !matches!(pl.as_str(), "done" | "active" | "crit" | "milestone")
                {
                    id = Some(p.to_string());
                }
            }
            let start = start.unwrap_or_else(|| {
                // 没有 after 也没有日期：接在时间线最右端
                rows.iter()
                    .filter_map(|r| match r {
                        Row::Task { start, dur, .. } => Some(start + dur),
                        _ => None,
                    })
                    .fold(i64::MAX, i64::min)
                    .min(i64::MAX - 1)
            });
            let start = if start == i64::MAX { 0 } else { start };
            if let Some(id) = id {
                task_ids.insert(id, start);
            }
            rows.push(Row::Task { name, start, dur });
        }
    }
    let tasks: Vec<(i64, i64)> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Task { start, dur, .. } => Some((*start, *dur)),
            _ => None,
        })
        .collect();
    if tasks.is_empty() {
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
    let t_min = tasks.iter().map(|t| t.0).min().unwrap_or(0);
    let t_max = tasks.iter().map(|t| t.0 + t.1).max().unwrap_or(1);
    let span = (t_max - t_min).max(1);
    let name_w = 110.0_f32;
    let row_h = 26.0;
    let title_h = if title.is_empty() { 0.0 } else { 22.0 };
    let axis_h = 22.0;
    let w = (ui.available_width().max(320.0)).min(900.0);
    let h = title_h + axis_h + rows.len() as f32 * row_h + 14.0;
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
    let tl_x = rect.min.x + name_w;
    let tl_w = rect.max.x - tl_x - 12.0;
    let x_of = |day: i64| tl_x + (day - t_min) as f32 / span as f32 * tl_w;
    let axis_y = rect.min.y + title_h;
    // 时间轴刻度：按总跨度取 4~6 个刻度
    let step = ((span + 4) / 5).max(1);
    let mut d = t_min;
    while d <= t_max {
        let x = x_of(d);
        p.line_segment(
            [Pos2::new(x, axis_y), Pos2::new(x, rect.max.y - 6.0)],
            Stroke::new(1.0, theme.border),
        );
        let (_, m, dd) = civil_from_days(d);
        p.text(
            Pos2::new(x + 2.0, axis_y + 2.0),
            Align2::LEFT_TOP,
            &format!("{m}/{dd}"),
            FontId::proportional(9.5),
            theme.dim,
        );
        d += step;
    }
    let mut y = axis_y + axis_h;
    let mut color_i = section_no.saturating_sub(1);
    for row in &rows {
        match row {
            Row::Section(s) => {
                color_i = section_no.saturating_sub(1);
                p.text(
                    Pos2::new(rect.min.x + 6.0, y + row_h / 2.0),
                    Align2::LEFT_CENTER,
                    s,
                    FontId::proportional(11.5),
                    theme.accent,
                );
            }
            Row::Task { name, start, dur } => {
                p.text(
                    Pos2::new(rect.min.x + 6.0, y + row_h / 2.0),
                    Align2::LEFT_CENTER,
                    name,
                    FontId::proportional(11.0),
                    theme.text,
                );
                let bx0 = x_of(*start);
                let bx1 = x_of(*start + *dur);
                let bar = Rect::from_min_max(
                    Pos2::new(bx0, y + 4.0),
                    Pos2::new(bx1.max(bx0 + 6.0), y + row_h - 4.0),
                );
                p.rect_filled(bar, 4.0, palette_color(theme, color_i));
                p.rect_stroke(
                    bar,
                    4.0,
                    Stroke::new(1.0, theme.border),
                    egui::StrokeKind::Outside,
                );
            }
        }
        y += row_h;
    }
    let _ = color_i;
    resp
}
