//! Author: aurelianus <aurelianus@126.com>

//! 渲染层：把 AST 画成富文本 / 表格 / 代码块 / 流程图 / 图片。
//!
//! 非活动块走这里；活动块由 editor.rs 用 TextEdit 显示源码，形成 Hybrid View。

use crate::ast::*;
use crate::document::OutlineItem;
use crate::theme::Theme;
use crate::workspace;
use egui::text::{LayoutJob, TextFormat};
use egui::{self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::collections::HashMap;
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
    /// 点击了表格单元格：(行, 列, 光标是否在文字末尾)。
    /// 进入编辑态时聚焦该单元格，光标落在点击位置附近而不是固定跳到第一格。
    TableCellClick(usize, usize, bool),
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
        ));
    }
    picked
}

/// 测试支持：language_combo 每帧记录（按钮矩形, popup id, 本帧是否被点, 弹层是否开）。
/// 供真实帧测试定位点击坐标与断言弹层状态。仅测试构建存在。
#[cfg(test)]
pub(crate) static COMBO_TRACE: std::sync::Mutex<
    Vec<(egui::Rect, egui::Id, bool, bool)>,
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
            // 加粗用主题单独配的强调色：中文字体只靠字重不够醒目
            let bc = theme.bold_color();
            for x in v {
                append_inline(
                    job, x, theme, size, bc, true, italic, strike, underline, linked,
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
            let mut f = theme.mono_fmt();
            f.color = theme.accent;
            f.background = theme.code_bg;
            job.append(t, 0.0, f);
        }
        Inline::Link { label, .. } => {
            for x in label {
                append_inline(
                    job, x, theme, size, color, bold, italic, strike, underline, true,
                );
            }
        }
        Inline::Image { alt, url } => {
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
            Inline::Image { alt, url } => {
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
) -> LayoutJob {
    let line_height = size * 1.45;
    let marker = TextFormat {
        font_id: FontId::new(0.1, FontFamily::Proportional),
        color: Color32::TRANSPARENT,
        line_height: Some(line_height),
        ..Default::default()
    };
    let bold_family = FontFamily::Name("markd-bold".into());

    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.wrap.break_anywhere = true;

    let chars: Vec<char> = text.chars().collect();

    // 预扫描行内标记 run：星号按「粗体 run(n>=2) / 斜体 run(n==1)」分组，
    // 波浪号、反引号各自一组。组内成对的标记隐藏渲染；落单的（手打的 *、
    // 敲了一半的 **、未闭合的反引号）必须按普通文字显示，光标紧挨着的 run
    // 也一样——不然编辑态按 Shift+8 打出的星号被渲染成 0.1px 透明字，
    // 看起来就像按键没生效。
    let mut lone: Vec<usize> = Vec::new();
    {
        let mut ital: Vec<usize> = Vec::new();
        let mut tilde: Vec<usize> = Vec::new();
        let mut tick: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < chars.len() {
            match chars[i] {
                '*' => {
                    let n = chars[i..].iter().take_while(|&&x| x == '*').count();
                    if n == 1 {
                        ital.push(i);
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
        mark_lone(&tilde, &mut lone);
        mark_lone(&tick, &mut lone);
    }
    // 光标停在 run 上或紧挨着它（字符下标）→ 显示
    let near_caret =
        |start: usize, n: usize| cursor.is_some_and(|c| start <= c && c <= start + n);

    let fmt_now =
        |code: bool, italic: bool, bold: bool, strike: bool| {
            if code {
                TextFormat {
                    font_id: FontId::new(size, FontFamily::Monospace),
                    color: theme.quote,
                    background: theme.code_bg,
                    expand_bg: 0.0,
                    line_height: Some(line_height),
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
                    line_height: Some(line_height),
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
            let f = if bold { fmt_now(code, italic, bold, strike) } else { marker.clone() };
            for k in i..i + 8 {
                push(&mut job, chars[k], &f);
            }
            bold = !bold;
            i += 8;
            continue;
        }
        if c == '<' && tag_at(&chars, i, "</strong>") {
            let f = if bold { marker.clone() } else { fmt_now(code, italic, bold, strike) };
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
            let f = if italic { fmt_now(code, italic, bold, strike) } else { marker.clone() };
            for k in i..i + 4 {
                push(&mut job, chars[k], &f);
            }
            italic = !italic;
            i += 4;
            continue;
        }
        if c == '<' && tag_at(&chars, i, "</em>") {
            let f = if italic { marker.clone() } else { fmt_now(code, italic, bold, strike) };
            for k in i..i + 5 {
                push(&mut job, chars[k], &f);
            }
            italic = !italic;
            i += 5;
            continue;
        }
        if c == '`' {
            let visible = lone.contains(&i) || near_caret(i, 1);
            let f = if visible {
                fmt_now(code, italic, bold, strike)
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
            let visible = (n == 1 && lone.contains(&i)) || near_caret(i, n);
            let f = if visible {
                fmt_now(code, italic, bold, strike)
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
            let visible = lone.contains(&i) || near_caret(i, 2);
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
                        push(&mut job, '[', &marker);
                        let link_fmt = TextFormat {
                            font_id: FontId::new(size, FontFamily::Proportional),
                            color: theme.link,
                            underline: Stroke::new(1.0, theme.link),
                            line_height: Some(line_height),
                            ..Default::default()
                        };
                        for k in i + 1..close {
                            push(&mut job, chars[k], &link_fmt);
                        }
                        for k in close..=end {
                            push(&mut job, chars[k], &marker);
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
    let resp = ui.add(
        egui::Label::new(egui::WidgetText::Galley(galley.clone())).sense(sense),
    );
    (resp, galley)
}

fn wrapped_label(
    ui: &mut egui::Ui,
    job: LayoutJob,
    max_w: f32,
    sense: Sense,
) -> egui::Response {
    layout_label(ui, job, max_w, sense).0
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
    let job = build_layout(inlines, theme, size, color, bold);
    let (resp, galley) = layout_label(ui, job, env.max_width, Sense::click());
    if resp.clicked() {
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            let local = p - resp.rect.min;
            let cursor = galley.cursor_from_pos(local);
            let char_idx = cursor.index.0;
            // 点到链接文字：直接打开，不再把这个块切成编辑态。
            // 点在普通文字上仍然是「把光标放到点击处」。
            if let Some(url) = link_at(inlines, char_idx) {
                env.actions.push(Action::OpenLink(url));
                return resp;
            }
            env.actions.push(Action::Caret(idx, local));
        }
    }
    let _ = idx;
    resp
}

fn image_response(
    ui: &mut egui::Ui,
    alt: &str,
    url: &str,
    env: &mut RenderEnv<'_>,
) -> egui::Response {
    if let Some(path) = workspace::resolve(env.base_dir, url) {
        if let Some(tex) = env.images.texture(env.ctx, &path) {
            let sz = tex.size_vec2();
            let max_w = ui.available_width().max(80.0);
            let scale = (max_w / sz.x.max(1.0)).min(1.0);
            let (rect, resp) =
                ui.allocate_exact_size(Vec2::new(sz.x * scale, sz.y * scale), Sense::click());
            ui.painter().image(
                tex.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            return resp;
        }
    }
    let mut f = env.theme.fmt(env.theme.body * 0.9);
    f.color = env.theme.dim;
    let mut job = LayoutJob::default();
    job.append(&format!("[图片: {} — {}]", alt, url), 0.0, f);
    ui.add(egui::Label::new(job).sense(Sense::click()))
}

/// 轻量词法着色，覆盖常见语言，不依赖 syntect。
/// 这里给每行末尾都补 `\n`，让区块底部多一行留白、看得更像代码框。
pub fn code_job(code: &str, lang: &str, theme: &Theme) -> LayoutJob {
    code_job_impl(code, lang, theme, true)
}

/// 编辑态用的高亮排版：**文本与缓冲区逐字符一致**（不补行尾换行），
/// 否则 TextEdit 的光标/选中会按错位的字符下标走。
pub fn code_job_edit(code: &str, lang: &str, theme: &Theme) -> LayoutJob {
    code_job_impl(code, lang, theme, false)
}

fn code_job_impl(code: &str, lang: &str, theme: &Theme, trailing_newline: bool) -> LayoutJob {
    const KW: &[&str] = &[
        "fn",
        "let",
        "mut",
        "pub",
        "use",
        "struct",
        "enum",
        "impl",
        "match",
        "if",
        "else",
        "for",
        "while",
        "return",
        "const",
        "static",
        "type",
        "trait",
        "mod",
        "self",
        "Self",
        "async",
        "await",
        "def",
        "class",
        "import",
        "from",
        "as",
        "in",
        "is",
        "not",
        "and",
        "or",
        "None",
        "True",
        "False",
        "var",
        "function",
        "export",
        "default",
        "new",
        "void",
        "int",
        "float",
        "double",
        "char",
        "bool",
        "string",
        "public",
        "private",
        "package",
        "interface",
        "try",
        "catch",
        "throw",
        "func",
        "defer",
        "go",
        "map",
        "range",
        "select",
        "case",
        "break",
        "continue",
        "where",
        "with",
        "do",
        "then",
        "end",
        "SELECT",
        "FROM",
        "WHERE",
    ];
    let hash_comment = matches!(
        lang.to_lowercase().as_str(),
        "python" | "py" | "bash" | "sh" | "shell" | "yaml" | "yml" | "toml" | "ruby" | "rb"
    );
    let [kw_color, str_color, num_color, comment_color] = theme.syntax();
    let mut job = LayoutJob::default();
    // 代码行常没有空格，允许在任意字符处换行以适配窄窗口。
    job.wrap.break_anywhere = true;
    let lines: Vec<&str> = code.split('\n').collect();
    for (n, line) in lines.iter().enumerate() {
        let line = *line;
        let c: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < c.len() {
            let comment = (hash_comment && c[i] == '#')
                || (!hash_comment && c[i] == '/' && i + 1 < c.len() && c[i + 1] == '/');
            if comment {
                let rest: String = c[i..].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = comment_color;
                job.append(&rest, 0.0, f);
                break;
            }
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
                let is_kw = KW.contains(&tok.as_str());
                let mut f = theme.mono_fmt();
                f.color = if is_num {
                    num_color
                } else if is_kw {
                    kw_color
                } else {
                    theme.text
                };
                if is_kw {
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

fn panel_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::NONE
        .fill(theme.code_bg)
        .corner_radius(6.0)
        .inner_margin(12.0)
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

fn parse_flow(code: &str) -> (Vec<(String, String, u8)>, Vec<(String, String, String)>, bool) {
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
        ((depth as f32) * (nw + gap) + gap, (widest as f32) * (nh + gap) + gap)
    } else {
        ((widest as f32) * (nw + gap) + gap, (depth as f32) * (nh + gap) + gap)
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
                let tx = if dx.abs() > 1e-4 { hw / dx.abs() } else { f32::INFINITY };
                let ty = if dy.abs() > 1e-4 { hh / dy.abs() } else { f32::INFINITY };
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
    for (id, label, shape) in &nodes {
        if let Some(c) = pos.get(id) {
            let r = Rect::from_center_size(*c, Vec2::new(nw, nh));
            match shape {
                // {判断} —— 决策节点画菱形
                2 => {
                    let pts = vec![
                        Pos2::new(c.x, r.top()),
                        Pos2::new(r.right(), c.y),
                        Pos2::new(c.x, r.bottom()),
                        Pos2::new(r.left(), c.y),
                    ];
                    p.add(egui::Shape::convex_polygon(
                        pts,
                        theme.code_bg,
                        Stroke::new(1.2, theme.accent),
                    ));
                }
                // (圆角节点) 画胶囊
                1 => {
                    p.rect_filled(r, nh / 2.0, theme.code_bg);
                    p.rect_stroke(
                        r,
                        nh / 2.0,
                        Stroke::new(1.2, theme.accent),
                        egui::StrokeKind::Outside,
                    );
                }
                // [普通节点] 画矩形
                _ => {
                    p.rect_filled(r, 5.0, theme.code_bg);
                    p.rect_stroke(
                        r,
                        5.0,
                        Stroke::new(1.2, theme.accent),
                        egui::StrokeKind::Outside,
                    );
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
        let swatch = Rect::from_center_size(
            Pos2::new(cx + r0 + 24.0, ly),
            Vec2::new(12.0, 12.0),
        );
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
                ui.add_space(if *level <= 2 { 12.0 } else { 7.0 });
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
                ui.add_space(4.0);
                r
            }
            Block::Paragraph { content, .. } => {
                if content.len() == 1 {
                    if let Inline::Image { alt, url } = &content[0] {
                        return image_response(ui, alt, url, env);
                    }
                }
                text_response(ui, content, theme, theme.body, color, false, env, idx)
            }
            Block::Quote { level, content, .. } => {
                ui.horizontal(|ui| {
                    for _ in 0..(*level).min(4) {
                        ui.add_space(8.0);
                    }
                    let (r, _) = ui.allocate_exact_size(Vec2::new(3.0, 18.0), Sense::hover());
                    ui.painter().rect_filled(r, 1.0, theme.accent);
                    ui.add_space(8.0);
                    text_response(ui, content, theme, theme.body, theme.quote, true, env, idx)
                })
                .inner
            }
            Block::ListItem { item, .. } => {
                let indent = (item.indent.min(12) as f32) * 8.0;
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    match item.checked {
                        Some(checked) => {
                            let mut c = checked;
                            let cb = ui.checkbox(&mut c, "");
                            if cb.clicked() {
                                env.actions.push(Action::ToggleTask(idx));
                            }
                        }
                        None => {
                            let marker = if item.marker.ends_with('.') || item.marker.ends_with(')')
                            {
                                format!("{}  ", item.marker)
                            } else {
                                "•  ".to_string()
                            };
                            let mut f = theme.fmt(theme.body);
                            f.color = theme.dim;
                            let mut job = LayoutJob::default();
                            job.append(&marker, 0.0, f);
                            ui.label(job);
                        }
                    }
                    text_response(ui, &item.content, theme, theme.body, color, false, env, idx)
                })
                .inner
            }
            Block::Code { lang, code, .. } => {
                let mut copied = false;
                let mut new_lang: Option<String> = None;
                let r = panel_frame(theme).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // 左上角的语言文字标签已移除：右上角的语言下拉里已经显示当前语言，再标一遍是冗余。
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // 右上角：复制 + 语言选择（改的是代码块的 info string）
                            if ui
                                .button(egui::RichText::new("复制").small())
                                .on_hover_cursor(egui::CursorIcon::Default)
                                .clicked()
                            {
                                copied = true;
                            }
                            if let Some(l) = language_combo(ui, ("code_lang", idx), lang) {
                                new_lang = Some(l);
                            }
                        });
                    });
                    ui.add_space(3.0);
                    wrapped_label(
                        ui,
                        code_job(code, lang, theme),
                        env.max_width,
                        Sense::hover(),
                    )
                });
                if copied {
                    env.ctx.copy_text(code.clone());
                    env.actions
                        .push(Action::Notify("复制成功".to_string()));
                }
                if let Some(l) = new_lang {
                    env.actions.push(Action::SetLang(idx, l));
                }
                // 只用代码文本区域做块定位，避免覆盖层挡住右上角的复制按钮。
                r.inner.interact(Sense::click())
            }
            Block::Table { head, rows, .. } => {
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
                let r = egui::Frame::NONE
                    .fill(theme.bg)
                    .stroke(Stroke::new(1.0, theme.table_line))
                    .corner_radius(4.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.set_min_width(col_w * cols as f32 + gap);
                        egui::Grid::new(("md_table", idx))
                            .min_col_width(col_w)
                            .max_col_width(col_w)
                            .show(ui, |ui| {
                                // 表头行：最深一档底色
                                let mut rects = Vec::new();
                                for c in head {
                                    let fr = egui::Frame::NONE
                                        .fill(theme.table_head)
                                        .inner_margin(3.0)
                                        .show(ui, |ui| {
                                            ui.set_min_width(col_w - 6.0);
                                            wrapped_label(
                                                ui,
                                                build_layout(
                                                    c,
                                                    theme,
                                                    theme.body * 0.95,
                                                    theme.heading,
                                                    true,
                                                ),
                                                col_w - 6.0,
                                                Sense::hover(),
                                            );
                                        });
                                    rects.push(fr.response.rect);
                                }
                                row_rects.push(rects);
                                ui.end_row();
                                // 数据行：正文底色（表头一档颜色就够分了）
                                for row in rows {
                                    let mut rects = Vec::new();
                                    for c in row {
                                        let fr = egui::Frame::NONE
                                            .fill(theme.bg)
                                            .inner_margin(3.0)
                                            .show(ui, |ui| {
                                                ui.set_min_width(col_w - 6.0);
                                                wrapped_label(
                                                    ui,
                                                    build_layout(
                                                        c,
                                                        theme,
                                                        theme.body * 0.95,
                                                        color,
                                                        false,
                                                    ),
                                                    col_w - 6.0,
                                                    Sense::hover(),
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
                for k in 1..cols {
                    let x = content.left() + k as f32 * (col_w + gap) - gap / 2.0;
                    p.line_segment(
                        [Pos2::new(x, content.top()), Pos2::new(x, content.bottom())],
                        stroke,
                    );
                }
                // 点击了哪个单元格：进入编辑态时直接聚焦那一格，光标落在
                // 点击位置附近（点在文字右半边 → 光标到末尾），不再固定跳到第一格。
                if r.response.clicked()
                    && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                {
                    for (ri, rects) in row_rects.iter().enumerate() {
                        if let Some(ci) = rects.iter().position(|x| x.contains(p)) {
                            let end = p.x > rects[ci].center().x;
                            env.actions.push(Action::TableCellClick(ri, ci, end));
                            break;
                        }
                    }
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
                let is_pie = code
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .is_some_and(|l| l.trim().starts_with("pie"));
                if is_pie {
                    draw_pie(ui, code, theme)
                } else {
                    draw_flow(ui, code, theme)
                }
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
        let code =
            "pie title 示例图表\n  \"分类一\" : 42\n  \"分类二\" : 31\n  \"分类三\" : 27";
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
        let job = inline_job(src, &theme, 16.0, theme.text, 400.0, None);
        assert_eq!(job.text, src, "排版后的文本必须与缓冲区完全一致");
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
            let mut env = RenderEnv {
                theme: &theme,
                ctx: &ctx,
                images: &mut images,
                base_dir: None,
                outline: &outline,
                actions: &mut actions,
                max_width: 800.0,
            };
            let resp = render_block(ui, 0, &block, &mut env, false);
            let w = resp.rect.width();
            assert!(w > 700.0, "表格应铺满可用宽度，实际 {w}");
        });
    }

    /// 单击链接文字直接打开链接；单击普通文字仍然只是把光标放到点击处。
    #[test]
    fn clicking_link_opens_it() {
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
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx,
                            images,
                            base_dir: None,
                            outline: &outline,
                            actions,
                            max_width: 700.0,
                        };
                        rect = render_block(ui, 0, block, &mut env, false).rect;
                    });
                },
            );
            out.textures_delta.clear();
            rect
        };
        let press = |pos: egui::Pos2, down: bool| -> Vec<egui::Event> {
            let mut events = Vec::new();
            if down {
                events.push(egui::Event::PointerMoved(pos));
            }
            events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: down,
                modifiers: egui::Modifiers::default(),
            });
            events
        };

        let link = Block::Paragraph {
            content: crate::ast::parse_inline("[点我](https://e.com)"),
            source: "[点我](https://e.com)".to_string(),
        };
        let mut images = ImageCache::new();
        let mut actions: Vec<Action> = Vec::new();
        let rect = run(&link, vec![], &mut images, &mut actions);
        assert!(!rect.is_negative());
        let pos = rect.center();
        let _ = run(&link, press(pos, true), &mut images, &mut actions);
        let _ = run(&link, press(pos, false), &mut images, &mut actions);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::OpenLink(u) if u == "https://e.com")),
            "单击链接必须打开: {actions:?}"
        );
        assert!(
            !actions.iter().any(|a| matches!(a, Action::Caret(..))),
            "点链接不应同时激活编辑块: {actions:?}"
        );

        let plain = Block::Paragraph {
            content: vec![Inline::Text("普通文字".to_string())],
            source: "普通文字".to_string(),
        };
        actions.clear();
        let rect = run(&plain, vec![], &mut images, &mut actions);
        let pos = rect.center();
        let _ = run(&plain, press(pos, true), &mut images, &mut actions);
        let _ = run(&plain, press(pos, false), &mut images, &mut actions);
        assert!(
            actions.iter().any(|a| matches!(a, Action::Caret(..))),
            "点普通文字应放置光标: {actions:?}"
        );
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
        };
        egui::__run_test_ui(|ui| {
            let ctx = ui.ctx().clone();
            let mut actions: Vec<Action> = Vec::new();
            let mut env = RenderEnv {
                theme: &theme,
                ctx: &ctx,
                images: &mut images,
                base_dir: None,
                outline: &outline,
                actions: &mut actions,
                max_width: 800.0,
            };
            let _ = render_block(ui, 0, &block, &mut env, false);
            assert!(actions.is_empty(), "未点击时不应产生 action");
        });
    }
}
