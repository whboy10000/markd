//! 渲染层：把 AST 画成富文本 / 表格 / 代码块 / 流程图 / 图片。
//!
//! 非活动块走这里；活动块由 editor.rs 用 TextEdit 显示源码，形成 Hybrid View。

use crate::ast::*;
use crate::document::OutlineItem;
use crate::theme::Theme;
use crate::workspace;
use egui::{self, Align2, Color32, FontId, FontFamily, Pos2, Rect, Sense, Stroke, Vec2};
use egui::text::LayoutJob;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub enum Action {
    ToggleTask(usize),
    OpenLink(String),
    Jump(usize),
    Notify(String),
}

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
            let mut f = if bold { theme.bold_fmt(size, color) } else { theme.fmt(size) };
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

pub fn build_layout(inlines: &[Inline], theme: &Theme, size: f32, color: Color32, bold: bool) -> LayoutJob {
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
        ("\\times", "×"), ("\\cdot", "·"), ("\\int", "∫"), ("\\sum", "∑"),
        ("\\alpha", "α"), ("\\beta", "β"), ("\\gamma", "γ"), ("\\delta", "δ"),
        ("\\pi", "π"), ("\\infty", "∞"), ("\\sqrt", "√"), ("\\leq", "≤"),
        ("\\geq", "≥"), ("\\neq", "≠"), ("\\rightarrow", "→"), ("\\Rightarrow", "⇒"),
        ("\\in", "∈"), ("\\partial", "∂"), ("\\nabla", "∇"), ("\\approx", "≈"),
        ("\\pm", "±"), ("\\frac", ""), ("\\left", ""), ("\\right", ""),
        ("{", ""), ("}", ""), ("$$", ""),
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
    let resp = ui.add(egui::Label::new(job).sense(Sense::click()));
    if resp.clicked() && ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
        if let Some(url) = first_link(inlines) {
            env.actions.push(Action::OpenLink(url));
        }
    }
    let _ = idx;
    resp
}

