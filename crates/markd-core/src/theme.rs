//! Author: aurelianus <aurelianus@126.com>

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
        matches!(
            self,
            ThemeKind::Dark | ThemeKind::Nord | ThemeKind::Solarized
        )
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
        let (
            bg,
            panel,
            text,
            dim,
            accent,
            code_bg,
            border,
            heading,
            link,
            quote,
            table_head,
            selection,
        ) = match kind {
            // 对齐 VS Code 默认浅色（Light+）：editor #FFFFFF、侧栏 #F3F3F3、链接 #006AB1、选区 #ADD6FF
            // 代码块背景 #F5F5F5（比正文 #FFFFFF 略深，比侧栏 #F3F3F3 略浅）
            ThemeKind::Light => (
                rgb(255, 255, 255),
                rgb(243, 243, 243),
                rgb(0, 0, 0),
                rgb(139, 139, 139),
                rgb(0, 106, 177),
                rgb(245, 245, 245),
                rgb(229, 229, 229),
                rgb(0, 0, 0),
                rgb(0, 106, 177),
                rgb(106, 115, 125),
                rgb(231, 231, 231),
                rgb(173, 214, 255),
            ),
            // 对齐 VS Code 默认深色（Dark+）：editor #1E1E1E、侧栏 #252526、链接 #3794FF、选区 #264F78
            ThemeKind::Dark => (
                rgb(30, 30, 30),
                rgb(37, 37, 38),
                rgb(212, 212, 212),
                rgb(133, 133, 133),
                rgb(55, 148, 255),
                rgb(45, 45, 45),
                rgb(69, 69, 69),
                rgb(255, 255, 255),
                rgb(55, 148, 255),
                rgb(156, 163, 175),
                rgb(51, 51, 51),
                rgb(38, 79, 120),
            ),
            ThemeKind::Github => (
                rgb(255, 255, 255),
                rgb(246, 248, 250),
                rgb(36, 41, 47),
                rgb(110, 118, 129),
                rgb(9, 105, 218),
                rgb(246, 248, 250),
                rgb(208, 215, 222),
                rgb(27, 31, 36),
                rgb(9, 105, 218),
                rgb(89, 99, 110),
                rgb(232, 237, 241),
                rgb(181, 213, 255),
            ),
            ThemeKind::Sepia => (
                rgb(244, 236, 216),
                rgb(236, 226, 203),
                rgb(76, 60, 42),
                rgb(150, 132, 105),
                rgb(155, 82, 44),
                rgb(236, 226, 205),
                rgb(214, 199, 172),
                rgb(66, 48, 32),
                rgb(155, 82, 44),
                rgb(133, 112, 85),
                rgb(238, 229, 208),
                rgb(210, 190, 152),
            ),
            ThemeKind::Nord => (
                rgb(46, 52, 64),
                rgb(40, 46, 58),
                rgb(216, 222, 233),
                rgb(122, 132, 148),
                rgb(136, 192, 208),
                rgb(52, 59, 72),
                rgb(67, 76, 94),
                rgb(236, 239, 244),
                rgb(143, 188, 187),
                rgb(135, 146, 163),
                rgb(59, 66, 82),
                rgb(76, 106, 133),
            ),
            ThemeKind::Solarized => (
                rgb(0, 43, 54),
                rgb(7, 54, 66),
                rgb(131, 148, 150),
                rgb(101, 123, 131),
                rgb(38, 139, 210),
                rgb(7, 54, 66),
                rgb(30, 76, 88),
                rgb(147, 161, 161),
                rgb(42, 161, 152),
                rgb(101, 123, 131),
                rgb(7, 54, 66),
                rgb(32, 92, 112),
            ),
        };
        Self {
            kind,
            bg,
            panel,
            text,
            dim,
            accent,
            code_bg,
            border,
            heading,
            link,
            quote,
            table_head,
            selection,
            body: 16.0,
            mono: 13.5,
        }
    }

    /// 加粗文字的强调色：中文加粗只靠字重不够醒目，
    /// 每个主题单独配一个暖色，和链接蓝、标题黑拉开距离。
    pub fn bold_color(&self) -> Color32 {
        match self.kind {
            ThemeKind::Light => rgb(180, 83, 9),
            ThemeKind::Dark => rgb(255, 198, 109),
            ThemeKind::Github => rgb(207, 34, 46),
            ThemeKind::Sepia => rgb(146, 43, 33),
            ThemeKind::Nord => rgb(235, 203, 139),
            ThemeKind::Solarized => rgb(181, 137, 0),
        }
    }

    /// 代码高亮配色：关键字 / 字符串 / 数字 / 注释。
    /// 浅色与深色直接取 VS Code 默认主题的 token 颜色，其它主题按各自色系微调。
    pub fn syntax(&self) -> [Color32; 4] {
        match self.kind {
            ThemeKind::Light => [
                rgb(0, 0, 255),
                rgb(163, 21, 21),
                rgb(9, 134, 88),
                rgb(0, 128, 0),
            ],
            ThemeKind::Dark => [
                rgb(86, 156, 214),
                rgb(206, 145, 120),
                rgb(181, 206, 168),
                rgb(106, 153, 85),
            ],
            ThemeKind::Github => [
                rgb(5, 80, 174),
                rgb(10, 48, 105),
                rgb(5, 80, 174),
                rgb(110, 119, 129),
            ],
            ThemeKind::Sepia => [
                rgb(139, 94, 0),
                rgb(160, 82, 45),
                rgb(160, 82, 45),
                rgb(154, 140, 122),
            ],
            ThemeKind::Nord => [
                rgb(129, 161, 193),
                rgb(163, 190, 140),
                rgb(180, 142, 173),
                rgb(97, 110, 136),
            ],
            ThemeKind::Solarized => [
                rgb(133, 153, 0),
                rgb(42, 161, 152),
                rgb(211, 54, 130),
                rgb(88, 110, 117),
            ],
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
        let mut visuals = if self.kind.is_dark() {
            Visuals::dark()
        } else {
            Visuals::light()
        };
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
