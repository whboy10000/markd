//! Author: aurelianus <aurelianus@126.com>

//! 主题系统：内置多套配色 + 全局字号/行高设置。

use egui::{Color32, Context, FontFamily, FontId, Visuals};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeKind {
    #[default]
    // 旧主题 Github/Sepia 是浅色系，反序列化时归到 Light，避免老配置失效。
    #[serde(alias = "Github", alias = "Sepia")]
    Light,
    // 旧主题 Nord/Solarized 是深色系，归到 Dark。
    #[serde(alias = "Nord", alias = "Solarized")]
    Dark,
}

impl ThemeKind {
    pub const ALL: [ThemeKind; 2] = [ThemeKind::Light, ThemeKind::Dark];

    pub fn name(self) -> &'static str {
        match self {
            ThemeKind::Light => "浅色",
            ThemeKind::Dark => "深色",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, ThemeKind::Dark)
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
    /// 代码块顶部功能栏背景：比 `code_bg` 深一档，区分「功能栏 / 显示区」。
    pub code_header: Color32,
    pub border: Color32,
    pub heading: Color32,
    pub link: Color32,
    pub quote: Color32,
    pub table_head: Color32,
    /// 表格线（外框 + 行列分隔线）。浅色主题固定 #F3F3F3，比通用 border 更浅一档。
    pub table_line: Color32,
    pub selection: Color32,
    pub body: f32,
    pub mono: f32,
}

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// 语法高亮的语言分组：决定用哪套 token 配色，让不同语言有区分度。
#[derive(Clone, Copy, PartialEq, Eq)]
enum LangFamily {
    Rust,
    Python,
    JavaScript,
    Sql,
    Generic,
}

fn lang_family(lang: &str) -> LangFamily {
    match lang.to_lowercase().as_str() {
        "rust" | "rs" => LangFamily::Rust,
        "python" | "py" => LangFamily::Python,
        "javascript" | "js" | "typescript" | "ts" | "jsx" | "tsx" | "vue" | "svelte" => {
            LangFamily::JavaScript
        }
        "sql" => LangFamily::Sql,
        _ => LangFamily::Generic,
    }
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
            code_header,
            border,
            heading,
            link,
            quote,
            table_head,
            selection,
        ) = match kind {
            // 对齐 VS Code 默认浅色（Light+）：editor #FFFFFF、侧栏 #F3F3F3、链接 #006AB1、选区 #ADD6FF
            // 代码块显示区 #FAFAFB，功能栏 #F0F0F2（比显示区深一档）
            ThemeKind::Light => (
                rgb(255, 255, 255),
                rgb(243, 243, 243),
                rgb(0, 0, 0),
                rgb(139, 139, 139),
                rgb(0, 106, 177),
                rgb(250, 250, 251),
                rgb(240, 240, 242),
                rgb(229, 229, 229),
                rgb(0, 0, 0),
                rgb(0, 106, 177),
                rgb(106, 115, 125),
                // 表格标题行 #F7F7F7：比数据行（#FFFFFF）略深，铺满整个标题行
                rgb(247, 247, 247),
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
                rgb(38, 38, 38),
                rgb(69, 69, 69),
                rgb(255, 255, 255),
                rgb(55, 148, 255),
                rgb(156, 163, 175),
                rgb(51, 51, 51),
                rgb(38, 79, 120),
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
            code_header,
            border,
            heading,
            link,
            quote,
            table_head,
            selection,
            body: 16.0,
            mono: 13.5,
            table_line: if kind == ThemeKind::Light {
                // 表格线统一用 #F3F3F3（与侧栏同色），比通用 border 更浅
                rgb(243, 243, 243)
            } else {
                border
            },
        }
    }

    /// 加粗文字的强调色：中文加粗只靠字重不够醒目，
    /// 每个主题单独配一个暖色，和链接蓝、标题黑拉开距离。
    pub fn bold_color(&self) -> Color32 {
        match self.kind {
            ThemeKind::Light => rgb(180, 83, 9),
            ThemeKind::Dark => rgb(255, 198, 109),
        }
    }

    /// 代码高亮配色：关键字 / 字符串 / 数字 / 注释。
    /// 按语言分组返回不同配色，让不同语言有区分度；浅色与深色各一套。
    pub fn syntax(&self, lang: &str) -> [Color32; 4] {
        let family = lang_family(lang);
        // 参考 VS Code（Light+/Dark+）与 GitHub 配色，保证在代码块背景上清晰协调。
        match (self.kind, family) {
            // ---- 浅色 ----
            (ThemeKind::Light, LangFamily::Rust) => [
                rgb(111, 66, 193),  // 关键字：紫
                rgb(3, 47, 98),     // 字符串：深蓝
                rgb(227, 98, 9),    // 数字：橙
                rgb(106, 115, 125), // 注释：灰
            ],
            (ThemeKind::Light, LangFamily::Python) => [
                rgb(0, 0, 255),     // 关键字：蓝
                rgb(34, 134, 58),   // 字符串：绿
                rgb(176, 0, 32),    // 数字：红
                rgb(112, 128, 144), // 注释：灰蓝
            ],
            (ThemeKind::Light, LangFamily::JavaScript) => [
                rgb(130, 80, 223),  // 关键字：紫
                rgb(10, 48, 105),   // 字符串：深蓝
                rgb(5, 80, 174),    // 数字：蓝
                rgb(106, 115, 125), // 注释：灰
            ],
            (ThemeKind::Light, LangFamily::Sql) => [
                rgb(5, 80, 174),    // 关键字：蓝
                rgb(163, 21, 21),   // 字符串：红
                rgb(0, 128, 128),   // 数字：青
                rgb(106, 115, 125), // 注释：灰
            ],
            (ThemeKind::Light, LangFamily::Generic) => [
                rgb(0, 0, 255),     // 关键字：蓝
                rgb(163, 21, 21),   // 字符串：红棕
                rgb(9, 134, 88),    // 数字：绿
                rgb(106, 115, 125), // 注释：灰
            ],
            // ---- 深色 ----
            (ThemeKind::Dark, LangFamily::Rust) => [
                rgb(197, 134, 192), // 关键字：紫
                rgb(206, 145, 120), // 字符串：橙
                rgb(215, 186, 125), // 数字：黄
                rgb(106, 153, 85),  // 注释：灰绿
            ],
            (ThemeKind::Dark, LangFamily::Python) => [
                rgb(197, 134, 192), // 关键字：紫
                rgb(152, 195, 121), // 字符串：绿
                rgb(86, 182, 194),  // 数字：青
                rgb(92, 99, 112),   // 注释：灰
            ],
            (ThemeKind::Dark, LangFamily::JavaScript) => [
                rgb(86, 156, 214),  // 关键字：蓝
                rgb(206, 145, 120), // 字符串：橙
                rgb(181, 206, 168), // 数字：绿
                rgb(106, 153, 85),  // 注释：灰绿
            ],
            (ThemeKind::Dark, LangFamily::Sql) => [
                rgb(86, 156, 214),  // 关键字：蓝
                rgb(206, 145, 120), // 字符串：橙
                rgb(78, 201, 176),  // 数字：青
                rgb(106, 153, 85),  // 注释：灰绿
            ],
            (ThemeKind::Dark, LangFamily::Generic) => [
                rgb(86, 156, 214),  // 关键字：蓝
                rgb(206, 145, 120), // 字符串：橙
                rgb(181, 206, 168), // 数字：绿
                rgb(106, 153, 85),  // 注释：灰绿
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
