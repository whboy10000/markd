//! 文档内查找 / 替换（弹窗），支持区分大小写与正则。
//!
//! 命中位置用「块下标 + 块可编辑文本里的字节区间」表示：块是渲染与编辑的最小单位，
//! 替换也只改命中的那个块（标题补 #、代码块补围栏），不会打乱其它块的解析。
//!
//! 快捷键：Cmd/Ctrl+F 打开查找，Ctrl+H（macOS 为 Ctrl+H / Cmd+Option+F）打开替换。

use crate::document::Document;
use crate::editor;
use crate::theme::Theme;
use egui::{self, RichText};
use regex::{NoExpand, Regex};

/// 命中上限：超大文档里「的」这类字符能匹配上万次，超出后不再收集，
/// 计数显示为上限值，避免弹窗一打开就卡住几秒。
const MAX_HITS: usize = 2000;

/// 单条命中：块下标 + 该块可编辑文本里的字节区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub block: usize,
    pub start: usize,
    pub end: usize,
}

/// 一次弹窗交互的结果，交给 app 去滚动/选中/刷新。
#[derive(Default)]
pub struct FindResult {
    /// 需要跳转并选中某条命中。
    pub jump: Option<Hit>,
    /// 文档被替换改过，app 需要刷新大纲和统计。
    pub changed: bool,
}

pub struct FindPanel {
    pub open: bool,
    /// 是否显示「替换为」一行（Ctrl+H 打开时为 true）。
    pub replace_row: bool,
    pub query: String,
    pub replacement: String,
    /// 查询按正则解释（否则按字面量）。
    pub regex: bool,
    /// 区分大小写。
    pub case: bool,
    pub hits: Vec<Hit>,
    /// 当前命中下标。
    pub current: usize,
    /// 计算命中时文档的修订号；文档一改就重算。
    rev: u64,
    /// 查询条件变了，本帧必须重算。
    stale: bool,
    /// 正则语法错误提示。
    pub error: Option<String>,
    /// 打开面板/切模式时把焦点放进查询框。
    focus: bool,
    /// 「已替换 N 处」这类临时提示。
    pub info: String,
}

impl Default for FindPanel {
    fn default() -> Self {
        Self {
            open: false,
            replace_row: false,
            query: String::new(),
            replacement: String::new(),
            regex: false,
            case: false,
            hits: Vec::new(),
            current: 0,
            rev: 0,
            stale: true,
            error: None,
            focus: false,
            info: String::new(),
        }
    }
}

impl FindPanel {
    /// 打开面板；`replace` = true 时同时显示替换行（Ctrl+H）。
    pub fn open_for(&mut self, replace: bool) {
        self.open = true;
        self.replace_row = replace;
        self.stale = true;
        self.focus = true;
        self.info.clear();
    }

    /// 真正用于匹配的正则：非正则模式把查询当字面量，不区分大小写时加 (?i)。
    fn build(&self) -> Result<Option<Regex>, String> {
        if self.query.is_empty() {
            return Ok(None);
        }
        let mine = if self.regex {
            // (?m) 让 ^ / $ 匹配每一行，代码块里的多行文本才好用
            format!("(?m){}", self.query)
        } else {
            regex::escape(&self.query)
        };
        let pat = if self.case {
            mine
        } else {
            format!("(?i){}", mine)
        };
        Regex::new(&pat).map(Some).map_err(|e| e.to_string())
    }

