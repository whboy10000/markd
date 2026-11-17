//! Mermaid XY 图表（xychart-beta）。

use super::*;

pub(crate) struct XyChart {
    pub(crate) title: String,
    pub(crate) x_labels: Vec<String>,
    pub(crate) y_title: String,
    pub(crate) y_min: f32,
    pub(crate) y_max: f32,
    pub(crate) bars: Vec<f32>,
    pub(crate) line: Vec<f32>,
}

fn list_values(s: &str) -> Vec<f32> {
    let Some(open) = s.find('[') else {
        return Vec::new();
    };
    let Some(close) = s[open..].find(']') else {
        return Vec::new();
    };
    s[open + 1..open + close]
        .split(',')
        .filter_map(|v| v.trim().parse::<f32>().ok())
        .collect()
}

fn list_strings(s: &str) -> Vec<String> {
    let Some(open) = s.find('[') else {
        return Vec::new();
    };
    let Some(close) = s[open..].find(']') else {
        return Vec::new();
    };
    s[open + 1..open + close]
        .split(',')
        .map(|v| v.trim().trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

pub(crate) fn parse_xychart(code: &str) -> XyChart {
    let mut c = XyChart {
        title: String::new(),
        x_labels: Vec::new(),
        y_title: String::new(),
        y_min: 0.0,
        y_max: 100.0,
        bars: Vec::new(),
        line: Vec::new(),
    };
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.to_lowercase().starts_with("xychart") {
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            c.title = t.trim().trim_matches('"').to_string();
        } else if let Some(rest) = l.strip_prefix("x-axis") {
            c.x_labels = list_strings(rest);
        } else if let Some(rest) = l.strip_prefix("y-axis") {
            if let Some((name, range)) = rest.split_once("-->") {
                let head = name.trim();
                c.y_title = quoted_head(head);
                if let Some(min) = head.split_whitespace().last().and_then(|v| v.parse().ok()) {
                    c.y_min = min;
                }
                c.y_max = range.trim().parse().unwrap_or(c.y_max);
            } else {
                c.y_title = rest.trim().trim_matches('"').to_string();
            }
        } else if let Some(rest) = l.strip_prefix("bar") {
            c.bars = list_values(rest);
        } else if let Some(rest) = l.strip_prefix("line") {
            c.line = list_values(rest);
        }
    }
    c
}

fn quoted_head(s: &str) -> String {
    if let Some(start) = s.find('"')
        && let Some(end) = s[1..].find('"')
    {
        return s[start + 1..start + 1 + end].to_string();
    }
    s.trim().to_string()
}

pub(crate) fn draw_xychart(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let c = parse_xychart(code);
    let n = c.bars.len().max(c.line.len()).max(c.x_labels.len());
    if n == 0 {
        return raw_code(ui, code, theme);
    }
    let max_val = c
        .bars
        .iter()
        .chain(c.line.iter())
        .copied()
        .fold(c.y_max, f32::max);
    let max_val = if max_val > c.y_min { max_val } else { c.y_min + 1.0 };
    let avail = ui.available_width().max(320.0).min(760.0);
    let title_h = if c.title.is_empty() { 0.0 } else { 22.0 };
    let plot_h = 190.0;
    let left = 48.0;
    let right = 12.0;
    let bottom = 26.0;
    let top = title_h + 10.0;
    let h = top + plot_h + bottom + 6.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail, h), Sense::click());
    let p = ui.painter();
    if !c.title.is_empty() {
        p.text(
            Pos2::new(rect.center().x, rect.top() + 2.0),
            Align2::CENTER_TOP,
            &c.title,
            FontId::proportional(13.0),
            theme.heading,
        );
    }
    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + left, rect.top() + top),
        Pos2::new(rect.right() - right, rect.top() + top + plot_h),
    );
    // 横向网格 + y 轴刻度。
    for i in 0..=4 {
        let y = plot.bottom() - plot.height() * i as f32 / 4.0;
        p.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(1.0, theme.border.gamma_multiply(0.7)),
        );
        let v = c.y_min + (max_val - c.y_min) * i as f32 / 4.0;
        p.text(
            Pos2::new(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            format!("{v:.0}"),
            FontId::proportional(9.5),
            theme.dim,
        );
    }
    if !c.y_title.is_empty() {
        p.text(
            Pos2::new(rect.left() + 2.0, plot.top()),
            Align2::LEFT_BOTTOM,
            &c.y_title,
            FontId::proportional(10.0),
            theme.dim,
        );
    }
    let slot = plot.width() / n as f32;
    let y_of = |v: f32| -> f32 {
        let t = ((v - c.y_min) / (max_val - c.y_min)).clamp(0.0, 1.0);
        plot.bottom() - t * plot.height()
    };
    // 柱状。
    let bar_w = (slot * 0.55).max(4.0);
    for (i, v) in c.bars.iter().enumerate() {
        let cx = plot.left() + slot * (i as f32 + 0.5);
        let y = y_of(*v);
        let bar = Rect::from_min_max(
            Pos2::new(cx - bar_w / 2.0, y),
            Pos2::new(cx + bar_w / 2.0, plot.bottom()),
        );
        p.rect_filled(bar, 2.0, theme.accent.gamma_multiply(0.55));
    }
    // 折线。
    if c.line.len() > 1 {
        let pts: Vec<Pos2> = c
            .line
            .iter()
            .enumerate()
            .map(|(i, v)| Pos2::new(plot.left() + slot * (i as f32 + 0.5), y_of(*v)))
            .collect();
        for w in pts.windows(2) {
            p.line_segment([w[0], w[1]], Stroke::new(2.0, theme.accent));
        }
        for pt in &pts {
            p.circle_filled(*pt, 3.0, theme.accent);
        }
    }
    // x 轴标签。
    for (i, label) in c.x_labels.iter().enumerate() {
        if i >= n {
            break;
        }
        let cx = plot.left() + slot * (i as f32 + 0.5);
        p.text(
            Pos2::new(cx, plot.bottom() + 5.0),
            Align2::CENTER_TOP,
            label,
            FontId::proportional(10.0),
            theme.dim,
        );
    }
    resp
}
