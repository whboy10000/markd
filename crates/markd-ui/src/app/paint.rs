//! 工具条自绘图标与图标按钮。

use super::*;
use egui::{Align2, FontId};

/// 撤销 / 恢复图标：一段圆弧 + 箭头（WPS 那种弯曲的回退箭头）。
/// right = true 时左右镜像，用于「重做」。
pub(super) fn painter_undo_redo(
    painter: &egui::Painter,
    rect: egui::Rect,
    bg: egui::Color32,
    fg: egui::Color32,
    right: bool,
) {
    painter.rect_filled(rect, egui::CornerRadius::same(4), bg);
    let stroke = egui::Stroke::new(1.5, fg);
    let h = rect.height();
    let c = rect.center() + egui::vec2(0.0, h * 0.08);
    let r = h * 0.21;
    // 圆弧从左下绕到右下（屏幕上就是一条向上的半圆）
    let a0 = std::f32::consts::PI * 0.92;
    let a1 = std::f32::consts::PI * 2.18;
    let mirror = |p: egui::Pos2| {
        if right {
            egui::pos2(2.0 * c.x - p.x, p.y)
        } else {
            p
        }
    };
    let mut pts: Vec<egui::Pos2> = Vec::new();
    let steps = 18;
    for i in 0..=steps {
        let t = a0 + (a1 - a0) * (i as f32 / steps as f32);
        pts.push(mirror(egui::pos2(c.x + r * t.cos(), c.y + r * t.sin())));
    }
    // 箭头画在 a0 那一端，指向圆弧外侧
    let tip_dir = {
        let d = pts[0] - pts[1];
        d.normalized()
    };
    let tip = pts[0] + tip_dir * (r * 0.75);
    let perp = egui::vec2(-tip_dir.y, tip_dir.x) * (r * 0.55);
    painter.add(egui::Shape::convex_polygon(
        vec![tip, pts[0] + perp, pts[0] - perp],
        fg,
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::line(pts, stroke));
}

/// 在给定矩形里画一个放大镜：左上偏的圆 + 右下斜柄，圆内留空。
pub(super) fn painter_search_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    bg: egui::Color32,
    fg: egui::Color32,
) {
    painter.rect_filled(rect, egui::CornerRadius::same(4), bg);
    let stroke = egui::Stroke::new(1.8, fg);
    let h = rect.height();
    let r = h * 0.27;
    let c = rect.center() + egui::vec2(-2.0, -2.0);
    painter.circle_stroke(c, r, stroke);
    // 斜柄从圆的右下 45° 处向外伸，长度略大于半径，看起来才像放大镜而不是字母 Q。
    let d = std::f32::consts::FRAC_1_SQRT_2;
    let a = c + egui::vec2(r * d, r * d);
    let b = a + egui::vec2(r * 1.3 * d, r * 1.3 * d);
    painter.line_segment([a, b], stroke);
}

/// 下半区的功能按钮：一个独立圆角盒子，上半放图标、下半放文字。
pub(super) fn toolbar_box(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    hover: &str,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(TOOLBOX_WIDTH, TOOLBOX_HEIGHT), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&resp);
        let bg = if resp.hovered() {
            visuals.weak_bg_fill
        } else {
            ui.visuals().widgets.inactive.weak_bg_fill
        };
        let fg = ui.visuals().text_color();
        let painter = ui.painter();
        painter.rect_filled(rect, egui::CornerRadius::same(7), bg);
        // 上半区画图标，下半区居中写文字；中间不画分隔线，靠留白自然分层。
        let icon_rect = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.max.x, rect.top() + TOOLBOX_HEIGHT * 0.56),
        );
        toolbar_icon(painter, icon_rect, fg, icon);
        painter.text(
            egui::pos2(rect.center().x, rect.top() + TOOLBOX_HEIGHT * 0.79),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(10.5),
            fg,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hover)
}

