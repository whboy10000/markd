//! Author: aurelianus <aurelianus@126.com>

//! 渲染层：把 AST 画成富文本 / 表格 / 代码块 / 流程图 / 图片。
//!
//! 非活动块走这里；活动块由 editor.rs 用 TextEdit 显示源码，形成 Hybrid View。

use crate::ast::*;
use crate::document::OutlineItem;
use crate::theme::{LangFamily, Theme, lang_family};
use crate::workspace;
use egui::text::{LayoutJob, TextFormat};
use egui::{self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub enum Action {
    ToggleTask(usize),
    OpenLink(String),
    Jump(usize),
    Notify(String),
    /// 修改代码块的语言（info string）。
    SetLang(usize, String),
    /// 点击了某个块：块下标 + 点击位置（相对块左上角），用于把光标放到点击处。
    Caret(usize, egui::Vec2),
    /// 点击了表格单元格：(块下标, 行, 列, 光标是否在文字末尾)。
    /// 进入编辑态时聚焦该单元格，光标落在点击位置附近而不是固定跳到第一格。
    TableCellClick(usize, usize, usize, bool),
    /// 表格工具栏的结构操作：(块下标, 操作)。
    TableOp(usize, crate::editor::TableOp),
    /// 请求删除某个代码块：app 层弹确认窗，确定后整块删除。
    AskDeleteBlock(usize),
    /// 折叠/展开某个代码块（渲染态右上角的折叠按钮）。
    FoldToggle(usize),
    /// 跨块拖选进展：(块下标, 字符下标, 是否拖动开始)。
    /// 拖动开始时锚定选区起点，之后每次移动更新选区终点。
    CrossDrag(usize, usize, bool),
    /// 双击选词：块下标 + 词的字符区间 [起, 止)。
    WordSel(usize, usize, usize),
    /// 三击选中整块（一行/一段）。
    BlockSel(usize),
    /// Shift+点击：从光标/选区锚点到点击位置全部选中。
    ShiftClick(usize, usize),
    /// 拖动图片右下角缩放：(块下标, 图片序号, 新宽度像素)。
    ResizeImage(usize, usize, u32),
}

/// 代码块可选语言，覆盖常见前端/后端/脚本/配置/标记语言。
/// 带搜索的语言选择下拉：顶部输入框实时过滤，语言按 A→Z 正序排列
/// （纯文本排最前）。返回用户选中的语言（与 current 不同时为 Some）。
#[allow(deprecated)]
pub fn language_combo(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
) -> Option<String> {
    let id = egui::Id::new(id_salt);
    let mut picked: Option<String> = None;
    let inner = egui::ComboBox::from_id_salt(id)
        .selected_text(if current.is_empty() {
            "纯文本".to_string()
        } else {
            current.to_string()
        })
        .width(118.0)
        // 默认 CloseOnClick 会在弹层内任意点击（包括点搜索框）时关闭整个下拉，
        // 表现就是「一点搜索框就消失、没法搜索」；改成只在外部点击时关闭，
        // 选中语言后再在 show_ui 返回之后手动收起。
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            let filter_id = id.with("filter");
            let mut filter = ui
                .ctx()
                .memory(|m| m.data.get_temp::<String>(filter_id).unwrap_or_default());
            ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .hint_text("搜索语言…")
                    .desired_width(110.0),
            );
            let f = filter.trim().to_lowercase();
            ui.ctx()
                .memory_mut(|m| m.data.insert_temp(filter_id, f.clone()));
            // A→Z 正序；"" (纯文本) 自然排最前
            let mut list = LANGUAGES.to_vec();
            list.sort_unstable();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for l in list {
                        if !f.is_empty() && !l.to_lowercase().contains(&f) {
                            continue;
                        }
                        let label = if l.is_empty() { "纯文本" } else { l };
                        if ui.selectable_label(current == l, label).clicked() {
                            picked = Some(l.to_string());
                        }
                    }
                });
        });
    // 选好了就收起下拉（CloseOnClickOutside 模式下不会自动关）。
    // 弹层 id 必须从 egui 自己返回的按钮响应派生（Popup::default_response_id）：
    // 之前手算 button_id.with("popup")，但 ComboBox 内部先把 id_salt 转成
    // IdSalt 再 make_persistent_id，与直接传 Id 的哈希路径不同，算出来的
    // id 和真实弹层对不上，close_id 是空操作——表现为选完语言下拉还挂在屏幕上。
    let popup_id = egui::Popup::default_response_id(&inner.response);
    if picked.is_some() {
        egui::Popup::close_id(ui.ctx(), popup_id);
    }
    #[cfg(test)]
    if let Ok(mut trace) = COMBO_TRACE.lock() {
        trace.push((
            inner.response.rect,
            popup_id,
            inner.response.clicked(),
            egui::Popup::is_id_open(ui.ctx(), popup_id),
            std::thread::current().id(),
        ));
    }
    picked
}

/// 测试支持：language_combo 每帧记录（按钮矩形, popup id, 本帧是否被点, 弹层是否开）。
/// 供真实帧测试定位点击坐标与断言弹层状态。仅测试构建存在。
#[cfg(test)]
pub(crate) static COMBO_TRACE: std::sync::Mutex<
    Vec<(egui::Rect, egui::Id, bool, bool, std::thread::ThreadId)>,
> = std::sync::Mutex::new(Vec::new());

pub const LANGUAGES: &[&str] = &[
    "",
    "json",
    "xml",
    "yaml",
    "toml",
    "ini",
    "csv",
    "rust",
    "c",
    "cpp",
    "csharp",
    "java",
    "kotlin",
    "scala",
    "swift",
    "go",
    "dart",
    "python",
    "javascript",
    "typescript",
    "jsx",
    "tsx",
    "vue",
    "svelte",
    "php",
    "ruby",
    "perl",
    "lua",
    "r",
    "julia",
    "matlab",
    "objective-c",
    "fsharp",
    "haskell",
    "ocaml",
    "clojure",
    "elixir",
    "erlang",
    "groovy",
    "lisp",
    "prolog",
    "fortran",
    "cobol",
    "sql",
    "plsql",
    "graphql",
    "prisma",
    "html",
    "css",
    "scss",
    "sass",
    "less",
    "stylus",
    "properties",
    "bash",
    "sh",
    "zsh",
    "fish",
    "powershell",
    "bat",
    "awk",
    "sed",
    "vim",
    "makefile",
    "dockerfile",
    "cmake",
    "nginx",
    "apache",
    "terraform",
    "hcl",
    "protobuf",
    "thrift",
    "markdown",
    "latex",
    "tex",
    "mermaid",
    "plantuml",
    "diff",
    "asm",
    "wasm",
    "solidity",
    "verilog",
    "vhdl",
    "text",
    "plaintext",
];

pub struct ImageCache(pub HashMap<PathBuf, Option<egui::TextureHandle>>);

impl ImageCache {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn texture(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
        if let Some(v) = self.0.get(path) {
            return v.clone();
        }
        let tex = (|| {
            let img = image::open(path).ok()?.to_rgba8();
            let (w, h) = (img.width() as usize, img.height() as usize);
            if w == 0 || h == 0 {
                return None;
            }
            let ci = egui::ColorImage::from_rgba_unmultiplied([w, h], img.as_raw());
            Some(ctx.load_texture(
                path.to_string_lossy().into_owned(),
                ci,
                egui::TextureOptions::LINEAR,
            ))
        })();
        self.0.insert(path.to_path_buf(), tex.clone());
        tex
    }
}

pub struct RenderEnv<'a> {
    pub theme: &'a Theme,
    pub ctx: &'a egui::Context,
    pub images: &'a mut ImageCache,
    pub base_dir: Option<&'a Path>,
    pub outline: &'a [OutlineItem],
    pub actions: &'a mut Vec<Action>,
    /// 正文排版宽度。显式传给每个文本块，避免在横向布局里
    /// Label 退回 `Extend`（无限宽）后长段落永远不换行。
    pub max_width: f32,
    /// 当前跨块选区 (锚块, 锚字符, 头块, 头字符)：渲染态块用它画选中底色。
    pub cross_sel: Option<(usize, usize, usize, usize)>,
    /// 已折叠的代码块下标集合：折叠态的代码块只画一个「展开」按钮和提示行。
    pub folded: &'a HashSet<usize>,
}

#[allow(clippy::too_many_arguments)]
fn append_inline(
    job: &mut LayoutJob,
    inl: &Inline,
    theme: &Theme,
    size: f32,
    color: Color32,
    bold: bool,
    italic: bool,
    strike: bool,
    underline: bool,
    linked: bool,
) {
    match inl {
        Inline::Text(t) => {
            let mut f = if bold {
                theme.bold_fmt(size, color)
            } else {
                theme.fmt(size)
            };
            f.color = if linked { theme.link } else { color };
            f.italics = italic;
            if strike {
                f.strikethrough = Stroke::new(1.0, f.color);
            }
            if underline {
                f.underline = Stroke::new(1.0, f.color);
            }
            if linked {
                f.underline = Stroke::new(1.0, theme.link);
            }
            job.append(t, 0.0, f);
        }
        Inline::Strong(v) => {
            // 继承当前正文色，用 markd-bold 真粗体字族——与编辑态 inline_job 一致。
            // 之前换成 bold_color() 强调色 + wght 伪粗，导致鼠标离开加粗文字
            // （从编辑态切到渲染态）时突然变色、且变细。
            for x in v {
                append_inline(
                    job, x, theme, size, color, true, italic, strike, underline, linked,
                );
            }
        }
        Inline::Underline(v) => {
            for x in v {
                append_inline(
                    job, x, theme, size, color, bold, italic, strike, true, linked,
                );
            }
        }
        Inline::Emph(v) => {
            for x in v {
                append_inline(
                    job, x, theme, size, color, bold, true, strike, underline, linked,
                );
            }
        }
        Inline::Strike(v) => {
            for x in v {
                append_inline(
                    job, x, theme, size, color, bold, italic, true, underline, linked,
                );
            }
        }
        Inline::Code(t) => {
            // 行内代码与正文同字号（等宽字体视觉上略小，但字号一致），
            // 用专属的浅灰/深灰背景做成「代码片」，不再用刺眼的主题蓝。
            let mut f = theme.fmt(size);
            f.font_id = FontId::new(size, FontFamily::Monospace);
            f.color = theme.inline_code_fg();
            f.background = theme.inline_code_bg();
            f.expand_bg = 2.5;
            job.append(t, 0.0, f);
        }
        Inline::Link { label, .. } => {
            for x in label {
                append_inline(
                    job, x, theme, size, color, bold, italic, strike, underline, true,
                );
            }
        }
        Inline::Image { alt, url, .. } => {
            let txt = if alt.is_empty() { url } else { alt };
            let mut f = theme.fmt(size * 0.88);
            f.color = theme.dim;
            job.append(&format!("[图片: {}]", txt), 0.0, f);
        }
        Inline::Math(t) => {
            let mut f = theme.mono_fmt();
            f.color = theme.heading;
            job.append(&math_pretty(t), 0.0, f);
        }
        Inline::Wiki(name) => {
            let mut f = theme.fmt(size);
            f.color = theme.accent;
            job.append(&format!("[[{}]]", name), 0.0, f);
        }
        Inline::Footnote(id) => {
            let mut f = theme.fmt(size * 0.75);
            f.color = theme.accent;
            job.append(&format!("[{}]", id), 0.0, f);
        }
        Inline::Break => job.append("\n", 0.0, theme.fmt(size)),
    }
}

pub fn build_layout(
    inlines: &[Inline],
    theme: &Theme,
    size: f32,
    color: Color32,
    bold: bool,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    // 长 URL / 无空格的中文也要按宽度折断，避免溢出窗口被裁掉。
    job.wrap.break_anywhere = true;
    for i in inlines {
        append_inline(
            &mut job, i, theme, size, color, bold, false, false, false, false,
        );
    }
    if inlines.is_empty() {
        job.append(" ", 0.0, theme.fmt(size));
    }
    job
}

/// 本块被跨块选区覆盖的字符区间（未按文本长度钳制）。
/// 返回 None 表示本块不在选区内。
fn block_sel_chars(
    sel: Option<(usize, usize, usize, usize)>,
    idx: usize,
) -> Option<(usize, usize)> {
    let (a, ac, b, bc) = sel?;
    let (s, sc, e, ec) = if (a, ac) <= (b, bc) {
        (a, ac, b, bc)
    } else {
        (b, bc, a, ac)
    };
    if idx < s || idx > e {
        return None;
    }
    let t0 = if idx == s { sc } else { 0 };
    let t1 = if idx == e { ec } else { usize::MAX };
    (t0 < t1).then_some((t0, t1))
}

/// 选区覆盖到行内代码时，把代码片底色换成本身是选区色。
///
/// 代码片背景和字形在同一个 TextShape 里绘制（先铺底、再画字），而选区蓝底
/// 是垫在 TextShape 下面的独立形状，所以代码片的灰底会把选区盖住——看起来
/// 就像这段行内代码没被选中。把底色直接换成选区色后，两者严丝合缝。
fn tint_selected_code_bg(job: &mut LayoutJob, theme: &Theme, sel: Option<(usize, usize)>) {
    let Some((a, b)) = sel else { return };
    if a >= b {
        return;
    }
    let code_bg = theme.inline_code_bg();
    let text = job.text.clone();
    for sec in &mut job.sections {
        if sec.format.background != code_bg {
            continue;
        }
        let start = text[..sec.byte_range.start.0].chars().count();
        let end = start
            + text[sec.byte_range.start.0..sec.byte_range.end.0]
                .chars()
                .count();
        if start < b && a < end {
            sec.format.background = theme.selection;
        }
    }
}

/// 列表的嵌套层级：按前导空格估算（2 空格一层，1 空格也算一层）。
/// 用于让子列表的标记和父列表不同。
pub fn list_level(indent: usize) -> usize {
    indent.div_ceil(2)
}

/// 有序列表的显示标记：顶层保留源文件里的数字（`1.`），子层级依次显示
/// 小写字母（`a.`）、小写罗马数字（`i.`），再往下循环。数字取自源标记，
/// 所以子列表自身的 1、2、3 会变成 a、b、c。
pub fn ordered_list_marker(marker: &str, level: usize) -> String {
    if level % 3 == 0 {
        return marker.to_string();
    }
    let tail = marker
        .chars()
        .last()
        .filter(|c| *c == '.' || *c == ')')
        .unwrap_or('.');
    let digits: String = marker.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: usize = digits.parse().unwrap_or(1).max(1);
    let body = if level % 3 == 1 {
        alpha_label(n)
    } else {
        roman_label(n)
    };
    format!("{body}{tail}")
}

/// 1 → a、26 → z、27 → aa。
fn alpha_label(n: usize) -> String {
    let mut n = n;
    let mut out = Vec::new();
    while n > 0 {
        let r = (n - 1) % 26;
        out.push((b'a' + r as u8) as char);
        n = (n - 1) / 26;
    }
    out.iter().rev().collect()
}

