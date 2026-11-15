//! Hybrid View 编辑器：活动块显示源码，其余块实时渲染成富文本。

use crate::ast::Block;
use crate::document::{Document, OutlineItem};
use crate::render::{self, Action, ImageCache, RenderEnv};
use crate::theme::Theme;
use egui::{self, Id, Key, TextEdit};
use std::path::Path;

#[derive(Default)]
pub struct EditorState {
    pub active: usize,
    pub focus_req: Option<usize>,
    pub scroll_req: Option<usize>,
    pub last_cursor: usize,
    pub sel: Option<(usize, usize)>,
    /// 终端持有键盘焦点时置 true：编辑区不渲染可编辑控件，避免把焦点抢回来。
    pub read_only: bool,
    /// 本帧是否点击了某个块（app 用它把焦点从终端交还给编辑区）。
    pub clicked: bool,
    /// 正在原地编辑的表格草稿：(块下标, 单元格文本, 对齐)。
    pub table_draft: Option<(usize, Vec<Vec<String>>, Vec<crate::ast::Align>)>,
    /// 点击进入编辑时，光标应落在的位置（相对块左上角）。
    pub pending_caret: Option<egui::Vec2>,
}

pub enum EditorEvent {
    None,
    Changed,
}

fn split_at_char(s: &str, pos: usize) -> (String, String) {
    let mut a = String::new();
    let mut b = String::new();
    for (i, c) in s.chars().enumerate() {
        if i < pos {
            a.push(c);
        } else {
            b.push(c);
        }
    }
    (a, b)
}

fn insert_at_char(s: &str, pos: usize, ins: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i == pos {
            out.push_str(ins);
        }
        out.push(c);
    }
    if pos >= s.chars().count() {
        out.push_str(ins);
    }
    out
}

fn inherited_marker(s: &str) -> String {
    let indent: String = s.chars().take_while(|c| *c == ' ').collect();
    let rest = &s[indent.len()..];
    let b = rest.as_bytes();
    if b.is_empty() {
        return String::new();
    }
    if (b[0] == b'-' || b[0] == b'*' || b[0] == b'+') && b.get(1) == Some(&b' ') {
        let after = &rest[2..];
        if after.starts_with("[ ] ") || after.starts_with("[x] ") || after.starts_with("[X] ") {
            return format!("{}- [ ] ", indent);
        }
        return format!("{}- ", indent);
    }
    let mut k = 0usize;
    while k < b.len() && b[k].is_ascii_digit() {
        k += 1;
    }
    if k > 0 && k + 1 < b.len() && (b[k] == b'.' || b[k] == b')') && b[k + 1] == b' ' {
        return format!("{}1. ", indent);
    }
    String::new()
}

fn pair_close(t: &str) -> Option<&'static str> {
    match t {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        "\"" => Some("\""),
        "'" => Some("'"),
        _ => None,
    }
}

/// 在活动块后插入一个模板块。
pub fn insert_template(doc: &mut Document, state: &mut EditorState, template: &str) {
    doc.record();
    let idx = state.active;
    doc.insert_block_after(idx, template);
    state.active = idx + 1;
    state.focus_req = Some(state.active);
    state.scroll_req = Some(state.active);
    state.last_cursor = template.chars().count();
}

/// 给活动块设置/替换行首前缀（标题、引用、列表等）。
pub fn apply_prefix(doc: &mut Document, state: &mut EditorState, prefix: &str) {
    let idx = state.active;
    let Some(b) = doc.blocks.get(idx) else { return; };
    let src = b.source().to_string();
    let cleaned = src
        .trim_start_matches('#')
        .trim_start_matches('>')
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ')
        .to_string();
    doc.record();
    doc.edit_block(idx, &format!("{}{}", prefix, cleaned));
    state.last_cursor = (prefix.chars().count() + cleaned.chars().count()).min(usize::MAX);
}