/// 按功能名画一个简洁的线性图标。只用 ASCII 字形和 painter 图元，
/// 不依赖 emoji / 扩展符号字体，避免不同平台上缺字变成豆腐块。
pub(super) fn toolbar_icon(painter: &egui::Painter, rect: egui::Rect, fg: Color32, icon: &str) {
    let c = rect.center();
    let stroke = egui::Stroke::new(1.6, fg);
    let text = |s: &str, size: f32, at: egui::Pos2| {
        painter.text(at, Align2::CENTER_CENTER, s, FontId::proportional(size), fg);
    };
    match icon {
        "undo" => painter_undo_redo(painter, rect, Color32::TRANSPARENT, fg, false),
        "redo" => painter_undo_redo(painter, rect, Color32::TRANSPARENT, fg, true),
        "chevron_up" => {
            painter.line_segment(
                [c + egui::vec2(-6.0, 3.0), c + egui::vec2(0.0, -3.0)],
                egui::Stroke::new(1.8, fg),
            );
            painter.line_segment(
                [c + egui::vec2(0.0, -3.0), c + egui::vec2(6.0, 3.0)],
                egui::Stroke::new(1.8, fg),
            );
        }
        "chevron_down" => {
            painter.line_segment(
                [c + egui::vec2(-6.0, -3.0), c + egui::vec2(0.0, 3.0)],
                egui::Stroke::new(1.8, fg),
            );
            painter.line_segment(
                [c + egui::vec2(0.0, 3.0), c + egui::vec2(6.0, 3.0)],
                egui::Stroke::new(1.8, fg),
            );
        }
        "search" => painter_search_icon(painter, rect, Color32::TRANSPARENT, fg),
        "paragraph" => text("P", 21.0, c),
        "h1" => text("H1", 18.0, c),
        "h2" => text("H2", 18.0, c),
        "h3" => text("H3", 18.0, c),
        "h4" => text("H4", 18.0, c),
        "h5" => text("H5", 18.0, c),
        "h6" => text("H6", 18.0, c),
        "bold" => text("B", 21.0, c),
        "underline" => {
            text("U", 20.0, c - egui::vec2(0.0, 2.0));
            painter.line_segment(
                [c + egui::vec2(-8.0, 9.0), c + egui::vec2(8.0, 9.0)],
                stroke,
            );
        }
        "italic" => text("I", 21.0, c),
        "strike" => {
            text("S", 21.0, c);
            painter.line_segment(
                [c + egui::vec2(-9.0, 0.0), c + egui::vec2(9.0, 0.0)],
                stroke,
            );
        }
        "list" => {
            for (i, len) in [10.0_f32, 13.0, 8.0].into_iter().enumerate() {
                let y = c.y - 9.0 + i as f32 * 9.0;
                painter.circle_filled(egui::pos2(c.x - 10.0, y), 1.6, fg);
                painter.line_segment(
                    [egui::pos2(c.x - 5.0, y), egui::pos2(c.x - 5.0 + len, y)],
                    stroke,
                );
            }
        }
        "ordered_list" => {
            for (i, n) in ["1.", "2.", "3."].into_iter().enumerate() {
                let y = c.y - 9.0 + i as f32 * 9.0;
                text(n, 9.0, egui::pos2(c.x - 11.0, y));
                painter.line_segment(
                    [egui::pos2(c.x - 3.0, y), egui::pos2(c.x + 11.0, y)],
                    stroke,
                );
            }
        }
        "task_list" => {
            for i in 0..3 {
                let y = c.y - 9.0 + i as f32 * 9.0;
                let box_rect = egui::Rect::from_center_size(
                    egui::pos2(c.x - 10.0, y),
                    egui::vec2(8.0, 8.0),
                );
                painter.rect_stroke(
                    box_rect,
                    egui::CornerRadius::same(1),
                    stroke,
                    egui::StrokeKind::Outside,
                );
                if i == 0 {
                    painter.line_segment(
                        [egui::pos2(c.x - 13.0, y), egui::pos2(c.x - 10.0, y + 2.5)],
                        stroke,
                    );
                    painter.line_segment(
                        [egui::pos2(c.x - 10.0, y + 2.5), egui::pos2(c.x - 6.0, y - 3.0)],
                        stroke,
                    );
                }
                painter.line_segment(
                    [egui::pos2(c.x - 3.0, y), egui::pos2(c.x + 11.0, y)],
                    stroke,
                );
            }
        }
        "quote" => text("\"", 30.0, c),
        "image" => {
            let r = egui::Rect::from_center_size(c, egui::vec2(24.0, 18.0));
            painter.rect_stroke(r, egui::CornerRadius::same(2), stroke, egui::StrokeKind::Outside);
            painter.circle_filled(c + egui::vec2(-6.0, -4.0), 2.0, fg);
            painter.line_segment(
                [r.left_bottom() + egui::vec2(2.0, -1.0), r.right_bottom() - egui::vec2(2.0, 2.0)],
                stroke,
            );
        }
        "link" => {
            let a = c + egui::vec2(-5.0, -4.0);
            let b = c + egui::vec2(5.0, 4.0);
            painter.rect_stroke(
                egui::Rect::from_center_size(a, egui::vec2(14.0, 8.0)),
                egui::CornerRadius::same(3),
                stroke,
                egui::StrokeKind::Outside,
            );
            painter.rect_stroke(
                egui::Rect::from_center_size(b, egui::vec2(14.0, 8.0)),
                egui::CornerRadius::same(3),
                stroke,
                egui::StrokeKind::Outside,
            );
        }
        "formula" => text("f(x)", 15.0, c),
        "code" => text("</>", 14.0, c),
        "table" => {
            let r = egui::Rect::from_center_size(c, egui::vec2(24.0, 16.0));
            painter.rect_stroke(
                r,
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Outside,
            );
            painter.line_segment(
                [egui::pos2(c.x, r.top()), egui::pos2(c.x, r.bottom())],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(r.left(), c.y), egui::pos2(r.right(), c.y)],
                stroke,
            );
        }
        "rule" => {
            for (dy, len) in [(-5.0_f32, 8.0), (0.0, 12.0), (5.0, 8.0)] {
                painter.line_segment(
                    [c + egui::vec2(-len, dy), c + egui::vec2(len, dy)],
                    stroke,
                );
            }
        }
        "chart" => {
            painter.line_segment(
                [c + egui::vec2(-12.0, 9.0), c + egui::vec2(12.0, 9.0)],
                stroke,
            );
            painter.line_segment(
                [c + egui::vec2(-12.0, 9.0), c + egui::vec2(-12.0, -9.0)],
                stroke,
            );
            for (i, h) in [7.0_f32, 12.0, 9.0].into_iter().enumerate() {
                let x = c.x - 7.0 + i as f32 * 7.0;
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x - 2.0, c.y + 8.0 - h),
                        egui::pos2(x + 2.0, c.y + 8.0),
                    ),
                    egui::CornerRadius::same(1),
                    fg,
                );
            }
        }
        "flowchart" => {
            let a = egui::Rect::from_center_size(c + egui::vec2(-9.0, -5.0), egui::vec2(10.0, 7.0));
            let b = egui::Rect::from_center_size(c + egui::vec2(9.0, 5.0), egui::vec2(10.0, 7.0));
            painter.rect_stroke(
                a,
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Outside,
            );
            painter.rect_stroke(
                b,
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Outside,
            );
            let mid = egui::pos2(c.x, a.center().y);
            painter.line_segment([egui::pos2(a.right(), mid.y), mid], stroke);
            painter.line_segment([mid, egui::pos2(mid.x, b.center().y)], stroke);
            painter.line_segment(
                [egui::pos2(mid.x, b.center().y), egui::pos2(b.left(), b.center().y)],
                stroke,
            );
        }
        "footnote" => text("FN", 14.0, c),
        "toc" => text("TOC", 12.0, c),
        "upload" => {
            let top = c + egui::vec2(0.0, -9.0);
            let bottom = c + egui::vec2(0.0, 5.0);
            painter.line_segment([top, bottom], stroke);
            painter.line_segment([top, c + egui::vec2(-5.0, -4.0)], stroke);
            painter.line_segment([top, c + egui::vec2(5.0, -4.0)], stroke);
            painter.line_segment(
                [c + egui::vec2(-10.0, 9.0), c + egui::vec2(10.0, 9.0)],
                stroke,
            );
        }
        "mermaid" => text("M", 21.0, c),
        "snapshot" => {
            painter.circle_stroke(c, 11.0, stroke);
            painter.line_segment([c, c + egui::vec2(0.0, -7.0)], stroke);
            painter.line_segment([c, c + egui::vec2(5.0, 3.0)], stroke);
        }
        "graph" => {
            let p = [c + egui::vec2(-10.0, 6.0), c + egui::vec2(9.0, -7.0), c + egui::vec2(10.0, 8.0)];
            painter.line_segment([p[0], p[1]], stroke);
            painter.line_segment([p[0], p[2]], stroke);
            for q in p {
                painter.circle_filled(q, 3.2, fg);
            }
        }
        "stats" => {
            for (i, h) in [10.0_f32, 17.0, 13.0].into_iter().enumerate() {
                let x = c.x - 10.0 + i as f32 * 10.0;
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x - 3.0, c.y + 9.0 - h),
                        egui::pos2(x + 3.0, c.y + 9.0),
                    ),
                    egui::CornerRadius::same(1),
                    fg,
                );
            }
        }
        _ => text("?", 20.0, c),
    }
}