/// 1 → i、4 → iv、9 → ix、14 → xiv。
fn roman_label(n: usize) -> String {
    const TABLE: &[(usize, &str)] = &[
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut n = n;
    let mut s = String::new();
    for (v, r) in TABLE {
        while n >= *v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// 无序列表标记：按层级在实心圆 / 空心圆 / 小方块之间循环，
/// 这样子列表一眼就能和父列表区分开。
pub fn draw_list_bullet(ui: &mut egui::Ui, level: usize, color: Color32, d: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(d + 2.0), Sense::hover());
    let p = ui.painter();
    let c = r.center();
    match level % 3 {
        0 => {
            p.circle_filled(c, d * 0.5, color);
        }
        1 => {
            p.circle_stroke(c, d * 0.42, Stroke::new(1.6, color));
        }
        _ => {
            p.rect_filled(Rect::from_center_size(c, Vec2::splat(d * 0.6)), 1.5, color);
        }
    }
}

pub fn math_pretty(tex: &str) -> String {
    let mut s = tex.to_string();
    const PAIRS: &[(&str, &str)] = &[
        ("\\times", "×"),
        ("\\cdot", "·"),
        ("\\int", "∫"),
        ("\\sum", "∑"),
        ("\\alpha", "α"),
        ("\\beta", "β"),
        ("\\gamma", "γ"),
        ("\\delta", "δ"),
        ("\\pi", "π"),
        ("\\infty", "∞"),
        ("\\sqrt", "√"),
        ("\\leq", "≤"),
        ("\\geq", "≥"),
        ("\\neq", "≠"),
        ("\\rightarrow", "→"),
        ("\\Rightarrow", "⇒"),
        ("\\in", "∈"),
        ("\\partial", "∂"),
        ("\\nabla", "∇"),
        ("\\approx", "≈"),
        ("\\pm", "±"),
        ("\\frac", ""),
        ("\\left", ""),
        ("\\right", ""),
        ("{", ""),
        ("}", ""),
        ("$$", ""),
    ];
    for (a, b) in PAIRS {
        s = s.replace(a, b);
    }
    s
}

/// 按 `build_layout` 的绘制顺序累计字符数，记录每个链接标签在 galley 里的字符区间。
/// 这样点击文字时能判断点到的到底是不是某个链接。
fn collect_link_ranges(inlines: &[Inline], out: &mut Vec<(usize, usize, String)>, pos: &mut usize) {
    for inl in inlines {
        match inl {
            Inline::Text(t) => *pos += t.chars().count(),
            Inline::Strong(v) | Inline::Emph(v) | Inline::Strike(v) | Inline::Underline(v) => {
                collect_link_ranges(v, out, pos)
            }
            Inline::Code(t) => *pos += t.chars().count(),
            Inline::Link { label, url } => {
                let start = *pos;
                collect_link_ranges(label, out, pos);
                out.push((start, *pos, url.clone()));
            }
            Inline::Image { alt, url, .. } => {
                let txt = if alt.is_empty() { url } else { alt };
                *pos += format!("[图片: {}]", txt).chars().count();
            }
            Inline::Math(t) => *pos += math_pretty(t).chars().count(),
            Inline::Wiki(name) => *pos += format!("[[{}]]", name).chars().count(),
            Inline::Footnote(id) => *pos += format!("[{}]", id).chars().count(),
            Inline::Break => *pos += 1,
        }
    }
}

/// 点击字符下标 `idx` 处时，如果它落在某个链接标签里就返回该链接地址。
fn link_at(inlines: &[Inline], idx: usize) -> Option<String> {
    let mut ranges = Vec::new();
    let mut pos = 0usize;
    collect_link_ranges(inlines, &mut ranges, &mut pos);
    ranges
        .into_iter()
        // 点到标签最后一个字的右半部分时，光标下标会落在区间末尾，也算命中。
        .find(|(a, z, _)| idx >= *a && idx <= *z)
        .map(|(_, _, url)| url)
}

/// 编辑态（TextEdit 里显示的是原始 Markdown）的链接命中测试。
/// 光标下标对应原始缓冲区，所以直接扫描 `[标签](地址)` 的标签区间。
pub fn link_at_markdown(text: &str, idx: usize) -> Option<String> {
    let c: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < c.len() {
        if c[i] == '\\' {
            i += 2;
            continue;
        }
        if c[i] == '[' && !(i + 1 < c.len() && c[i + 1] == '[') {
            if let Some(close) = (i + 1..c.len()).find(|&k| c[k] == ']')
                && close + 1 < c.len()
                && c[close + 1] == '('
                && let Some(rel) = (close + 2..c.len()).find(|&k| c[k] == ')')
            {
                if idx > i && idx <= close {
                    let raw: String = c[close + 2..rel].iter().collect();
                    return Some(
                        raw.split_whitespace()
                            .next()
                            .unwrap_or("")
                            .trim_matches(|ch| ch == '"' || ch == '\'')
                            .to_string(),
                    );
                }
                i = rel + 1;
                continue;
            }
        }
        i += 1;
    }
    None
}

/// 编辑态的行内排版：**缓冲区仍然是原始 Markdown**（保证光标/字节偏移正确），
/// 但把 `**`、`*`、`~~`、`` ` ``、`[label](url)` 这些标记用 0.1px 透明字渲染，
/// 于是看起来就是渲染后的富文本，而不是源码。
pub fn inline_job(
    text: &str,
    theme: &Theme,
    size: f32,
    color: Color32,
    wrap_width: f32,
    cursor: Option<usize>,
    // 当前文本选区（缓冲区字符下标）：选中行内代码时把代码片底色换成选区色，
    // 否则代码片的灰底会盖住 TextEdit 画的选区，看起来像没选中。
    sel: Option<(usize, usize)>,
) -> LayoutJob {
    let marker = TextFormat {
        font_id: FontId::new(0.1, FontFamily::Proportional),
        color: Color32::TRANSPARENT,
        line_height: None,
        ..Default::default()
    };
    let bold_family = FontFamily::Name("markd-bold".into());

    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.wrap.break_anywhere = true;

    let chars: Vec<char> = text.chars().collect();

    // 预扫描行内标记 run：星号按「粗体 run(n>=2) / 斜体 run(n==1)」分组，
    // 波浪号、反引号各自一组。组内成对的标记隐藏渲染；落单的（手打的 *、
    // 敲了一半的 **、未闭合的反引号）按普通文字显示——否则打字时看不见刚
    // 敲的星号，像按键没生效。斜体/删除线的配对标记始终隐藏；加粗和行内代码
    // 在光标进入其内部时露出这一对标记（部分源码模式），方便看清哪些字加了
    // 格式、哪些没加、以及标记落点。
    let mut lone: Vec<usize> = Vec::new();
    // 所有反引号位置，用于判断光标是否落在某个行内代码内部。
    let mut tick: Vec<usize> = Vec::new();
    // 成对加粗 run 的起始下标（每两个一组：开标记、闭标记）。
    let bold_runs: Vec<usize>;
    {
        let mut ital: Vec<usize> = Vec::new();
        let mut bold: Vec<usize> = Vec::new();
        let mut tilde: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < chars.len() {
            match chars[i] {
                '*' => {
                    let n = chars[i..].iter().take_while(|&&x| x == '*').count();
                    if n == 1 {
                        ital.push(i);
                    } else {
                        bold.push(i);
                    }
                    i += n;
                }
                '~' if chars.get(i + 1) == Some(&'~') => {
                    tilde.push(i);
                    i += 2;
                }
                '`' => {
                    tick.push(i);
                    i += 1;
                }
                _ => i += 1,
            }
        }
        // 同组奇数个时，最后一个 run 没有配对对象（落单）
        let mark_lone = |v: &[usize], out: &mut Vec<usize>| {
            if v.len() % 2 == 1 {
                out.push(v[v.len() - 1]);
            }
        };
        mark_lone(&ital, &mut lone);
        mark_lone(&bold, &mut lone);
        mark_lone(&tilde, &mut lone);
        mark_lone(&tick, &mut lone);
        bold_runs = bold;
    }
    // 光标落在某对反引号之间（含两反引号中间的空代码）→ 露出这一对反引号。
    // 加粗同理：光标在 `**粗体**` 内部时露出这一对 `**`。
    let mut reveal_tick: Vec<usize> = Vec::new();
    let mut reveal_bold: Vec<usize> = Vec::new();
    if let Some(c) = cursor {
        for pair in tick.chunks_exact(2) {
            let (a, b) = (pair[0], pair[1]);
            if a < c && c <= b {
                reveal_tick.push(a);
                reveal_tick.push(b);
            }
        }
        for pair in bold_runs.chunks_exact(2) {
            let (a, b) = (pair[0], pair[1]);
            if a < c && c <= b {
                reveal_bold.push(a);
                reveal_bold.push(b);
            }
        }
    }

    let fmt_now = |code: bool, italic: bool, bold: bool, strike: bool| {
        if code {
            TextFormat {
                font_id: FontId::new(size, FontFamily::Monospace),
                color: theme.inline_code_fg(),
                background: theme.inline_code_bg(),
                expand_bg: 2.5,
                line_height: None,
                ..Default::default()
            }
        } else {
            TextFormat {
                font_id: FontId::new(
                    size,
                    if bold {
                        bold_family.clone()
                    } else {
                        FontFamily::Proportional
                    },
                ),
                color,
                italics: italic,
                strikethrough: if strike {
                    Stroke::new(1.0, color)
                } else {
                    Stroke::NONE
                },
                line_height: None,
                ..Default::default()
            }
        }
    };

    let mut i = 0usize;
    let mut bold = false;
    let mut italic = false;
    let mut strike = false;
    let mut code = false;

    fn push(job: &mut LayoutJob, c: char, f: &TextFormat) {
        let mut buf = [0u8; 4];
        job.append(c.encode_utf8(&mut buf), 0.0, f.clone());
    }

    // 判断下标 i 处是否是完整标签（如 "<strong>"）
    fn tag_at(chars: &[char], i: usize, tag: &str) -> bool {
        let t: Vec<char> = tag.chars().collect();
        i + t.len() <= chars.len() && chars[i..i + t.len()] == t[..]
    }

    while i < chars.len() {
        let c = chars[i];

        // <strong> / </strong>：与 ** 一致地隐藏标签、切换粗体；
        // 配不上对（孤立的开/闭标签）就按普通文字原样显示
        if c == '<' && tag_at(&chars, i, "<strong>") {
            let f = if bold {
                fmt_now(code, italic, bold, strike)
            } else {
                marker.clone()
            };
            for k in i..i + 8 {
                push(&mut job, chars[k], &f);
            }
            bold = !bold;
            i += 8;
            continue;
        }
        if c == '<' && tag_at(&chars, i, "</strong>") {
            let f = if bold {
                marker.clone()
            } else {
                fmt_now(code, italic, bold, strike)
            };
            for k in i..i + 9 {
                push(&mut job, chars[k], &f);
            }
            bold = !bold;
            i += 9;
            continue;
        }
        // <em> / </em>：与 * 一致地隐藏标签、切换斜体；
        // 配不上对（孤立的开/闭标签）就按普通文字原样显示
        if c == '<' && tag_at(&chars, i, "<em>") {
            let f = if italic {
                fmt_now(code, italic, bold, strike)
            } else {
                marker.clone()
            };
            for k in i..i + 4 {
                push(&mut job, chars[k], &f);
            }
            italic = !italic;
            i += 4;
            continue;
        }
        if c == '<' && tag_at(&chars, i, "</em>") {
            let f = if italic {
                marker.clone()
            } else {
                fmt_now(code, italic, bold, strike)
            };
            for k in i..i + 5 {
                push(&mut job, chars[k], &f);
            }
            italic = !italic;
            i += 5;
            continue;
        }
        if c == '`' {
            let f = if lone.contains(&i) {
                fmt_now(code, italic, bold, strike)
            } else if reveal_tick.contains(&i) {
                // 光标在代码内：这一对反引号按代码样式显示，和正文连成一片，
                // 用户可以直接删改这段源码。
                fmt_now(true, italic, bold, strike)
            } else {
                marker.clone()
            };
            push(&mut job, c, &f);
            code = !code;
            i += 1;
            continue;
        }
        if c == '*' {
            let n = chars[i..].iter().take_while(|&&x| x == '*').count();
            let f = if lone.contains(&i) {
                fmt_now(code, italic, bold, strike)
            } else if reveal_bold.contains(&i) {
                // 光标在加粗范围内：露出这一对 **（普通字重，避免闭合标记
                // 跟着变粗、看起来像多敲了一个星号）。
                fmt_now(code, italic, false, strike)
            } else {
                marker.clone()
            };
            for _ in 0..n {
                push(&mut job, '*', &f);
            }
            if n >= 2 {
                bold = !bold;
            } else {
                italic = !italic;
            }
            i += n;
            continue;
        }
        if c == '~' && i + 1 < chars.len() && chars[i + 1] == '~' {
            let visible = lone.contains(&i);
            let f = if visible {
                fmt_now(code, italic, bold, strike)
            } else {
                marker.clone()
            };
            push(&mut job, '~', &f);
            push(&mut job, '~', &f);
            strike = !strike;
            i += 2;
            continue;
        }
        if c == '[' {
            let close = chars[i..].iter().position(|ch| *ch == ']').map(|k| i + k);
            if let Some(close) = close {
                if close + 1 < chars.len() && chars[close + 1] == '(' {
                    if let Some(rel) = chars[close + 1..].iter().position(|ch| *ch == ')') {
                        let end = close + 1 + rel;
                        let link_fmt = TextFormat {
                            font_id: FontId::new(size, FontFamily::Proportional),
                            color: theme.link,
                            underline: Stroke::new(1.0, theme.link),
                            line_height: None,
                            ..Default::default()
                        };
                        // 光标在这个链接的源码范围内（标签或地址）→ 露出
                        // `[标签](地址)`，进入可编辑的源码模式。
                        let revealed = cursor.is_some_and(|c| i < c && c <= end);
                        if revealed {
                            let plain = fmt_now(false, italic, bold, strike);
                            let url_fmt = TextFormat {
                                font_id: FontId::new(size, FontFamily::Proportional),
                                color: theme.link,
                                line_height: None,
                                ..Default::default()
                            };
                            push(&mut job, chars[i], &plain); // '['
                            for k in i + 1..close {
                                push(&mut job, chars[k], &plain);
                            }
                            push(&mut job, chars[close], &plain); // ']'
                            push(&mut job, chars[close + 1], &plain); // '('
                            for k in close + 2..end {
                                push(&mut job, chars[k], &url_fmt);
                            }
                            push(&mut job, chars[end], &plain); // ')'
                        } else {
                            push(&mut job, '[', &marker);
                            for k in i + 1..close {
                                push(&mut job, chars[k], &link_fmt);
                            }
                            for k in close..=end {
                                push(&mut job, chars[k], &marker);
                            }
                        }
                        i = end + 1;
                        continue;
                    }
                }
            }
        }

        let fmt = fmt_now(code, italic, bold, strike);
        push(&mut job, c, &fmt);
        i += 1;
    }
    tint_selected_code_bg(&mut job, theme, sel);
    job
}

/// 用显式宽度排版成 galley 再交给 Label，并把 galley 一并返回供点击命中测试。
/// `Label` 默认按 Ui 的 wrap 模式决定宽度，横向布局（引用/列表）里会退化成
/// 无限宽导致长段落不换行；这里直接给定宽度，任何布局下都会换行。
fn layout_label(
    ui: &mut egui::Ui,
    mut job: LayoutJob,
    max_w: f32,
    sense: Sense,
) -> (egui::Response, std::sync::Arc<egui::Galley>) {
    let avail = ui.available_width();
    let w = if avail.is_finite() {
        avail.min(max_w)
    } else {
        max_w
    };
    job.wrap.max_width = w.max(80.0);
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let resp = ui.add(egui::Label::new(egui::WidgetText::Galley(galley.clone())).sense(sense));
    (resp, galley)
}

fn wrapped_label(ui: &mut egui::Ui, job: LayoutJob, max_w: f32, sense: Sense) -> egui::Response {
    layout_label(ui, job, max_w, sense).0
}

/// 单元格内按列对齐方式放置文字：左=默认；居中=vertical_centered；
/// 右=right_to_left 贴右。修复对齐按钮无效的 bug：此前渲染态完全没
/// 使用 align 数组，点了「居中/右对齐」源码变了但显示仍是左对齐。
fn align_cell_label(
    ui: &mut egui::Ui,
    job: LayoutJob,
    max_w: f32,
    a: crate::ast::Align,
) -> egui::Response {
    match a {
        crate::ast::Align::Left => wrapped_label(ui, job, max_w, Sense::hover()),
        crate::ast::Align::Center => {
            ui.vertical_centered(|ui| wrapped_label(ui, job, max_w, Sense::hover()))
                .inner
        }
        crate::ast::Align::Right => {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                wrapped_label(ui, job, max_w, Sense::hover())
            })
            .inner
        }
    }
}

fn text_response(
    ui: &mut egui::Ui,
    inlines: &[Inline],
    theme: &Theme,
    size: f32,
    color: Color32,
    bold: bool,
    env: &mut RenderEnv<'_>,
    idx: usize,
) -> egui::Response {
    let mut job = build_layout(inlines, theme, size, color, bold);
    // 选区覆盖到本块的行内代码时，代码片底色改成选区色，避免灰底盖住选区蓝底。
    tint_selected_code_bg(&mut job, theme, block_sel_chars(env.cross_sel, idx));
    // 先占一个绘制槽：跨块选区底色必须垫在文字下层（在 Label 之前画），
    // 否则蓝色矩形会盖住文字，看起来像"字没了"。
    let bg_slot = ui.painter().add(egui::Shape::Noop);
    let (resp, galley) = layout_label(ui, job, env.max_width, Sense::click_and_drag());
    // 行右侧空白也要可点（Typora 行为）：命中区从文字左缘延伸到本行右缘。
    // 之前响应矩形只有文字 galley 的宽度，点行尾空白毫无反应；扩展后点击
    // 仍走 cursor_from_pos，超出行末的 x 会被钳到该行最后一个字符之后。
    let row_rect = Rect::from_min_max(
        resp.rect.min,
        Pos2::new(ui.max_rect().right(), resp.rect.bottom()),
    );
    let ext = ui
        .interact(
            row_rect,
            egui::Id::new(("row_ext", idx, resp.rect.top().to_bits())),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::Text);
    let resp = resp.union(ext);
    // 跨块拖选：在渲染态块上按住拖动，锚定/更新全局选区（show_editor 帧末聚合）。
    // 锚定必须在按下帧立即做（button_pressed）：egui 的 drag_started 晚一帧
    // 触发，那时指针已移到终点，起点字符就丢了。拖进别的块时由
    // 「选区进行中 + 指针压在本块上」的判定接棒上报，选区才能跨块延伸。
    //
    // 终点只由「指针当前压在哪个块上」决定，不能用 resp.dragged()：
    // egui 把 drag 归给按下的起始块，拖动期间它每帧都上报；块按 idx 升序渲染，
    // 向上拖时起始块的上报晚于指针所在块，聚合时会把终点覆盖回起始块——
    // 表现为选区漏掉指针上方的段落（看上去没选中、也复制不到）。
    let raw_pointer = ui.input(|i| i.pointer.interact_pos());
    // 拖选时把越界指针钳进视口：鼠标甩到可视区域外时，越界点往往落在块与块
    // 之间的空档（或文档末尾之后），没有块能命中，选区就不再延伸。钳到视口
    // 边缘后，配合 show_editor 的越界自动滚动，滚到哪儿就选到哪儿。
    //
    // 但「按下」不能钳：点工具条/菜单时指针在视口上方，钳下来会落到最上面的
    // 块上，凭空建立 (0, 7) 这种零长度选区，把用户真正的拖选覆盖掉——表现就是
    // 拖选好文字再点「引用/列表」等按钮，格式加到了错误的位置。
    let clip = ui.clip_rect();
    let in_viewport = raw_pointer.is_some_and(|p| clip.contains(p));
    let pointer = raw_pointer.map(|p| {
        if in_viewport {
            p
        } else {
            Pos2::new(p.x, p.y.clamp(clip.top() + 1.0, clip.bottom() - 1.0))
        }
    });
    let pointer_down = ui.input(|i| i.pointer.primary_down());
    let pressed_now = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
    let over_this = pointer.is_some_and(|p| resp.rect.contains(p));
    if pressed_now && in_viewport && over_this {
        if let Some(p) = pointer {
            let local = p - resp.rect.min;
            let c = galley.cursor_from_pos(local).index.0;
            env.actions.push(Action::CrossDrag(idx, c, true));
        }
    } else if !pressed_now && env.cross_sel.is_some() && pointer_down && over_this {
        if let Some(p) = pointer {
            let local = p - resp.rect.min;
            let c = galley.cursor_from_pos(local).index.0;
            env.actions.push(Action::CrossDrag(idx, c, false));
        }
    } else if !pressed_now && pointer_down && in_viewport && over_this {
        // 拖动起点不在渲染态块上（最常见：起点在活动块的 TextEdit 里，被它
        // 吃掉了按下事件），拖到本块时才建立跨块选区：以活动块的光标为锚点。
        // 没有这条，从编辑态段落往下拖就完全选不中（用户反馈的"断断续续"）。
        if let Some(p) = pointer {
            let local = p - resp.rect.min;
            let c = galley.cursor_from_pos(local).index.0;
            env.actions.push(Action::ShiftClick(idx, c));
        }
    }
    // 跨块选区底色：本块被选区覆盖时，按选中字符区间收集高亮矩形，
    // 最后填进前面预留的槽位（垫在文字下层）。
    let mut sel_shapes: Vec<egui::Shape> = Vec::new();
    if let Some((a, ac, b, bc)) = env.cross_sel {
        let (start_b, start_c, end_b, end_c) = if (a, ac) <= (b, bc) {
            (a, ac, b, bc)
        } else {
            (b, bc, a, ac)
        };
        if idx >= start_b && idx <= end_b {
            let text_len = galley.text().chars().count();
            let s0 = if idx == start_b {
                start_c.min(text_len)
            } else {
                0
            };
            let e0 = if idx == end_b {
                end_c.min(text_len)
            } else {
                text_len
            };
            if s0 < e0 {
                // 逐行求选中区间与行的交集：pos_from_cursor 取行内 x 段
                let mut acc = 0usize;
                for row in &galley.rows {
                    let n = row.char_count_including_newline().0;
                    let (rs, re) = (acc, acc + n);
                    acc = re;
                    let lo = s0.clamp(rs, re);
                    let hi = e0.clamp(rs, re);
                    if hi <= lo {
                        // 整行被跨过的空行（只有换行符）：画一小段行首高亮
                        if s0 <= rs && e0 >= re {
                            let rect = Rect::from_min_max(
                                Pos2::new(0.0, row.pos.y),
                                Pos2::new(6.0, row.pos.y + row.row.size.y),
                            )
                            .translate(resp.rect.min.to_vec2());
                            sel_shapes.push(egui::Shape::rect_filled(rect, 0.0, theme.selection));
                        }
                        continue;
                    }
                    let x0 = galley.pos_from_cursor(egui::text::CCursor::new(lo)).left();
                    // egui 的 pos_from_cursor 是「插入点」矩形（零宽）：索引 i
                    // 的 x 是第 i 个字符的左边界。所以选中延伸到行末时要用
                    // 边界 re（最后一字之后的插入点），不能用 re-1 的右缘——
                    // 那样每行最后一个字永远少一截、不被高亮覆盖。
                    let x1 = if e0 >= re {
                        galley.pos_from_cursor(egui::text::CCursor::new(re)).left()
                    } else {
                        galley.pos_from_cursor(egui::text::CCursor::new(hi)).left()
                    };
                    let rect = Rect::from_min_max(
                        Pos2::new(x0, row.pos.y),
                        Pos2::new(x1.max(x0), row.pos.y + row.row.size.y),
                    )
                    .translate(resp.rect.min.to_vec2());
                    sel_shapes.push(egui::Shape::rect_filled(rect, 0.0, theme.selection));
                }
            }
        }
    }
    if !sel_shapes.is_empty() {
        // 填进预留槽位：形状位于 Label 之前，渲染时在文字下层
        ui.painter().set(bg_slot, egui::Shape::Vec(sel_shapes));
    }
    // 双击选词 / 三击选整段 / Shift+点击扩选。必须在普通 clicked 之前处理：
    // egui 里双击的第二击也会触发 clicked，命中后直接 return，避免叠加 Caret。
    if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
        let local = p - resp.rect.min;
        let char_idx = galley.cursor_from_pos(local).index.0;
        let text: Vec<char> = galley.text().chars().collect();
        if resp.triple_clicked() {
            // 三击：选中整个块（整行/整段）
            env.actions.push(Action::BlockSel(idx));
            return resp;
        }
        if resp.double_clicked() {
            // 双击：选中点击处的词（连续字母/数字/汉字；标点只选该字符）
            let (a, z) = word_bounds(&text, char_idx);
            env.actions.push(Action::WordSel(idx, a, z));
            return resp;
        }
        if resp.clicked() && ui.input(|i| i.modifiers.shift) {
            // Shift+点击：从光标位置到点击位置之间全部选中（不用按住拖动）
            env.actions.push(Action::ShiftClick(idx, char_idx));
            return resp;
        }
    }
    if resp.clicked() {
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            let local = p - resp.rect.min;
            let cursor = galley.cursor_from_pos(local);
            let char_idx = cursor.index.0;
            // 链接：Cmd/Ctrl+点击才跳转；普通点击只激活编辑并露出该链接的
            // Markdown 源码（见 inline_job），方便直接改地址。
            if let Some(url) = link_at(inlines, char_idx)
                && ui.input(|i| i.modifiers.command || i.modifiers.ctrl)
            {
                env.actions.push(Action::OpenLink(url));
                return resp;
            }
            env.actions.push(Action::Caret(idx, local));
        }
    }
    let _ = idx;
    resp
}

/// 双击选词的词边界：从字符下标 pos 向两侧扩展。
/// 连续的字母/数字/汉字（含下划线）算一个词；空白/标点只选单个字符。
fn word_bounds(text: &[char], pos: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let pos = pos.min(text.len() - 1);
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if is_word(text[pos]) {
        let mut a = pos;
        while a > 0 && is_word(text[a - 1]) {
            a -= 1;
        }
        let mut z = pos + 1;
        while z < text.len() && is_word(text[z]) {
            z += 1;
        }
        (a, z)
    } else {
        (pos, pos + 1)
    }
}

/// 图片预览：限宽限高、居中、带细边框；右下角可拖动缩放。
///
/// 返回 `(响应, 拖动产生的新宽度)`：调用方把新宽度写回文档的
/// `{width=N}` 属性，下一帧就按新宽度显示。
pub fn image_preview(
    ui: &mut egui::Ui,
    alt: &str,
    url: &str,
    theme: &Theme,
    ctx: &egui::Context,
    images: &mut ImageCache,
    base_dir: Option<&Path>,
    width_attr: Option<u32>,
    grip: (usize, usize),
) -> (egui::Response, Option<u32>) {
    if let Some(path) = workspace::resolve(base_dir, url) {
        if let Some(tex) = images.texture(ctx, &path) {
            let natural = tex.size_vec2();
            let max_w = ui.available_width().max(80.0);
            // 预览不铺满整屏：宽度不超正文，高度最多占视口的六成。
            let max_h = (ui.clip_rect().height() * 0.6).max(160.0);
            let aspect = (natural.x / natural.y.max(1.0)).max(0.01);
            // 默认按原始尺寸；有 width 属性（拖动缩放写回的）就按属性显示。
            let target_w = width_attr.map(|w| w as f32).unwrap_or(natural.x);
            let mut scale = (target_w / natural.x.max(1.0))
                .min(max_w / natural.x.max(1.0))
                .min(max_h / natural.y.max(1.0));
            // 最小宽度 48，但不要为了这个下限突破视口限制。
            let min_scale = (48.0 / natural.x.max(1.0))
                .min(max_w / natural.x.max(1.0))
                .min(max_h / natural.y.max(1.0));
            scale = scale.max(min_scale);
            let size = Vec2::new((natural.x * scale).max(16.0), (natural.y * scale).max(16.0));
            // 整行占满正文宽度，图片在行内居中：插图后不会孤零零贴在左边。
            let (row, resp) = ui.allocate_exact_size(Vec2::new(max_w, size.y), Sense::click());
            let rect = Rect::from_center_size(row.center(), size);
            let painter = ui.painter();
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            // 细边框：白色/浅色截图在浅色主题下不会和背景糊在一起。
            painter.rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, theme.border),
                egui::StrokeKind::Inside,
            );
            // 右下角缩放手柄：只在悬停（或正在拖）时显示，不打扰正常阅读。
            let grip_center = rect.max - Vec2::splat(9.0);
            let grip_rect = Rect::from_center_size(grip_center, Vec2::splat(18.0));
            let grip_resp = ui
                .interact(
                    grip_rect,
                    egui::Id::new(("img_grip", grip.0, grip.1)),
                    Sense::drag(),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeNwSe);
            if resp.hovered() || grip_resp.hovered() || grip_resp.dragged() {
                let p = ui.painter();
                for k in 0..3 {
                    let off = 2.0 + k as f32 * 3.5;
                    p.line_segment(
                        [
                            Pos2::new(grip_center.x - 6.0 + off, grip_center.y + 6.0),
                            Pos2::new(grip_center.x + 6.0, grip_center.y + 6.0 - off),
                        ],
                        Stroke::new(1.4, theme.dim),
                    );
                }
            }
            let mut resized = None;
            if grip_resp.dragged() {
                if let Some(p) = grip_resp.interact_pointer_pos() {
                    // 图片水平居中：以中心为轴，指针到中心的距离 ×2 就是目标宽度。
                    let want = (p.x - row.center().x) * 2.0;
                    let limit = max_w.min(max_h * aspect).max(48.0);
                    let w = want.clamp(48.0, limit).round() as u32;
                    if width_attr != Some(w) {
                        resized = Some(w);
                    }
                }
            }
            return (resp.on_hover_text(url), resized);
        }
    }
    // 图片找不到（路径写错/文件被删）时给一个可点击的占位框，
    // 比一行灰字更容易认出这里原本是图片。
    let name = if alt.is_empty() { url } else { alt };
    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 56.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(row, 4.0, theme.code_bg);
    painter.rect_stroke(
        row,
        4.0,
        egui::Stroke::new(1.0, theme.border),
        egui::StrokeKind::Inside,
    );
    let f = theme.fmt(theme.body * 0.9);
    painter.text(
        row.center(),
        egui::Align2::CENTER_CENTER,
        format!("图片未找到：{name}"),
        f.font_id,
        theme.dim,
    );
    (resp.on_hover_text(url), None)
}

