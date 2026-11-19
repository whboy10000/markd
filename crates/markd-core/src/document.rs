//! Author: aurelianus <aurelianus@126.com>

//! 文档模型：块列表、撤销/重做、统计、大纲、双向链接。

use crate::ast::{self, Block, Inline};
use std::path::PathBuf;
use std::time::Instant;

/// 一次块级变更：`[at, at+old_len)` 这段块被换成了 `new_len` 个块。
///
/// 视图层靠它把排版缓存（块高表、行号表）只作废在受影响区间上；
/// 整篇重解析（撤销、源码模式落盘、粘贴多块）用 `Rebuild` 表示「全作废」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Splice { at: usize, old_len: usize, new_len: usize },
    Rebuild,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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

/// 一个块的统计量。整篇 `Stats` 是它沿块求和，所以改一块只重算一块：
/// 字数是逐块可加的（按空白切词与 CJK 计数都不跨块），行数同理
/// ——块源码用 `\n` 拼接，总行数正好是各块行号之和。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BlockStat {
    words: usize,
    chars: usize,
    cjk: usize,
    lines: usize,
}

fn stat_of(b: &Block) -> BlockStat {
    let plain = b.plain();
    let cjk = plain.chars().filter(|c| is_cjk(*c)).count();
    BlockStat {
        words: plain.split_whitespace().count() + cjk,
        chars: plain.chars().filter(|c| !c.is_whitespace()).count(),
        cjk,
        lines: b.source().split('\n').count(),
    }
}

/// 整表重建：小文档直接串行，大文档按块分片到多核。
/// `plain()` 要遍历行内树，1000 万字串行约 275ms，并行后约 40ms。
fn build_stats(blocks: &[Block]) -> Vec<BlockStat> {
    let serial = |blocks: &[Block]| blocks.iter().map(stat_of).collect::<Vec<_>>();
    if blocks.len() < 4096 {
        return serial(blocks);
    }
    let threads = std::thread::available_parallelism()
        .map(|n| n.get().min(8))
        .unwrap_or(1);
    if threads < 2 {
        return serial(blocks);
    }
    let mut out = vec![BlockStat::default(); blocks.len()];
    let chunk = blocks.len().div_ceil(threads);
    std::thread::scope(|s| {
        for (dst, src) in out.chunks_mut(chunk).zip(blocks.chunks(chunk)) {
            s.spawn(move || {
                for (d, b) in dst.iter_mut().zip(src) {
                    *d = stat_of(b);
                }
            });
        }
    });
    out
}

/// 一步可逆的编辑。存的是**被改动那几块的源码**，不是整篇文本：
/// 1000 万字里打一个字，撤销记录只有几十字节，而不是 10MB 全文拷贝
/// （旧实现每次快照都存整篇，200 步深就要 3.3GB，且一次 Ctrl+Z 要重解析 1.5s）。
///
/// - `Splice`：`blocks[at..at+old.len())` 与 `new` 互换；
/// - `Full`：整篇块列表换成 `new`（源码模式落盘、粘贴出多块等整篇重建）。
#[derive(Clone, Debug)]
pub enum EditOp {
    Splice {
        at: usize,
        old: Vec<String>,
        new: Vec<String>,
    },
    Full {
        old: Vec<String>,
        new: Vec<String>,
    },
}

impl EditOp {
    /// 正反两方向的 (被替换源码, 替换后源码, 起始下标)。
    fn sides(&self, forward: bool) -> (&Vec<String>, &Vec<String>, usize) {
        match self {
            EditOp::Splice { at, old, new } => {
                if forward {
                    (old, new, *at)
                } else {
                    (new, old, *at)
                }
            }
            EditOp::Full { old, new } => {
                if forward {
                    (old, new, 0)
                } else {
                    (new, old, 0)
                }
            }
        }
    }

    fn is_full(&self) -> bool {
        matches!(self, EditOp::Full { .. })
    }
}

