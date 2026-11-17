//! Mermaid 图表渲染：流程图、时序图、饼图、类图、ER 图、甘特图、旅程图与 Git 图。
//! 所有图表都直接用 egui 画布绘制，不依赖外部渲染器。

use super::frames::*;
use crate::theme::Theme;
use egui::{self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::collections::HashMap;

mod boxes;
mod blockdiagram;
mod c4;
mod class;
mod cynefin;
mod er;
mod flow;
mod gantt;
mod git;
mod journey;
mod kanban;
mod mindmap;
mod packet;
mod pie;
mod quadrant;
mod radar;
mod railroad;
mod requirement;
mod sankey;
mod sequence;
mod swimlane;
mod timeline;
mod treemap;
mod xychart;

pub(crate) use self::{
    boxes::*, blockdiagram::*, c4::*, class::*, cynefin::*, er::*, flow::*, gantt::*, git::*,
    journey::*, kanban::*, mindmap::*, packet::*, pie::*, quadrant::*, radar::*, railroad::*,
    requirement::*, sankey::*, sequence::*, swimlane::*, timeline::*, treemap::*, xychart::*,
};

/// 无法识别的图表源码：按代码保真显示（绝不乱画）。
pub(crate) fn raw_code(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
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
    r.response.interact(Sense::click())
}