fn image_response(ui: &mut egui::Ui, alt: &str, url: &str, env: &mut RenderEnv<'_>) -> egui::Response {
    if let Some(path) = workspace::resolve(env.base_dir, url) {
        if let Some(tex) = env.images.texture(env.ctx, &path) {
            let sz = tex.size_vec2();
            let max_w = ui.available_width().max(80.0);
            let scale = (max_w / sz.x.max(1.0)).min(1.0);
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(sz.x * scale, sz.y * scale), Sense::click());
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
pub fn code_job(code: &str, lang: &str, theme: &Theme) -> LayoutJob {
    const KW: &[&str] = &[
        "fn", "let", "mut", "pub", "use", "struct", "enum", "impl", "match", "if", "else", "for",
        "while", "return", "const", "static", "type", "trait", "mod", "self", "Self", "async",
        "await", "def", "class", "import", "from", "as", "in", "is", "not", "and", "or", "None",
        "True", "False", "var", "function", "export", "default", "new", "void", "int", "float",
        "double", "char", "bool", "string", "public", "private", "package", "interface", "try",
        "catch", "throw", "func", "defer", "go", "map", "range", "select", "case", "break",
        "continue", "where", "with", "do", "then", "end", "SELECT", "FROM", "WHERE",
    ];
    let hash_comment = matches!(
        lang.to_lowercase().as_str(),
        "python" | "py" | "bash" | "sh" | "shell" | "yaml" | "yml" | "toml" | "ruby" | "rb"
    );
    let mut job = LayoutJob::default();
    // 代码行常没有空格，允许在任意字符处换行以适配窄窗口。
    job.wrap.break_anywhere = true;
    for line in code.split('\n') {
        let c: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < c.len() {
            let comment = (hash_comment && c[i] == '#')
                || (!hash_comment && c[i] == '/' && i + 1 < c.len() && c[i + 1] == '/');
            if comment {
                let rest: String = c[i..].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = theme.dim;
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
                f.color = Color32::from_rgb(122, 168, 108);
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
                let is_num = tok.chars().next().map(|x| x.is_ascii_digit()).unwrap_or(false);
                let is_kw = KW.contains(&tok.as_str());
                let mut f = theme.mono_fmt();
                f.color = if is_num {
                    Color32::from_rgb(206, 148, 88)
                } else if is_kw {
                    theme.accent
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
        job.append("\n", 0.0, theme.mono_fmt());
    }
    if code.is_empty() {
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
                ui.label(egui::RichText::new(line).monospace().small().color(theme.dim));
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
            p.rect_stroke(r, 5.0, Stroke::new(1.2, theme.accent), egui::StrokeKind::Outside);
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
                let r = text_response(ui, content, theme, theme.heading_size(*level), theme.heading, true, env, idx);
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
                            let marker = if item.marker.ends_with('.') || item.marker.ends_with(')') {
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
                let r = panel_frame(theme).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(if lang.is_empty() { "code" } else { lang.as_str() })
                                .small()
                                .color(theme.dim),
                        );
                        // 右上角一键复制按钮
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(egui::RichText::new("复制").small())
                                .on_hover_cursor(egui::CursorIcon::Default)
                                .clicked()
                            {
                                copied = true;
                            }
                        });
                    });
                    ui.add_space(3.0);
                    ui.label(code_job(code, lang, theme))
                });
                if copied {
                    env.ctx.copy_text(code.clone());
                    env.actions.push(Action::Notify("代码已复制到剪贴板".to_string()));
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
                        // 列宽上限跟随可用宽度，表格不会把内容撑出窗口。
                        let cols = head.len().max(1);
                        let max_col = (ui.available_width() / cols as f32).max(60.0);
                        egui::Grid::new(("md_table", idx))
                            .striped(true)
                            .min_col_width(48.0)
                            .max_col_width(max_col)
                            .show(ui, |ui| {
                            for c in head {
                                egui::Frame::NONE
                                    .fill(theme.table_head)
                                    .inner_margin(3.0)
                                    .show(ui, |ui| {
                                        ui.label(build_layout(c, theme, theme.body * 0.95, theme.heading, true));
                                    });
                            }
                            ui.end_row();
                            for row in rows {
                                for c in row {
                                    ui.label(build_layout(c, theme, theme.body * 0.95, color, false));
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
                    [Pos2::new(r.min.x, r.center().y), Pos2::new(r.max.x, r.center().y)],
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
                        ui.label(job)
                    })
                });
                r.response.interact(Sense::click())
            }
            Block::Diagram { code, .. } => draw_flow(ui, code, theme),
            Block::FrontMatter { yaml, .. } => {
                let r = panel_frame(theme).show(ui, |ui| {
                    ui.label(egui::RichText::new("YAML Front Matter").small().color(theme.dim));
                    for line in yaml.lines() {
                        ui.label(egui::RichText::new(line).monospace().small().color(theme.quote));
                    }
                });
                r.response.interact(Sense::click())
            }
            Block::Footnote { id, content, .. } => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("[^{}]", id)).small().color(theme.accent));
                    text_response(ui, content, theme, theme.body * 0.9, theme.quote, false, env, idx)
                })
                .inner
            }
            Block::Toc { .. } => {
                let r = panel_frame(theme).show(ui, |ui| {
                    ui.label(egui::RichText::new("目录").strong().color(theme.heading));
                    for item in env.outline {
                        let indent = ((item.level.saturating_sub(1)) as f32) * 14.0;
                        let resp = ui
                            .horizontal(|ui| {
                                ui.add_space(indent);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(format!("• {}", item.title)).color(theme.link),
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
                let (_, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 9.0), Sense::click());
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
            };
            let _ = render_block(ui, 0, &block, &mut env, false);
            assert!(actions.is_empty(), "未点击时不应产生 action");
        });
    }
}
