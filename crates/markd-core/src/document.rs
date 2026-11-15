//! 文档模型：块列表、撤销/重做、统计、大纲、双向链接。

use crate::ast::{self, Block, Inline};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub words: usize,
    pub chars: usize,
    pub cjk: usize,
    pub lines: usize,
    pub reading_min: usize,
}

#[derive(Clone, Debug)]
pub struct OutlineItem {
    pub level: u8,
    pub title: String,
    pub block: usize,
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
        || ('\u{3400}'..='\u{4dbf}').contains(&c)
        || ('\u{3040}'..='\u{30ff}').contains(&c)
        || ('\u{ac00}'..='\u{d7af}').contains(&c)
}

pub struct Document {
    pub path: Option<PathBuf>,
    pub blocks: Vec<Block>,
    pub dirty: bool,
    undo: Vec<String>,
    redo: Vec<String>,
    last_record: Instant,
}

impl Document {
    pub fn new(text: &str, path: Option<PathBuf>) -> Self {
        Self {
            path,
            blocks: ast::parse(text),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            last_record: Instant::now(),
        }
    }

    pub fn source(&self) -> String {
        self.blocks.iter().map(|b| b.source()).collect::<Vec<_>>().join("\n")
    }

    /// 记录一次可撤销的快照（去重、限长）。
    pub fn record(&mut self) {
        let s = self.source();
        if self.undo.last() != Some(&s) {
            self.undo.push(s);
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.last_record = Instant::now();
    }

    /// 连续输入时节流，避免每个按键都产生一条撤销记录。
    pub fn maybe_record(&mut self) {
        if self.last_record.elapsed().as_millis() > 450 {
            self.record();
        }
    }

    pub fn set_source(&mut self, s: &str) {
        self.blocks = ast::parse(s);
        self.dirty = true;
    }

    /// 编辑单个块。
    ///
    /// 绝大多数输入只改动一个块，因此优先只解析这一块并原地替换，避免每次按键
    /// 都把整篇文档重新解析一遍（10 万字时全文重解析约 20ms+）。
    /// 只有当这一块的源码解析出多个块（例如粘贴了空行）时才回退到全文重解析。
    pub fn edit_block(&mut self, idx: usize, new_src: &str) {
        let mut parsed = ast::parse(new_src);
        if parsed.len() == 1 {
            if let Some(b) = self.blocks.get_mut(idx) {
                *b = parsed.pop().unwrap();
            }
            self.dirty = true;
            return;
        }
        if let Some(b) = self.blocks.get_mut(idx) {
            *b.source_mut() = new_src.to_string();
        }
        let s = self.source();
        self.blocks = ast::parse(&s);
        self.dirty = true;
    }

    /// 在 `idx` 之后插入新块；只解析这段新源码，不重解析全文。
    pub fn insert_block_after(&mut self, idx: usize, source: &str) {
        let at = if idx < self.blocks.len() { idx + 1 } else { self.blocks.len() };
        let parsed = ast::parse(source);
        if parsed.is_empty() {
            self.blocks.insert(at, Block::Blank { source: source.to_string() });
        } else {
            for (k, b) in parsed.into_iter().enumerate() {
                self.blocks.insert(at + k, b);
            }
        }
        self.dirty = true;
    }

    pub fn remove_block(&mut self, idx: usize) {
        if self.blocks.len() <= 1 || idx >= self.blocks.len() {
            return;
        }
        self.blocks.remove(idx);
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo.pop() {
            let cur = self.source();
            self.redo.push(cur);
            self.blocks = ast::parse(&prev);
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo.pop() {
            let cur = self.source();
            self.undo.push(cur);
            self.blocks = ast::parse(&next);
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        if let Some(p) = self.path.clone() {
            std::fs::write(p, self.source())?;
            self.dirty = false;
        }
        Ok(())
    }

    pub fn title(&self) -> String {
        for b in &self.blocks {
            if let Some((_, t)) = b.heading() {
                if !t.trim().is_empty() {
                    return t;
                }
            }
        }
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "未命名文档".to_string())
    }

    pub fn stats(&self) -> Stats {
        let src = self.blocks.iter().map(|b| b.plain()).collect::<Vec<_>>().join(" ");
        let chars = src.chars().filter(|c| !c.is_whitespace()).count();
        let cjk = src.chars().filter(|c| is_cjk(*c)).count();
        let latin = src.split_whitespace().count();
        let words = latin + cjk;
        let lines = self.source().split('\n').count();
        let reading_min = ((words as f32 / 300.0).ceil() as usize).max(1);
        Stats { words, chars, cjk, lines, reading_min }
    }

    pub fn outline(&self) -> Vec<OutlineItem> {
        self.blocks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.heading().map(|(level, title)| OutlineItem { level, title, block: i }))
            .collect()
    }

}

/// 收集块中的顶层行内节点，供搜索/双链提取使用。
pub fn block_inlines(b: &Block) -> Vec<&Inline> {
    match b {
        Block::Heading { content, .. }
        | Block::Paragraph { content, .. }
        | Block::Quote { content, .. }
        | Block::Footnote { content, .. } => content.iter().collect(),
        Block::ListItem { item, .. } => item.content.iter().collect(),
        Block::Table { head, rows, .. } => {
            let mut v: Vec<&Inline> = head.iter().flatten().collect();
            for row in rows {
                v.extend(row.iter().flatten());
            }
            v
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_and_undo() {
        let mut d = Document::new("# 标题\n\n正文", None);
        d.record();
        d.edit_block(0, "## 新标题");
        assert!(d.source().starts_with("## 新标题"));
        assert!(d.dirty);
        assert!(d.undo());
        assert!(d.source().starts_with("# 标题"));
        assert!(d.redo());
        assert!(d.source().starts_with("## 新标题"));
    }

    #[test]
    fn outline_and_stats() {
        let d = Document::new("# 一\n\n## 二\n\n中文正文 abc", None);
        let o = d.outline();
        assert_eq!(o.len(), 2);
        assert_eq!(o[1].level, 2);
        let s = d.stats();
        assert!(s.cjk >= 4);
        assert!(s.words > 0);
        assert_eq!(d.title(), "一");
    }

    #[test]
    fn insert_block() {
        let mut d = Document::new("A\n\nB", None);
        d.insert_block_after(0, "新块");
        assert!(d.source().contains("新块"));
    }
}