/// 轻量词法着色，覆盖常见语言，不依赖 syntect。
/// 这里给每行末尾都补 `\n`，让区块底部多一行留白、看得更像代码框。
/// 各语言的关键字表。大小写敏感：SQL 用大写，其余多为小写。
/// 按语言族区分，避免把 A 语言的关键字误标到 B 语言。
fn keywords_for(lang: &str) -> &'static [&'static str] {
    match lang_family(lang) {
        LangFamily::Rust => &[
            "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
            "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
            "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
            "trait", "true", "type", "unsafe", "use", "where", "while",
        ],
        LangFamily::Python => &[
            "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
            "elif", "else", "except", "False", "finally", "for", "from", "global", "if", "import",
            "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return",
            "self", "True", "try", "while", "with", "yield",
        ],
        LangFamily::JavaScript => &[
            "as",
            "async",
            "await",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "default",
            "delete",
            "do",
            "else",
            "enum",
            "export",
            "extends",
            "false",
            "finally",
            "for",
            "from",
            "function",
            "if",
            "implements",
            "import",
            "in",
            "instanceof",
            "interface",
            "let",
            "new",
            "null",
            "of",
            "private",
            "protected",
            "public",
            "readonly",
            "return",
            "static",
            "super",
            "switch",
            "this",
            "throw",
            "true",
            "try",
            "typeof",
            "undefined",
            "var",
            "void",
            "while",
            "yield",
        ],
        LangFamily::CLike => &[
            "auto",
            "bool",
            "break",
            "byte",
            "case",
            "catch",
            "char",
            "class",
            "const",
            "continue",
            "default",
            "delete",
            "do",
            "double",
            "else",
            "enum",
            "extends",
            "false",
            "final",
            "finally",
            "float",
            "for",
            "foreach",
            "func",
            "goto",
            "if",
            "implements",
            "import",
            "int",
            "interface",
            "long",
            "namespace",
            "new",
            "nullptr",
            "NULL",
            "package",
            "private",
            "protected",
            "public",
            "return",
            "short",
            "sizeof",
            "static",
            "struct",
            "super",
            "switch",
            "template",
            "this",
            "throw",
            "true",
            "try",
            "typedef",
            "typename",
            "unsigned",
            "using",
            "var",
            "virtual",
            "void",
            "while",
        ],
        LangFamily::Css => &[
            "@import",
            "@media",
            "@keyframes",
            "@font-face",
            "from",
            "to",
            "important",
            "inherit",
            "initial",
            "none",
            "auto",
            "solid",
            "dashed",
            "block",
            "flex",
            "grid",
            "absolute",
            "relative",
            "fixed",
            "center",
            "bold",
            "italic",
        ],
        LangFamily::Data => &[
            "true", "false", "null", "True", "False", "None", "yes", "no", "on", "off",
        ],
        LangFamily::Shell => &[
            "alias", "break", "case", "cd", "continue", "declare", "do", "done", "echo", "elif",
            "else", "esac", "exit", "export", "false", "fi", "for", "function", "if", "in",
            "local", "readonly", "return", "set", "source", "then", "true", "unset", "while",
        ],
        LangFamily::Sql => &[
            "ADD",
            "ALL",
            "ALTER",
            "AND",
            "AS",
            "ASC",
            "AVG",
            "BETWEEN",
            "BY",
            "CASE",
            "COUNT",
            "CREATE",
            "CROSS",
            "DATABASE",
            "DEFAULT",
            "DELETE",
            "DESC",
            "DISTINCT",
            "DROP",
            "ELSE",
            "END",
            "EXISTS",
            "FALSE",
            "FROM",
            "FULL",
            "GROUP",
            "HAVING",
            "IN",
            "INDEX",
            "INNER",
            "INSERT",
            "INTO",
            "IS",
            "JOIN",
            "KEY",
            "LEFT",
            "LIKE",
            "LIMIT",
            "MAX",
            "MIN",
            "NOT",
            "NULL",
            "OFFSET",
            "ON",
            "OR",
            "ORDER",
            "OUTER",
            "PRIMARY",
            "REFERENCES",
            "RIGHT",
            "SELECT",
            "SET",
            "SUM",
            "TABLE",
            "THEN",
            "TRUE",
            "UNION",
            "UPDATE",
            "VALUES",
            "VIEW",
            "WHERE",
        ],
        LangFamily::Markup | LangFamily::Generic => &[],
    }
}

/// 该语言的行注释前缀（返回 true 表示从此处到行尾是注释）。
fn line_comment_at(family: LangFamily, c: &[char], i: usize) -> bool {
    match family {
        // # : Python / Shell / YAML / TOML
        LangFamily::Python | LangFamily::Shell | LangFamily::Data => c[i] == '#',
        // -- : SQL
        LangFamily::Sql => c[i] == '-' && i + 1 < c.len() && c[i + 1] == '-',
        // // : Rust / JS / C-like / CSS；HTML/XML 用 <!-- -->（块注释处理）
        _ => c[i] == '/' && i + 1 < c.len() && c[i + 1] == '/',
    }
}

/// 该语言是否支持 /* */ 块注释。
fn has_block_comment(family: LangFamily) -> bool {
    matches!(
        family,
        LangFamily::Rust | LangFamily::JavaScript | LangFamily::CLike | LangFamily::Css
    )
}

/// HTML/XML 注释 <!-- -->。
fn is_markup_comment(c: &[char], i: usize) -> bool {
    c[i] == '<' && i + 3 < c.len() && c[i + 1] == '!' && c[i + 2] == '-' && c[i + 3] == '-'
}

pub fn code_job(code: &str, lang: &str, theme: &Theme) -> LayoutJob {
    code_job_impl(code, lang, theme, true)
}

/// 编辑态用的高亮排版：**文本与缓冲区逐字符一致**（不补行尾换行），
/// 否则 TextEdit 的光标/选中会按错位的字符下标走。
pub fn code_job_edit(code: &str, lang: &str, theme: &Theme) -> LayoutJob {
    code_job_impl(code, lang, theme, false)
}

fn code_job_impl(code: &str, lang: &str, theme: &Theme, trailing_newline: bool) -> LayoutJob {
    let family = lang_family(lang);
    let kw = keywords_for(lang);
    let [
        kw_color,
        str_color,
        num_color,
        comment_color,
        fun_color,
        type_color,
    ] = theme.syntax(lang);
    let markup = matches!(family, LangFamily::Markup);
    let css = matches!(family, LangFamily::Css);
    let data = matches!(family, LangFamily::Data);
    let block_comments = has_block_comment(family);

    let mut job = LayoutJob::default();
    // 代码行常没有空格，允许在任意字符处换行以适配窄窗口。
    job.wrap.break_anywhere = true;
    let lines: Vec<&str> = code.split('\n').collect();
    // 跨行块注释状态：None=不在块注释中；Some(true)=<!-- -->；Some(false)=/* */
    let mut in_block: Option<bool> = None;

    for (n, line) in lines.iter().enumerate() {
        let c: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < c.len() {
            // 块注释中：找结束符
            if let Some(is_markup) = in_block {
                let end_seq: &[char] = if is_markup {
                    &['-', '-', '>']
                } else {
                    &['*', '/']
                };
                let pos = (i..=c.len().saturating_sub(end_seq.len()))
                    .find(|&k| &c[k..k + end_seq.len()] == end_seq);
                if let Some(pos) = pos {
                    let end = pos + end_seq.len();
                    let rest: String = c[i..end].iter().collect();
                    let mut f = theme.mono_fmt();
                    f.color = comment_color;
                    job.append(&rest, 0.0, f);
                    i = end;
                    in_block = None;
                } else {
                    let rest: String = c[i..].iter().collect();
                    let mut f = theme.mono_fmt();
                    f.color = comment_color;
                    job.append(&rest, 0.0, f);
                    break;
                }
                continue;
            }
            // 块注释开始
            if markup && is_markup_comment(&c, i) {
                in_block = Some(true);
                let rest: String = c[i..i + 4].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = comment_color;
                job.append(&rest, 0.0, f);
                i += 4;
                continue;
            }
            if block_comments && c[i] == '/' && i + 1 < c.len() && c[i + 1] == '*' {
                in_block = Some(false);
                let rest: String = c[i..i + 2].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = comment_color;
                job.append(&rest, 0.0, f);
                i += 2;
                continue;
            }
            // 行注释
            if line_comment_at(family, &c, i) {
                let rest: String = c[i..].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = comment_color;
                job.append(&rest, 0.0, f);
                break;
            }
            // 字符串（含引号）
            if c[i] == '"' || c[i] == '\'' || c[i] == '`' {
                let q = c[i];
                let mut j = i + 1;
                while j < c.len() && c[j] != q {
                    j += 1;
                }
                let end = (j + 1).min(c.len());
                let s: String = c[i..end].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = str_color;
                job.append(&s, 0.0, f);
                i = end;
                continue;
            }
            // HTML/XML 标签：<tag 的标签名用类型色
            if markup
                && c[i] == '<'
                && i + 1 < c.len()
                && (c[i + 1].is_alphabetic() || c[i + 1] == '/')
            {
                let mut j = i + 1;
                if c[j] == '/' {
                    j += 1;
                }
                while j < c.len() && (c[j].is_alphanumeric() || c[j] == '-' || c[j] == ':') {
                    j += 1;
                }
                let s: String = c[i..j].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = type_color;
                job.append(&s, 0.0, f);
                i = j;
                continue;
            }
            // 标识符 / 数字
            if c[i].is_alphanumeric() || c[i] == '_' {
                let mut j = i;
                while j < c.len() && (c[j].is_alphanumeric() || c[j] == '_') {
                    j += 1;
                }
                let tok: String = c[i..j].iter().collect();
                let is_num = tok
                    .chars()
                    .next()
                    .map(|x| x.is_ascii_digit())
                    .unwrap_or(false);
                let is_kw = kw.contains(&tok.as_str());
                // 下一个非空白字符是 '(' → 函数名；大写开头 → 类型/常量
                let next = c[j..].iter().find(|&&x| !x.is_whitespace()).copied();
                let is_call = next == Some('(');
                let is_type = tok.chars().next().is_some_and(|x| x.is_uppercase());
                // YAML 的 key: / CSS 的 属性:
                let is_prop = (data || css) && next == Some(':');
                let (color, bold) = if is_num {
                    (num_color, false)
                } else if is_kw {
                    (kw_color, true)
                } else if is_call {
                    (fun_color, false)
                } else if is_type {
                    (type_color, false)
                } else if is_prop {
                    (fun_color, false)
                } else {
                    (theme.text, false)
                };
                let mut f = theme.mono_fmt();
                f.color = color;
                if bold {
                    f.coords = egui::epaint::text::VariationCoords::new([(b"wght", 700.0)]);
                }
                job.append(&tok, 0.0, f);
                i = j;
                continue;
            }
            let mut f = theme.mono_fmt();
            f.color = theme.dim;
            job.append(&c[i].to_string(), 0.0, f);
            i += 1;
        }
        // 编辑态不补最后一行之后的换行，保证 job.text 与缓冲区完全一致
        if trailing_newline || n + 1 < lines.len() {
            job.append("\n", 0.0, theme.mono_fmt());
        }
    }
    if code.is_empty() && trailing_newline {
        job.append(" ", 0.0, theme.mono_fmt());
    }
    job
}

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

/// 从一行里提取节点定义。返回 (id, 标签, 形状)：0=矩形[]、1=圆角()、2=菱形{}。
fn extract_nodes(line: &str) -> Vec<(String, String, u8)> {
    let c: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < c.len() {
        if c[i].is_alphanumeric() || c[i] == '_' {
            let s = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let id: String = c[s..i].iter().collect();
            if i < c.len() && matches!(c[i], '[' | '(' | '{') {
                let (close, shape) = match c[i] {
                    '[' => (']', 0_u8),
                    '(' => (')', 1),
                    _ => ('}', 2),
                };
                let start = i + 1;
                let mut j = start;
                while j < c.len() && c[j] != close {
                    j += 1;
                }
                let label: String = c[start..j].iter().collect();
                out.push((id, label, shape));
                i = j.saturating_add(1);
                continue;
            }
        }
        i += 1;
    }
    out
}

