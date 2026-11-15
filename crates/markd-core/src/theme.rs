//! 主题系统：内置多套配色 + 全局字号/行高设置。

use egui::{Color32, Context, FontFamily, FontId, Visuals};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeKind {
    #[default]
    Light,
    Dark,
    Github,
    Sepia,
    Nord,
    Solarized,
}

impl ThemeKind {
    pub const ALL: [ThemeKind; 6] = [
        ThemeKind::Light,
        ThemeKind::Dark,
        ThemeKind::Github,
        ThemeKind::Sepia,
        ThemeKind::Nord,
        ThemeKind::Solarized,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ThemeKind::Light => "默认 · 浅色",
            ThemeKind::Dark => "默认 · 深色",
            ThemeKind::Github => "GitHub",
            ThemeKind::Sepia => "羊皮纸",
            ThemeKind::Nord => "Nord",
            ThemeKind::Solarized => "Solarized",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, ThemeKind::Dark | ThemeKind::Nord | ThemeKind::Solarized)
    }
}

#[derive(Clone)]
pub struct Theme {
    pub kind: ThemeKind,
    pub bg: Color32,
    pub panel: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub code_bg: Color32,
    pub border: Color32,
    pub heading: Color32,
    pub link: Color32,
    pub quote: Color32,
    pub table_head: Color32,
    pub selection: Color32,
    pub body: f32,
    pub mono: f32,
}

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Theme {
    pub fn get(kind: ThemeKind) -> Self {
        let (bg, panel, text, dim, accent, code_bg, border, heading, link, quote, table_head, selection) = match kind {
            ThemeKind::Light => (
                rgb(255, 255, 255), rgb(247, 247, 248), rgb(38, 38, 40), rgb(140, 140, 145),
                rgb(46, 117, 182), rgb(245, 245, 247), rgb(224, 224, 228), rgb(28, 28, 30),
                rgb(46, 117, 182), rgb(120, 120, 125), rgb(249, 249, 251), rgb(200, 222, 248),
            ),
            ThemeKind::Dark => (
                rgb(32, 33, 36), rgb(26, 27, 30), rgb(222, 224, 227), rgb(120, 122, 128),
                rgb(108, 166, 240), rgb(42, 44, 48), rgb(62, 64, 70), rgb(240, 242, 245),
                rgb(120, 178, 250), rgb(140, 142, 148), rgb(38, 40, 44), rgb(55, 90, 140),
            ),
            ThemeKind::Github => (
                rgb(255, 255, 255), rgb(246, 248, 250), rgb(36, 41, 47), rgb(110, 118, 129),
                rgb(9, 105, 218), rgb(246, 248, 250), rgb(208, 215, 222), rgb(27, 31, 36),
                rgb(9, 105, 218), rgb(89, 99, 110), rgb(246, 248, 250), rgb(181, 213, 255),
            ),
            ThemeKind::Sepia => (
                rgb(244, 236, 216), rgb(236, 226, 203), rgb(76, 60, 42), rgb(150, 132, 105),
                rgb(155, 82, 44), rgb(236, 226, 205), rgb(214, 199, 172), rgb(66, 48, 32),
                rgb(155, 82, 44), rgb(133, 112, 85), rgb(238, 229, 208), rgb(222, 197, 160),
            ),
            ThemeKind::Nord => (
                rgb(46, 52, 64), rgb(40, 46, 58), rgb(216, 222, 233), rgb(122, 132, 148),
                rgb(136, 192, 208), rgb(52, 59, 72), rgb(67, 76, 94), rgb(236, 239, 244),
                rgb(143, 188, 187), rgb(135, 146, 163), rgb(50, 57, 70), rgb(76, 106, 133),
            ),
            ThemeKind::Solarized => (
                rgb(0, 43, 54), rgb(7, 54, 66), rgb(131, 148, 150), rgb(101, 123, 131),
                rgb(38, 139, 210), rgb(7, 54, 66), rgb(30, 76, 88), rgb(147, 161, 161),
                rgb(42, 161, 152), rgb(101, 123, 131), rgb(10, 60, 72), rgb(32, 92, 112),
            ),
        };
        Self {
            kind, bg, panel, text, dim, accent, code_bg, border, heading, link, quote,
            table_head, selection, body: 16.0, mono: 13.5,
        }
    }

    pub fn heading_size(&self, level: u8) -> f32 {
        let factor = match level {
            1 => 1.9,
            2 => 1.55,
            3 => 1.3,
            4 => 1.15,
            5 => 1.05,
            _ => 1.0,
        };
        self.body * factor
    }

    pub fn fmt(&self, size: f32) -> egui::text::TextFormat {
        egui::text::TextFormat {
            font_id: FontId::new(size, FontFamily::Proportional),
            color: self.text,
            ..Default::default()
        }
    }

    pub fn mono_fmt(&self) -> egui::text::TextFormat {
        egui::text::TextFormat {
            font_id: FontId::new(self.mono, FontFamily::Monospace),
            color: self.text,
            ..Default::default()
        }
    }

    pub fn bold_fmt(&self, size: f32, color: Color32) -> egui::text::TextFormat {
        egui::text::TextFormat {
            font_id: FontId::new(size, FontFamily::Proportional),
            color,
            coords: egui::epaint::text::VariationCoords::new([(b"wght", 700.0)]),
            ..Default::default()
        }
    }

    pub fn apply(&self, ctx: &Context) {
        let mut visuals = if self.kind.is_dark() { Visuals::dark() } else { Visuals::light() };
        visuals.override_text_color = Some(self.text);
        visuals.panel_fill = self.panel;
        visuals.window_fill = self.bg;
        visuals.extreme_bg_color = self.code_bg;
        visuals.hyperlink_color = self.link;
        visuals.selection.bg_fill = self.selection;
        visuals.widgets.noninteractive.bg_fill = self.bg;
        visuals.widgets.inactive.bg_fill = self.code_bg;
        visuals.widgets.hovered.bg_fill = self.border;
        visuals.widgets.active.bg_fill = self.selection;
        ctx.set_visuals(visuals);
    }
}
