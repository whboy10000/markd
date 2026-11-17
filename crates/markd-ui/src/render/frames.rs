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