fn first_id(s: &str) -> String {
    s.trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

fn parse_flow(
    code: &str,
) -> (
    Vec<(String, String, u8)>,
    Vec<(String, String, String)>,
    bool,
) {
    let mut nodes: Vec<(String, String, u8)> = Vec::new();
    let mut edges: Vec<(String, String, String)> = Vec::new();
    let mut horizontal = false;
    for raw in code.lines() {
        let l = raw.trim();
        if l.starts_with("graph LR")
            || l.starts_with("flowchart LR")
            || l.starts_with("graph RL")
            || l.starts_with("flowchart RL")
        {
            horizontal = true;
        }
        if l.starts_with("graph")
            || l.starts_with("flowchart")
            || l.starts_with("sequenceDiagram")
            || l.starts_with("gantt")
            || l.is_empty()
            || l.starts_with("%%")
        {
            continue;
        }
        for (id, label, shape) in extract_nodes(l) {
            if !nodes.iter().any(|(n, _, _)| n == &id) {
                nodes.push((id, label, shape));
            }
        }
        if let Some(arrow) = l.find("-->").or_else(|| l.find("---")) {
            let left = &l[..arrow];
            let mut after = &l[arrow + 3..];
            let mut label = String::new();
            if after.starts_with('|') {
                if let Some(e) = after[1..].find('|') {
                    label = after[1..1 + e].to_string();
                    after = &after[2 + e..];
                }
            }
            let a = first_id(left);
            let b = first_id(after);
            if !a.is_empty() && !b.is_empty() {
                edges.push((a, b, label));
            }
        }
    }
    (nodes, edges, horizontal)
}

fn draw_flow(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (nodes, edges, horizontal) = parse_flow(code);
    if nodes.is_empty() {
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

    let nw = 116.0_f32;
    let nh = 34.0_f32;
    let gap = 26.0_f32;

    // 分层布局：没有入边的节点在第 0 层，其余节点深度 = 前驱最大深度 + 1。
    // 有限次迭代求解最长路径，环状引用靠迭代上限截断，不会死循环。
    // TD/TB 自上而下逐层排（行 = 层），LR/RL 自左向右逐层排（列 = 层），
    // 不再按固定每行 3 个摆格子——那样 TD 图会被横着摆成一排。
    let mut layer: HashMap<String, usize> =
        nodes.iter().map(|(id, _, _)| (id.clone(), 0)).collect();
    for _ in 0..nodes.len() {
        let mut changed = false;
        for (a, b, _) in &edges {
            let la = layer.get(a.as_str()).copied();
            if let Some(lb) = layer.get_mut(b.as_str()) {
                let la = la.unwrap_or(0);
                if la + 1 > *lb {
                    *lb = la + 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let depth = layer.values().copied().max().unwrap_or(0) + 1;
    let mut layers: Vec<Vec<(String, String, u8)>> = vec![Vec::new(); depth];
    for n in &nodes {
        layers[layer[&n.0]].push(n.clone());
    }
    // 单层最多多少个节点决定画布宽/高；孤立节点都落在第 0 层。
    let widest = layers.iter().map(|g| g.len()).max().unwrap_or(1).max(1);
    let (w, h) = if horizontal {
        (
            (depth as f32) * (nw + gap) + gap,
            (widest as f32) * (nh + gap) + gap,
        )
    } else {
        (
            (widest as f32) * (nw + gap) + gap,
            (depth as f32) * (nh + gap) + gap,
        )
    };

    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w.max(160.0), h.max(60.0)), Sense::click());
    let p = ui.painter();
    let mut pos: HashMap<String, Pos2> = HashMap::new();
    for (li, group) in layers.iter().enumerate() {
        for (gi, (id, _, _)) in group.iter().enumerate() {
            let (x, y) = if horizontal { (li, gi) } else { (gi, li) };
            pos.insert(
                id.clone(),
                Pos2::new(
                    rect.min.x + gap + (x as f32) * (nw + gap) + nw / 2.0,
                    rect.min.y + gap + (y as f32) * (nh + gap) + nh / 2.0,
                ),
            );
        }
    }
    let edge_stroke = Stroke::new(1.5, theme.dim);
    for (a, b, label) in &edges {
        if let (Some(pa), Some(pb)) = (pos.get(a), pos.get(b)) {
            let dx = pb.x - pa.x;
            let dy = pb.y - pa.y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < 1.0 {
                continue;
            }
            let (ux, uy) = (dx / dist, dy / dist);
            // 线段只画在两个节点框之间（从边缘到边缘），箭头顶在目标框边上，
            // 不再从中心到中心、整条线都被节点盖住。
            let trim = |hw: f32, hh: f32, dx: f32, dy: f32| -> f32 {
                let tx = if dx.abs() > 1e-4 {
                    hw / dx.abs()
                } else {
                    f32::INFINITY
                };
                let ty = if dy.abs() > 1e-4 {
                    hh / dy.abs()
                } else {
                    f32::INFINITY
                };
                tx.min(ty)
            };
            let t0 = trim(nw / 2.0 + 2.0, nh / 2.0 + 2.0, ux, uy);
            let t1 = dist - trim(nw / 2.0 + 4.0, nh / 2.0 + 4.0, -ux, -uy);
            if t1 <= t0 {
                continue;
            }
            let s = Pos2::new(pa.x + ux * t0, pa.y + uy * t0);
            let e = Pos2::new(pa.x + ux * t1, pa.y + uy * t1);
            p.line_segment([s, e], edge_stroke);
            // 箭头：从终点往回画两条短斜线组成的 V 形
            let a0 = (e.y - s.y).atan2(e.x - s.x);
            for da in [-0.5_f32, 0.5] {
                let ang = a0 + std::f32::consts::PI + da;
                let wing = Pos2::new(e.x + ang.cos() * 7.0, e.y + ang.sin() * 7.0);
                p.line_segment([e, wing], edge_stroke);
            }
            if !label.is_empty() {
                let mid = Pos2::new((s.x + e.x) / 2.0, (s.y + e.y) / 2.0);
                p.text(
                    mid,
                    Align2::CENTER_CENTER,
                    label,
                    FontId::new(11.0, FontFamily::Proportional),
                    theme.dim,
                );
            }
        }
    }
    let node_i = |id: &str| -> usize { nodes.iter().position(|n| &n.0 == id).unwrap_or(0) };
    for (id, label, shape) in &nodes {
        if let Some(c) = pos.get(id) {
            let r = Rect::from_center_size(*c, Vec2::new(nw, nh));
            let fill = palette_color(theme, node_i(id));
            let stroke_c = Stroke::new(1.2, theme.border);
            match shape {
                // {判断} —— 决策节点画菱形
                2 => {
                    let pts = vec![
                        Pos2::new(c.x, r.top()),
                        Pos2::new(r.right(), c.y),
                        Pos2::new(c.x, r.bottom()),
                        Pos2::new(r.left(), c.y),
                    ];
                    p.add(egui::Shape::convex_polygon(pts, fill, stroke_c));
                }
                // (圆角节点) 画胶囊
                1 => {
                    p.rect_filled(r, nh / 2.0, fill);
                    p.rect_stroke(r, nh / 2.0, stroke_c, egui::StrokeKind::Outside);
                }
                // [普通节点] 画矩形
                _ => {
                    p.rect_filled(r, 5.0, fill);
                    p.rect_stroke(r, 5.0, stroke_c, egui::StrokeKind::Outside);
                }
            }
            p.text(
                r.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::new(12.0, FontFamily::Proportional),
                theme.text,
            );
        }
    }
    resp
}

/// 解析 mermaid 饼图：`pie title 标题` 加若干行 `"名称" : 数值`。
fn parse_pie(code: &str) -> (String, Vec<(String, f64)>) {
    let mut title = String::new();
    let mut data = Vec::new();
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l == "pie" || l == "showData" || l.starts_with("%%") {
            continue;
        }
        if let Some(t) = l.strip_prefix("pie title ") {
            title = t.trim().to_string();
            continue;
        }
        if l.starts_with('"') {
            if let Some(e) = l[1..].find('"') {
                let label = l[1..1 + e].to_string();
                let rest = l[1 + e..].trim_start_matches('"').trim();
                let val = rest
                    .split(':')
                    .next_back()
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .unwrap_or(0.0);
                if !label.is_empty() {
                    data.push((label, val));
                }
            }
        }
    }
    (title, data)
}

const PIE_PALETTE: [Color32; 6] = [
    Color32::from_rgb(76, 141, 255),
    Color32::from_rgb(255, 159, 67),
    Color32::from_rgb(46, 204, 113),
    Color32::from_rgb(231, 76, 60),
    Color32::from_rgb(155, 89, 182),
    Color32::from_rgb(26, 188, 156),
];

/// 圆扇形：用「圆心 + 弧上采样点」的三角扇逼近，兼容当前 egui 版本
/// （它没有 Shape::circle_sector）。
fn pie_sector(center: Pos2, radius: f32, start: f64, end: f64, color: Color32) -> egui::Shape {
    let span = (end - start).max(0.0);
    let steps = ((span / 0.08).ceil() as usize).max(2);
    let mut pts = vec![center];
    for k in 0..=steps {
        let a = start + span * (k as f64) / (steps as f64);
        pts.push(Pos2::new(
            center.x + (a as f32).cos() * radius,
            center.y + (a as f32).sin() * radius,
        ));
    }
    let mut mesh = egui::Mesh::default();
    for pt in &pts {
        mesh.colored_vertex(*pt, color);
    }
    let mut idx = Vec::with_capacity(steps * 3);
    for k in 0..steps {
        let b = (k + 1) as u32;
        idx.extend_from_slice(&[0, b, b + 1]);
    }
    mesh.indices = idx;
    egui::Shape::mesh(mesh)
}

// ---------------- Mermaid 时序图（sequenceDiagram） ----------------

#[derive(Debug, PartialEq, Clone)]
enum SeqItem {
    /// from/to 是参与者下标；dashed = `-->`，cross = `x` 结尾箭头（-x / --x）。
    Message {
        from: usize,
        to: usize,
        label: String,
        dashed: bool,
        cross: bool,
    },
    /// over 是参与者下标区间（单元素 = 单个参与者上方）。
    Note { over: (usize, usize), label: String },
}

struct SeqParticipant {
    id: String,
    label: String,
}

/// 解析 sequenceDiagram：participant/actor 声明、四类箭头消息、Note、title。
/// 识别不了的控制行（activate/loop/alt/autonumber 等）直接跳过，不影响其余内容。
fn parse_sequence(code: &str) -> (Vec<SeqParticipant>, Vec<SeqItem>, String) {
    let mut parts: Vec<SeqParticipant> = Vec::new();
    let mut items: Vec<SeqItem> = Vec::new();
    let mut title = String::new();

    fn ensure_part(parts: &mut Vec<SeqParticipant>, id: &str) -> usize {
        if let Some(i) = parts.iter().position(|p| p.id == id) {
            return i;
        }
        parts.push(SeqParticipant {
            id: id.to_string(),
            label: id.to_string(),
        });
        parts.len() - 1
    }

    for raw in code.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_lowercase();
        if lower == "sequencediagram" || lower.starts_with("sequencediagram ") {
            continue;
        }
        if lower.starts_with("title") {
            title = line[5..].trim().to_string();
            continue;
        }
        if lower.starts_with("participant ") || lower.starts_with("actor ") {
            let rest = &line[lower.find(' ').unwrap_or(0) + 1..];
            let (pid, alias) = match rest.find(" as ") {
                Some(i) => (rest[..i].trim(), rest[i + 4..].trim()),
                None => (rest.trim(), ""),
            };
            if pid.is_empty() {
                continue;
            }
            let idx = ensure_part(&mut parts, pid);
            if !alias.is_empty() {
                parts[idx].label = alias.to_string();
            }
            continue;
        }
        if lower.starts_with("note ") {
            let rest = &line[5..];
            let (scope, text) = match rest.find(':') {
                Some(i) => (&rest[..i], rest[i + 1..].trim()),
                None => (rest, ""),
            };
            let st = scope.trim();
            // 参与者名区分大小写，只对前缀关键字做小写匹配，截取时用原文
            let sl = st.to_lowercase();
            let ids: Vec<&str> = if sl.strip_prefix("over ").is_some() {
                st[5..]
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect()
            } else if sl.strip_prefix("left of ").is_some() {
                vec![st[8..].trim()]
            } else if sl.strip_prefix("right of ").is_some() {
                vec![st[9..].trim()]
            } else {
                vec![]
            };
            if ids.is_empty() || text.is_empty() {
                continue;
            }
            let idxs: Vec<usize> = ids.iter().map(|id| ensure_part(&mut parts, id)).collect();
            let lo = idxs.iter().copied().min().unwrap_or(0);
            let hi = idxs.iter().copied().max().unwrap_or(0);
            items.push(SeqItem::Note {
                over: (lo, hi),
                label: text.to_string(),
            });
            continue;
        }
        // 控制行：跳过（activate/autonumber/loop/alt/else/end/opt/par/rect 等）
        const SKIP: [&str; 11] = [
            "autonumber",
            "activate ",
            "deactivate ",
            "loop ",
            "alt ",
            "else",
            "end",
            "opt ",
            "par ",
            "rect ",
            "critical ",
        ];
        if SKIP.iter().any(|k| lower == *k || lower.starts_with(k)) {
            continue;
        }
        // 消息行：`A->>B: 文本` / `A-->>B` / `A-x B` / `A--xB`
        if let Some(ci) = line.find(':') {
            let (left, label) = (&line[..ci], line[ci + 1..].trim());
            // 依次找箭头标记。必须先匹配更长的 "-->>"/"--x"：如果把 "->>"
            // 排在前面，"D-->>S" 会命中第二个 '-' 开始的 "->>"，把发送方
            // 切成 "D-"、接收方切成 "S-"，凭空多出两个参与者。
            const ARROWS: [&str; 6] = ["-->>", "->>", "--x", "-x", "--", "->"];
            if let Some(arrow) = ARROWS.iter().find(|a| left.contains(**a)) {
                let ai = left.find(arrow).unwrap();
                let a = left[..ai].trim();
                let b = left[ai + arrow.len()..].trim();
                if a.is_empty() || b.is_empty() {
                    continue;
                }
                let dashed = arrow.starts_with("--");
                let cross = arrow.ends_with('x');
                let from = ensure_part(&mut parts, a);
                let to = ensure_part(&mut parts, b);
                items.push(SeqItem::Message {
                    from,
                    to,
                    label: label.to_string(),
                    dashed,
                    cross,
                });
            }
        }
    }
    (parts, items, title)
}

/// 虚线：egui 的 Stroke 不支持 dash，手动画短段。
fn paint_dashed_line(p: &egui::Painter, from: Pos2, to: Pos2, color: Color32, width: f32) {
    let dash = 5.0;
    let gap = 3.0;
    let len = (to - from).length();
    if len <= 0.5 {
        return;
    }
    let dir = (to - from) / len;
    let mut t = 0.0;
    while t < len {
        let end = (t + dash).min(len);
        p.line_segment(
            [from + dir * t, from + dir * end],
            Stroke::new(width, color),
        );
        t = end + gap;
    }
}

fn draw_sequence(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (parts, items, title) = parse_sequence(code);
    if parts.is_empty() {
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
    let row_h = 34.0;
    let self_h = 44.0;
    let note_h = 30.0;
    let box_h = 26.0;
    let pad = 14.0;
    let title_h = if title.is_empty() { 0.0 } else { 22.0 };
    let n = parts.len();
    let w = ui.available_width().max(260.0);
    let slots = (w - 2.0 * pad) / n as f32;
    let total_h = title_h + box_h + 10.0 + items.len() as f32 * row_h + 14.0;
    // Sense::click 让图形可点击进入编辑态改代码（与流程图/饼图一致）。
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, total_h), Sense::click());
    let p = ui.painter();

    // 参与者水平均分；标签过长的盒子以文字宽度为准，但不超过槽宽
    let x_of = |i: usize| rect.min.x + pad + slots * (i as f32 + 0.5);
    if !title.is_empty() {
        p.text(
            Pos2::new(rect.center().x, rect.min.y + 2.0),
            Align2::CENTER_TOP,
            &title,
            FontId::proportional(13.0),
            theme.heading,
        );
    }
    let top_y = rect.min.y + title_h + 2.0;
    let bottom_y = rect.max.y - 10.0;
    // 生命线（虚线）先画，箭头和盒子盖在上面
    for i in 0..n {
        let x = x_of(i);
        paint_dashed_line(
            p,
            Pos2::new(x, top_y + box_h),
            Pos2::new(x, bottom_y),
            theme.dim.gamma_multiply(0.6),
            1.0,
        );
    }
    // 参与者盒子
    for (i, part) in parts.iter().enumerate() {
        let cx = x_of(i);
        let bw = ((part.label.chars().count() as f32) * 12.0 + 20.0).min((slots - 6.0).max(40.0));
        let bx = Rect::from_center_size(
            Pos2::new(cx, top_y + box_h / 2.0),
            Vec2::new(bw, box_h - 6.0),
        );
        p.rect_filled(bx, 4.0, theme.table_head);
        p.rect_stroke(
            bx,
            4.0,
            Stroke::new(1.0, theme.border),
            egui::StrokeKind::Outside,
        );
        p.text(
            bx.center(),
            Align2::CENTER_CENTER,
            &part.label,
            FontId::proportional(12.0),
            theme.text,
        );
    }
    // 消息 / Note 按源码顺序排布
    let mut y = top_y + box_h + 12.0;
    for it in &items {
        match it {
            SeqItem::Message {
                from,
                to,
                label,
                dashed,
                cross,
            } => {
                let x0 = x_of(*from);
                let x1 = x_of(*to);
                if from == to {
                    // 自环消息：向右绕一个小圈，标签写在右侧
                    let x = x0;
                    let loop_w = 26.0;
                    let y1 = y + 6.0;
                    let y2 = y + 26.0;
                    p.line_segment(
                        [Pos2::new(x, y1), Pos2::new(x + loop_w, y1)],
                        Stroke::new(1.0, theme.text),
                    );
                    p.line_segment(
                        [Pos2::new(x + loop_w, y1), Pos2::new(x + loop_w, y2)],
                        Stroke::new(1.0, theme.text),
                    );
                    p.line_segment(
                        [Pos2::new(x + loop_w, y2), Pos2::new(x + 2.0, y2)],
                        Stroke::new(1.0, theme.text),
                    );
                    // 箭头尖（指向左，回到生命线）
                    p.line_segment(
                        [Pos2::new(x + 2.0, y2), Pos2::new(x + 9.0, y2 - 4.0)],
                        Stroke::new(1.0, theme.text),
                    );
                    p.line_segment(
                        [Pos2::new(x + 2.0, y2), Pos2::new(x + 9.0, y2 + 4.0)],
                        Stroke::new(1.0, theme.text),
                    );
                    if !label.is_empty() {
                        p.text(
                            Pos2::new(x + loop_w + 8.0, (y1 + y2) / 2.0),
                            Align2::LEFT_CENTER,
                            label,
                            FontId::proportional(12.0),
                            theme.text,
                        );
                    }
                    y += self_h;
                } else {
                    let (xa, xb) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
                    if *dashed {
                        paint_dashed_line(p, Pos2::new(x0, y), Pos2::new(x1, y), theme.text, 1.0);
                    } else {
                        p.line_segment(
                            [Pos2::new(x0, y), Pos2::new(x1, y)],
                            Stroke::new(1.2, theme.text),
                        );
                    }
                    // 箭头尖（指向 to 端）
                    let dir = ((x1 - x0) as f32).signum();
                    let tip = Pos2::new(x1, y);
                    if *cross {
                        // -x：叉形箭头
                        p.line_segment(
                            [
                                tip - egui::vec2(7.0 * dir, 6.0),
                                tip + egui::vec2(1.0 * dir, 0.0),
                            ],
                            Stroke::new(1.2, theme.text),
                        );
                        p.line_segment(
                            [
                                tip - egui::vec2(7.0 * dir, -6.0),
                                tip + egui::vec2(1.0 * dir, 0.0),
                            ],
                            Stroke::new(1.2, theme.text),
                        );
                    } else {
                        // 三角箭头
                        p.line_segment(
                            [tip - egui::vec2(9.0 * dir, 5.0), tip],
                            Stroke::new(1.2, theme.text),
                        );
                        p.line_segment(
                            [tip - egui::vec2(9.0 * dir, -5.0), tip],
                            Stroke::new(1.2, theme.text),
                        );
                    }
                    if !label.is_empty() {
                        p.text(
                            Pos2::new((xa + xb) / 2.0, y - 6.0),
                            Align2::CENTER_BOTTOM,
                            label,
                            FontId::proportional(12.0),
                            theme.text,
                        );
                    }
                    y += row_h;
                }
            }
            SeqItem::Note { over, label } => {
                let xa = x_of(over.0);
                let xb = x_of(over.1);
                let cx = (xa + xb) / 2.0;
                let bw = ((label.chars().count() as f32) * 12.0 + 24.0)
                    .max(60.0)
                    .min((xb - xa + slots * 0.9).max(60.0));
                let nb = Rect::from_center_size(
                    Pos2::new(cx, y + note_h / 2.0 - 4.0),
                    Vec2::new(bw, note_h - 10.0),
                );
                p.rect_filled(nb, 2.0, theme.code_bg);
                p.rect_stroke(
                    nb,
                    2.0,
                    Stroke::new(1.0, theme.border),
                    egui::StrokeKind::Outside,
                );
                p.text(
                    nb.center(),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(11.5),
                    theme.dim,
                );
                y += note_h;
            }
        }
    }
    resp
}

/// 状态图 → 流程图语法转换：stateDiagram 的 `A --> B: 标签` 改写成
/// flowchart 的 `A -->|标签| B`；`[*]` 起止标记换成圆角节点（起点/终点），
/// 否则 first_id 取不到标识符、含 [*] 的边会被整条丢弃。note/state 声明跳过。
fn transform_state_to_flow(code: &str) -> String {
    let mut out = String::from("flowchart TD\n");
    for raw in code.lines() {
        let l = raw.trim();
        let lower = l.to_lowercase();
        if l.is_empty()
            || lower.starts_with("statediagram")
            || lower.starts_with("note ")
            || lower.starts_with("state ")
        {
            continue;
        }
        // [*] 必须换成带方括号标签的普通节点：裸 id 会被 first_id
        // （只取字母数字）判成空串，整条边连带节点一起被丢弃。
        let l2 = l
            .replace("[*] -->", "ST0[●开始] -->")
            .replace("--> [*]", "--> ST9[●结束]");
        let l: &str = &l2;
        // `A --> B: 标签` → `A -->|标签| B`
        if let Some(ci) = l.find(':').filter(|_| l.contains("-->")) {
            let left = &l[..ci];
            let label = l[ci + 1..].trim();
            if !label.is_empty() && !left.contains('|') {
                out.push_str(&format!("{left}|{label}|\n"));
                continue;
            }
        }
        out.push_str(l);
        out.push('\n');
    }
    out
}

/// 指针命中哪一行哪一列的单元格（表格工具栏的行列基准）。
fn hit_cell(row_rects: &[Vec<Rect>], p: Pos2) -> Option<(usize, usize)> {
    for (ri, row) in row_rects.iter().enumerate() {
        for (ci, r) in row.iter().enumerate() {
            if r.contains(p) {
                return Some((ri, ci));
            }
        }
    }
    None
}

// ---------------- 图表主题配色与通用「盒子+连线」布局 ----------------

/// 图表节点配色：浅色主题柔和马卡龙系、深色主题低饱和系，节点按序循环取色。
fn diagram_palette(theme: &Theme) -> Vec<Color32> {
    if theme.kind.is_dark() {
        vec![
            Color32::from_rgb(56, 89, 132),
            Color32::from_rgb(72, 110, 82),
            Color32::from_rgb(116, 84, 130),
            Color32::from_rgb(140, 104, 66),
            Color32::from_rgb(66, 116, 124),
        ]
    } else {
        vec![
            Color32::from_rgb(219, 234, 254),
            Color32::from_rgb(220, 245, 218),
            Color32::from_rgb(243, 226, 250),
            Color32::from_rgb(255, 237, 213),
            Color32::from_rgb(213, 245, 244),
        ]
    }
}

fn palette_color(theme: &Theme, i: usize) -> Color32 {
    let pal = diagram_palette(theme);
    pal[i % pal.len()]
}

#[derive(Clone, Copy, PartialEq)]
enum EdgeStyle {
    Solid,
    Dashed,
}

#[derive(Clone, Copy, PartialEq)]
enum ArrowHead {
    None,
    Triangle,
    HollowTriangle,
    FilledDiamond,
    HollowDiamond,
}

struct BoxNode {
    id: String,
    label: String,
    members: Vec<String>,
}

struct BoxEdge {
    a: String,
    b: String,
    label: String,
    style: EdgeStyle,
    head_a: ArrowHead,
    head_b: ArrowHead,
}

/// 通用「盒子 + 连线」渲染：类图 / ER 图 / 状态图共用。
/// 盒子按网格均布，连线画在盒子下层、从边缘到边缘，装饰端点按样式绘制。
fn draw_boxes_edges(
    ui: &mut egui::Ui,
    theme: &Theme,
    nodes: &[BoxNode],
    edges: &[BoxEdge],
) -> egui::Response {
    let empty = Vec::new();
    let nodes = if nodes.is_empty() { &empty } else { nodes };
    let n = nodes.len();
    if n == 0 {
        let r = panel_frame(theme).show(ui, |ui| {
            ui.label(
                egui::RichText::new("（没有可绘制的内容）")
                    .small()
                    .color(theme.dim),
            );
        });
        return r.response.interact(Sense::click());
    }
    // 盒子尺寸：按各自文字内容算，网格取最大值。
    let measure = |s: &str, size: f32| -> f32 { s.chars().count() as f32 * size };
    let node_sizes: Vec<Vec2> = nodes
        .iter()
        .map(|nd| {
            let inner_w = measure(&nd.label, 12.5).max(
                nd.members
                    .iter()
                    .map(|m| measure(m, 11.0))
                    .fold(0.0, f32::max),
            );
            Vec2::new(
                (inner_w + 28.0).min(220.0),
                30.0 + nd.members.len() as f32 * 16.0,
            )
        })
        .collect();
    let cols = ((n as f32).sqrt().ceil() as usize).max(1);
    let rows = n.div_ceil(cols);
    let mut cell = Vec2::ZERO;
    for r in 0..rows {
        for c in 0..cols {
            let i = r * cols + c;
            if i < n {
                cell = Vec2::new(cell.x.max(node_sizes[i].x), cell.y.max(node_sizes[i].y));
            }
        }
    }
    cell += Vec2::new(30.0, 30.0);
    let w = (cell.x * cols as f32).max(240.0);
    let h = (cell.y * rows as f32).max(120.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let p = ui.painter();

    // 节点位置：网格均布（格子内居中）。
    let mut centers: HashMap<String, Pos2> = HashMap::new();
    let mut sizes: HashMap<String, Vec2> = HashMap::new();
    for (i, nd) in nodes.iter().enumerate() {
        let (r, c) = (i / cols, i % cols);
        let cx = rect.min.x + cell.x * c as f32 + cell.x / 2.0;
        let cy = rect.min.y + cell.y * r as f32 + cell.y / 2.0;
        centers.insert(nd.id.clone(), Pos2::new(cx, cy));
        sizes.insert(nd.id.clone(), node_sizes[i]);
    }

    // 连线先画（垫在盒子下层），装饰箭头/菱形画在端点。
    for e in edges {
        let (Some(pa), Some(pb)) = (centers.get(&e.a), centers.get(&e.b)) else {
            continue;
        };
        let size_a = sizes.get(&e.a).copied().unwrap_or(Vec2::splat(80.0));
        let size_b = sizes.get(&e.b).copied().unwrap_or(Vec2::splat(80.0));
        let dx = pb.x - pa.x;
        let dy = pb.y - pa.y;
        let dist = (dx * dx + dy * dy).sqrt().max(1.0);
        let (ux, uy) = (dx / dist, dy / dist);
        let trim = |hw: f32, hh: f32, dx: f32, dy: f32| -> f32 {
            let tx = if dx.abs() > 1e-4 {
                hw / dx.abs()
            } else {
                f32::INFINITY
            };
            let ty = if dy.abs() > 1e-4 {
                hh / dy.abs()
            } else {
                f32::INFINITY
            };
            tx.min(ty)
        };
        let t0 = trim(size_a.x / 2.0, size_a.y / 2.0, ux, uy);
        let t1 = dist - trim(size_b.x / 2.0, size_b.y / 2.0, -ux, -uy);
        if t1 <= t0 + 4.0 {
            continue;
        }
        let s = Pos2::new(pa.x + ux * t0, pa.y + uy * t0);
        let epos = Pos2::new(pa.x + ux * t1, pa.y + uy * t1);
        let stroke = Stroke::new(
            1.3,
            if e.style == EdgeStyle::Dashed {
                theme.dim
            } else {
                theme.text.gamma_multiply(0.8)
            },
        );
        if e.style == EdgeStyle::Dashed {
            paint_dashed_line(p, s, epos, stroke.color, stroke.width);
        } else {
            p.line_segment([s, epos], stroke);
        }
        paint_arrow_head(p, epos, (ux, uy), stroke.color, e.head_b);
        paint_arrow_head(p, s, (-ux, -uy), stroke.color, e.head_a);
        if !e.label.is_empty() {
            let mid = Pos2::new((s.x + epos.x) / 2.0, (s.y + epos.y) / 2.0);
            p.text(
                mid,
                Align2::CENTER_CENTER,
                &e.label,
                FontId::proportional(11.0),
                theme.dim,
            );
        }
    }

    // 盒子后画（盖住连线端点），填充色按调色板循环。
    for (i, nd) in nodes.iter().enumerate() {
        let c = centers[&nd.id];
        let sz = node_sizes[i];
        let r = Rect::from_center_size(c, sz);
        let fill = palette_color(theme, i);
        p.rect_filled(r, 6.0, fill);
        p.rect_stroke(
            r,
            6.0,
            Stroke::new(1.2, theme.border),
            egui::StrokeKind::Outside,
        );
        p.text(
            Pos2::new(r.center().x, r.top() + 4.0),
            Align2::CENTER_TOP,
            &nd.label,
            FontId::proportional(12.5),
            theme.heading,
        );
        for (mi, m) in nd.members.iter().enumerate() {
            p.text(
                Pos2::new(r.left() + 12.0, r.top() + 24.0 + mi as f32 * 16.0),
                Align2::LEFT_CENTER,
                m,
                FontId::monospace(11.0),
                theme.text.gamma_multiply(0.85),
            );
        }
    }
    resp
}

fn paint_arrow_head(
    p: &egui::Painter,
    tip: Pos2,
    dir: (f32, f32),
    color: Color32,
    head: ArrowHead,
) {
    if head == ArrowHead::None {
        return;
    }
    let (ux, uy) = dir;
    let ang = uy.atan2(ux);
    let stroke = Stroke::new(1.3, color);
    match head {
        ArrowHead::None => {}
        ArrowHead::Triangle => {
            for da in [-0.5_f32, 0.5] {
                let a = ang + std::f32::consts::PI + da;
                p.line_segment(
                    [tip, Pos2::new(tip.x + a.cos() * 8.0, tip.y + a.sin() * 8.0)],
                    stroke,
                );
            }
        }
        ArrowHead::HollowTriangle => {
            let back = ang + std::f32::consts::PI;
            let pts = vec![
                tip,
                Pos2::new(
                    tip.x + (back + 0.45).cos() * 11.0,
                    tip.y + (back + 0.45).sin() * 11.0,
                ),
                Pos2::new(
                    tip.x + (back - 0.45).cos() * 11.0,
                    tip.y + (back - 0.45).sin() * 11.0,
                ),
            ];
            let fill = if head == ArrowHead::HollowTriangle {
                Color32::TRANSPARENT
            } else {
                color
            };
            let _ = fill;
            p.add(egui::Shape::convex_polygon(pts, fill, stroke));
        }
        ArrowHead::FilledDiamond | ArrowHead::HollowDiamond => {
            let back = ang + std::f32::consts::PI;
            let mid = Pos2::new(tip.x + ux * 7.0, tip.y + uy * 7.0);
            let pts = vec![
                tip,
                Pos2::new(
                    mid.x + (back + 0.6).cos() * 7.0,
                    mid.y + (back + 0.6).sin() * 7.0,
                ),
                Pos2::new(tip.x + ux * 14.0, tip.y + uy * 14.0),
                Pos2::new(
                    mid.x + (back - 0.6).cos() * 7.0,
                    mid.y + (back - 0.6).sin() * 7.0,
                ),
            ];
            let fill = if head == ArrowHead::FilledDiamond {
                color
            } else {
                Color32::TRANSPARENT
            };
            p.add(egui::Shape::convex_polygon(pts, fill, stroke));
        }
    }
}

/// 解析 mermaid 类图：`class X { 成员 }` 与 `A <|-- B : 标签` 关系。
fn parse_class(code: &str) -> (Vec<BoxNode>, Vec<BoxEdge>) {
    let mut nodes: Vec<BoxNode> = Vec::new();
    let mut edges: Vec<BoxEdge> = Vec::new();
    let mut cur_members: Option<(String, Vec<String>)> = None;
    let ensure = |nodes: &mut Vec<BoxNode>, id: &str| -> usize {
        if let Some(i) = nodes.iter().position(|n| n.id == id) {
            return i;
        }
        nodes.push(BoxNode {
            id: id.to_string(),
            label: id.to_string(),
            members: Vec::new(),
        });
        nodes.len() - 1
    };
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty()
            || l.starts_with("%%")
            || l.starts_with("classDiagram")
            || l.starts_with("note ")
        {
            continue;
        }
        if l == "}" {
            if let Some((id, members)) = cur_members.take() {
                if let Some(i) = nodes.iter().position(|n| n.id == id) {
                    nodes[i].members = members;
                }
            }
            continue;
        }
        if let Some(rest) = l
            .strip_prefix("class ")
            .or_else(|| l.strip_prefix("interface "))
        {
            // `class X` 或 `class X { ... }`（单行成员用 ~ 分隔的少见，忽略）
            let name = rest.trim().trim_end_matches('{').trim();
            if name.is_empty() {
                continue;
            }
            ensure(&mut nodes, name);
            if rest.contains('{') {
                cur_members = Some((name.to_string(), Vec::new()));
            }
            continue;
        }
        if l.contains('{') {
            let name = l.trim_end_matches('{').trim();
            if !name.is_empty() {
                ensure(&mut nodes, name);
                cur_members = Some((name.to_string(), Vec::new()));
            }
            continue;
        }
        if cur_members.is_some() {
            if let Some((_, members)) = cur_members.as_mut() {
                members.push(l.to_string());
            }
            continue;
        }
        // 关系行：A <|-- B / A *-- B / A o-- B / A ..> B / A --> B
        const RELS: [(&str, ArrowHead, ArrowHead, EdgeStyle); 6] = [
            (
                "<|--",
                ArrowHead::HollowTriangle,
                ArrowHead::None,
                EdgeStyle::Solid,
            ),
            (
                "*--",
                ArrowHead::FilledDiamond,
                ArrowHead::None,
                EdgeStyle::Solid,
            ),
            (
                "o--",
                ArrowHead::HollowDiamond,
                ArrowHead::None,
                EdgeStyle::Solid,
            ),
            (
                "..>",
                ArrowHead::None,
                ArrowHead::Triangle,
                EdgeStyle::Dashed,
            ),
            (
                "<..",
                ArrowHead::Triangle,
                ArrowHead::None,
                EdgeStyle::Dashed,
            ),
            (
                "-->",
                ArrowHead::None,
                ArrowHead::Triangle,
                EdgeStyle::Solid,
            ),
        ];
        let mut matched = false;
        for (sym, ha, hb, st) in RELS {
            if let Some(i) = l.find(sym) {
                let a = l[..i].trim();
                let mut b = l[i + sym.len()..].trim();
                let mut label = String::new();
                if let Some(ci) = b.find(':') {
                    label = b[ci + 1..].trim().to_string();
                    b = b[..ci].trim();
                }
                if a.is_empty() || b.is_empty() {
                    break;
                }
                ensure(&mut nodes, a);
                ensure(&mut nodes, b);
                edges.push(BoxEdge {
                    a: a.to_string(),
                    b: b.to_string(),
                    label,
                    style: st,
                    head_a: ha,
                    head_b: hb,
                });
                matched = true;
                break;
            }
        }
        let _ = matched;
    }
    (nodes, edges)
}

/// 解析 mermaid ER 图：`USER ||--o{ ORDER : 拥有` 与实体属性块。
fn parse_er(code: &str) -> (Vec<BoxNode>, Vec<BoxEdge>) {
    let mut nodes: Vec<BoxNode> = Vec::new();
    let mut edges: Vec<BoxEdge> = Vec::new();
    let mut cur_members: Option<(String, Vec<String>)> = None;
    let mut ensure = |nodes: &mut Vec<BoxNode>, id: &str| -> usize {
        if let Some(i) = nodes.iter().position(|n| n.id == id) {
            return i;
        }
        nodes.push(BoxNode {
            id: id.to_string(),
            label: id.to_string(),
            members: Vec::new(),
        });
        nodes.len() - 1
    };
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.starts_with("erDiagram") {
            continue;
        }
        if l == "}" {
            if let Some((id, members)) = cur_members.take() {
                if let Some(i) = nodes.iter().position(|n| n.id == id) {
                    nodes[i].members = members;
                }
            }
            continue;
        }
        // 关系行必须先于实体块判断：`USER ||--o{ ORDER : 下单` 里也有 '{'，
        // 会被误当成实体块的开始，整行变成一个节点。
        if l.contains("--") {
            parse_er_relation(l, &mut nodes, &mut edges, &mut ensure);
            continue;
        }
        if l.contains('{') {
            let name = l.trim_end_matches('{').trim();
            if !name.is_empty() {
                ensure(&mut nodes, name);
                cur_members = Some((name.to_string(), Vec::new()));
            }
            continue;
        }
        if cur_members.is_some() {
            if let Some((_, members)) = cur_members.as_mut() {
                members.push(l.to_string());
            }
            continue;
        }
        // （关系解析已上移到 parse_er_relation）
    }
    (nodes, edges)
}