    /// 文档或查询条件变化后重算命中。空查询 / 非法正则直接清空。
    fn sync(&mut self, doc: &Document) {
        if !self.stale && self.rev == doc.rev {
            return;
        }
        self.stale = false;
        self.rev = doc.rev;
        self.hits.clear();
        self.error = None;
        let re = match self.build() {
            Ok(Some(re)) => re,
            Ok(None) => {
                self.current = 0;
                return;
            }
            Err(e) => {
                self.error = Some(e);
                self.current = 0;
                return;
            }
        };
        'outer: for (i, b) in doc.blocks.iter().enumerate() {
            let text = editor::block_buffer(b);
            for m in re.find_iter(&text) {
                if m.start() == m.end() {
                    continue; // 空匹配没有可替换的内容
                }
                self.hits.push(Hit {
                    block: i,
                    start: m.start(),
                    end: m.end(),
                });
                if self.hits.len() >= MAX_HITS {
                    break 'outer;
                }
            }
        }
        if self.current >= self.hits.len() {
            self.current = 0;
        }
    }

    /// 把命中的字节区间换成该块缓冲里的字符下标，供编辑器选中文字。
    pub fn hit_chars(&self, doc: &Document, hit: Hit) -> Option<(usize, usize)> {
        let b = doc.blocks.get(hit.block)?;
        let text = editor::block_buffer(b);
        let end = hit.end.min(text.len());
        let start = hit.start.min(end);
        Some((text[..start].chars().count(), text[..end].chars().count()))
    }

    /// 前后跳一条（会绕回另一端）。
    fn step(&mut self, delta: isize) -> Option<Hit> {
        if self.hits.is_empty() {
            return None;
        }
        let n = self.hits.len() as isize;
        self.current = (self.current as isize + delta).rem_euclid(n) as usize;
        self.hits.get(self.current).copied()
    }

    /// 替换当前命中：正则模式按 $1/${name} 展开，字面量模式原样插入。
    fn replace_current(&mut self, doc: &mut Document) -> bool {
        let Some(hit) = self.hits.get(self.current).copied() else {
            return false;
        };
        let Ok(Some(re)) = self.build() else {
            return false;
        };
        let Some(block) = doc.blocks.get(hit.block) else {
            return false;
        };
        let text = editor::block_buffer(block);
        if hit.end > text.len() || hit.start > hit.end {
            return false;
        }
        let rep = if self.regex {
            let mut s = String::new();
            match re.captures_at(&text, hit.start) {
                Some(c) => {
                    c.expand(&self.replacement, &mut s);
                    s
                }
                None => self.replacement.clone(),
            }
        } else {
            self.replacement.clone()
        };
        let next = format!("{}{}{}", &text[..hit.start], rep, &text[hit.end..]);
        let src = editor::rebuild_source(block, &next);
        doc.record();
        doc.edit_block(hit.block, &src);
        self.stale = true;
        self.info = "已替换 1 处".to_string();
        true
    }

    /// 全部替换；返回替换处数。
    fn replace_all(&mut self, doc: &mut Document) -> usize {
        let Ok(Some(re)) = self.build() else {
            return 0;
        };
        let mut edits: Vec<(usize, String)> = Vec::new();
        let mut total = 0;
        for (i, b) in doc.blocks.iter().enumerate() {
            let text = editor::block_buffer(b);
            let n = re.find_iter(&text).filter(|m| m.start() != m.end()).count();
            if n == 0 {
                continue;
            }
            let new_text = if self.regex {
                re.replace_all(&text, self.replacement.as_str())
                    .into_owned()
            } else {
                re.replace_all(&text, NoExpand(&self.replacement))
                    .into_owned()
            };
            if new_text != text {
                total += n;
                edits.push((i, editor::rebuild_source(b, &new_text)));
            }
        }
        if edits.is_empty() {
            return 0;
        }
        doc.record();
        for (i, src) in edits {
            doc.edit_block(i, &src);
        }
        self.stale = true;
        self.info = format!("已替换 {} 处", total);
        total
    }

    /// 画弹窗。命中跳转/替换结果通过返回值交给 app。
    pub fn show(&mut self, ctx: &egui::Context, doc: &mut Document, theme: &Theme) -> FindResult {
        let mut out = FindResult::default();
        if !self.open {
            return out;
        }
        self.sync(doc);

        let mut open = true;
        let mut next = false;
        let mut prev = false;
        let mut do_replace = false;
        let mut do_all = false;
        let mut toggled = false;
        let mut esc = false;

        egui::Window::new("查找")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-18.0, 96.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let q = egui::TextEdit::singleline(&mut self.query)
                        .hint_text("查找")
                        .desired_width(180.0)
                        .show(ui);
                    if self.focus {
                        q.response.request_focus();
                        self.focus = false;
                    }
                    if q.response.changed() {
                        self.stale = true;
                    }
                    if q.response.has_focus() {
                        let (enter, shift) =
                            ui.input(|i| (i.key_pressed(egui::Key::Enter), i.modifiers.shift));
                        if enter {
                            if shift {
                                prev = true;
                            } else {
                                next = true;
                            }
                        }
                    }
                    let count = if self.hits.is_empty() {
                        "0/0".to_string()
                    } else {
                        format!("{}/{}", self.current + 1, self.hits.len())
                    };
                    ui.label(RichText::new(count).color(theme.dim));
                    if ui
                        .add_enabled(!self.hits.is_empty(), egui::Button::new("↑"))
                        .on_hover_text("上一处 (Shift+Enter)")
                        .clicked()
                    {
                        prev = true;
                    }
                    if ui
                        .add_enabled(!self.hits.is_empty(), egui::Button::new("↓"))
                        .on_hover_text("下一处 (Enter)")
                        .clicked()
                    {
                        next = true;
                    }
                    if ui
                        .selectable_label(self.case, "Aa")
                        .on_hover_text("区分大小写")
                        .clicked()
                    {
                        self.case = !self.case;
                        toggled = true;
                    }
                    if ui
                        .selectable_label(self.regex, ".*")
                        .on_hover_text("正则表达式")
                        .clicked()
                    {
                        self.regex = !self.regex;
                        toggled = true;
                    }
                    // 没有快捷键也能进替换：这里永远可以手动展开替换行。
                    if ui
                        .selectable_label(self.replace_row, "替换")
                        .on_hover_text("显示/隐藏替换行 (Ctrl+H)")
                        .clicked()
                    {
                        self.replace_row = !self.replace_row;
                    }
                });
                if self.replace_row {
                    ui.horizontal(|ui| {
                        let r = egui::TextEdit::singleline(&mut self.replacement)
                            .hint_text("替换为（正则可用 $1）")
                            .desired_width(180.0)
                            .show(ui);
                        if r.response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            do_replace = true;
                        }
                        if ui
                            .add_enabled(!self.hits.is_empty(), egui::Button::new("替换"))
                            .clicked()
                        {
                            do_replace = true;
                        }
                        if ui
                            .add_enabled(!self.hits.is_empty(), egui::Button::new("全部"))
                            .on_hover_text("全部替换")
                            .clicked()
                        {
                            do_all = true;
                        }
                    });
                }
                if let Some(e) = &self.error {
                    ui.label(
                        RichText::new(e)
                            .small()
                            .color(egui::Color32::from_rgb(220, 100, 90)),
                    );
                } else if !self.info.is_empty() {
                    ui.label(RichText::new(&self.info).small().color(theme.dim));
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    esc = true;
                }
            });

        if toggled {
            self.stale = true;
            self.sync(doc);
        }
        if next && let Some(h) = self.step(1) {
            out.jump = Some(h);
        }
        if prev && let Some(h) = self.step(-1) {
            out.jump = Some(h);
        }
        if do_replace && self.replace_current(doc) {
            out.changed = true;
            // 替换后原地重算：current 不变，正好指向下一处（被替换的那条已消失）
            self.sync(doc);
            if let Some(h) = self.hits.get(self.current).copied() {
                out.jump = Some(h);
            }
        }
        if do_all && self.replace_all(doc) > 0 {
            out.changed = true;
            self.sync(doc);
        }
        self.open = open && !esc;
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn panel(query: &str, regex: bool, case: bool) -> FindPanel {
        let mut p = FindPanel::default();
        p.open = true;
        p.query = query.to_string();
        p.regex = regex;
        p.case = case;
        p
    }

    /// 字面量查找默认不区分大小写，打开 Aa 后区分。
    #[test]
    fn literal_search_respects_case_toggle() {
        let doc = Document::new("abc ABC", None);
        let mut p = panel("abc", false, false);
        p.sync(&doc);
        assert_eq!(p.hits.len(), 2);
        p.case = true;
        p.stale = true;
        p.sync(&doc);
        assert_eq!(p.hits.len(), 1);
    }

    /// 字面量模式下的特殊字符不该被当成正则（比如「.」只匹配点号）。
    #[test]
    fn literal_search_escapes_regex_metachars() {
        let doc = Document::new("a.b axb", None);
        let mut p = panel("a.b", false, false);
        p.sync(&doc);
        assert_eq!(p.hits.len(), 1, "「.」应按字面量匹配");
    }

    /// 正则模式支持捕获组，替换串里的 $1 / $2 按组展开。
    #[test]
    fn regex_search_and_capture_expansion() {
        let mut doc = Document::new("foo-123 bar-456", None);
        let mut p = panel(r"(\w+)-(\d+)", true, false);
        p.sync(&doc);
        assert_eq!(p.hits.len(), 2);
        p.replacement = "$2-$1".to_string();
        assert!(p.replace_current(&mut doc));
        assert_eq!(doc.source(), "123-foo bar-456");
        assert!(doc.dirty);
        assert_eq!(p.replace_all(&mut doc), 1);
        assert_eq!(doc.source(), "123-foo 456-bar");
    }

    /// 标题块查找的是去掉 # 之后的文字，替换后 # 由 build 补回来。
    #[test]
    fn heading_markers_are_hidden_and_restored() {
        let mut doc = Document::new("# 标题\n\n正文", None);
        let mut p = panel("#", false, false);
        p.sync(&doc);
        assert!(p.hits.is_empty(), "可编辑文本里没有 #");

        let mut p = panel("标题", false, false);
        p.sync(&doc);
        assert_eq!(p.hits.len(), 1);
        p.replacement = "新标题".to_string();
        assert!(p.replace_current(&mut doc));
        assert_eq!(doc.source(), "# 新标题\n\n正文");
    }

    /// 全部替换会跨块处理，并保留各块原有的 Markdown 结构。
    #[test]
    fn replace_all_spans_blocks() {
        let mut doc = Document::new("# aaa\n\nbbb aaa", None);
        let mut p = panel("aaa", false, false);
        p.sync(&doc);
        assert_eq!(p.hits.len(), 2);
        p.replacement = "XXX".to_string();
        assert_eq!(p.replace_all(&mut doc), 2);
        assert_eq!(doc.source(), "# XXX\n\nbbb XXX");
    }

    /// 真实文档结构（标题 / 段落 / 列表）里全部替换，Markdown 标记要原样保留。
    #[test]
    fn replace_all_keeps_markdown_structure() {
        let mut doc = Document::new(
            "# 旧标题

正文里有旧内容，也有旧词。

- 旧事项",
            None,
        );
        let mut p = panel("旧", false, false);
        p.sync(&doc);
        assert_eq!(p.hits.len(), 4);
        p.replacement = "新".to_string();
        assert_eq!(p.replace_all(&mut doc), 4);
        let src = doc.source();
        assert!(src.contains("# 新标题"), "{src}");
        assert!(src.contains("- 新事项"), "{src}");
        assert!(!src.contains("旧"), "{src}");
    }

    /// 命中区间要按字符（而不是字节）交给编辑器，中文才能正确选中。
    #[test]
    fn hit_chars_use_char_offsets() {
        let doc = Document::new("中文 abc", None);
        let mut p = panel("abc", false, false);
        p.sync(&doc);
        let h = p.hits[0];
        assert_eq!(p.hit_chars(&doc, h), Some((3, 6)));
    }

    /// 非法正则给出错误提示，而不是崩溃或空匹配。
    #[test]
    fn invalid_regex_reports_error() {
        let doc = Document::new("abc", None);
        let mut p = panel("(", true, false);
        p.sync(&doc);
        assert!(p.error.is_some());
        assert!(p.hits.is_empty());
    }
}