/// 用标记包裹当前选区（无选区时包裹光标所在位置）。
pub fn wrap(doc: &mut Document, state: &mut EditorState, open: &str, close: &str) {
    let idx = state.active;
    let Some(b) = doc.blocks.get(idx) else { return; };
    let src = b.source().to_string();
    let len = src.chars().count();
    let (a, z) = state.sel.unwrap_or_else(|| {
        let p = state.last_cursor.min(len);
        (p, p)
    });
    let a = a.min(len);
    let z = z.min(len);
    let mut out = insert_at_char(&src, a, open);
    out = insert_at_char(&out, z + open.chars().count(), close);
    doc.record();
    doc.edit_block(idx, &out);
}

pub fn toggle_task(doc: &mut Document, idx: usize) {
    let Some(b) = doc.blocks.get(idx) else { return; };
    let s = b.source().to_string();
    let new = if s.contains("[ ]") {
        s.replacen("[ ]", "[x]", 1)
    } else {
        s.replacen("[x]", "[ ]", 1).replacen("[X]", "[ ]", 1)
    };
    if new != s {
        doc.record();
        doc.edit_block(idx, &new);
    }
}

/// 由单元格内容还原出 Markdown 表格源码。
fn table_source(grid: &[Vec<String>], align: &[crate::ast::Align]) -> String {
    use crate::ast::Align;
    let cols = grid.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut s = String::new();
    let row = |cells: &Vec<String>| {
        let mut line = String::from("|");
        for c in 0..cols {
            line.push(' ');
            line.push_str(cells.get(c).map(|x| x.as_str()).unwrap_or(""));
            line.push_str(" |");
        }
        line
    };
    if let Some(head) = grid.first() {
        s.push_str(&row(head));
        s.push('\n');
    }
    s.push('|');
    for c in 0..cols {
        s.push_str(match align.get(c).copied().unwrap_or(Align::Left) {
            Align::Left => " :--- |",
            Align::Center => " :---: |",
            Align::Right => " ---: |",
        });
    }
    for r in grid.iter().skip(1) {
        s.push('\n');
        s.push_str(&row(r));
    }
    s
}

/// 活动块的「编辑形态」：标题只编辑文字、代码块只编辑代码（不含围栏），
/// 像 Typora 那样直接在渲染结果上改，而不是切到 Markdown 源码。
#[derive(Clone)]
enum EditKind {
    Source,
    Heading(u8),
    Code(String),
}

impl EditKind {
    fn of(b: &Block) -> Self {
        match b {
            Block::Heading { level, .. } => EditKind::Heading(*level),
            Block::Code { lang, .. } => EditKind::Code(lang.clone()),
            _ => EditKind::Source,
        }
    }

    /// 把编辑文本还原成 Markdown 源码。
    fn rebuild(&self, text: &str) -> String {
        match self {
            EditKind::Source => text.to_string(),
            EditKind::Heading(level) => {
                format!("{} {}", "#".repeat((*level).max(1) as usize), text.trim_end())
            }
            EditKind::Code(lang) => {
                let mut s = String::with_capacity(text.len() + lang.len() + 8);
                s.push_str("```");
                s.push_str(lang);
                s.push('\n');
                s.push_str(text);
                if !text.ends_with('\n') {
                    s.push('\n');
                }
                s.push_str("```");
                s
            }
        }
    }
}