/// ER 关系行：`A ||--o{ B : 标签`。实体名 = "--" 两侧剥掉基数记号
/// （记号字符只有 | o } { 这几种）之后的单词。
fn parse_er_relation(
    l: &str,
    nodes: &mut Vec<BoxNode>,
    edges: &mut Vec<BoxEdge>,
    ensure: &mut dyn FnMut(&mut Vec<BoxNode>, &str) -> usize,
) {
    let Some(i) = l.find("--") else { return };
    let a_part = l[..i].trim();
    let after = &l[i..];
    if after.len() < 6 {
        return;
    }
    let dash_pos = after.find("--").unwrap_or(0);
    let left_mark = &after[..dash_pos];
    let right_raw = &after[dash_pos + 2..];
    let (b, label) = match right_raw.find(':') {
        Some(ci) => (
            right_raw[..ci].trim(),
            right_raw[ci + 1..].trim().to_string(),
        ),
        None => (right_raw.trim(), String::new()),
    };
    let strip_mark = |s: &str| -> String {
        s.trim()
            .trim_start_matches(|c: char| " |o}{".contains(c))
            .trim_end_matches(|c: char| " |o}{".contains(c))
            .to_string()
    };
    let a = strip_mark(a_part);
    let b = strip_mark(b);
    if a.is_empty() || b.is_empty() {
        return;
    }
    ensure(nodes, &a);
    ensure(nodes, &b);
    let many = left_mark.contains('o') || left_mark.contains('{');
    edges.push(BoxEdge {
        a,
        b,
        label: if label.is_empty() {
            String::new()
        } else {
            format!("{label} (1 : N)")
        },
        style: EdgeStyle::Solid,
        head_a: ArrowHead::None,
        head_b: ArrowHead::None,
    });
    let _ = many;
}

/// 解析 mermaid 甘特图：section 分组 + 任务（日期/after 链 + 时长）。
/// 返回 (标题, [(分组或任务, 起始天, 时长天, 是否分组头, 颜色序号)])。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
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

fn draw_gantt(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
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

/// 解析 mermaid 旅程图：section 分组 + `任务: 分数: 演员`。
fn draw_journey(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let mut title = String::new();
    struct JSection {
        name: String,
        tasks: Vec<(String, i64, String)>,
    }
    let mut sections: Vec<JSection> = Vec::new();
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") {
            continue;
        }
        let lower = l.to_lowercase();
        if lower == "journey" {
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
                sections.push(JSection {
                    name: sec.trim().to_string(),
                    tasks: Vec::new(),
                });
                continue;
            }
        }
        let parts: Vec<&str> = l.split(':').map(|s| s.trim()).collect();
        if parts.len() >= 2 && !parts[0].is_empty() {
            let score = parts
                .get(1)
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(3);
            let actors = parts.get(2).unwrap_or(&"").to_string();
            if let Some(sec) = sections.last_mut() {
                sec.tasks
                    .push((parts[0].to_string(), score.clamp(1, 5), actors));
            }
        }
    }
    let total_tasks: usize = sections.iter().map(|s| s.tasks.len()).sum();
    if total_tasks == 0 {
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
    let title_h = if title.is_empty() { 0.0 } else { 22.0 };
    let sec_h = 96.0;
    let w = ui.available_width().max(320.0).min(900.0);
    let h = title_h + sections.len() as f32 * sec_h + 10.0;
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
    let mut y = rect.min.y + title_h + 4.0;
    for (si, sec) in sections.iter().enumerate() {
        p.text(
            Pos2::new(rect.min.x + 12.0, y + 8.0),
            Align2::LEFT_CENTER,
            &sec.name,
            FontId::proportional(12.0),
            theme.accent,
        );
        let n = sec.tasks.len().max(1);
        let seg = (w - 40.0) / n as f32;
        let line_y = y + 34.0;
        if sec.tasks.len() > 1 {
            p.line_segment(
                [
                    Pos2::new(rect.min.x + 20.0 + seg / 2.0, line_y),
                    Pos2::new(rect.min.x + 20.0 + seg * (n as f32 - 0.5), line_y),
                ],
                Stroke::new(1.5, theme.border),
            );
        }
        for (ti, (name, score, actors)) in sec.tasks.iter().enumerate() {
            let cx = rect.min.x + 20.0 + seg * (ti as f32 + 0.5);
            let radius = 7.0 + *score as f32 * 1.2;
            let fill = palette_color(theme, si + ti);
            p.circle_filled(Pos2::new(cx, line_y), radius, fill);
            p.circle_stroke(
                Pos2::new(cx, line_y),
                radius,
                Stroke::new(1.2, theme.border),
            );
            p.text(
                Pos2::new(cx, line_y),
                Align2::CENTER_CENTER,
                &score.to_string(),
                FontId::proportional(10.0),
                theme.text,
            );
            p.text(
                Pos2::new(cx, line_y + radius + 6.0),
                Align2::CENTER_TOP,
                name,
                FontId::proportional(11.0),
                theme.text,
            );
            if !actors.is_empty() {
                p.text(
                    Pos2::new(cx, line_y + radius + 22.0),
                    Align2::CENTER_TOP,
                    actors,
                    FontId::proportional(10.0),
                    theme.dim,
                );
            }
        }
        y += sec_h;
    }
    resp
}

