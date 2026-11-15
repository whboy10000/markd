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
}

/// 代码块可选语言，覆盖常见前端/后端/脚本/配置/标记语言。
const LANGUAGES: &[&str] = &[
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
    "xml",
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

fn append_inline(
    job: &mut LayoutJob,
    inl: &Inline,
    theme: &Theme,
    size: f32,
    color: Color32,
    bold: bool,
    italic: bool,
    strike: bool,
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
            if linked {
                f.underline = Stroke::new(1.0, theme.link);
            }
            job.append(t, 0.0, f);
        }
        Inline::Strong(v) => {
            for x in v {
                append_inline(job, x, theme, size, color, true, italic, strike, linked);
            }
        }
        Inline::Emph(v) => {
            for x in v {
                append_inline(job, x, theme, size, color, bold, true, strike, linked);
            }
        }
        Inline::Strike(v) => {
            for x in v {
                append_inline(job, x, theme, size, color, bold, italic, true, linked);
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
                append_inline(job, x, theme, size, color, bold, italic, strike, true);
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
        append_inline(&mut job, i, theme, size, color, bold, false, false, false);
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

fn first_link(inlines: &[Inline]) -> Option<String> {
    for i in inlines {
        match i {
            Inline::Link { url, .. } => return Some(url.clone()),
            Inline::Strong(v) | Inline::Emph(v) | Inline::Strike(v) => {
                if let Some(u) = first_link(v) {
                    return Some(u);
                }
            }
            _ => {}
        }
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
    let mut i = 0usize;
    let mut bold = false;
    let mut italic = false;
    let mut strike = false;
    let mut code = false;

    fn push(job: &mut LayoutJob, c: char, f: &TextFormat) {
        let mut buf = [0u8; 4];
        job.append(c.encode_utf8(&mut buf), 0.0, f.clone());
    }

    while i < chars.len() {
        let c = chars[i];

        if c == '`' {
            push(&mut job, c, &marker);
            code = !code;
            i += 1;
            continue;
        }
        if c == '*' {
            let mut n = 0;
            while i + n < chars.len() && chars[i + n] == '*' {
                n += 1;
            }
            for _ in 0..n {
                push(&mut job, '*', &marker);
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
            push(&mut job, '~', &marker);
            push(&mut job, '~', &marker);
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

        let fmt = if code {
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
        };
        push(&mut job, c, &fmt);
        i += 1;
    }
    job
}

/// 用显式宽度排版成 galley 再交给 Label。
/// `Label` 默认按 Ui 的 wrap 模式决定宽度，横向布局（引用/列表）里会退化成
/// 无限宽导致长段落不换行；这里直接给定宽度，任何布局下都会换行。
fn wrapped_label(
    ui: &mut egui::Ui,
    mut job: LayoutJob,
    max_w: f32,
    sense: Sense,
) -> egui::Response {
    let avail = ui.available_width();
    let w = if avail.is_finite() {
        avail.min(max_w)
    } else {
        max_w
    };
    job.wrap.max_width = w.max(80.0);
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.add(egui::Label::new(egui::WidgetText::Galley(galley)).sense(sense))
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
    let resp = wrapped_label(ui, job, env.max_width, Sense::click());
    if resp.clicked() {
        // 记录点击相对块左上角的位置，激活后据此把光标放到点击处
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            env.actions.push(Action::Caret(idx, p - resp.rect.min));
        }
    }
    if resp.clicked() && ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
        if let Some(url) = first_link(inlines) {
            env.actions.push(Action::OpenLink(url));
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

fn extract_nodes(line: &str) -> Vec<(String, String)> {
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
                let close = match c[i] {
                    '[' => ']',
                    '(' => ')',
                    _ => '}',
                };
                let start = i + 1;
                let mut j = start;
                while j < c.len() && c[j] != close {
                    j += 1;
                }
                let label: String = c[start..j].iter().collect();
                out.push((id, label));
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

fn parse_flow(code: &str) -> (Vec<(String, String)>, Vec<(String, String, String)>, bool) {
    let mut nodes: Vec<(String, String)> = Vec::new();
    let mut edges: Vec<(String, String, String)> = Vec::new();
    let mut horizontal = false;
    for raw in code.lines() {
        let l = raw.trim();
        if l.starts_with("graph LR") || l.starts_with("flowchart LR") || l.starts_with("graph RL") {
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
        for (id, label) in extract_nodes(l) {
            if !nodes.iter().any(|(n, _)| n == &id) {
                nodes.push((id, label));
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
    // 每行能放几个节点由可用宽度决定：窗口变窄就自动折行，不再无限横向拉长。
    let avail = ui.available_width().max(160.0);
    let per_row = if horizontal {
        ((avail - gap) / (nw + gap)).floor().max(1.0) as usize
    } else {
        3_usize.max(1)
    };
    let rows = nodes.len().div_ceil(per_row).max(1);
    let cols = nodes.len().min(per_row);
    let w = (cols as f32) * (nw + gap) + gap;
    let h = (rows as f32) * (nh + gap) + gap;

    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w.max(160.0), h.max(60.0)), Sense::hover());
    let p = ui.painter();
    let mut pos: HashMap<String, Pos2> = HashMap::new();
    for (k, (id, _)) in nodes.iter().enumerate() {
        let (x, y) = (k % per_row, k / per_row);
        pos.insert(
            id.clone(),
            Pos2::new(
                rect.min.x + gap + (x as f32) * (nw + gap) + nw / 2.0,
                rect.min.y + gap + (y as f32) * (nh + gap) + nh / 2.0,
            ),
        );
    }
    for (a, b, label) in &edges {
        if let (Some(pa), Some(pb)) = (pos.get(a), pos.get(b)) {
            p.line_segment([*pa, *pb], Stroke::new(1.5, theme.dim));
            if !label.is_empty() {
                let mid = Pos2::new((pa.x + pb.x) / 2.0, (pa.y + pb.y) / 2.0);
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
    for (id, label) in &nodes {
        if let Some(c) = pos.get(id) {
            let r = Rect::from_center_size(*c, Vec2::new(nw, nh));
            p.rect_filled(r, 5.0, theme.code_bg);
            p.rect_stroke(
                r,
                5.0,
                Stroke::new(1.2, theme.accent),
                egui::StrokeKind::Outside,
            );
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
                        ui.label(
                            egui::RichText::new(if lang.is_empty() {
                                "code"
                            } else {
                                lang.as_str()
                            })
                            .small()
                            .color(theme.dim),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // 右上角：复制 + 语言选择（改的是代码块的 info string）
                            if ui
                                .button(egui::RichText::new("复制").small())
                                .on_hover_cursor(egui::CursorIcon::Default)
                                .clicked()
                            {
                                copied = true;
                            }
                            let mut picked = lang.clone();
                            egui::ComboBox::from_id_salt(("code_lang", idx))
                                .selected_text(if picked.is_empty() {
                                    "纯文本".to_string()
                                } else {
                                    picked.clone()
                                })
                                .width(118.0)
                                .show_ui(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .max_height(320.0)
                                        .show(ui, |ui| {
                                            for l in LANGUAGES {
                                                let label =
                                                    if l.is_empty() { "纯文本" } else { *l };
                                                if ui
                                                    .selectable_label(picked == *l, label)
                                                    .clicked()
                                                {
                                                    picked = l.to_string();
                                                }
                                            }
                                        });
                                });
                            if picked != *lang {
                                new_lang = Some(picked);
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
                        .push(Action::Notify("代码已复制到剪贴板".to_string()));
                }
                if let Some(l) = new_lang {
                    env.actions.push(Action::SetLang(idx, l));
                }
                // 只用代码文本区域做块定位，避免覆盖层挡住右上角的复制按钮。
                r.inner.interact(Sense::click())
            }
            Block::Table { head, rows, .. } => {
                let r = egui::Frame::NONE
                    .fill(theme.bg)
                    .stroke(Stroke::new(1.0, theme.border))
                    .corner_radius(4.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        // 表格随窗口浮动：列宽平分可用宽度（扣掉内边距与列间距），
                        // 窗口变宽表就变宽；窄窗口下自动收窄并让单元格内文字换行。
                        let cols = head.len().max(1);
                        let gap = ui.spacing().item_spacing.x * cols.saturating_sub(1) as f32;
                        let col_w =
                            ((ui.available_width() - gap).max(120.0) / cols as f32).max(56.0);
                        ui.set_min_width(col_w * cols as f32 + gap);
                        egui::Grid::new(("md_table", idx))
                            .min_col_width(col_w)
                            .max_col_width(col_w)
                            .show(ui, |ui| {
                                for c in head {
                                    egui::Frame::NONE
                                        .fill(theme.table_head)
                                        .inner_margin(3.0)
                                        .show(ui, |ui| {
                                            ui.set_min_width(col_w - 12.0);
                                            wrapped_label(
                                                ui,
                                                build_layout(
                                                    c,
                                                    theme,
                                                    theme.body * 0.95,
                                                    theme.heading,
                                                    true,
                                                ),
                                                col_w,
                                                Sense::hover(),
                                            );
                                        });
                                }
                                ui.end_row();
                                // 数据行只保留正文底色：表头一档颜色就够分了，
                                // 首列加深和斑马纹按需求撤掉。
                                for row in rows {
                                    for c in row {
                                        egui::Frame::NONE.fill(theme.bg).inner_margin(3.0).show(
                                            ui,
                                            |ui| {
                                                ui.set_min_width(col_w - 12.0);
                                                wrapped_label(
                                                    ui,
                                                    build_layout(
                                                        c,
                                                        theme,
                                                        theme.body * 0.95,
                                                        color,
                                                        false,
                                                    ),
                                                    col_w,
                                                    Sense::hover(),
                                                );
                                            },
                                        );
                                    }
                                    ui.end_row();
                                }
                            });
                    });
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
            Block::Diagram { code, .. } => draw_flow(ui, code, theme),
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

    /// 渲染一个代码块（含右上角复制按钮），确认控件树可正常构建、
    /// 且未点击时不会产生任何 action（避免误触发复制）。
    /// 编辑态排版必须保留原始字符，否则光标/字节偏移会和缓冲区错位。
    #[test]
    fn inline_job_keeps_raw_text() {
        let theme = Theme::get(ThemeKind::Light);
        let src = "**粗** *斜* `码` ~~删~~ [链](https://e.com) 普通文字";
        let job = inline_job(src, &theme, 16.0, theme.text, 400.0);
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