#[allow(clippy::too_many_arguments, deprecated)]
pub fn show_editor(
    ui: &mut egui::Ui,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    ctx: &egui::Context,
    images: &mut ImageCache,
    base_dir: Option<&Path>,
    outline: &[OutlineItem],
    source_mode: bool,
    focus_mode: bool,
    typewriter: bool,
    actions: &mut Vec<Action>,
) -> EditorEvent {
    // ---------------- 源码模式：整篇文档一个多行编辑器 ----------------
    if source_mode {
        let mut src = doc.source();
        let out = TextEdit::multiline(&mut src)
            .id(Id::new("source_editor"))
            .font(egui::TextStyle::Monospace)
            .desired_width(ui.available_width())
            .frame(egui::Frame::NONE)
            .code_editor()
            .show(ui);
        if out.response.changed() {
            doc.maybe_record();
            doc.set_source(&src);
            return EditorEvent::Changed;
        }
        return EditorEvent::None;
    }

    let active = state.active.min(doc.blocks.len().saturating_sub(1));
    state.active = active;
    let active_multiline = doc.blocks.get(active).map(|b| b.is_multiline()).unwrap_or(false);

    let key_state = ctx.input(|i| {
        (
            i.key_pressed(Key::Enter) && !i.modifiers.command && !i.modifiers.ctrl,
            i.key_pressed(Key::Backspace),
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::Tab),
            i.modifiers.shift,
        )
    });
    let (enter, backspace, up, down, tab, shift) = key_state;
    let typed_pair: Option<String> = ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Text(t) => pair_close(t).map(|s| s.to_string()),
            _ => None,
        })
    });

    // 只维护「活动块」的可编辑缓冲；其它块直接借用 doc.blocks。
    // 之前每帧把所有块的源码 clone 一遍，10 万字时要分配上千个 String。
    let edit_kind = doc.blocks.get(active).map(EditKind::of).unwrap_or(EditKind::Source);
    let mut active_buf = match doc.blocks.get(active) {
        Some(b) if !matches!(edit_kind, EditKind::Source) => b.plain(),
        Some(b) => b.source().to_string(),
        None => String::new(),
    };
    let mut changed = false;
    let mut new_active: Option<usize> = None;
    let mut active_rect: Option<egui::Rect> = None;
    let mut cursor_idx = active_buf.chars().count();
    let mut selection: Option<(usize, usize)> = None;
    let mut pair_insert: Option<(usize, String)> = None;
    let mut table_edit_src: Option<String> = None;

    egui::ScrollArea::vertical()
        .id_salt("editor_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let blocks = &doc.blocks;
            // 正文宽度随窗口浮动，固定占可用宽度的约 90%；不设死宽度，
            // 所有块都按这个宽度换行，窄窗口下也不会把长段落横向裁掉。
            let outer_w = ui.available_width();
            let page_w = (outer_w * 0.9).max(200.0);
            ui.horizontal_top(|ui| {
                let pad = ((outer_w - page_w) / 2.0).max(0.0);
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(page_w);
                    for idx in 0..blocks.len() {
                        let block = &blocks[idx];
                        let compact = matches!(block, Block::ListItem { .. } | Block::Blank { .. });
                        let is_active = idx == active;
                        let editing = is_active && !state.read_only;

                        let resp = if editing && matches!(block, Block::Table { .. }) {
                            // 表格：像 Typora 一样直接在表格单元格里编辑，不看管道源码。
                            if let Block::Table { head, rows, align, .. } = block {
                                let need = !matches!(&state.table_draft, Some((i, _, _)) if *i == idx);
                                if need {
                                    let mut grid: Vec<Vec<String>> = Vec::new();
                                    grid.push(head.iter().map(|c| crate::ast::inline_plain(c)).collect());
                                    for r in rows.iter() {
                                        grid.push(r.iter().map(|c| crate::ast::inline_plain(c)).collect());
                                    }
                                    state.table_draft = Some((idx, grid, align.clone()));
                                }
                            }
                            let mut edited = false;
                            let frame = egui::Frame::NONE
                                .fill(theme.bg)
                                .stroke(egui::Stroke::new(1.0, theme.border))
                                .corner_radius(4.0)
                                .inner_margin(6.0);
                            let inner = frame.show(ui, |ui| {
                                if let Some((_, grid, _)) = state.table_draft.as_mut() {
                                    egui::Grid::new(("table_edit", idx))
                                        .striped(true)
                                        .show(ui, |ui| {
                                            for row in grid.iter_mut() {
                                                for cell in row.iter_mut() {
                                                    let r = ui.add(
                                                        egui::TextEdit::singleline(cell)
                                                            .desired_width(140.0)
                                                            .frame(egui::Frame::NONE),
                                                    );
                                                    if r.changed() {
                                                        edited = true;
                                                    }
                                                }
                                                ui.end_row();
                                            }
                                        });
                                }
                            });
                            if edited {
                                if let Some((_, grid, aligns)) = state.table_draft.as_ref() {
                                    table_edit_src = Some(table_source(grid, aligns));
                                }
                                changed = true;
                            }
                            inner.response.interact(egui::Sense::click())
                        } else if editing {
                            let id = Id::new(("block_edit", idx));
                            // 标题只编辑文字、代码块只编辑代码（不含围栏），像 Typora 一样直接改。
                            let rows = if matches!(edit_kind, EditKind::Code(_)) {
                                3
                            } else if block.is_multiline() {
                                2
                            } else {
                                1
                            };
                            // 编辑态行内排版：缓冲区还是原始 Markdown，但标记被隐藏，
                            // 看起来就是渲染后的富文本，而不是源码。
                            let mut layouter =
                                |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                                    let job = render::inline_job(
                                        buf.as_str(),
                                        theme,
                                        theme.body,
                                        theme.text,
                                        wrap,
                                    );
                                    ui.fonts_mut(|f| f.layout_job(job))
                                };
                            let mut te = TextEdit::multiline(&mut active_buf)
                                .id(id)
                                .desired_rows(rows)
                                .desired_width(page_w);
                            te = match &edit_kind {
                                EditKind::Heading(level) => te
                                    .font(egui::FontId::new(
                                        theme.heading_size(*level),
                                        egui::FontFamily::Proportional,
                                    ))
                                    .text_color(theme.heading)
                                    .frame(egui::Frame::NONE)
                                    .return_key(Option::<egui::KeyboardShortcut>::None),
                                EditKind::Code(_) => te
                                    .font(egui::FontId::new(theme.mono, egui::FontFamily::Monospace))
                                    .text_color(theme.text)
                                    .frame(
                                        egui::Frame::NONE
                                            .fill(theme.code_bg)
                                            .inner_margin(12.0)
                                            .corner_radius(6.0),
                                    ),
                                EditKind::Source => te
                                    .font(egui::FontId::new(
                                        theme.body,
                                        egui::FontFamily::Proportional,
                                    ))
                                    .frame(egui::Frame::NONE)
                                    .return_key(Option::<egui::KeyboardShortcut>::None)
                                    .layouter(&mut layouter),
                            };
                            let out = te.show(ui);
                            // 点击进入编辑：把光标放到刚才点击的位置，而不是跳到末尾/居中
                            if let Some(off) = state.pending_caret.take() {
                                let ccursor = out.galley.cursor_from_pos(off);
                                let mut ts = out.state.clone();
                                ts.cursor.set_char_range(Some(
                                    egui::text::CCursorRange::one(ccursor),
                                ));
                                ts.store(ui.ctx(), id);
                                cursor_idx =
                                    ccursor.index.0.min(active_buf.chars().count());
                            }
                            let cr = out.cursor_range;
                            let resp = out.response;
                            if resp.changed() {
                                changed = true;
                            }
                            let len = active_buf.chars().count();
                            if let Some(r) = cr {
                                let a = r.primary.index.0;
                                let b = r.secondary.index.0;
                                cursor_idx = a.min(len);
                                selection = if a == b {
                                    None
                                } else {
                                    Some((a.min(b).min(len), a.max(b).min(len)))
                                };
                            }
                            if let Some(f) = state.focus_req {
                                if f == idx {
                                    resp.request_focus();
                                }
                            }
                            resp.response
                        } else {
                            let mut env = RenderEnv {
                                theme,
                                ctx,
                                images,
                                base_dir,
                                outline,
                                actions,
                                max_width: page_w,
                            };
                            render::render_block(ui, idx, block, &mut env, focus_mode)
                        };
                        if resp.clicked() {
                            new_active = Some(idx);
                            state.clicked = true;
                        }
                        if is_active {
                            active_rect = Some(resp.rect);
                            if !editing {
                                state.sel = None;
                            }
                        }
                        ui.add_space(if compact { 1.0 } else { 6.0 });
                    }
                });
            });
            // 输入时让活动块始终留在视口内：换行把内容顶下去时视图跟着走。
            if !state.read_only {
                if let Some(r) = active_rect {
                    if !ui.clip_rect().contains_rect(r) {
                        ui.scroll_to_rect(r, None);
                    }
                }
            }
            // 打字机模式只在光标移动时居中。以前每帧都强制 scroll_to_rect，
            // 会把用户用滚轮翻到的位置立刻拉回去，表现就是「滚轮滚不动」。
            if typewriter && !state.read_only && cursor_idx != state.last_cursor {
                if let Some(r) = active_rect {
                    ui.scroll_to_rect(r, Some(egui::Align::Center));
                }
            }
            // 大纲 / TOC / 新建块等显式跳转请求。必须 take()：
            // 之前只写不读，所以点大纲标题完全不会滚动。
            if state.scroll_req.take().is_some() {
                if let Some(r) = active_rect {
                    ui.scroll_to_rect(r, Some(egui::Align::TOP));
                }
            }
        });

    if !state.read_only {
        state.last_cursor = cursor_idx;
        state.sel = selection;
    }
    state.focus_req = None;

    // ---------------- 自动配对（括号与引号） ----------------
    if changed {
        if let Some(close) = &typed_pair {
            let pos = cursor_idx.min(active_buf.chars().count());
            pair_insert = Some((pos, close.clone()));
        }
    }
    if let Some((pos, close)) = pair_insert {
        active_buf = insert_at_char(&active_buf, pos, &close);
        changed = true;
    }

    // ---------------- 普通输入写回 ----------------
    if changed {
        doc.maybe_record();
        let src = table_edit_src
            .clone()
            .unwrap_or_else(|| edit_kind.rebuild(&active_buf));
        doc.edit_block(active, &src);
        return EditorEvent::Changed;
    }

    // ---------------- 结构性按键 ----------------
    if let Some(i) = new_active {
        if i != active {
            state.active = i;
            state.focus_req = Some(i);
            state.last_cursor = 0;
            ctx.request_repaint();
        }
    }

    if enter && !active_multiline {
        let buf = active_buf.clone();
        let pos = state.last_cursor.min(buf.chars().count());
        let (left, right) = split_at_char(&buf, pos);
        let marker = if matches!(edit_kind, EditKind::Source) {
            inherited_marker(&buf)
        } else {
            String::new()
        };
        doc.record();
        doc.edit_block(active, &edit_kind.rebuild(&left));
        doc.insert_block_after(active, &format!("{}{}", marker, right));
        state.active = active + 1;
        state.focus_req = Some(state.active);
        state.scroll_req = Some(state.active);
        state.last_cursor = marker.chars().count();
        return EditorEvent::Changed;
    }

    if backspace && state.last_cursor == 0 && active > 0 {
        let cur = active_buf.clone();
        let mut prev = doc
            .blocks
            .get(active - 1)
            .map(|b| b.source().to_string())
            .unwrap_or_default();
        prev.push_str(&cur);
        doc.record();
        doc.edit_block(active - 1, &prev);
        doc.remove_block(active);
        state.active = active - 1;
        state.focus_req = Some(state.active);
        state.last_cursor = prev.chars().count().saturating_sub(cur.chars().count());
        return EditorEvent::Changed;
    }

    if up && state.last_cursor == 0 && active > 0 {
        state.active = active - 1;
        state.focus_req = Some(state.active);
        state.last_cursor = 0;
        ctx.request_repaint();
    }
    if down && state.active + 1 < doc.blocks.len() {
        state.active = active + 1;
        state.focus_req = Some(state.active);
        state.last_cursor = 0;
        ctx.request_repaint();
    }
    if tab {
        let cur = active_buf.clone();
        let updated = if shift {
            cur.strip_prefix("  ").unwrap_or(&cur).to_string()
        } else {
            format!("  {}", cur)
        };
        if updated != cur {
            doc.record();
            doc.edit_block(active, &updated);
            return EditorEvent::Changed;
        }
    }

    EditorEvent::None
}