/// 源码列表 → 块列表。空源码解析结果是空列表，这里补成空块，
/// 保证「一块源码」始终还原成「一个块」，撤销后下标不会错位。
fn blocks_from(srcs: &[String]) -> Vec<Block> {
    let mut out = Vec::with_capacity(srcs.len());
    for s in srcs {
        let parsed = ast::parse(s);
        if parsed.is_empty() {
            out.push(Block::Blank {
                source: s.clone(),
            });
        } else {
            out.extend(parsed);
        }
    }
    out
}

/// 整篇替换能压多小：首尾按「整块源码全等」比对能相同的前后缀，
/// 中间那段才是真正改过的块。返回 `(起点, 旧段末, 新段末)`。
fn diff_range(old: &[String], new: &[String]) -> (usize, usize, usize) {
    let p = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let tail = old[p..]
        .iter()
        .rev()
        .zip(new[p..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (p, old.len() - tail, new.len() - tail)
}

pub struct Document {
    pub path: Option<PathBuf>,
    pub blocks: Vec<Block>,
    pub dirty: bool,
    /// 内容修订号：每次改动 +1。弹窗类功能（查找）用它判断要不要重算，避免每帧全量扫描。
    pub rev: u64,
    /// 已闭合的撤销组：一组 = 一次 `record()` 边界之间的所有编辑，Ctrl+Z 整组回放。
    undo: Vec<Vec<EditOp>>,
    redo: Vec<Vec<EditOp>>,
    /// 自上次 `record()` 以来累积的编辑，下一次 `record()` 把它闭合成一组。
    pending: Vec<EditOp>,
    last_record: Instant,
    /// 自上次 `take_changes` 以来发生过的块级变更，供视图精确作废排版缓存。
    changes: Vec<Change>,
    /// 与 `blocks` 一一对应的统计量，`stats()` 只对它求和。
    /// 统一在 `touch` 里随块级变更同步。
    bstats: Vec<BlockStat>,
}

/// 一组里最多攒多少条编辑：超过就先闭合，避免「一次 Ctrl+Z 回放十万步」。
const MAX_OPS_PER_GROUP: usize = 4096;

impl Document {
    pub fn new(text: &str, path: Option<PathBuf>) -> Self {
        let blocks = ast::parse(text);
        let bstats = build_stats(&blocks);
        Self {
            path,
            blocks,
            dirty: false,
            rev: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            pending: Vec::new(),
            last_record: Instant::now(),
            changes: Vec::new(),
            bstats,
        }
    }

    pub fn source(&self) -> String {
        self.blocks
            .iter()
            .map(|b| b.source())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 内容变了：置脏、递增修订号，记下这次块级变更，并同步统计表。
    ///
    /// 统计表放在这里同步，而不是每个改动点各写一遍：`Change` 已经精确描述了
    /// 「哪些块被换掉、换成了几个」，任何忘记同步的改动都会在这里露出来。
    fn touch(&mut self, c: Change) {
        self.dirty = true;
        self.rev += 1;
        match c {
            Change::Splice { at, old_len, new_len } => {
                let at = at.min(self.blocks.len());
                let end = (at + new_len).min(self.blocks.len());
                let stats: Vec<BlockStat> = self.blocks[at..end].iter().map(stat_of).collect();
                let b_at = at.min(self.bstats.len());
                let b_end = (b_at + old_len).min(self.bstats.len());
                self.bstats.splice(b_at..b_end, stats);
            }
            Change::Rebuild => self.bstats = build_stats(&self.blocks),
        }
        if self.changes.last() == Some(&Change::Rebuild) {
            return;
        }
        // 变更表被写满（批量格式化、替换全部）时退化成整篇作废，
        // 宁可多排一次版，也不能让缓存和真实块错位。
        if self.changes.len() >= 512 {
            self.changes.clear();
        }
        self.changes.push(c);
    }

    /// 取走并清空累积的块级变更：视图每帧调用一次，据此作废排版缓存。
    pub fn take_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }

    /// 闭合当前编辑段，作为一次撤销边界。调用约定是「改动之前调 `record()`」，
    /// 于是紧随其后的那步编辑落进新的一段里，Ctrl+Z 正好整段回退。
    ///
    /// 没有实际编辑时不留空组：空组会让撤销「按了没反应」。
    pub fn record(&mut self) {
        self.close_pending();
        self.last_record = Instant::now();
    }

    /// 连续输入时节流，避免每个按键都产生一条撤销记录。
    pub fn maybe_record(&mut self) {
        if self.last_record.elapsed().as_millis() > 450 {
            self.record();
        }
    }

    /// 把未闭合的编辑段收进撤销栈。与 `record()` 的区别：不清 redo 栈，
    /// 所以撤销时先收下在途编辑不会把「重做」弄丢。
    fn close_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let group = std::mem::take(&mut self.pending);
        self.undo.push(group);
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
    }

    /// 记一步可撤销编辑。真实编辑一旦发生，重做链就断了——放在这里清，而不是
    /// 放在 `record()`：调用方约定「改动之前 record」，若 record 时清掉 redo，
    /// 撤销到底再打一个字就会把重做历史抹掉。
    ///
    /// 批量改动（格式化全文）可能攒很多条，超上限先闭合成组，
    /// 免得一次 Ctrl+Z 要回放十万步。
    fn push_op(&mut self, op: EditOp) {
        self.redo.clear();
        self.pending.push(op);
        if self.pending.len() >= MAX_OPS_PER_GROUP {
            self.close_pending();
        }
    }

    /// 当前块列表的源码快照，供整篇替换的记录用。
    fn srcs(&self) -> Vec<String> {
        self.blocks.iter().map(|b| b.source().to_string()).collect()
    }

    /// 回放一步编辑；`forward == false` 即撤销它。同时告诉视图要作废哪一段。
    fn apply_op(&mut self, op: &EditOp, forward: bool) {
        let (from, to, at) = op.sides(forward);
        let blocks = blocks_from(to);
        let new_len = blocks.len();
        if op.is_full() {
            self.blocks = blocks;
            self.touch(Change::Rebuild);
            return;
        }
        // 下标越界只可能来自上游 bug；钳住比 panic 好，撤销栈本身仍是完整真相。
        let at = at.min(self.blocks.len());
        let end = (at + from.len()).min(self.blocks.len());
        self.blocks.splice(at..end, blocks);
        self.touch(Change::Splice {
            at,
            old_len: end - at,
            new_len,
        });
    }

    /// 记一次「整篇块列表替换」，能压小就压成 Splice。
    ///
    /// 一律记全量快照的话，1000 万字每敲一个字留 37MB、200 组撤销上限下能涨到
    /// 7GB；而源码模式打字实际只改一个块，压成首尾差集后只留那一块。
    /// 差异摊到整篇（导入、整篇格式化）压不出东西，仍记 Full 让视图整篇作废缓存。
    fn push_srcs_op(&mut self, old: Vec<String>, new: Vec<String>) {
        if old == new {
            return;
        }
        let (at, old_end, new_end) = diff_range(&old, &new);
        let mid = (old_end - at) + (new_end - at);
        if mid * 2 >= old.len() + new.len() {
            self.push_op(EditOp::Full { old, new });
            return;
        }
        self.push_op(EditOp::Splice {
            at,
            old: old[at..old_end].to_vec(),
            new: new[at..new_end].to_vec(),
        });
    }

    pub fn set_source(&mut self, s: &str) {
        let old = self.srcs();
        self.blocks = ast::parse(s);
        self.touch(Change::Rebuild);
        self.push_srcs_op(old, self.srcs());
    }

    /// 编辑单个块。
    ///
    /// 绝大多数输入只改动一个块，因此优先只解析这一块并原地替换，避免每次按键
    /// 都把整篇文档重新解析一遍（10 万字时全文重解析约 20ms+）。
    /// 只有当这一块的源码解析出多个块（例如粘贴了空行）时才回退到全文重解析。
    ///
    /// 源码没变的编辑（把图片拖回原宽度、重复落盘同一内容）整条路径直接跳过：
    /// 不置脏、不记撤销。否则一次拖动几十帧会攒出几十步「按了没反应」的 Ctrl+Z。
    pub fn edit_block(&mut self, idx: usize, new_src: &str) {
        // 内容被删空：保留这个块（换成空块），否则全文重解析会把块直接删掉，
        // 块下标整体前移，编辑器的 active/光标全部错位，退格也就再也合并不回上一行。
        if new_src.trim().is_empty() {
            if let Some(b) = self.blocks.get_mut(idx) {
                let old = b.source().to_string();
                if old.is_empty() {
                    return;
                }
                *b = Block::Blank {
                    source: String::new(),
                };
                self.touch(Change::Splice {
                    at: idx,
                    old_len: 1,
                    new_len: 1,
                });
                self.push_op(EditOp::Splice {
                    at: idx,
                    old: vec![old],
                    new: vec![String::new()],
                });
            }
            return;
        }
        let mut parsed = ast::parse(new_src);
        if parsed.len() == 1 {
            if let Some(b) = self.blocks.get_mut(idx) {
                let old = b.source().to_string();
                *b = parsed.pop().unwrap();
                // 记录的是块自己的规范源码（不是原始入参）：入参可能被解析器
                // 归一化（去掉首尾空白等），回放时才不会再引入一点差异。
                let new = b.source().to_string();
                if old == new {
                    return;
                }
                self.touch(Change::Splice {
                    at: idx,
                    old_len: 1,
                    new_len: 1,
                });
                self.push_op(EditOp::Splice {
                    at: idx,
                    old: vec![old],
                    new: vec![new],
                });
            }
            return;
        }
        // 这一块解析出了多个块（典型场景：粘贴里带空行）：只能整篇重解析，
        // 块下标从此不可信，排版缓存整篇作废。
        let old_all = self.srcs();
        if let Some(b) = self.blocks.get_mut(idx) {
            *b.source_mut() = new_src.to_string();
        }
        let s = self.source();
        self.blocks = ast::parse(&s);
        self.touch(Change::Rebuild);
        let new_all = self.srcs();
        self.push_srcs_op(old_all, new_all);
    }

    /// 在 `idx` 之后插入新块；只解析这段新源码，不重解析全文。
    pub fn insert_block_after(&mut self, idx: usize, source: &str) {
        let at = if idx < self.blocks.len() {
            idx + 1
        } else {
            self.blocks.len()
        };
        self.insert_at(at, source);
    }

    /// 在 `idx` 之前插入新块；`idx == 0` 时插到文档最前。
    /// 与 `insert_block_after` 一样只解析这段新源码，不重解析全文。
    pub fn insert_block_before(&mut self, idx: usize, source: &str) {
        let at = idx.min(self.blocks.len());
        self.insert_at(at, source);
    }

    fn insert_at(&mut self, at: usize, source: &str) {
        let parsed = ast::parse(source);
        let new_srcs: Vec<String> = if parsed.is_empty() {
            vec![source.to_string()]
        } else {
            parsed.iter().map(|b| b.source().to_string()).collect()
        };
        let blocks = blocks_from(&new_srcs);
        let inserted = blocks.len();
        self.blocks.splice(at..at, blocks);
        self.touch(Change::Splice {
            at,
            old_len: 0,
            new_len: inserted,
        });
        self.push_op(EditOp::Splice {
            at,
            old: Vec::new(),
            new: new_srcs,
        });
    }

    pub fn remove_block(&mut self, idx: usize) {
        if self.blocks.len() <= 1 || idx >= self.blocks.len() {
            return;
        }
        let removed = self.blocks.remove(idx);
        let old = removed.source().to_string();
        drop(removed);
        self.touch(Change::Splice {
            at: idx,
            old_len: 1,
            new_len: 0,
        });
        self.push_op(EditOp::Splice {
            at: idx,
            old: vec![old],
            new: Vec::new(),
        });
    }

    pub fn undo(&mut self) -> bool {
        // 在途的未闭合编辑先收下，撤销才够得着它。
        self.close_pending();
        while let Some(group) = self.undo.pop() {
            if group.is_empty() {
                continue;
            }
            // 组内逆序回放：靠后的编辑先改变了下标，必须最先撤掉。
            for op in group.iter().rev() {
                self.apply_op(op, false);
            }
            // 撤销记录本身是双向的（apply_op 的 forward 参数决定方向），
            // 重做只需把这一步按原顺序再放一遍，不必再造交换过 old/new 的副本。
            self.redo.push(group);
            return true;
        }
        false
    }

    pub fn redo(&mut self) -> bool {
        if let Some(group) = self.redo.pop() {
            for op in group.iter() {
                self.apply_op(op, true);
            }
            self.undo.push(group);
            true
        } else {
            false
        }
    }

    /// 撤销/重做栈深度与占用字符数，供压力测试与诊断用。
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// 撤销历史占用（字节）。含 `pending`：连续输入时编辑先攒在途段，
    /// 不计进来会把「每键复制整篇」这类泄漏读成 0。
    pub fn undo_bytes(&self) -> usize {
        let op = |o: &EditOp| match o {
            EditOp::Splice { old, new, .. } | EditOp::Full { old, new } => {
                old.iter().map(|s| s.len()).sum::<usize>() + new.iter().map(|s| s.len()).sum::<usize>()
            }
        };
        let groups = |gs: &Vec<Vec<EditOp>>| gs.iter().flatten().map(op).sum::<usize>();
        groups(&self.undo) + groups(&self.redo) + self.pending.iter().map(op).sum::<usize>()
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

    /// 全文统计。沿块求和，不复制全文：1000 万字约 0.2ms，
    /// 于是「输入时刷新字数」不会再占用一次整篇扫描。
    pub fn stats(&self) -> Stats {
        debug_assert_eq!(
            self.bstats.len(),
            self.blocks.len(),
            "统计表与块列表错位：某处改动块却没同步统计量"
        );
        let mut s = Stats::default();
        for b in &self.bstats {
            s.words += b.words;
            s.chars += b.chars;
            s.cjk += b.cjk;
            s.lines += b.lines;
        }
        // 没有块时源码是空串，`split('\n')` 仍算一行。
        if self.blocks.is_empty() {
            s.lines = 1;
        }
        s.reading_min = ((s.words as f32 / 300.0).ceil() as usize).max(1);
        s
    }

    pub fn outline(&self) -> Vec<OutlineItem> {
        self.blocks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                b.heading().map(|(level, title)| OutlineItem {
                    level,
                    title,
                    block: i,
                })
            })
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

    /// 自动保存最终落到 Document::save：写盘后必须清掉脏标记，
    /// 否则每次检查都会重复写同一个文件。
    #[test]
    fn save_writes_file_and_clears_dirty() {
        let dir = std::env::temp_dir().join(format!("markd-save-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("doc.md");
        let mut d = Document::new("# 标题", Some(file.clone()));
        d.record();
        d.edit_block(0, "# 改过");
        assert!(d.dirty);
        d.save().unwrap();
        assert!(!d.dirty, "保存后应清掉脏标记");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "# 改过");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 未命名文档没有路径：save() 是空操作，脏状态必须保留（等用户选路径）。
    #[test]
    fn save_without_path_keeps_dirty() {
        let mut d = Document::new("# 标题", None);
        d.record();
        d.edit_block(0, "# 改过");
        assert!(d.save().is_ok());
        assert!(d.dirty, "没有路径时不该清掉脏标记");
    }

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

    /// 参考实现：把每块正文拼成全串再数（优化前 `stats()` 就是这么做）。
    fn stats_reference(doc: &Document) -> Stats {
        let src = doc
            .blocks
            .iter()
            .map(|b| b.plain())
            .collect::<Vec<_>>()
            .join(" ");
        let chars = src.chars().filter(|c| !c.is_whitespace()).count();
        let cjk = src.chars().filter(|c| is_cjk(*c)).count();
        let words = src.split_whitespace().count() + cjk;
        let lines = doc.source().split('\n').count();
        Stats {
            words,
            chars,
            cjk,
            lines,
            reading_min: ((words as f32 / 300.0).ceil() as usize).max(1),
        }
    }

    /// `stats()` 现在只沿「每块统计量」求和，不再复制全文。
    /// 因此每一步块级改动之后都必须与全量扫描逐字段相等：
    /// 漏同步统计表不会崩，只会把字数悄悄算错，只能靠这条断言兜住。
    #[test]
    fn stats_match_full_scan_after_every_edit() {
        let text = "# 标题\n\n中文正文 abc\n\n> 引用 **加粗**\n\n- 列表项\n\n- 续行甲\n  续行乙\n\n```rust\nfn f() {}\n```";
        let mut d = Document::new(text, None);
        let para = d
            .blocks
            .iter()
            .position(|b| b.source().contains("中文正文"))
            .expect("样例里要有正文段落");
        let check = |d: &Document, step: &str| {
            assert_eq!(d.stats(), stats_reference(d), "{step} 之后统计量与全量扫描不一致");
        };
        check(&d, "打开");
        d.record();
        d.edit_block(para, "中文正文 abc def 又加了一句");
        check(&d, "改段落");
        d.record();
        d.insert_block_after(para, "新插入的一段，中文若干。");
        check(&d, "插块");
        d.record();
        let last = d.blocks.len() - 1;
        d.remove_block(last);
        check(&d, "删末块");
        d.record();
        d.edit_block(0, "");
        check(&d, "清空首块");
        d.record();
        d.set_source("整篇替换\n\n- 甲\n\n- 乙");
        check(&d, "整篇替换");
        assert!(d.undo());
        check(&d, "撤销整篇替换");
        assert!(d.redo());
        check(&d, "重做整篇替换");
        // 粘贴带空行：一块解析出多块，走全文重解析那条回退路径
        let head = d.blocks.len() - 1;
        d.record();
        d.edit_block(head, "甲\n\n乙");
        check(&d, "粘贴出多块");
        assert!(d.undo());
        check(&d, "撤销粘贴");
    }

    /// 空文档没有块，但源码是空串、`split('\n')` 算一行：行数不能算成 0。
    #[test]
    fn stats_of_empty_document() {
        let d = Document::new("", None);
        assert!(d.blocks.is_empty() || d.blocks.len() == 1);
        assert_eq!(d.stats(), stats_reference(&d));
    }

    #[test]
    fn insert_block() {
        let mut d = Document::new("A\n\nB", None);
        d.insert_block_after(0, "新块");
        assert!(d.source().contains("新块"));
    }

    /// 撤销按 `record()` 边界整组回退：连续输入被节流切成一段，
    /// 一次 Ctrl+Z 撤掉一整段，而不是一个字一个字撤。
    #[test]
    fn undo_rolls_back_whole_record_group() {
        let mut d = Document::new("A\n\nB\n\nC", None);
        d.record();
        d.edit_block(0, "A1");
        d.record();
        d.edit_block(0, "A2");
        d.record();
        d.edit_block(4, "C1");
        assert_eq!(d.undo_depth(), 2, "最后一段还没闭合，栈里只有前两段");

        assert!(d.undo());
        assert_eq!(d.source(), "A2\n\nB\n\nC");
        assert!(d.undo());
        assert_eq!(d.source(), "A1\n\nB\n\nC");
        assert!(d.undo());
        assert_eq!(d.source(), "A\n\nB\n\nC");
        assert!(!d.undo(), "撤到最初状态之后没有可撤的了");

        assert!(d.redo());
        assert_eq!(d.source(), "A1\n\nB\n\nC");
        assert!(d.redo());
        assert!(d.redo());
        assert_eq!(d.source(), "A2\n\nB\n\nC1");
        assert!(!d.redo());
    }

    /// 一组里既有删块又有插入时，回放必须逆序，否则下标会互相踩。
    /// 这里不手算下标（空行也是块），只要求「撤干净 == 原文、重做完 == 改后」。
    #[test]
    fn undo_replays_mixed_ops_in_reverse_order() {
        let mut d = Document::new("A\n\nB\n\nC\n\nD", None);
        let orig = d.source();
        d.record();
        d.remove_block(2);
        d.insert_block_after(0, "新");
        d.remove_block(1);
        let edited = d.source();
        assert_ne!(edited, orig, "这三步应该真的改动了文档");
        d.record();

        assert!(d.undo());
        assert_eq!(d.source(), orig, "一组多步必须整组精确回退");
        assert!(d.redo());
        assert_eq!(d.source(), edited, "重做必须精确复原");
    }

    /// 删空一个块走的是「原地换成空块」，撤销必须把原文一字不差放回去。
    #[test]
    fn undo_restores_emptied_block() {
        let mut d = Document::new("# 标题\n\n正文", None);
        d.record();
        d.edit_block(0, "");
        d.record();
        assert!(d.undo());
        assert_eq!(d.source(), "# 标题\n\n正文");
        assert!(d.redo());
        assert_eq!(d.source(), "\n\n正文", "重做应回到「标题被清空」的状态");
    }

    /// 整篇替换（源码模式落盘、导入）记录成 Full，同样能精确往返。
    #[test]
    fn undo_covers_full_replace() {
        let mut d = Document::new("旧\n\n内容", None);
        d.record();
        d.set_source("# 新\n\n内容\n\n- 项");
        assert_eq!(d.blocks.len(), 5);
        assert!(d.undo());
        assert_eq!(d.source(), "旧\n\n内容");
        assert!(d.redo());
        assert_eq!(d.source(), "# 新\n\n内容\n\n- 项");
    }

    /// 源码模式在长文档中间敲一个字：撤销记录必须只留改动的那一块。
    /// 旧实现每敲一个字存两份整篇（1000 万字 = 37MB/键），撤销栈 200 组上限下
    /// 能吃到 7GB；压成 Splice 后与文档大小无关，且撤销/重做仍逐字节还原。
    #[test]
    fn set_source_in_the_middle_records_a_splice() {
        let text = (0..600)
            .map(|i| format!("这是第 {i} 段正文，用来撑起一篇不小的文档。\n\n"))
            .collect::<String>();
        let mut d = Document::new(&text, None);
        d.record();
        let base = d.undo_bytes();
        // 在第 300 段中间插两个字（块数不变，只动一块）。
        let at = text.find("这是第 300 段").unwrap() + 6;
        let edited = format!("{}插入{}", &text[..at], &text[at..]);
        d.set_source(&edited);
        assert!(
            d.undo_bytes() - base < text.len() / 4,
            "撤销记录该只含改动块：实际新增 {} 字节，全文才 {} 字节",
            d.undo_bytes() - base,
            text.len()
        );
        assert!(d.undo());
        assert_eq!(d.source(), text, "撤销应逐字节还原全文");
        assert!(d.redo());
        assert_eq!(d.source(), edited, "重做同样精确");
        // 每段之间那个空行也是块：600 段 = 600 段落块 + 601 空块。
        assert_eq!(d.blocks.len(), 1201, "中间插字不该改变块数");
    }

    /// 差异摊到整篇（导入、整篇重排）压不出小区间，仍走全量记录。
    #[test]
    fn whole_document_replacement_still_records_full() {
        let mut d = Document::new("甲\n\n乙\n\n丙", None);
        d.record();
        d.set_source("# 一\n\n# 二\n\n# 三\n\n# 四");
        assert!(d.undo());
        assert_eq!(d.source(), "甲\n\n乙\n\n丙");
    }

    /// 撤销栈按「编辑量」增长，不按「文档大小」增长：
    /// 这是 1000 万字能撤销的历史前提（旧实现每步复制整篇）。
    #[test]
    fn undo_stack_scales_with_edits_not_document() {
        let text = (0..400)
            .map(|i| format!("这是第 {i} 段正文，用来撑起一篇不小的文档。\n\n"))
            .collect::<String>();
        let mut d = Document::new(&text, None);
        let doc_bytes = d.source().len();
        d.record();
        for k in 0..20 {
            d.edit_block(k * 7, &format!("改过的第 {k} 段，内容比原来短。"));
            d.record();
        }
        assert_eq!(d.undo_depth(), 20);
        assert!(
            d.undo_bytes() < doc_bytes / 4,
            "20 步编辑的栈 {} 字节，接近整篇 {doc_bytes} 字节，说明还在复制全文",
            d.undo_bytes()
        );
        // 20 步一次撤干净，逐字比对原文。
        for _ in 0..20 {
            assert!(d.undo());
        }
        assert_eq!(d.source(), text);
    }

    /// 没有实际编辑时不留空组：否则 Ctrl+Z 会出现「按了没反应」。
    #[test]
    fn record_without_edit_leaves_no_empty_group() {
        let mut d = Document::new("A\n\nB", None);
        d.record();
        d.record();
        d.record();
        assert_eq!(d.undo_depth(), 0);
        d.edit_block(0, "A2");
        d.record();
        assert_eq!(d.undo_depth(), 1);
        assert!(d.undo());
        assert_eq!(d.source(), "A\n\nB");
    }

    /// 写了个一模一样的内容（拖图片宽度又拖回原值、重复落盘）必须整条跳过：
    /// 不置脏、不进撤销栈。否则一次拖动攒几十步「按了没反应」的撤销。
    #[test]
    fn no_op_edit_records_nothing() {
        let mut d = Document::new("![a](x.png \"{width=300}\")", None);
        let src = d.blocks[0].source().to_string();
        d.edit_block(0, &src);
        assert!(!d.dirty, "内容没变不该置脏");
        d.record();
        assert_eq!(d.undo_depth(), 0, "内容没变不该留下撤销步");

        // 已经是空块再写空，同样不留步
        let mut d = Document::new("\n\n正文", None);
        d.record();
        d.edit_block(0, "");
        assert_eq!(d.undo_depth(), 0);
    }

    /// 归一化只发生在解析器真正改写源码的场景（代码块围栏重建等）；
    /// 段落的首尾空格是内容的一部分，写回相同规范源码才算没变。
    /// 这里锁住「同一块反复写回自己的规范源码 = 无操作」，拖动图片宽度就走这条。
    #[test]
    fn rewriting_canonical_source_is_not_an_edit() {
        let mut d = Document::new("段落文字\n\n- 列表项\n\n> 引用\n\n```\n代码\n```", None);
        for _ in 0..30 {
            for i in 0..d.blocks.len() {
                let src = d.blocks[i].source().to_string();
                d.edit_block(i, &src);
            }
        }
        assert!(!d.dirty, "反复写回规范源码不该把文档标脏");
        d.record();
        assert_eq!(d.undo_depth(), 0, "30 轮无变化写入不该留下撤销步");
    }

    /// 撤销之后再打新字，重做历史必须作废（与旧实现一致）。
    #[test]
    fn new_edit_clears_redo() {
        let mut d = Document::new("A\n\nB", None);
        d.record();
        d.edit_block(0, "A2");
        d.record();
        assert!(d.undo());
        assert_eq!(d.source(), "A\n\nB");
        // 块下标：[A, 空行, B]，改最后一块。
        d.edit_block(2, "B2");
        assert!(!d.redo(), "已有新编辑，重做链应断开");
        assert_eq!(d.source(), "A\n\nB2");
    }
}