/// 解析并绘制 mermaid gitGraph：commit/branch/checkout/merge 状态机。
fn draw_git(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    struct Lane {
        name: String,
        dots: Vec<(usize, usize)>, // (序号, 合并来源 lane 下标；usize::MAX = 普通提交)
    }
    let mut lanes: Vec<Lane> = vec![Lane {
        name: "main".to_string(),
        dots: Vec::new(),
    }];
    let mut cur = 0usize;
    let mut seq = 0usize;
    for raw in code.lines() {
        let l = raw.trim();
        let lower = l.to_lowercase();
        if l.is_empty() || lower == "gitgraph" || l.starts_with("%%") {
            continue;
        }
        if let Some(rest) = lower.strip_prefix("branch ") {
            let name = rest.trim();
            if !lanes.iter().any(|ln| ln.name == name) {
                lanes.push(Lane {
                    name: name.to_string(),
                    dots: Vec::new(),
                });
                cur = lanes.len() - 1;
            } else if let Some(i) = lanes.iter().position(|ln| ln.name == name) {
                cur = i;
            }
            continue;
        }
        if let Some(rest) = lower.strip_prefix("checkout ") {
            if let Some(i) = lanes.iter().position(|ln| ln.name == rest.trim()) {
                cur = i;
            }
            continue;
        }
        if lower.starts_with("merge ") {
            let src = lower[6..].trim();
            if let Some(i) = lanes.iter().position(|ln| ln.name == src) {
                seq += 1;
                lanes[cur].dots.push((seq, i));
            }
            continue;
        }
        if lower.starts_with("commit") {
            seq += 1;
            lanes[cur].dots.push((seq, usize::MAX));
        }
    }
    let total = seq;
    if total == 0 {
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
    let lane_h = 44.0;
    let pad_l = 64.0;
    let w = ui.available_width().max(320.0).min(900.0);
    let h = lanes.len() as f32 * lane_h + 30.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let p = ui.painter();
    let seg = (w - pad_l - 20.0) / (total as f32 + 0.5);
    let x_of = |s: usize| rect.min.x + pad_l + s as f32 * seg;
    let y_of = |i: usize| rect.min.y + 22.0 + i as f32 * lane_h;
    // 分支名标签
    for (i, lane) in lanes.iter().enumerate() {
        p.text(
            Pos2::new(rect.min.x + 8.0, y_of(i)),
            Align2::LEFT_CENTER,
            &lane.name,
            FontId::monospace(11.0),
            palette_color(theme, i),
        );
    }
    // 车道横线 + 提交点
    for (i, lane) in lanes.iter().enumerate() {
        if let (Some(first), Some(last)) = (lane.dots.first(), lane.dots.last()) {
            p.line_segment(
                [
                    Pos2::new(x_of(first.0), y_of(i)),
                    Pos2::new(x_of(last.0), y_of(i)),
                ],
                Stroke::new(2.0, palette_color(theme, i).gamma_multiply(0.9)),
            );
        }
        for (s, merge_from) in &lane.dots {
            let c = Pos2::new(x_of(*s), y_of(i));
            if *merge_from != usize::MAX {
                // 合并提交：从来源车道画一条折线过来
                let src_y = y_of(*merge_from);
                p.line_segment(
                    [Pos2::new(x_of(*s), src_y), Pos2::new(c.x, c.y)],
                    Stroke::new(1.5, theme.dim),
                );
                p.circle_filled(c, 6.0, palette_color(theme, *merge_from));
            } else {
                p.circle_filled(c, 6.0, palette_color(theme, i));
            }
            p.circle_stroke(c, 6.0, Stroke::new(1.4, theme.text));
        }
    }
    resp
}

fn draw_pie(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (title, data) = parse_pie(code);
    if data.is_empty() {
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
    let total: f64 = data.iter().map(|(_, v)| v.abs().max(0.0)).sum();
    let total = if total <= 0.0 { 1.0 } else { total };
    let r0 = 62.0_f32;
    let legend_w = 160.0_f32;
    let title_h = if title.is_empty() { 0.0 } else { 22.0 };
    let avail = ui.available_width().max(200.0);
    let w = (r0 * 2.0 + legend_w + 48.0).min(avail).max(180.0);
    let h = title_h + r0 * 2.0 + 16.0;
    // Sense::click 让图形本身可点击：点一下就能重新进入编辑态改代码，
    // 不然插入流程图后就再也改不了了（此前是 Sense::hover，点击永远无效）。
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let p = ui.painter();
    let cx = rect.min.x + 16.0 + r0;
    let cy = rect.min.y + title_h + 8.0 + r0;
    if !title.is_empty() {
        p.text(
            Pos2::new(rect.min.x + 16.0, rect.min.y + 4.0),
            Align2::LEFT_TOP,
            &title,
            FontId::new(12.0, FontFamily::Proportional),
            theme.text,
        );
    }
    // 从正上方开始、顺时针按数值占比分扇区；图例在右侧列出名称和百分比。
    let mut a0 = -std::f64::consts::FRAC_PI_2;
    for (k, (label, val)) in data.iter().enumerate() {
        let frac = (val.abs().max(0.0) / total) as f32;
        let a1 = a0 + (frac as f64) * std::f64::consts::TAU;
        let color = PIE_PALETTE[k % PIE_PALETTE.len()];
        p.add(pie_sector(Pos2::new(cx, cy), r0, a0, a1, color));
        let ly = cy + r0 - 16.0 - (k.min(7) as f32) * 22.0;
        let swatch = Rect::from_center_size(Pos2::new(cx + r0 + 24.0, ly), Vec2::new(12.0, 12.0));
        p.rect_filled(swatch, 3.0, color);
        p.text(
            Pos2::new(swatch.right() + 6.0, ly),
            Align2::LEFT_CENTER,
            format!("{} {:.0}%", label, frac * 100.0),
            FontId::new(12.0, FontFamily::Proportional),
            theme.text,
        );
        a0 = a1;
    }
    resp
}

pub fn render_block(
    ui: &mut egui::Ui,
    idx: usize,
    block: &Block,
    env: &mut RenderEnv<'_>,
    dimmed: bool,
) -> egui::Response {
    let theme = env.theme;
    let color = if dimmed { theme.dim } else { theme.text };
    let inner = ui.scope(|ui| {
        if dimmed {
            ui.set_opacity(0.5);
        }
        match block {
            Block::Heading { level, content, .. } => {
                // 标题上下留白：尽量紧凑，且与编辑态（标题无额外留白）接近，
                // 否则进出编辑时整块会跳动。之前 8/6px 让 H1–H3 行距明显偏大。
                ui.add_space(if *level <= 2 { 3.0 } else { 2.0 });
                let r = text_response(
                    ui,
                    content,
                    theme,
                    theme.heading_size(*level),
                    theme.heading,
                    true,
                    env,
                    idx,
                );
                ui.add_space(3.0);
                r
            }
            Block::Paragraph { content, .. } => {
                if content.len() == 1 {
                    if let Inline::Image { alt, url, width } = &content[0] {
                        let (resp, resized) = image_preview(
                            ui,
                            alt,
                            url,
                            theme,
                            env.ctx,
                            env.images,
                            env.base_dir,
                            *width,
                            (idx, 0),
                        );
                        if let Some(w) = resized {
                            env.actions.push(Action::ResizeImage(idx, 0, w));
                        }
                        return resp;
                    }
                }
                // 连续多行图片（中间没有空行）会被解析成同一个段落：
                // 逐张渲染成图片预览，否则会退化成一行行「[图片: …]」占位文字。
                if content.iter().any(|i| matches!(i, Inline::Image { .. }))
                    && content
                        .iter()
                        .all(|i| matches!(i, Inline::Image { .. } | Inline::Break))
                {
                    let mut merged: Option<egui::Response> = None;
                    let mut no = 0usize;
                    for inl in content {
                        if let Inline::Image { alt, url, width } = inl {
                            let (r, resized) = image_preview(
                                ui,
                                alt,
                                url,
                                theme,
                                env.ctx,
                                env.images,
                                env.base_dir,
                                *width,
                                (idx, no),
                            );
                            if let Some(w) = resized {
                                env.actions.push(Action::ResizeImage(idx, no, w));
                            }
                            no += 1;
                            merged = Some(match merged.take() {
                                Some(prev) => prev.union(r),
                                None => r,
                            });
                        }
                    }
                    return merged.expect("段落里至少有一张图片");
                }
                text_response(ui, content, theme, theme.body, color, false, env, idx)
            }
            Block::Quote { level, content, .. } => {
                ui.horizontal(|ui| {
                    ui.add_space((*level).min(4) as f32 * 8.0);
                    // 竖线不再预先占一个固定高度的矩形：先给正文留出「线宽 + 间距」，
                    // 拿到文字实际高度后再补画竖线，软换行后的每一行都能被竖线贯穿。
                    ui.add_space(11.0);
                    let resp =
                        text_response(ui, content, theme, theme.body, theme.quote, true, env, idx);
                    let r = resp.rect;
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            Pos2::new(r.left() - 11.0, r.top()),
                            Pos2::new(r.left() - 8.0, r.bottom()),
                        ),
                        1.0,
                        theme.accent,
                    );
                    resp
                })
                .inner
            }
            Block::ListItem { item, .. } => {
                let indent = (item.indent.min(12) as f32) * 8.0;
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    // 标记与正文更紧凑：默认 item_spacing 是 8px，列表看着离得太远。
                    ui.spacing_mut().item_spacing.x = 5.0;
                    match item.checked {
                        Some(checked) => {
                            // 任务列表复选框放大一档（默认 14px 偏小），并加深方框描边，
                            // 否则浅色主题下白底白框几乎看不见。
                            ui.spacing_mut().icon_width = 19.0;
                            ui.spacing_mut().icon_width_inner = 11.0;
                            ui.spacing_mut().icon_spacing = 3.0;
                            let v = &mut ui.visuals_mut().widgets;
                            v.inactive.bg_stroke = egui::Stroke::new(1.0, theme.dim);
                            v.hovered.bg_stroke = egui::Stroke::new(1.0, theme.accent);
                            v.active.bg_stroke = egui::Stroke::new(1.0, theme.accent);
                            let mut c = checked;
                            let cb = ui.checkbox(&mut c, "");
                            if cb.clicked() {
                                env.actions.push(Action::ToggleTask(idx));
                            }
                        }
                        None => {
                            let level = list_level(item.indent);
                            if item.marker.ends_with('.') || item.marker.ends_with(')') {
                                // 有序列表：顶层数字，子层依次用 a. / i.（与父列表区分）。
                                let mut f = theme.fmt(theme.body);
                                f.color = theme.dim;
                                let mut job = LayoutJob::default();
                                job.append(&ordered_list_marker(&item.marker, level), 0.0, f);
                                ui.label(job);
                            } else {
                                // 无序列表：标记形状随层级变化（实心圆 → 空心圆 → 小方块）。
                                let d = (theme.body * 0.6).clamp(7.0, 14.0);
                                let color = theme.dim.lerp_to_gamma(theme.text, 0.55);
                                draw_list_bullet(ui, level, color, d);
                            }
                        }
                    }
                    text_response(ui, &item.content, theme, theme.body, color, false, env, idx)
                })
                .inner
            }
            Block::Code {
                lang, code, quote, ..
            } => {
                let folded = env.folded.contains(&idx);
                // 折叠动画：1=完全展开、0=完全折叠，中间平滑过渡。
                let openness = env.ctx.animate_value_with_time(
                    egui::Id::new(("code_fold", idx)),
                    if folded { 0.0 } else { 1.0 },
                    0.18,
                );
                let lines = code.lines().count();
                let mut copied = false;
                let mut new_lang: Option<String> = None;
                // 引用内的代码块：左侧留出缩进 + 竖线位置，代码面板整体右移，
                // 竖线按面板实际高度画，和引用文字连成一条。
                let hr = ui.horizontal(|ui| {
                    if *quote > 0 {
                        ui.add_space((*quote).min(4) as f32 * 8.0 + 11.0);
                    }
                    // 外层横向布局会传染给面板内容：显式回到纵向，否则功能栏
                    // 和代码会被并排放置、代码列被挤成一条很窄的竖条。
                    let r = ui
                        .vertical(|ui| {
                            panel_frame(theme).show(ui, |ui| {
                                // 功能栏：左语言 + 右操作按钮（code_header 背景色块）
                                code_header_frame(theme).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if let Some(l) =
                                            language_combo(ui, ("code_lang", idx), lang)
                                        {
                                            new_lang = Some(l);
                                        }
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                // 最右：折叠/展开；其次删除（弹确认窗）；再复制
                                                let fold_label =
                                                    if folded { "展开" } else { "折叠" };
                                                if ui
                                                    .button(
                                                        egui::RichText::new(fold_label)
                                                            .size(theme.body * 0.85),
                                                    )
                                                    .on_hover_cursor(egui::CursorIcon::Default)
                                                    .clicked()
                                                {
                                                    env.actions.push(Action::FoldToggle(idx));
                                                }
                                                if !folded {
                                                    if ui
                                                        .button(
                                                            egui::RichText::new("删除")
                                                                .size(theme.body * 0.85),
                                                        )
                                                        .on_hover_cursor(egui::CursorIcon::Default)
                                                        .clicked()
                                                    {
                                                        env.actions
                                                            .push(Action::AskDeleteBlock(idx));
                                                    }
                                                    if ui
                                                        .button(
                                                            egui::RichText::new("复制")
                                                                .size(theme.body * 0.85),
                                                        )
                                                        .on_hover_cursor(egui::CursorIcon::Default)
                                                        .clicked()
                                                    {
                                                        copied = true;
                                                    }
                                                }
                                            },
                                        );
                                    });
                                });
                                ui.add_space(6.0);
                                code_divider(ui, theme);
                                ui.add_space(6.0);
                                // 代码区：完全折叠只留提示；过渡中代码淡出、提示淡入。
                                if openness <= 0.0 {
                                    ui.label(
                                        egui::RichText::new(format!("已折叠 {lines} 行代码"))
                                            .size(theme.body * 0.9)
                                            .italics()
                                            .color(theme.dim),
                                    )
                                } else {
                                    let code_resp = ui
                                        .scope(|ui| {
                                            ui.set_opacity(openness);
                                            wrapped_label(
                                                ui,
                                                code_job(code, lang, theme),
                                                env.max_width,
                                                Sense::hover(),
                                            )
                                        })
                                        .inner;
                                    if openness < 1.0 {
                                        ui.scope(|ui| {
                                            ui.set_opacity(1.0 - openness);
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "已折叠 {lines} 行代码"
                                                ))
                                                .size(theme.body * 0.9)
                                                .italics()
                                                .color(theme.dim),
                                            );
                                        });
                                    }
                                    code_resp
                                }
                            })
                        })
                        .inner;
                    if *quote > 0 {
                        let rect = r.response.rect;
                        ui.painter().rect_filled(
                            Rect::from_min_max(
                                Pos2::new(rect.left() - 11.0, rect.top()),
                                Pos2::new(rect.left() - 8.0, rect.bottom()),
                            ),
                            1.0,
                            theme.accent,
                        );
                    }
                    r
                });
                let r = hr.inner;
                if copied {
                    env.ctx.copy_text(code.clone());
                    env.actions.push(Action::Notify("复制成功".to_string()));
                }
                if let Some(l) = new_lang {
                    env.actions.push(Action::SetLang(idx, l));
                }
                // 只用代码文本区域做块定位，避免覆盖层挡住右上角的复制按钮。
                // 矩形扩到整个面板（含功能栏）：跨块拖选时整块叠一层选区色，
                // 不会只给代码文字画一个大小不一的蓝框。
                let content = r.inner.interact(Sense::click());
                content.union(r.response)
            }
            Block::Table {
                head,
                rows,
                align,
                source,
                ..
            } => {
                // 表格随窗口浮动：列宽平分可用宽度（扣除内边距与列间距），
                // 窗口变宽表就变宽；窄窗口下自动收窄并让单元格内文字换行。
                // 列宽公式与编辑态（editor.rs 表格编辑分支）完全一致，
                // 保证点击进入编辑时列宽不变、表格不跳。
                let cols = head.len().max(1);
                let avail = (ui.available_width() - 12.0).max(120.0);
                let gap = ui.spacing().item_spacing.x * cols.saturating_sub(1) as f32;
                let col_w = ((avail - gap) / cols as f32).max(56.0);
                // 记录每个单元格的矩形，用于画「单线」分隔（不是每格一个框）
                let mut row_rects: Vec<Vec<egui::Rect>> = Vec::new();
                let mut copied = false;
                let mut op: Option<crate::editor::TableOp> = None;
                let cell_key = egui::Id::new(("table_cell_ref", idx));
                // 功能区背景：浅色主题用指定的 #F2F2F4，深色主题沿用 panel
                let tb_fill = if theme.kind == crate::theme::ThemeKind::Light {
                    Color32::from_rgb(0xF2, 0xF2, 0xF4)
                } else {
                    theme.panel
                };
                let r = egui::Frame::NONE
                    .fill(theme.bg)
                    .stroke(Stroke::new(1.0, theme.table_line))
                    .corner_radius(4.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.set_min_width(col_w * cols as f32 + gap);
                        // ---- 第一区：功能区（常驻，不随悬停出现/消失）----
                        // 行/列操作以最后悬停的单元格为基准（Grid 渲染后写入记忆）；
                        // 复制与删除表格固定在最右侧右上角。
                        egui::Frame::NONE
                            .fill(tb_fill)
                            .corner_radius(3.0)
                            .inner_margin(4.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.style_mut().spacing.item_spacing.x = 4.0;
                                    let (hr, hc) = ui
                                        .ctx()
                                        .memory(|m| m.data.get_temp::<(usize, usize)>(cell_key))
                                        .unwrap_or((0, 0));
                                    let btn = |ui: &mut egui::Ui, label: &str, active: bool| {
                                        let txt =
                                            egui::RichText::new(label).size(theme.body * 0.85);
                                        let txt =
                                            if active { txt.color(theme.accent) } else { txt };
                                        ui.button(txt).clicked()
                                    };
                                    let all = |a: crate::ast::Align| align.iter().all(|x| *x == a);
                                    use crate::ast::Align;
                                    use crate::editor::TableOp;
                                    if btn(ui, "+ 行", false) {
                                        op = Some(TableOp::InsertRow(hr));
                                    }
                                    if btn(ui, "− 行", false) {
                                        op = Some(TableOp::DeleteRow(hr));
                                    }
                                    if btn(ui, "+ 列", false) {
                                        op = Some(TableOp::InsertCol(hc));
                                    }
                                    if btn(ui, "− 列", false) {
                                        op = Some(TableOp::DeleteCol(hc));
                                    }
                                    ui.separator();
                                    if btn(ui, "左对齐", all(Align::Left)) {
                                        op = Some(TableOp::SetAlign(Align::Left));
                                    }
                                    if btn(ui, "居中", all(Align::Center)) {
                                        op = Some(TableOp::SetAlign(Align::Center));
                                    }
                                    if btn(ui, "右对齐", all(Align::Right)) {
                                        op = Some(TableOp::SetAlign(Align::Right));
                                    }
                                    // 最右侧右上角：删除表格（最右）+ 复制
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .button(
                                                    egui::RichText::new("删除表格")
                                                        .size(theme.body * 0.85),
                                                )
                                                .clicked()
                                            {
                                                op = Some(TableOp::Delete);
                                            }
                                            if ui
                                                .button(
                                                    egui::RichText::new("复制")
                                                        .size(theme.body * 0.85),
                                                )
                                                .clicked()
                                            {
                                                copied = true;
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(4.0);
                        // ---- 第二区：表头行；第三区：数据行（文字按列对齐）----
                        egui::Grid::new(("md_table", idx))
                            .min_col_width(col_w)
                            .max_col_width(col_w)
                            .show(ui, |ui| {
                                // 表头行：最深一档底色，文字按列对齐
                                let mut rects = Vec::new();
                                for (ci, c) in head.iter().enumerate() {
                                    let fr = egui::Frame::NONE
                                        .fill(theme.table_head)
                                        .inner_margin(3.0)
                                        .show(ui, |ui| {
                                            ui.set_min_width(col_w - 6.0);
                                            align_cell_label(
                                                ui,
                                                build_layout(
                                                    c,
                                                    theme,
                                                    theme.body * 0.95,
                                                    theme.heading,
                                                    true,
                                                ),
                                                col_w - 6.0,
                                                align
                                                    .get(ci)
                                                    .copied()
                                                    .unwrap_or(crate::ast::Align::Left),
                                            );
                                        });
                                    rects.push(fr.response.rect);
                                }
                                row_rects.push(rects);
                                ui.end_row();
                                // 数据行：正文底色（表头一档颜色就够分了），文字按列对齐
                                for row in rows {
                                    let mut rects = Vec::new();
                                    for (ci, c) in row.iter().enumerate() {
                                        let fr = egui::Frame::NONE
                                            .fill(theme.bg)
                                            .inner_margin(3.0)
                                            .show(ui, |ui| {
                                                ui.set_min_width(col_w - 6.0);
                                                align_cell_label(
                                                    ui,
                                                    build_layout(
                                                        c,
                                                        theme,
                                                        theme.body * 0.95,
                                                        color,
                                                        false,
                                                    ),
                                                    col_w - 6.0,
                                                    align
                                                        .get(ci)
                                                        .copied()
                                                        .unwrap_or(crate::ast::Align::Left),
                                                );
                                            });
                                        rects.push(fr.response.rect);
                                    }
                                    row_rects.push(rects);
                                    ui.end_row();
                                }
                            });
                    });
                // 分隔线：行与行、列与列之间只画一条线（不是每个单元格一个盒子）。
                // 注意 shrink2 是「每边减去全额」：frame 内边距 6px，这里必须用 6
                // 才是真正的内容区；之前误用 12 导致所有分隔线整体内移 6px，
                // 列分隔线切进第二列的文字（文字与单元格"融合"的元凶）。
                let content = r.response.rect.shrink2(egui::vec2(6.0, 6.0));
                let stroke = Stroke::new(1.0, theme.table_line);
                let p = ui.painter();
                // 标题行底色铺满整行：单元格之间（item_spacing）和两端露出的缝隙
                // 用标题色补齐，只盖缝隙不盖文字。
                if let Some(row) = row_rects.first()
                    && row.len() == cols
                {
                    let top = row.iter().map(|x| x.top()).fold(f32::MAX, f32::min);
                    let bottom = row.iter().map(|x| x.bottom()).fold(f32::MIN, f32::max);
                    for k in 1..cols {
                        let left = row[k - 1].right();
                        let right = row[k].left();
                        if right > left {
                            p.rect_filled(
                                Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, bottom)),
                                0.0,
                                theme.table_head,
                            );
                        }
                    }
                    // 行两端的缝隙：对齐分隔线用的 content 宽度
                    if content.left() < row[0].left() {
                        p.rect_filled(
                            Rect::from_min_max(
                                Pos2::new(content.left(), top),
                                Pos2::new(row[0].left(), bottom),
                            ),
                            0.0,
                            theme.table_head,
                        );
                    }
                    if content.right() > row[cols - 1].right() {
                        p.rect_filled(
                            Rect::from_min_max(
                                Pos2::new(row[cols - 1].right(), top),
                                Pos2::new(content.right(), bottom),
                            ),
                            0.0,
                            theme.table_head,
                        );
                    }
                }
                for i in 1..row_rects.len() {
                    let next_top = row_rects[i].first().map(|x| x.top());
                    let prev_bottom = row_rects[i - 1]
                        .iter()
                        .map(|x| x.bottom())
                        .fold(f32::MIN, f32::max);
                    if let Some(t) = next_top {
                        let y = (t + prev_bottom) / 2.0;
                        p.line_segment(
                            [Pos2::new(content.left(), y), Pos2::new(content.right(), y)],
                            stroke,
                        );
                    }
                }
                // 列分隔线的纵向范围只覆盖表格网格（第一行顶到最后一行底）：
                // 内容区顶部现在是右上角的复制按钮行，不能让线穿过去。
                let grid_top = row_rects
                    .first()
                    .and_then(|r| r.first())
                    .map(|x| x.top())
                    .unwrap_or(content.top());
                let grid_bottom = row_rects
                    .last()
                    .and_then(|r| r.first())
                    .map(|x| x.bottom())
                    .unwrap_or(content.bottom());
                // 列分隔线与编辑态一致：取相邻单元格实际矩形间隙的中点，
                // 不能按 col_w+gap 估算——gap 是所有列间距之和，列数多时
                // 分隔线会逐列右移，压到左对齐的文字上（文字与边框重合）。
                if let Some(first) = row_rects.first()
                    && first.len() == cols
                {
                    for k in 1..cols {
                        let x = (first[k - 1].right() + first[k].left()) / 2.0;
                        p.line_segment([Pos2::new(x, grid_top), Pos2::new(x, grid_bottom)], stroke);
                    }
                }
                // 点击了哪个单元格：进入编辑态时直接聚焦那一格，光标落在
                // 点击位置附近（点在文字右半边 → 光标到末尾），不再固定跳到第一格。
                if r.response.clicked()
                    && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                {
                    for (ri, rects) in row_rects.iter().enumerate() {
                        if let Some(ci) = rects.iter().position(|x| x.contains(p)) {
                            let end = p.x > rects[ci].center().x;
                            env.actions.push(Action::TableCellClick(idx, ri, ci, end));
                            break;
                        }
                    }
                }
                // 悬停单元格时更新基准 (hr,hc)：功能区的行/列操作按最近悬停定位
                if let Some(hp) = ui.input(|i| i.pointer.hover_pos())
                    && let Some(hit) = hit_cell(&row_rects, hp)
                {
                    ui.ctx().memory_mut(|m| m.data.insert_temp(cell_key, hit));
                }
                if let Some(op) = op {
                    env.actions.push(Action::TableOp(idx, op));
                }
                if copied {
                    env.ctx.copy_text(source.clone());
                    env.actions.push(Action::Notify("复制成功".to_string()));
                }
                r.response.interact(Sense::click())
            }
            Block::Rule { .. } => {
                let w = ui.available_width();
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 18.0), Sense::click());
                ui.painter().line_segment(
                    [
                        Pos2::new(r.min.x, r.center().y),
                        Pos2::new(r.max.x, r.center().y),
                    ],
                    Stroke::new(1.0, theme.border),
                );
                resp
            }
            Block::Math { tex, .. } => {
                let r = panel_frame(theme).show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        let mut f = theme.mono_fmt();
                        f.color = theme.heading;
                        let mut job = LayoutJob::default();
                        job.append(&math_pretty(tex), 0.0, f);
                        wrapped_label(ui, job, env.max_width, Sense::hover())
                    })
                });
                r.response.interact(Sense::click())
            }
            Block::Diagram { code, .. } => {
                let folded = env.folded.contains(&idx);
                // 折叠动画：与代码块一致。
                let openness = env.ctx.animate_value_with_time(
                    egui::Id::new(("diagram_fold", idx)),
                    if folded { 0.0 } else { 1.0 },
                    0.18,
                );
                let lines = code.lines().count();
                let mut copied = false;
                let mut new_lang: Option<String> = None;
                // 图类型：按首行关键字分发到对应的自绘渲染器。
                let kind = code
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .map(|l| l.trim().to_lowercase())
                    .unwrap_or_default();
                let r = panel_frame(theme).show(ui, |ui| {
                    // 功能栏：左语言 + 右操作按钮（默认 mermaid，选其它语言即转为普通代码块）。
                    code_header_frame(theme).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if let Some(l) = language_combo(ui, ("diagram_lang", idx), "mermaid") {
                                new_lang = Some(l);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let fold_label = if folded { "展开" } else { "折叠" };
                                    if ui
                                        .button(
                                            egui::RichText::new(fold_label).size(theme.body * 0.85),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::Default)
                                        .clicked()
                                    {
                                        env.actions.push(Action::FoldToggle(idx));
                                    }
                                    if !folded {
                                        // 与代码块一致：删除按钮弹确认窗，确定才删、取消不删
                                        if ui
                                            .button(
                                                egui::RichText::new("删除").size(theme.body * 0.85),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::Default)
                                            .clicked()
                                        {
                                            env.actions.push(Action::AskDeleteBlock(idx));
                                        }
                                        if ui
                                            .button(
                                                egui::RichText::new("复制").size(theme.body * 0.85),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::Default)
                                            .clicked()
                                        {
                                            copied = true;
                                        }
                                    }
                                },
                            );
                        });
                    });
                    ui.add_space(6.0);
                    code_divider(ui, theme);
                    ui.add_space(6.0);
                    // 图表区：完全折叠只留提示；过渡中图表淡出、提示淡入。
                    if openness <= 0.0 {
                        ui.label(
                            egui::RichText::new(format!("已折叠 {lines} 行代码"))
                                .size(theme.body * 0.9)
                                .italics()
                                .color(theme.dim),
                        )
                    } else {
                        // 渲染器支持流程图/时序图/饼图等；其余类型按代码保真显示，
                        // 绝不乱画。每个分支返回图表「内容区域」的响应——用它做块定位，
                        // 不覆盖右上角的按钮行（否则点「折叠」会连带着把块激活成编辑态）。
                        let body_resp = ui
                            .scope(|ui| {
                                ui.set_opacity(openness);
                                if kind.starts_with("pie") {
                                    draw_pie(ui, code, theme)
                                } else if kind.starts_with("sequencediagram") {
                                    draw_sequence(ui, code, theme)
                                } else if kind.starts_with("flowchart") || kind.starts_with("graph")
                                {
                                    draw_flow(ui, code, theme)
                                } else if kind.starts_with("statediagram") {
                                    // 状态图结构与流程图同构：边直接复用流程图布局绘制
                                    draw_flow(ui, &transform_state_to_flow(code), theme)
                                } else if kind.starts_with("classdiagram") {
                                    let (nodes, edges) = parse_class(code);
                                    draw_boxes_edges(ui, theme, &nodes, &edges)
                                } else if kind.starts_with("erdiagram") {
                                    let (nodes, edges) = parse_er(code);
                                    draw_boxes_edges(ui, theme, &nodes, &edges)
                                } else if kind.starts_with("gantt") {
                                    draw_gantt(ui, code, theme)
                                } else if kind.starts_with("journey") {
                                    draw_journey(ui, code, theme)
                                } else if kind.starts_with("gitgraph") {
                                    draw_git(ui, code, theme)
                                } else {
                                    ui.vertical(|ui| {
                                        ui.set_min_width(env.max_width - 24.0);
                                        for line in code.lines() {
                                            ui.label(
                                                egui::RichText::new(line)
                                                    .monospace()
                                                    .small()
                                                    .color(theme.dim),
                                            );
                                        }
                                    })
                                    .response
                                }
                            })
                            .inner;
                        if openness < 1.0 {
                            ui.scope(|ui| {
                                ui.set_opacity(1.0 - openness);
                                ui.label(
                                    egui::RichText::new(format!("已折叠 {lines} 行代码"))
                                        .size(theme.body * 0.9)
                                        .italics()
                                        .color(theme.dim),
                                );
                            });
                        }
                        body_resp
                    }
                });
                if copied {
                    env.ctx.copy_text(code.clone());
                    env.actions.push(Action::Notify("复制成功".to_string()));
                }
                if let Some(l) = new_lang {
                    env.actions.push(Action::SetLang(idx, l));
                }
                // 只用图表内容区域做块定位，避免覆盖层挡住右上角的折叠/复制按钮。
                // 矩形扩到整个面板，跨块拖选时整块叠一层选区色。
                let content = r.inner.interact(Sense::click());
                content.union(r.response)
            }
            Block::FrontMatter { yaml, .. } => {
                let r = panel_frame(theme).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("YAML Front Matter")
                            .small()
                            .color(theme.dim),
                    );
                    for line in yaml.lines() {
                        ui.label(
                            egui::RichText::new(line)
                                .monospace()
                                .small()
                                .color(theme.quote),
                        );
                    }
                });
                r.response.interact(Sense::click())
            }
            Block::Footnote { id, content, .. } => {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("[^{}]", id))
                            .small()
                            .color(theme.accent),
                    );
                    text_response(
                        ui,
                        content,
                        theme,
                        theme.body * 0.9,
                        theme.quote,
                        false,
                        env,
                        idx,
                    )
                })
                .inner
            }
            Block::Toc { .. } => {
                let r = panel_frame(theme).show(ui, |ui| {
                    // 盒子宽度 ≈ 最长标题的两倍：目录条目长短不一时，
                    // 收缩到最宽那条的宽度会让整个框看起来又窄又小。
                    let longest_w = env
                        .outline
                        .iter()
                        .map(|item| {
                            ui.fonts_mut(|f| {
                                f.layout_no_wrap(
                                    format!("• {}", item.title),
                                    egui::FontId::proportional(theme.body),
                                    theme.link,
                                )
                                .rect
                                .width()
                            })
                        })
                        .fold(0.0_f32, f32::max);
                    let box_w = (longest_w * 2.0).clamp(300.0, env.max_width);
                    ui.set_min_width(box_w);
                    ui.label(egui::RichText::new("目录").strong().color(theme.heading));
                    for item in env.outline {
                        let indent = ((item.level.saturating_sub(1)) as f32) * 14.0;
                        let resp = ui
                            .horizontal(|ui| {
                                ui.add_space(indent);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(format!("• {}", item.title))
                                            .color(theme.link),
                                    )
                                    .sense(Sense::click()),
                                )
                            })
                            .inner;
                        if resp.clicked() {
                            env.actions.push(Action::Jump(item.block));
                        }
                    }
                });
                r.response.interact(Sense::click())
            }
            Block::Blank { .. } => {
                let (_, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 9.0), Sense::click());
                resp
            }
        }
    });
    inner.inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeKind;

    /// 回归：跨块选区高亮必须 ① 覆盖最后一个字（不能每行少一截）
    /// ② 画在文字下层（否则蓝底盖住文字，看起来"字没了"）。
    #[test]
    fn cross_sel_highlight_covers_last_char_and_sits_behind_text() {
        let theme = Theme::get(ThemeKind::Light);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let block = Block::Paragraph {
            content: vec![Inline::Text("abcde".to_string())],
            source: "abcde".to_string(),
        };
        let outline: Vec<OutlineItem> = Vec::new();
        let folded: HashSet<usize> = HashSet::new();
        let mut images = ImageCache::new();
        let mut actions: Vec<Action> = Vec::new();
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &c,
                        images: &mut images,
                        base_dir: None,
                        outline: &outline,
                        actions: &mut actions,
                        max_width: 700.0,
                        cross_sel: Some((0, 0, 0, 5)),
                        folded: &folded,
                    };
                    let _ = render_block(ui, 0, &block, &mut env, false);
                });
            },
        );
        // 选区底色被塞进 Shape::Vec 里，先摊平再看
        fn flatten<'a>(s: &'a egui::Shape, out: &mut Vec<&'a egui::Shape>) {
            match s {
                egui::Shape::Vec(v) => {
                    for x in v {
                        flatten(x, out);
                    }
                }
                other => out.push(other),
            }
        }
        let mut flat: Vec<&egui::Shape> = Vec::new();
        for cs in &out.shapes {
            flatten(&cs.shape, &mut flat);
        }
        let mut sel: Option<(usize, egui::Rect)> = None;
        let mut txt: Option<(usize, egui::Rect)> = None;
        for (i, shape) in flat.iter().enumerate() {
            match shape {
                egui::Shape::Rect(r) if r.fill == theme.selection => sel = Some((i, r.rect)),
                egui::Shape::Text(t) if t.galley.text() == "abcde" => {
                    txt = Some((i, egui::Rect::from_min_size(t.pos, t.galley.size())))
                }
                _ => {}
            }
        }
        let (sel_idx, sel_rect) = sel.expect("应画出选区底色");
        let (txt_idx, txt_rect) = txt.expect("应渲染文字");
        assert!(
            sel_rect.right() >= txt_rect.right() - 0.5,
            "选区右缘必须覆盖最后一个字：选区={sel_rect:?} 文字={txt_rect:?}"
        );
        assert!(
            sel_idx < txt_idx,
            "选区底色必须在文字之前绘制（垫在文字下层）：{sel_idx} vs {txt_idx}"
        );
        out.textures_delta.clear();
    }

    /// 双击选词边界：连续字母/数字/汉字算一个词，标点只选单字符。
    #[test]
    fn word_bounds_selects_word_or_char() {
        let t: Vec<char> = "hello 世界!".chars().collect();
        // "hello" = 0..5
        assert_eq!(word_bounds(&t, 2), (0, 5));
        // 空格（下标 5）只选它自己
        assert_eq!(word_bounds(&t, 5), (5, 6));
        // 连续汉字 "世界" = 6..8
        assert_eq!(word_bounds(&t, 6), (6, 8));
        assert_eq!(word_bounds(&t, 7), (6, 8));
        // 标点 "!" 只选单字符
        assert_eq!(word_bounds(&t, 8), (8, 9));
        // 空文本
        assert_eq!(word_bounds(&[], 0), (0, 0));
    }

    /// 时序图解析：participant 声明、四类箭头、Note、title。
    /// 重点回归两个解析 bug："-->>" 的第一个 '-' 属于发送方（箭头匹配必须
    /// 先长后短，否则发送方被切成 "D-"）；Note 的参与者名区分大小写。
    #[test]
    fn parse_sequence_basics() {
        let code = "sequenceDiagram\n  title 下单流程\n  participant U as 用户\n  participant S as 服务端\n  participant D as 数据库\n  U->>S: 提交订单\n  S->>D: 写入\n  D-->>S: 成功\n  S-->>U: 返回结果\n  Note over S,D: 事务保证\n  U->>U: 刷新页面\n  Note right of U: 备注";
        let (parts, items, title) = parse_sequence(code);
        assert_eq!(title, "下单流程");
        // 只有 U/S/D 三个参与者，不允许凭空多出 "D-"/"S-"/小写 "s"/"d"
        let ids: Vec<&str> = parts.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["U", "S", "D"], "参与者应恰为三个: {:?}", ids);
        assert_eq!(parts[0].label, "用户");
        let mut msgs = items
            .iter()
            .filter(|i| matches!(i, SeqItem::Message { .. }));
        let notes = items
            .iter()
            .filter(|i| matches!(i, SeqItem::Note { .. }))
            .count();
        assert_eq!(notes, 2, "两条 Note 都要解析出来");
        // 第一条：U->>S 实线
        if let Some(SeqItem::Message {
            from,
            to,
            label,
            dashed,
            cross,
        }) = msgs.next()
        {
            assert_eq!((*from, *to), (0, 1));
            assert_eq!(label, "提交订单");
            assert!(!dashed && !cross);
        } else {
            panic!("第一条应为消息");
        }
        // D-->>S：虚线，方向 D→S
        if let Some(SeqItem::Message {
            from, to, dashed, ..
        }) = items.iter().nth(2)
        {
            assert_eq!((*from, *to), (2, 1), "D-->>S 应为 D 指向 S");
            assert!(dashed, "-->> 应识别为虚线");
        }
        // Note over S,D 覆盖 1..2 区间
        if let Some(SeqItem::Note { over, label }) =
            items.iter().find(|i| matches!(i, SeqItem::Note { .. }))
        {
            assert_eq!(*over, (1, 2), "Note over S,D 应覆盖下标 1..2");
            assert_eq!(label, "事务保证");
        }
        // 自环消息 from == to
        assert!(
            items
                .iter()
                .any(|i| matches!(i, SeqItem::Message { from, to, .. } if from == to)),
            "U->>U 应解析出自环消息"
        );
    }

    /// 「流程图」按钮插入的模板：节点、菱形形状、连线标签、TD 竖排方向都要解析出来。
    #[test]
    fn parse_flowchart_template() {
        let code = "flowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]";
        let (nodes, edges, horizontal) = parse_flow(code);
        assert!(!horizontal, "TD 应为竖排方向");
        assert_eq!(nodes.len(), 3, "三个节点都要解析出来: {:?}", nodes);
        assert_eq!(nodes[0], ("A".into(), "开始".into(), 0_u8), "矩形节点");
        assert_eq!(nodes[1], ("B".into(), "判断".into(), 2), "决策节点应为菱形");
        assert_eq!(nodes[2], ("C".into(), "结束".into(), 0), "矩形节点");
        assert_eq!(edges.len(), 2, "两条连线: {:?}", edges);
        assert_eq!(edges[0], ("A".into(), "B".into(), String::new()));
        assert_eq!(edges[1], ("B".into(), "C".into(), "是".into()), "连线标签");
    }

    /// 分层布局的依据：入度为零的节点在第 0 层，后继节点逐层加深（最长路径）。
    /// draw_flow 按这个层级决定行号（TD）/列号（LR），方向不再失效。
    #[test]
    fn layered_depth_follows_edges() {
        // A --> B --> C：竖排时应该三行，从上到下 A、B、C
        let code = "flowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]";
        let (nodes, edges, _) = parse_flow(code);
        let mut layer: HashMap<String, usize> =
            nodes.iter().map(|(id, _, _)| (id.clone(), 0)).collect();
        for _ in 0..nodes.len() {
            let mut changed = false;
            for (a, b, _) in &edges {
                let la = layer.get(a.as_str()).copied();
                if let Some(lb) = layer.get_mut(b.as_str()) {
                    let la = la.unwrap_or(0);
                    if la + 1 > *lb {
                        *lb = la + 1;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        assert_eq!(layer["A"], 0);
        assert_eq!(layer["B"], 1);
        assert_eq!(layer["C"], 2, "TD 应自上而下分层");
    }

    /// 「图表」按钮插入的饼图模板：标题和数值都要解析出来，不含中文冒号坑。
    #[test]
    fn parse_pie_template() {
        let code = "pie title 示例图表\n  \"分类一\" : 42\n  \"分类二\" : 31\n  \"分类三\" : 27";
        let (title, data) = parse_pie(code);
        assert_eq!(title, "示例图表");
        assert_eq!(data.len(), 3, "三条数据: {:?}", data);
        assert_eq!(data[0], ("分类一".into(), 42.0));
        assert_eq!(data[2], ("分类三".into(), 27.0));
        // 中文标签后紧跟半角冒号（无空格）的写法也不能出错
        let (_, data) = parse_pie("pie\n\"A\":8\n\"B\":2");
        assert_eq!(data, vec![("A".into(), 8.0), ("B".into(), 2.0)]);
    }

    /// LR 方向（横向）解析 + 孤立 %% 注释行不产生节点。
    #[test]
    fn parse_flow_direction_and_comments() {
        let (nodes, _, horizontal) = parse_flow("graph LR\n  A[甲] --> B[乙]\n  %% 注释");
        assert!(horizontal, "LR 应为横排方向");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].1, "甲");
    }

    /// 流程图/饼图图形必须可点击：注入真实的按下+松开事件，clicked() 必须触发。
    /// 这是「点击图形重新进入编辑态」的前提——此前是 Sense::hover，点击永远无效，
    /// 导致插入流程图后无法再次编辑（回归测试）。
    #[test]
    fn diagram_graphic_is_clickable() {
        let theme = Theme::get(ThemeKind::Light);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        for code in [
            "flowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]",
            "pie title 示例图表\n  \"分类一\" : 42\n  \"分类二\" : 31",
        ] {
            let is_pie = code
                .lines()
                .find(|l| !l.trim().is_empty())
                .is_some_and(|l| l.trim().starts_with("pie"));
            let draw = |ui: &mut egui::Ui| -> egui::Response {
                if is_pie {
                    draw_pie(ui, code, &theme)
                } else {
                    draw_flow(ui, code, &theme)
                }
            };
            let run = |events: Vec<egui::Event>, clicked: &mut bool, rect: &mut egui::Rect| {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |root| {
                        egui::CentralPanel::default().show(root, |ui| {
                            let r = draw(ui);
                            *clicked = r.clicked();
                            *rect = r.rect;
                        });
                    },
                );
                out.textures_delta.clear();
            };
            // 第一帧：渲染图形并记录矩形
            let mut rect = egui::Rect::NOTHING;
            let mut clicked = false;
            run(vec![], &mut clicked, &mut rect);
            assert!(!rect.is_negative(), "图形应有实际尺寸: {code}");
            // 图形中心注入按下与松开。必须放进 RawInput 的 events：
            // `input_mut` 写入的是当前帧输入，下一次 begin_pass 会被新输入覆盖。
            let pos = rect.center();
            run(
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                &mut clicked,
                &mut rect,
            );
            run(
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
                &mut clicked,
                &mut rect,
            );
            assert!(
                clicked,
                "点击图形必须触发 clicked()（否则插入后无法再次编辑）: {code}"
            );
        }
    }

    /// 单击链接文字的命中测试：渲染态按 AST 标签区间，编辑态按原始 Markdown。
    #[test]
    fn link_click_hit_test() {
        let inlines = crate::ast::parse_inline("看[这段](https://e.com)文字");
        assert_eq!(link_at(&inlines, 1).as_deref(), Some("https://e.com"));
        assert_eq!(link_at(&inlines, 3).as_deref(), Some("https://e.com"));
        assert_eq!(link_at(&inlines, 0), None);
        assert_eq!(link_at(&inlines, 4), None);

        let src = "看[这段](https://e.com)文字";
        assert_eq!(link_at_markdown(src, 2).as_deref(), Some("https://e.com"));
        assert_eq!(link_at_markdown(src, 4).as_deref(), Some("https://e.com"));
        assert_eq!(link_at_markdown(src, 0), None);
    }

    /// 编辑态排版必须保留原始字符，否则光标/字节偏移会和缓冲区错位。
    #[test]
    fn inline_job_keeps_raw_text() {
        let theme = Theme::get(ThemeKind::Light);
        let src = "**粗** *斜* `码` ~~删~~ [链](https://e.com) 普通文字";
        let job = inline_job(src, &theme, 16.0, theme.text, 400.0, None, None);
        assert_eq!(job.text, src, "排版后的文本必须与缓冲区完全一致");
    }

    /// 加粗文字的外观在「编辑态(inline_job)」与「渲染态(build_layout)」必须一致：
    /// 都用 markd-bold 真粗体、都用普通正文色。否则鼠标离开加粗文字（退出编辑态）
    /// 时会突然变色（强调色）且变细（wght 对静态字体不生效）。
    #[test]
    fn bold_looks_same_in_edit_and_render_modes() {
        use crate::ast::{Inline, parse_inline};
        let theme = Theme::get(ThemeKind::Light);
        let bold_family = egui::FontFamily::Name("markd-bold".into());

        // 编辑态：**粗** 的正文段（egui 0.37 LayoutSection 无 text 字段，按 byte_range 取）
        let edit = inline_job("**粗体**", &theme, 16.0, theme.text, 400.0, None, None);
        let edit_bold = edit
            .sections
            .iter()
            .find(|s| {
                let (a, b) = (s.byte_range.start.0, s.byte_range.end.0);
                edit.text.get(a..b) == Some("粗体")
            })
            .expect("应有加粗正文段");
        assert_eq!(edit_bold.format.font_id.family, bold_family);
        assert_eq!(edit_bold.format.color, theme.text);

        // 渲染态：解析出的 Inline::Strong
        let inlines = parse_inline("**粗体**");
        let render = build_layout(&inlines, &theme, 16.0, theme.text, false);
        let render_bold = render
            .sections
            .iter()
            .find(|s| {
                let (a, b) = (s.byte_range.start.0, s.byte_range.end.0);
                render.text.get(a..b) == Some("粗体")
            })
            .expect("应有加粗正文段");
        assert_eq!(
            render_bold.format.font_id.family, bold_family,
            "渲染态加粗字体应与编辑态一致（真粗体），实际 {:?}",
            render_bold.format.font_id.family
        );
        assert_eq!(
            render_bold.format.color, theme.text,
            "渲染态加粗颜色应与编辑态一致（不变色），实际 {:?}",
            render_bold.format.color
        );
    }

    /// 光标在加粗范围内时露出这一对 `**`（部分源码模式），方便看清哪段加粗、
    /// 哪段没有；光标在加粗外仍隐藏。斜体/删除线保持“配对标记始终隐藏”。
    #[test]
    fn inline_job_reveals_paired_bold_markers_inside_span() {
        let theme = Theme::get(ThemeKind::Light);
        // "**粗**"：*0 *1 粗2 *3 *4；加粗 span = (0, 3)
        for cursor in [1, 2, 3] {
            let job = inline_job(
                "**粗**",
                &theme,
                16.0,
                theme.text,
                400.0,
                Some(cursor),
                None,
            );
            // 首尾 section 即开头/结尾的标记：显现时是正常 16px 字体，
            // 隐藏时是 0.1px 透明字体。
            let first = job.sections.first().expect("应有排版段");
            let last = job.sections.last().expect("应有排版段");
            assert!(
                first.format.font_id.size >= 15.0 && last.format.font_id.size >= 15.0,
                "cursor={cursor} 时加粗标记应显现，实际 {job:?}"
            );
            // 露出的标记用普通字重，不能跟着加粗。
            assert_eq!(
                first.format.font_id.family,
                FontFamily::Proportional,
                "露出的 ** 应是普通字重"
            );
        }

        // 光标在加粗范围之外（标记前/闭标记之后）→ 仍隐藏。
        for cursor in [0, 4, 5] {
            let job = inline_job(
                "**粗**",
                &theme,
                16.0,
                theme.text,
                400.0,
                Some(cursor),
                None,
            );
            let first = job.sections.first().expect("应有排版段");
            let last = job.sections.last().expect("应有排版段");
            assert!(
                first.format.font_id.size < 1.0 && last.format.font_id.size < 1.0,
                "cursor={cursor} 时加粗标记应隐藏，实际 {job:?}"
            );
        }

        // 斜体/删除线的配对标记仍保持隐藏（只对加粗做源码露出）。
        for (src, cursor) in [("~~删~~", 2), ("*斜*", 1)] {
            let job = inline_job(src, &theme, 16.0, theme.text, 400.0, Some(cursor), None);
            let first = job.sections.first().expect("应有排版段");
            let last = job.sections.last().expect("应有排版段");
            assert!(
                first.format.font_id.size < 1.0 && last.format.font_id.size < 1.0,
                "cursor={cursor} 时 [{src}] 的配对标记应保持隐藏，实际 {job:?}"
            );
        }
    }

    /// 行内代码例外：光标进入反引号内部时露出这一对反引号（源码模式），
    /// 光标在代码外时仍然隐藏；字号与正文一致，背景使用行内代码专属配色。
    #[test]
    fn inline_job_reveals_code_ticks_inside_span() {
        let theme = Theme::get(ThemeKind::Light);

        // 光标在内容里（插入点在首反引号之后、尾反引号之前）→ 露出。
        for cursor in [1, 2] {
            let job = inline_job("`码`", &theme, 16.0, theme.text, 400.0, Some(cursor), None);
            let first = job.sections.first().expect("应有排版段");
            let last = job.sections.last().expect("应有排版段");
            assert!(
                first.format.font_id.size >= 15.0 && last.format.font_id.size >= 15.0,
                "cursor={cursor} 时行内代码的反引号应显现，实际 {job:?}"
            );
            assert_eq!(
                first.format.background,
                theme.inline_code_bg(),
                "露出的反引号也应有代码背景"
            );
        }

        // 光标在代码外 → 仍隐藏。
        for cursor in [0, 3] {
            let job = inline_job("`码`", &theme, 16.0, theme.text, 400.0, Some(cursor), None);
            let first = job.sections.first().expect("应有排版段");
            assert!(
                first.format.font_id.size < 1.0,
                "cursor={cursor} 时代码外的反引号应隐藏"
            );
        }

        // 正文与反引号同字号，且文字/背景使用行内代码配色。
        let job = inline_job("`码`", &theme, 16.0, theme.text, 400.0, Some(1), None);
        for section in &job.sections {
            assert_eq!(
                section.format.font_id.size, 16.0,
                "行内代码字号应与正文一致"
            );
            assert_eq!(section.format.background, theme.inline_code_bg());
            assert_eq!(section.format.color, theme.inline_code_fg());
        }
    }

    /// 光标落在链接源码范围内时露出 `[标签](地址)`（URL 用链接色），
    /// 光标在链接外时仍隐藏标记、只显示带下划线的标签。
    #[test]
    fn inline_job_reveals_link_source_inside_span() {
        let theme = Theme::get(ThemeKind::Light);
        let src = "[点我](https://e.com)";

        // 光标在标签/地址里 → 露出完整源码，全部字符都是正常字号。
        for cursor in [1, 2, 5, 18] {
            let job = inline_job(src, &theme, 16.0, theme.text, 600.0, Some(cursor), None);
            assert!(
                job.sections.iter().all(|s| s.format.font_id.size >= 15.0),
                "cursor={cursor} 时链接源码应显现: {job:?}"
            );
        }

        // 露出的 URL 用链接色，方便辨认和修改（标签文字仍是正文色）。
        let job = inline_job(src, &theme, 16.0, theme.text, 600.0, Some(1), None);
        let url_section = job
            .sections
            .iter()
            .find(|s| s.format.color == theme.link && s.format.font_id.size >= 15.0)
            .expect("应能找到 URL 的排版段");
        assert_eq!(
            url_section.format.color, theme.link,
            "露出的 URL 应用链接色: {job:?}"
        );

        // 光标在链接外 → 方括号/圆括号仍隐藏，只剩标签文字。
        for cursor in [0, 19] {
            let job = inline_job(src, &theme, 16.0, theme.text, 600.0, Some(cursor), None);
            let first = job.sections.first().expect("应有排版段");
            let last = job.sections.last().expect("应有排版段");
            assert!(
                first.format.font_id.size < 1.0 && last.format.font_id.size < 1.0,
                "cursor={cursor} 时链接标记应隐藏: {job:?}"
            );
        }
    }

    /// 选区覆盖行内代码时，代码片背景应换成选区色——否则代码片灰底会盖住
    /// 垫在文字下层的选区蓝底，看起来像这段代码没被选中。
    #[test]
    fn selected_inline_code_bg_uses_selection_color() {
        let theme = Theme::get(ThemeKind::Light);
        // "前 `码` 后"：前0 空格1 `2 码3 `4 → 代码内容在字符 3
        let sel = Some((2usize, 5usize));

        let job = inline_job("前 `码` 后", &theme, 16.0, theme.text, 400.0, None, sel);
        let chip = job
            .sections
            .iter()
            .find(|s| s.format.background == theme.selection)
            .expect("被选中的行内代码底色应换成选区色");
        assert_eq!(
            chip.format.color,
            theme.inline_code_fg(),
            "换底色不应影响代码文字颜色"
        );

        // 选区不在代码上时保持原代码片底色。
        let job = inline_job(
            "前 `码` 后",
            &theme,
            16.0,
            theme.text,
            400.0,
            None,
            Some((0, 1)),
        );
        assert!(
            job.sections
                .iter()
                .any(|s| s.format.background == theme.inline_code_bg()),
            "未选中代码时应保持代码片底色"
        );

        // 渲染态走同一套染色。
        let inlines = crate::ast::parse_inline("前 `码` 后");
        let mut job = build_layout(&inlines, &theme, 16.0, theme.text, false);
        tint_selected_code_bg(&mut job, &theme, sel);
        assert!(
            job.sections
                .iter()
                .any(|s| s.format.background == theme.selection),
            "渲染态选中的行内代码也应换成选区色"
        );
    }

    /// 子列表标记要和父列表不同：无序变形状，有序变编号样式。
    #[test]
    fn nested_list_markers_differ_by_level() {
        // 层级换算（默认 2 空格一层）
        assert_eq!(list_level(0), 0);
        assert_eq!(list_level(1), 1);
        assert_eq!(list_level(2), 1);
        assert_eq!(list_level(3), 2);
        assert_eq!(list_level(4), 2);
        assert_eq!(list_level(5), 3);
        assert_eq!(list_level(6), 3);

        // 顶层保留源数字与后缀
        assert_eq!(ordered_list_marker("1.", 0), "1.");
        assert_eq!(ordered_list_marker("2)", 0), "2)");
        // 一层：小写字母；二层：小写罗马数字
        assert_eq!(ordered_list_marker("1.", 1), "a.");
        assert_eq!(ordered_list_marker("2)", 1), "b)");
        assert_eq!(ordered_list_marker("3.", 2), "iii.");
        // 三层循环回数字样式
        assert_eq!(ordered_list_marker("1.", 3), "1.");
        // 超过 26 的字母
        assert_eq!(ordered_list_marker("27.", 1), "aa.");
    }

    /// 无序列表的圆点在子层级必须换形状：实心圆 → 空心圆 → 小方块。
    #[test]
    fn unordered_bullet_shape_changes_with_level() {
        let theme = Theme::get(ThemeKind::Light);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 300.0));
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    for level in 0..3 {
                        draw_list_bullet(ui, level, theme.text, 10.0);
                        ui.add_space(16.0);
                    }
                });
            },
        );
        out.textures_delta.clear();
        let mut filled = 0;
        let mut hollow = 0;
        let mut square = 0;
        for cs in &out.shapes {
            match &cs.shape {
                egui::Shape::Circle(c) if c.fill != Color32::TRANSPARENT => filled += 1,
                egui::Shape::Circle(c) if c.stroke.width > 0.0 => hollow += 1,
                egui::Shape::Rect(r) if r.fill == theme.text => square += 1,
                _ => {}
            }
        }
        assert_eq!(filled, 1, "顶层应是实心圆");
        assert_eq!(hollow, 1, "第二层应是空心圆");
        assert_eq!(square, 1, "第三层应是小方块");
    }

    /// 落单的标记（用户正在输入、尚未闭合）仍要显现，否则按键像没生效。
    #[test]
    fn inline_job_shows_lone_bold_marker() {
        let theme = Theme::get(ThemeKind::Light);
        let job = inline_job("**粗体", &theme, 16.0, theme.text, 400.0, Some(2), None);
        let first = job.sections.first().expect("应有排版段");
        assert!(
            first.format.font_id.size >= 15.0,
            "未闭合的 ** 应显现: {job:?}"
        );
    }

    /// 15 种语言的关键字/函数/类型/标签都要被着色，且编辑态文本与缓冲区一致。
    #[test]
    fn code_highlights_all_supported_languages() {
        let theme = Theme::get(ThemeKind::Dark);
        let cases: &[(&str, &str, &[&str])] = &[
            ("rust", "fn main() { let x = 1; }", &["fn", "let", "main"]),
            (
                "python",
                "def foo():\n    return 42",
                &["def", "return", "foo"],
            ),
            ("bash", "echo hi", &["echo"]),
            ("zsh", "if true; then\nfi", &["if", "then", "fi"]),
            ("java", "public class Main {}", &["public", "class", "Main"]),
            ("xml", "<tag attr=\"v\">t</tag>", &["tag"]),
            (
                "go",
                "package main\nfunc run() {}",
                &["package", "func", "run"],
            ),
            ("c", "int main() { return 0; }", &["int", "main", "return"]),
            (
                "c++",
                "int main() { return 0; }",
                &["int", "main", "return"],
            ),
            ("css", "body { color: red; }", &["color"]),
            ("yml", "key: value", &["key"]),
            ("yaml", "key: value", &["key"]),
            ("html", "<div id=\"x\">hi</div>", &["div"]),
            (
                "sql",
                "SELECT * FROM users WHERE id = 1",
                &["SELECT", "FROM", "WHERE"],
            ),
            (
                "vue",
                "<template><div>hi</div></template>",
                &["template", "div"],
            ),
        ];
        for &(lang, code, toks) in cases {
            let edit = code_job_edit(code, lang, &theme);
            assert_eq!(edit.text, code, "编辑态文本必须与缓冲区一致: {lang}");
            let job = code_job(code, lang, &theme);
            assert!(job.sections.len() > 2, "{lang} 应产生多段高亮: {job:?}");
            for tok in toks {
                let pos = code.find(tok).expect("token 应在代码里");
                let colored = job.sections.iter().any(|s| {
                    let a = s.byte_range.start.0;
                    let b = s.byte_range.end.0;
                    a <= pos
                        && pos < b
                        && s.format.color != theme.text
                        && s.format.color != theme.dim
                });
                assert!(colored, "{lang} 的 `{tok}` 应被着色: {job:?}");
            }
        }
    }

    /// 各语言的注释语法（# / // / -- / /* */ / <!-- -->）都要按注释色渲染。
    #[test]
    fn code_highlights_comments_across_languages() {
        let theme = Theme::get(ThemeKind::Dark);
        let cases: &[(&str, &str, &str)] = &[
            ("rust", "// 注释", "// 注释"),
            ("python", "# 注释", "# 注释"),
            ("bash", "# 注释", "# 注释"),
            ("sql", "-- 注释", "-- 注释"),
            ("html", "<!-- 注释 -->", "<!-- 注释 -->"),
            ("c", "/* 注释 */", "/* 注释 */"),
        ];
        for &(lang, code, comment) in cases {
            let [_, _, _, comment_color, _, _] = theme.syntax(lang);
            let job = code_job(code, lang, &theme);
            let pos = job.text.find(comment).expect("注释应在代码里");
            let is_comment_color = job.sections.iter().any(|s| {
                let a = s.byte_range.start.0;
                let b = s.byte_range.end.0;
                a <= pos && pos < b && s.format.color == comment_color
            });
            assert!(is_comment_color, "{lang} 的注释应按注释色渲染: {job:?}");
        }
    }

    /// 代码块在编辑态复用同一套高亮排版：排版文本必须与缓冲区完全一致，
    /// 否则 TextEdit 的光标/选中会错位；同时应当有多段不同颜色。
    #[test]
    fn code_job_keeps_raw_text_and_colors_keywords() {
        let theme = Theme::get(ThemeKind::Dark);
        let src = "fn main() {\n    let x = \"s\"; // 注释\n}";
        // 编辑态：排版文本必须和缓冲区逐字符一致，否则光标/选中会错位
        let edit = code_job_edit(src, "rust", &theme);
        assert_eq!(edit.text, src);
        assert!(edit.sections.len() > 1, "应当有多个着色段");
        // 展示态：末尾多补一个换行，当作代码框的底部留白
        assert_eq!(code_job(src, "rust", &theme).text, format!("{src}\n"));
    }

    /// 表格宽度跟随可用宽度：三列应铺满编辑区，而不是被内容挤成一个小方块。
    #[test]
    fn table_resizes_with_available_width() {
        use crate::ast::Align;
        let theme = Theme::get(ThemeKind::Dark);
        let mut images = ImageCache::new();
        let outline: Vec<OutlineItem> = Vec::new();
        let mk = |s: &str| vec![Inline::Text(s.to_string())];
        let block = Block::Table {
            head: vec![mk("名称"), mk("说明"), mk("状态")],
            rows: vec![vec![mk("a"), mk("b"), mk("c")]],
            align: vec![Align::Left, Align::Left, Align::Left],
            source: String::new(),
        };
        egui::__run_test_ui(|ui| {
            ui.set_width(800.0);
            let ctx = ui.ctx().clone();
            let mut actions: Vec<Action> = Vec::new();
            let folded: HashSet<usize> = HashSet::new();
            let mut env = RenderEnv {
                theme: &theme,
                ctx: &ctx,
                images: &mut images,
                base_dir: None,
                outline: &outline,
                actions: &mut actions,
                max_width: 800.0,
                cross_sel: None,
                folded: &folded,
            };
            let resp = render_block(ui, 0, &block, &mut env, false);
            let w = resp.rect.width();
            assert!(w > 700.0, "表格应铺满可用宽度，实际 {w}");
        });
    }

    /// 单击链接不跳转，只把光标放进链接（随后露出源码供修改）；
    /// 只有 Cmd 或 Ctrl+单击才真正打开链接。
    #[test]
    fn clicking_link_requires_command_or_ctrl() {
        let theme = Theme::get(ThemeKind::Light);
        let outline: Vec<OutlineItem> = Vec::new();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let press = |pos: egui::Pos2, down: bool, modifiers: egui::Modifiers| -> Vec<egui::Event> {
            let mut events = Vec::new();
            if down {
                events.push(egui::Event::PointerMoved(pos));
            }
            // RawInput 没有 modifiers 字段；修饰键只能通过 ModifiersChanged 事件传入。
            events.push(egui::Event::ModifiersChanged(modifiers));
            events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: down,
                modifiers,
            });
            events
        };

        // 每个场景用独立的 Context：共享 Context 时第二次点击会被判成双击而提前 return。
        let click = |modifiers: egui::Modifiers| -> (egui::Rect, Vec<Action>) {
            let ctx = egui::Context::default();
            let link = Block::Paragraph {
                content: crate::ast::parse_inline("[点我](https://e.com)"),
                source: "[点我](https://e.com)".to_string(),
            };
            let mut images = ImageCache::new();
            let mut actions: Vec<Action> = Vec::new();
            let mut run = |events: Vec<egui::Event>, actions: &mut Vec<Action>| -> egui::Rect {
                let mut rect = egui::Rect::NOTHING;
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |root| {
                        egui::CentralPanel::default().show(root, |ui| {
                            let ctx = ui.ctx().clone();
                            let folded: HashSet<usize> = HashSet::new();
                            let mut env = RenderEnv {
                                theme: &theme,
                                ctx: &ctx,
                                images: &mut images,
                                base_dir: None,
                                outline: &outline,
                                actions,
                                max_width: 700.0,
                                cross_sel: None,
                                folded: &folded,
                            };
                            rect = render_block(ui, 0, &link, &mut env, false).rect;
                        });
                    },
                );
                out.textures_delta.clear();
                rect
            };
            let rect = run(vec![], &mut actions);
            let pos = rect.center();
            let _ = run(press(pos, true, modifiers), &mut actions);
            let _ = run(press(pos, false, modifiers), &mut actions);
            (rect, actions)
        };

        // 普通单击：只激活编辑，不打开链接。
        let (rect, actions) = click(egui::Modifiers::default());
        assert!(!rect.is_negative());
        assert!(
            actions.iter().any(|a| matches!(a, Action::Caret(..))),
            "普通单击链接应把光标放进链接: {actions:?}"
        );
        assert!(
            !actions.iter().any(|a| matches!(a, Action::OpenLink(_))),
            "普通单击不应打开链接: {actions:?}"
        );

        // Cmd / Ctrl+单击：打开链接，不再触发编辑光标。
        for modifiers in [
            egui::Modifiers {
                command: true,
                ..Default::default()
            },
            egui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
        ] {
            let (_, actions) = click(modifiers);
            assert!(
                actions
                    .iter()
                    .any(|a| matches!(a, Action::OpenLink(u) if u == "https://e.com")),
                "{modifiers:?} + 单击应打开链接: {actions:?}"
            );
            assert!(
                !actions.iter().any(|a| matches!(a, Action::Caret(..))),
                "{modifiers:?} + 单击不应同时激活编辑块: {actions:?}"
            );
        }
    }

    /// 单击普通文字应放置光标（独立 ctx，避免与链接点击连成双击）。
    #[test]
    fn clicking_text_places_caret() {
        let theme = Theme::get(ThemeKind::Light);
        let outline: Vec<OutlineItem> = Vec::new();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let run = |block: &Block,
                   events: Vec<egui::Event>,
                   images: &mut ImageCache,
                   actions: &mut Vec<Action>|
         -> egui::Rect {
            let mut rect = egui::Rect::NOTHING;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let c = ui.ctx().clone();
                        let folded: HashSet<usize> = HashSet::new();
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &c,
                            images,
                            base_dir: None,
                            outline: &outline,
                            actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &folded,
                        };
                        rect = render_block(ui, 0, block, &mut env, false).rect;
                    });
                },
            );
            out.textures_delta.clear();
            rect
        };
        let plain = Block::Paragraph {
            content: vec![Inline::Text("普通文字".to_string())],
            source: "普通文字".to_string(),
        };
        let mut images = ImageCache::new();
        let mut actions: Vec<Action> = Vec::new();
        let rect = run(&plain, vec![], &mut images, &mut actions);
        let pos = rect.center();
        let _ = run(
            &plain,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            &mut images,
            &mut actions,
        );
        let _ = run(
            &plain,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            &mut images,
            &mut actions,
        );
        assert!(
            actions.iter().any(|a| matches!(a, Action::Caret(..))),
            "点普通文字应放置光标: {actions:?}"
        );
    }

    /// 点击一行文字右侧的空白（超出 galley 宽度）也应命中该块：
    /// 光标应落到该行最后一个字符之后，而不是毫无反应。
    #[test]
    fn clicking_row_blank_places_caret_at_line_end() {
        let theme = Theme::get(ThemeKind::Light);
        let outline: Vec<OutlineItem> = Vec::new();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let run = |block: &Block,
                   events: Vec<egui::Event>,
                   images: &mut ImageCache,
                   actions: &mut Vec<Action>|
         -> egui::Rect {
            let mut rect = egui::Rect::NOTHING;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let ctx = ui.ctx().clone();
                        let folded: HashSet<usize> = HashSet::new();
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx,
                            images,
                            base_dir: None,
                            outline: &outline,
                            actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &folded,
                        };
                        rect = render_block(ui, 0, block, &mut env, false).rect;
                    });
                },
            );
            out.textures_delta.clear();
            rect
        };
        let plain = Block::Paragraph {
            content: vec![Inline::Text("短文字".to_string())],
            source: "短文字".to_string(),
        };
        let mut images = ImageCache::new();
        let mut actions: Vec<Action> = Vec::new();
        let rect = run(&plain, vec![], &mut images, &mut actions);
        // 点击文字右侧的行内空白：块文字只有三个字（约几十像素），
        // 点击点放在行右缘内、文字 galley 之外。
        let pos = egui::Pos2::new(rect.right() - 20.0, rect.center().y);
        assert!(
            pos.x > rect.min.x + 100.0,
            "点击点必须落在文字 galley 之外的空白区"
        );
        let press = |pos: egui::Pos2, down: bool| -> Vec<egui::Event> {
            let mut events = Vec::new();
            events.push(egui::Event::PointerMoved(pos));
            events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: down,
                modifiers: egui::Modifiers::default(),
            });
            events
        };
        let _ = run(&plain, press(pos, true), &mut images, &mut actions);
        let _ = run(&plain, press(pos, false), &mut images, &mut actions);
        assert!(
            actions.iter().any(|a| matches!(a, Action::Caret(..))),
            "点行尾空白应放置光标到行末: {actions:?}"
        );
    }

    /// 图片预览：大图压到视口高度的六成以内，小图保持原尺寸不放大。
    #[test]
    fn image_preview_is_capped_to_viewport() {
        let theme = Theme::get(ThemeKind::Light);
        let dir = std::env::temp_dir().join(format!("markd-img-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbaImage::from_pixel(200, 1200, image::Rgba([200, 30, 30, 255]))
            .save(dir.join("tall.png"))
            .unwrap();
        image::RgbaImage::from_pixel(50, 40, image::Rgba([30, 30, 200, 255]))
            .save(dir.join("small.png"))
            .unwrap();
        let outline: Vec<OutlineItem> = Vec::new();
        let folded: HashSet<usize> = HashSet::new();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut images = ImageCache::new();
        let mut actions: Vec<Action> = Vec::new();
        let mut heights = [0.0f32; 2];
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &c,
                        images: &mut images,
                        base_dir: Some(&dir),
                        outline: &outline,
                        actions: &mut actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                    };
                    for (i, url) in ["tall.png", "small.png"].iter().enumerate() {
                        let block = Block::Paragraph {
                            content: vec![Inline::Image {
                                alt: "图".to_string(),
                                url: url.to_string(),
                                width: None,
                            }],
                            source: format!("![图]({url})"),
                        };
                        heights[i] = render_block(ui, i, &block, &mut env, false).rect.height();
                    }
                });
            },
        );
        out.textures_delta.clear();
        assert!(heights[0] > 300.0, "大图应有一定高度：{}", heights[0]);
        assert!(
            heights[0] <= 361.0,
            "大图预览应压到视口高度的六成以内，实际 {}",
            heights[0]
        );
        assert!(
            (heights[1] - 40.0).abs() < 1.5,
            "小图应保持原尺寸不放大，实际 {}",
            heights[1]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 渲染一个代码块（含右上角复制按钮），确认控件树可正常构建、
    /// 且未点击时不会产生任何 action（避免误触发复制）。
    #[test]
    fn code_block_renders_with_copy_button() {
        let theme = Theme::get(ThemeKind::Light);
        let mut images = ImageCache::new();
        let outline: Vec<OutlineItem> = Vec::new();
        let block = Block::Code {
            lang: "rust".to_string(),
            code: "fn main() {}".to_string(),
            source: "```rust\nfn main() {}\n```".to_string(),
            quote: 0,
        };
        egui::__run_test_ui(|ui| {
            let ctx = ui.ctx().clone();
            let mut actions: Vec<Action> = Vec::new();
            let folded: HashSet<usize> = HashSet::new();
            let mut env = RenderEnv {
                theme: &theme,
                ctx: &ctx,
                images: &mut images,
                base_dir: None,
                outline: &outline,
                actions: &mut actions,
                max_width: 800.0,
                cross_sel: None,
                folded: &folded,
            };
            let _ = render_block(ui, 0, &block, &mut env, false);
            assert!(actions.is_empty(), "未点击时不应产生 action");
        });
    }

    /// 折叠态的代码块不渲染代码，只留一行提示，整体高度应明显矮于展开态。
    #[test]
    fn folded_code_block_renders_collapsed() {
        let theme = Theme::get(ThemeKind::Light);
        let outline: Vec<OutlineItem> = Vec::new();
        let block = Block::Code {
            lang: "rust".to_string(),
            code: "fn main() {}\nfn foo() {}\nfn bar() {}\nfn baz() {}".to_string(),
            source: "```rust\nfn main() {}\nfn foo() {}\nfn bar() {}\nfn baz() {}\n```".to_string(),
            quote: 0,
        };
        // 展开/折叠各用独立 ctx：折叠动画跨帧，共享 ctx 会让「折叠态」仍处于
        // 动画起点（openness≈1），看起来像没折叠。
        let render_height = |folded: bool| -> f32 {
            let ctx = egui::Context::default();
            let mut images = ImageCache::new();
            let mut actions: Vec<Action> = Vec::new();
            let set: HashSet<usize> = if folded {
                [0].into_iter().collect()
            } else {
                HashSet::new()
            };
            let mut height = 0.0;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let c = ui.ctx().clone();
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &c,
                            images: &mut images,
                            base_dir: None,
                            outline: &outline,
                            actions: &mut actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &set,
                        };
                        height = render_block(ui, 0, &block, &mut env, false).rect.height();
                    });
                },
            );
            out.textures_delta.clear();
            height
        };
        let expanded_h = render_height(false);
        assert!(expanded_h > 40.0, "展开态应占较大高度: {expanded_h}");
        let collapsed_h = render_height(true);
        assert!(
            collapsed_h < expanded_h,
            "折叠态应比展开态矮: expanded={expanded_h} collapsed={collapsed_h}"
        );
    }

    /// 代码块右上角必须真的渲染出「折叠」按钮，且点击它会产生 FoldToggle。
    /// 这是「用户看不到折叠按钮」的直接回归：按钮文本「折叠」要出现在渲染输出里。
    #[test]
    fn code_block_fold_button_clickable() {
        let theme = Theme::get(ThemeKind::Light);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let block = Block::Code {
            lang: "rust".to_string(),
            code: "fn main() {}".to_string(),
            source: "```rust\nfn main() {}\n```".to_string(),
            quote: 0,
        };
        let outline: Vec<OutlineItem> = Vec::new();
        let folded: HashSet<usize> = HashSet::new();

        // 第一帧：渲染代码块，从 shapes 里定位「折叠」按钮文本的中心。
        let mut images = ImageCache::new();
        let mut actions: Vec<Action> = Vec::new();
        let mut fold_pos = egui::Pos2::ZERO;
        {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx,
                            images: &mut images,
                            base_dir: None,
                            outline: &outline,
                            actions: &mut actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &folded,
                        };
                        let _ = render_block(ui, 0, &block, &mut env, false);
                    });
                },
            );
            for cs in &out.shapes {
                if let egui::Shape::Text(ts) = &cs.shape
                    && ts.galley.text() == "折叠"
                {
                    fold_pos = ts.pos + ts.galley.rect.center().to_vec2();
                }
            }
            out.textures_delta.clear();
        }
        assert!(fold_pos != egui::Pos2::ZERO, "代码块应渲染出「折叠」按钮");

        // 按下 + 松开：真实点击这个按钮。
        actions.clear();
        {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![
                        egui::Event::PointerMoved(fold_pos),
                        egui::Event::PointerButton {
                            pos: fold_pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::default(),
                        },
                    ],
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx,
                            images: &mut images,
                            base_dir: None,
                            outline: &outline,
                            actions: &mut actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &folded,
                        };
                        let _ = render_block(ui, 0, &block, &mut env, false);
                    });
                },
            );
            out.textures_delta.clear();
        }
        {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerButton {
                        pos: fold_pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::default(),
                    }],
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx,
                            images: &mut images,
                            base_dir: None,
                            outline: &outline,
                            actions: &mut actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &folded,
                        };
                        let _ = render_block(ui, 0, &block, &mut env, false);
                    });
                },
            );
            out.textures_delta.clear();
        }
        assert!(
            actions.iter().any(|a| matches!(a, Action::FoldToggle(0))),
            "点击「折叠」按钮应产生 FoldToggle: {actions:?}"
        );
    }

    /// 回归：点击 Mermaid 图表块的「折叠」按钮必须只折叠、不激活块。
    /// 之前图表分支用整个盒子的 response 做块定位，点折叠按钮会连带着
    /// `new_active` 把块切进编辑态、显示源码——表现为「点了没折叠反而显示源码」。
    #[test]
    fn diagram_fold_button_does_not_activate_block() {
        let theme = Theme::get(ThemeKind::Light);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let block = Block::Diagram {
            kind: crate::ast::DiagramKind::Flow,
            code: "flowchart TD\n  A[开始] --> B[结束]".to_string(),
            source: "```mermaid\nflowchart TD\n  A[开始] --> B[结束]\n```".to_string(),
        };
        let outline: Vec<OutlineItem> = Vec::new();
        let folded: HashSet<usize> = HashSet::new();

        // 第一帧：定位「折叠」按钮。
        let mut images = ImageCache::new();
        let mut actions: Vec<Action> = Vec::new();
        let mut fold_pos = egui::Pos2::ZERO;
        {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx,
                            images: &mut images,
                            base_dir: None,
                            outline: &outline,
                            actions: &mut actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &folded,
                        };
                        let _ = render_block(ui, 0, &block, &mut env, false);
                    });
                },
            );
            for cs in &out.shapes {
                if let egui::Shape::Text(ts) = &cs.shape
                    && ts.galley.text() == "折叠"
                {
                    fold_pos = ts.pos + ts.galley.rect.center().to_vec2();
                }
            }
            out.textures_delta.clear();
        }
        assert!(fold_pos != egui::Pos2::ZERO, "图表块应渲染出「折叠」按钮");

        // 按下 + 松开：真实点击，记录块响应是否被「误点」。
        actions.clear();
        let mut block_clicked = false;
        {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![
                        egui::Event::PointerMoved(fold_pos),
                        egui::Event::PointerButton {
                            pos: fold_pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::default(),
                        },
                    ],
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx,
                            images: &mut images,
                            base_dir: None,
                            outline: &outline,
                            actions: &mut actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &folded,
                        };
                        let resp = render_block(ui, 0, &block, &mut env, false);
                        block_clicked = resp.clicked();
                    });
                },
            );
            out.textures_delta.clear();
        }
        {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerButton {
                        pos: fold_pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::default(),
                    }],
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx,
                            images: &mut images,
                            base_dir: None,
                            outline: &outline,
                            actions: &mut actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &folded,
                        };
                        let resp = render_block(ui, 0, &block, &mut env, false);
                        block_clicked = resp.clicked();
                    });
                },
            );
            out.textures_delta.clear();
        }
        assert!(
            actions.iter().any(|a| matches!(a, Action::FoldToggle(0))),
            "点击「折叠」应产生 FoldToggle: {actions:?}"
        );
        assert!(
            !block_clicked,
            "点击「折叠」按钮不应激活图表块（否则会进入编辑态显示源码）"
        );
    }

    /// 回归：点击 Mermaid 图表块的「删除」按钮必须产生 AskDeleteBlock
    /// （app 层据此弹确认窗），且不误激活块。行为与代码块删除按钮一致。
    #[test]
    fn diagram_delete_button_requests_confirm() {
        let theme = Theme::get(ThemeKind::Light);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let block = Block::Diagram {
            kind: crate::ast::DiagramKind::Flow,
            code: "flowchart TD\n  A[开始] --> B[结束]".to_string(),
            source: "```mermaid\nflowchart TD\n  A[开始] --> B[结束]\n```".to_string(),
        };
        let outline: Vec<OutlineItem> = Vec::new();
        let folded: HashSet<usize> = HashSet::new();

        // 第一帧：定位「删除」按钮。
        let mut images = ImageCache::new();
        let mut actions: Vec<Action> = Vec::new();
        let mut delete_pos = egui::Pos2::ZERO;
        {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx,
                            images: &mut images,
                            base_dir: None,
                            outline: &outline,
                            actions: &mut actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &folded,
                        };
                        let _ = render_block(ui, 0, &block, &mut env, false);
                    });
                },
            );
            for cs in &out.shapes {
                if let egui::Shape::Text(ts) = &cs.shape
                    && ts.galley.text() == "删除"
                {
                    delete_pos = ts.pos + ts.galley.rect.center().to_vec2();
                }
            }
            out.textures_delta.clear();
        }
        assert!(delete_pos != egui::Pos2::ZERO, "图表块应渲染出「删除」按钮");

        // 按下 + 松开：真实点击。
        actions.clear();
        let mut block_clicked = false;
        for pressed in [true, false] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerButton {
                        pos: delete_pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    }],
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx,
                            images: &mut images,
                            base_dir: None,
                            outline: &outline,
                            actions: &mut actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &folded,
                        };
                        let resp = render_block(ui, 0, &block, &mut env, false);
                        block_clicked |= resp.clicked();
                    });
                },
            );
            out.textures_delta.clear();
        }
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::AskDeleteBlock(0))),
            "点击「删除」应产生 AskDeleteBlock 交给 app 层弹确认窗: {actions:?}"
        );
        assert!(!block_clicked, "点击「删除」按钮不应激活图表块");
    }
}
