//! 工具条自绘图标与图标按钮。

use super::*;

/// 工具条上的自绘图标按钮（放大镜等）：不依赖 emoji 字体，
/// 尺寸按普通按钮算（文字高度 + 上下内边距），底色与描边色都跟随主题。
pub(super) fn icon_button(
    ui: &mut egui::Ui,
    hover: &str,
    draw: fn(&egui::Painter, egui::Rect, egui::Color32, egui::Color32),
) -> bool {
    let pad = ui.spacing().button_padding;
    let h = (ui.text_style_height(&egui::TextStyle::Button) + 2.0 * pad.y)
        .max(ui.spacing().interact_size.y);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(h + 2.0, h), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&resp);
        // 底色用 weak_bg_fill（和普通 Button 同一个字段）；描边色用主题文字色：
        // theme.apply() 设的是 override_text_color，不读它自制图标会和按钮文字不同色。
        draw(
            ui.painter(),
            rect,
            visuals.weak_bg_fill,
            ui.visuals().text_color(),
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::Default)
        .on_hover_text(hover)
        .clicked()
}

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

/// 撤销图标（向左回退）
pub(super) fn painter_undo_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    bg: egui::Color32,
    fg: egui::Color32,
) {
    painter_undo_redo(painter, rect, bg, fg, false);
}

/// 重做图标（向右回退）
pub(super) fn painter_redo_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    bg: egui::Color32,
    fg: egui::Color32,
) {
    painter_undo_redo(painter, rect, bg, fg, true);
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
