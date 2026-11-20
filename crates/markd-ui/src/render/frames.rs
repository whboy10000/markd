//! 面板与代码块外框样式。

use super::*;

pub(crate) fn panel_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::NONE
        .fill(theme.code_bg)
        .corner_radius(6.0)
        .inner_margin(12.0)
}

/// 代码块功能栏与代码区之间的细分隔线。
pub(crate) fn code_divider(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme.border);
}

/// 代码块顶部功能栏的背景框：比显示区深一档（`code_header`），横贯盒子宽度。
pub(crate) fn code_header_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::NONE
        .fill(theme.code_header)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 4))
}

/// 叠在「不透明面板内容之上」的选区底色。
///
/// 代码块、表格单元格这些块画在自己的面板底色上，垫在下层的选区色会被面板盖掉，
/// 所以改用同色半透明盖在文字上面：文字依然清晰，又能看出选中了哪几行 / 哪几格。
pub(crate) fn selection_overlay(theme: &Theme) -> egui::Color32 {
    let c = theme.selection;
    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 110)
}
