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

    let mut bufs: Vec<String> = doc.blocks.iter().map(|b| b.source().to_string()).collect();
    let mut changed = false;
    let mut new_active: Option<usize> = None;
    let mut active_rect: Option<egui::Rect> = None;
    let mut cursor_idx = bufs.get(active).map(|s| s.chars().count()).unwrap_or(0);
    let mut selection: Option<(usize, usize)> = None;
    let mut pair_insert: Option<(usize, String)> = None;

    egui::ScrollArea::vertical()
        .id_salt("editor_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // 正文宽度随窗口浮动，固定占可用宽度的约 90%；不设死宽度，
            // 所有块都按这个宽度换行，窄窗口下也不会把长段落横向裁掉。
            let outer_w = ui.available_width();
            let page_w = (outer_w * 0.9).max(200.0);
            ui.horizontal_top(|ui| {
                let pad = ((outer_w - page_w) / 2.0).max(0.0);
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(page_w);
                    for idx in 0..bufs.len() {
                        let is_active = idx == active;
                        let block = doc.blocks.get(idx).cloned().unwrap_or(Block::Blank { source: String::new() });
                        let resp = if is_active {
                            let id = Id::new(("block_edit", idx));
                            // 全部用 multiline：它会按 desired_width 自动换行并随内容增高。
                            // singleline 会把长段落横向裁掉，逼用户放大窗口才看得见。
                            let te = TextEdit::multiline(&mut bufs[idx])
                                .desired_rows(if block.is_multiline() { 2 } else { 1 });
                            let out = te
                                .id(id)
                                .font(egui::TextStyle::Body)
                                .desired_width(page_w)
                                .frame(egui::Frame::NONE)
                                .return_key(Option::<egui::KeyboardShortcut>::None)
                                .show(ui);
                            let cr = out.cursor_range;
                            let resp = out.response;
                            if resp.changed() {
                                changed = true;
                            }
                            let len = bufs[idx].chars().count();
                            if let Some(r) = cr {
                                let a = r.primary.index.0;
                                let b = r.secondary.index.0;
                                cursor_idx = a.min(len);
                                selection = if a == b { None } else { Some((a.min(b).min(len), a.max(b).min(len))) };
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
                            };
                            render::render_block(ui, idx, &block, &mut env, focus_mode)
                        };
                        if resp.clicked() {
                            new_active = Some(idx);
                        }
                        if is_active {
                            active_rect = Some(resp.rect);
                        }
                        let compact = matches!(block, Block::ListItem { .. } | Block::Blank { .. });
                        ui.add_space(if compact { 1.0 } else { 6.0 });
                    }
                });
            });
            if typewriter {
                if let Some(r) = active_rect {
                    ui.scroll_to_rect(r, Some(egui::Align::Center));
                }
            }
        });

    state.last_cursor = cursor_idx;
    state.sel = selection;
    state.focus_req = None;

    // ---------------- 自动配对（括号与引号） ----------------
    if changed {
        if let Some(close) = &typed_pair {
            let pos = cursor_idx.min(bufs[active].chars().count());
            pair_insert = Some((pos, close.clone()));
        }
    }
    if let Some((pos, close)) = pair_insert {
        bufs[active] = insert_at_char(&bufs[active], pos, &close);
        changed = true;
    }

    // ---------------- 普通输入写回 ----------------
    if changed {
        doc.maybe_record();
        doc.edit_block(active, &bufs[active]);
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
        let buf = bufs.get(active).cloned().unwrap_or_default();
        let pos = state.last_cursor.min(buf.chars().count());
        let (left, right) = split_at_char(&buf, pos);
        let marker = inherited_marker(&buf);
        doc.record();
        doc.edit_block(active, &left);
        doc.insert_block_after(active, &format!("{}{}", marker, right));
        state.active = active + 1;
        state.focus_req = Some(state.active);
        state.scroll_req = Some(state.active);
        state.last_cursor = marker.chars().count();
        return EditorEvent::Changed;
    }

    if backspace && state.last_cursor == 0 && active > 0 {
        let cur = bufs.get(active).cloned().unwrap_or_default();
        let mut prev = bufs.get(active - 1).cloned().unwrap_or_default();
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
        let cur = bufs.get(active).cloned().unwrap_or_default();
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