#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: f32, f: impl FnMut(&mut egui::Ui)) {
        frame_sized(width, 900.0, f);
    }

    fn frame_sized(width: f32, height: f32, mut f: impl FnMut(&mut egui::Ui)) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| f(ui));
        });
        // 无头环境下没有渲染器消费纹理增删，必须显式清掉，否则析构会 panic。
        out.textures_delta.clear();
    }

    /// 回归：编辑区正文（段落 / 活动块 / 引用）都必须按窗口宽度软换行。
    #[test]
    fn long_paragraph_wraps_in_editor_column() {
        let text = "word ".repeat(300);
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut label_size = egui::Vec2::ZERO;
        let mut edit_size = egui::Vec2::ZERO;
        let mut quote_size = egui::Vec2::ZERO;
        frame(700.0, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("t")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let outer_w = ui.available_width();
                    let page_w = (outer_w * 0.9).max(200.0);
                    ui.horizontal_top(|ui| {
                        let pad = ((outer_w - page_w) / 2.0).max(0.0);
                        ui.add_space(pad);
                        ui.vertical(|ui| {
                            ui.set_width(page_w);
                            label_size = ui.add(egui::Label::new(text.clone())).rect.size();
                            let mut buf = text.clone();
                            let out = TextEdit::multiline(&mut buf)
                                .desired_rows(1)
                                .desired_width(page_w)
                                .frame(egui::Frame::NONE)
                                .return_key(Option::<egui::KeyboardShortcut>::None)
                                .show(ui);
                            edit_size = out.response.response.rect.size();

                            // 引用块内部是横向布局，最容易退化成不换行。
                            let quote = Block::Quote {
                                level: 1,
                                content: vec![crate::ast::Inline::Text(text.clone())],
                                source: text.clone(),
                            };
                            let ctx_q = ui.ctx().clone();
                            let mut env = RenderEnv {
                                theme: &theme,
                                ctx: &ctx_q,
                                images: &mut images,
                                base_dir: None,
                                outline: &[],
                                actions: &mut actions,
                                max_width: page_w,
                            };
                            quote_size =
                                render::render_block(ui, 0, &quote, &mut env, false).rect.size();
                        });
                    });
                });
        });
        assert!(label_size.y > 40.0, "Label 没有换行：size={label_size:?}");
        assert!(label_size.x <= 700.0, "Label 超出窗口：size={label_size:?}");
        assert!(edit_size.y > 40.0, "TextEdit 没有换行：size={edit_size:?}");
        assert!(quote_size.y > 40.0, "引用块没有换行：size={quote_size:?}");
        assert!(quote_size.x <= 700.0, "引用块超出窗口：size={quote_size:?}");
    }

    /// 原地编辑：标题只编辑文字、代码块只编辑代码，写回时再补回 Markdown 标记。
    #[test]
    fn edit_kind_rebuilds_markdown() {
        assert_eq!(EditKind::Heading(2).rebuild("标题"), "## 标题");

        let src = EditKind::Code("rust".to_string()).rebuild("fn main() {}");
        assert_eq!(src, "```rust\nfn main() {}\n```");
        let blocks = crate::ast::parse(&src);
        assert!(
            matches!(&blocks[0], Block::Code { lang, code, .. } if lang == "rust" && code == "fn main() {}"),
            "代码块写回后应能还原成同一段代码"
        );

        // 多行代码同样可往返
        let src = EditKind::Code(String::new()).rebuild("a\nb");
        let blocks = crate::ast::parse(&src);
        assert!(matches!(&blocks[0], Block::Code { code, .. } if code == "a\nb"));
    }

    /// 大纲跳转：点击标题后应把该块设为活动块，并消费滚动请求
    /// （之前 scroll_req 只写不读，所以点大纲完全不会滚动）。
    #[test]
    fn outline_jump_sets_active_block_and_consumes_scroll_request() {
        let mut text = String::new();
        for i in 0..40 {
            text.push_str(&format!("段落 {i}\n\n"));
        }
        text.push_str("# 目标标题\n\n结尾段落\n");

        let mut doc = Document::new(&text, None);
        let outline = doc.outline();
        let target = outline.last().expect("应有标题").block;
        assert!(target > 30, "标题应位于文档后部，实际 block={target}");

        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut state = EditorState::default();
        state.active = target;
        state.scroll_req = Some(target);

        frame(800.0, |ui| {
            let ctx = ui.ctx().clone();
            let _ = show_editor(
                ui,
                &mut doc,
                &mut state,
                &theme,
                &ctx,
                &mut images,
                None,
                &outline,
                false,
                false,
                false,
                &mut actions,
            );
        });

        assert_eq!(state.active, target);
        assert!(
            state.scroll_req.is_none(),
            "滚动请求应被 show_editor 消费，否则点大纲不会滚动"
        );
    }

    /// 10 万字文档的帧率验证：模拟滚动到不同位置，测量每帧耗时。
    #[test]
    fn bench_100k_doc_scroll() {
        use std::time::Instant;
        let para = "这是一段用于性能测试的正文，包含 **加粗**、*斜体*、`行内代码` 和 [链接](https://example.com) 以及中文标点。".repeat(3);
        let mut text = String::new();
        for i in 0..470 {
            if i % 20 == 0 {
                text.push_str(&format!("## 标题 {i}\n\n"));
            }
            text.push_str(&para);
            text.push_str("\n\n");
        }
        println!("[100k] chars = {}", text.chars().count());

        let mut doc = Document::new(&text, None);
        let t = Instant::now();
        let _ = crate::ast::parse(&text);
        println!("[100k] parse = {:?}", t.elapsed());

        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut state = EditorState::default();
        let outline = doc.outline();
        let total = doc.blocks.len();
        println!("[100k] blocks = {total}");

        // 模拟滚动：每帧跳到文档的不同位置（会强制渲染一批全新的块）。
        let mut worst = std::time::Duration::ZERO;
        let mut sum = std::time::Duration::ZERO;
        let mut n = 0;
        for frac in 0..20 {
            let target = (total.saturating_sub(1) * frac) / 20;
            state.active = target;
            state.scroll_req = Some(target);
            let t = Instant::now();
            frame(1200.0, |ui| {
                let ctx = ui.ctx().clone();
                let _ = show_editor(
                    ui, &mut doc, &mut state, &theme, &ctx, &mut images, None, &outline,
                    false, false, false, &mut actions,
                );
            });
            let e = t.elapsed();
            sum += e;
            worst = worst.max(e);
            n += 1;
            if frac % 5 == 0 {
                println!("[100k] jump to block {target}/{total} = {e:?}");
            }
        }
        println!("[100k] avg frame = {:?}, worst = {:?}", sum / n, worst);

        // 连续输入：每帧改一个字，测编辑回写耗时
        let t = Instant::now();
        for k in 0..20 {
            let src = format!("连续输入测试 {k}");
            doc.edit_block(5, &src);
        }
        println!("[100k] 20 次 edit_block = {:?}", t.elapsed());
    }

    /// 性能基准（打印耗时）：10 万字级别的文档，看各环节开销。
    #[test]
    fn bench_large_doc() {
        use std::time::Instant;
        let para = "这是一段用于性能测试的正文，包含 **加粗**、*斜体*、`行内代码` 和 [链接](https://example.com) 以及中文标点。".repeat(3);
        let mut text = String::new();
        for i in 0..300 {
            if i % 20 == 0 {
                text.push_str(&format!("## 标题 {i}\n\n"));
            }
            text.push_str(&para);
            text.push_str("\n\n");
        }
        println!("[bench] chars = {}", text.chars().count());

        let t = Instant::now();
        let blocks = crate::ast::parse(&text);
        println!("[bench] parse = {:?}, blocks = {}", t.elapsed(), blocks.len());

        let mut doc = Document::new(&text, None);
        let t = Instant::now();
        let _ = doc.stats();
        println!("[bench] stats = {:?}", t.elapsed());
        let t = Instant::now();
        let _ = doc.outline();
        println!("[bench] outline = {:?}", t.elapsed());

        let t = Instant::now();
        doc.edit_block(5, "改了一下的段落");
        println!("[bench] edit_block = {:?}", t.elapsed());

        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut state = EditorState::default();
        let outline = doc.outline();
        for i in 0..4 {
            let t = Instant::now();
            frame(1000.0, |ui| {
                let ctx = ui.ctx().clone();
                let _ = show_editor(
                    ui, &mut doc, &mut state, &theme, &ctx, &mut images, None, &outline,
                    false, false, false, &mut actions,
                );
            });
            println!("[bench] editor frame {i} = {:?}", t.elapsed());
        }
        // 视口只有 300pt 时，只要渲染少数几个块，用来对比每个块的成本。
        for i in 0..3 {
            let t = Instant::now();
            frame_sized(1000.0, 300.0, |ui| {
                let ctx = ui.ctx().clone();
                let _ = show_editor(
                    ui, &mut doc, &mut state, &theme, &ctx, &mut images, None, &outline,
                    false, false, false, &mut actions,
                );
            });
            println!("[bench] small-viewport frame {i} = {:?}", t.elapsed());
        }
    }
}

