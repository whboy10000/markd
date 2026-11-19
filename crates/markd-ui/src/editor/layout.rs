//! Author: aurelianus <aurelianus@126.com>

//! 块排版表：视口裁剪所需的「每块高度 + 顶边位置 + 起始行号」。
//!
//! 为什么需要这张表：混合模式原先每帧把全部块排版一遍，帧成本与文档规模成正比
//! （实测每块约 23µs，1000 万字 = 24 万块 → 静置帧 5.7s、首帧 13.9s）。有了
//! 每块顶边，一帧里就只需要排版看得见的那几十块，帧成本与文档规模无关。
//!
//! 表里两个数用 0 当「未测量」哨兵：块高 0（任何块排版后至少有一行高）、
//! 源码行数 0（`split('\n').count()` 恒 ≥1）。未测量的块用兜底值估算，
//! 一旦被排版就用实测值替换，所以滚动条长度与行号会随着翻阅自我收敛。

use crate::ast::Block;
use crate::document::Change;

/// 未测量块的兜底高度初值（像素）：一行正文 + 块间距的量级。
/// 排版过程中会被「已测量块的平均高度」取代。
const FALLBACK_H: f32 = 24.0;
/// 未测量块的兜底行数。
const FALLBACK_LINES: u32 = 1;
/// 视口上下各多排版多少块：吸收滚轮惯性、补齐相邻块的接缝（引用竖线跨块续画），
/// 并避免每滚一行就重新测量一批块。
pub const MARGIN_BLOCKS: usize = 24;
/// 实测高度相差超过它才算「块高变了」，触发前缀和重算。
/// 小于它的抖动（亚像素、字体微调）不值得重排整篇。
const EPS: f32 = 0.5;

/// 块排版表。长度始终等于块数；`apply` 之后由视图把可见块的实测值喂进来。
#[derive(Clone, Debug)]
pub struct BlockLayout {
    /// 每块占用的推进高度（含块后的间距）。0 = 未测量。
    heights: Vec<f32>,
    /// 每块的源码行数。0 = 未测量。
    lines: Vec<u32>,
    /// 顶边前缀和：`tops[i]` = 第 i 块顶边相对内容顶部；长度 = 块数 + 1，
    /// 末项 = 内容总高。
    tops: Vec<f32>,
    /// 起始行号前缀和：`line_tops[i]` = 第 i 块之前累计的行数（0 基）。
    line_tops: Vec<u32>,
    /// `tops`/`line_tops` 需要从哪个下标往后重算；`usize::MAX` = 已是最新。
    dirty_from: usize,
    /// 已测量块的高度合计与块数：用来当未测量块的兜底高度。
    sum_h: f64,
    n_measured: u64,
    /// 还没测到行数的块数：为 0 时行号表可以直接跳过全表扫描。
    lines_unknown: usize,
    /// 兜底高度（= 已测量均值，没有测量结果时用 FALLBACK_H）。
    fallback_h: f32,
    /// 影响块高的外部条件指纹（版心宽度、字号、专注模式、行号栏）。
    key: [i32; 4],
}

impl Default for BlockLayout {
    fn default() -> Self {
        Self {
            heights: Vec::new(),
            lines: Vec::new(),
            tops: vec![0.0],
            line_tops: vec![0],
            dirty_from: usize::MAX,
            sum_h: 0.0,
            n_measured: 0,
            lines_unknown: 0,
            fallback_h: FALLBACK_H,
            key: [0; 4],
        }
    }
}

impl BlockLayout {
    pub fn len(&self) -> usize {
        self.heights.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    /// 内容总高（未测量块按兜底高度计）。调用前会保证前缀和是最新的。
    pub fn total_height(&mut self) -> f32 {
        self.build();
        *self.tops.last().unwrap_or(&0.0)
    }

    /// 第 `idx` 块的顶边。越界钳到最后一块，方便调用方少写判空。
    pub fn top(&mut self, idx: usize) -> f32 {
        self.build();
        let i = idx.min(self.heights.len().saturating_sub(1));
        if self.heights.is_empty() { 0.0 } else { self.tops[i] }
    }

    /// 第 `idx` 块之前累计的源码行数（0 基）。行号栏显示时要 +1。
    pub fn lines_before(&mut self, idx: usize) -> u32 {
        self.build();
        let i = idx.min(self.heights.len().saturating_sub(1));
        if self.heights.is_empty() {
            0
        } else {
            self.line_tops[i]
        }
    }

    /// 还没测到行数的块数。>0 且需要显示行号时，视图会调 `fill_lines`。
    pub fn lines_missing(&self) -> usize {
        self.lines_unknown
    }

    /// 外部条件（窗口宽度、字号、模式）变了：高度全部重测，行数保留。
    /// 用指纹比较而不是逐个浮点数比较，避免亚像素抖动引发整篇重排。
    pub fn rebase(&mut self, key: [i32; 4]) {
        if self.key != key {
            self.key = key;
            self.reset_heights();
        }
    }

    /// 第 `idx` 块当前已知的高度：实测值优先，否则兜底值。恒 > 0。
    pub fn height_of(&mut self, idx: usize) -> f32 {
        self.build();
        if idx >= self.heights.len() {
            return self.fallback_h.max(1.0);
        }
        self.height(idx)
    }

    /// 应用文档变更：只作废受影响区间，其余块高继续复用。
    pub fn apply(&mut self, changes: &[Change]) {
        for c in changes {
            match *c {
                Change::Rebuild => self.clear(),
                Change::Splice {
                    at,
                    old_len,
                    new_len,
                } => self.splice(at, old_len, new_len),
            }
        }
    }

    /// 整篇作废：块与旧表已无任何对应关系。
    pub fn clear(&mut self) {
        self.heights.clear();
        self.lines.clear();
        self.lines_unknown = 0;
        self.sum_h = 0.0;
        self.n_measured = 0;
        self.fallback_h = FALLBACK_H;
        self.dirty_from = 0;
    }

    /// 与块数对齐（防御用：变更日志漏记时，多退少补，补的部分一律未知）。
    pub fn sync(&mut self, n: usize) {
        let cur = self.heights.len();
        if n < cur {
            self.splice(n, cur - n, 0);
        } else if n > cur {
            self.splice(cur, 0, n - cur);
        }
        debug_assert_eq!(self.heights.len(), n);
    }

    /// 记录一个块的实测结果：`height` = 本块占用的推进高度（含块后间距），
    /// `lines` = 本块源码行数。
    pub fn measure(&mut self, idx: usize, height: f32, lines: u32) {
        if idx >= self.heights.len() {
            return;
        }
        let height = if height.is_finite() { height.max(1.0) } else { 1.0 };
        let old = self.heights[idx];
        if old == 0.0 {
            self.sum_h += height as f64;
            self.n_measured += 1;
            self.refresh_fallback();
        } else if (old - height).abs() > EPS {
            // 已测量的块高度变了（换行、折叠、字号）：更新均值。
            self.sum_h += (height - old) as f64;
            self.refresh_fallback();
        } else {
            // 亚像素抖动：不改表，也不触发重算。
            self.set_lines(idx, lines);
            return;
        }
        self.heights[idx] = height;
        self.set_lines(idx, lines);
        if idx < self.dirty_from {
            self.dirty_from = idx;
        }
    }

    /// 补齐所有还没有行数的块的源码行数。只在需要显示行号、且确实缺行数时调用：
    /// 首次打开带行号的文档扫一遍全文，之后每个新块/改动块只补它自己。
    pub fn fill_lines(&mut self, blocks: &[Block]) {
        if self.lines_unknown == 0 || blocks.len() != self.heights.len() {
            return;
        }
        for (i, b) in blocks.iter().enumerate() {
            if self.lines[i] == 0 {
                self.lines[i] = count_lines(b);
                self.lines_unknown -= 1;
                if i < self.dirty_from {
                    self.dirty_from = i;
                }
            }
        }
    }

    /// 视口内需要排版的块区间（含上下边距），返回半开区间。
    /// `view_top`/`view_height` 是内容坐标系里视口的位置与高度。
    pub fn visible_range(&mut self, view_top: f32, view_height: f32) -> std::ops::Range<usize> {
        let n = self.heights.len();
        if n == 0 {
            return 0..0;
        }
        self.build();
        let bottom = view_top + view_height.max(1.0);
        // first = 顶边不超过 view_top 的最后一块（它的下半截还看得见）。
        let mut first = lower_bound(&self.tops, view_top);
        first = first.saturating_sub(1).min(n - 1);
        // last（不含）= 第一块顶边已到/超过视口底边的块。
        let mut last = lower_bound(&self.tops, bottom);
        if last > n {
            last = n;
        }
        // 边距：向上多排 MARGIN 块（覆盖惯性滚动与接缝），向下同理。
        first = first.saturating_sub(MARGIN_BLOCKS);
        last = (last + MARGIN_BLOCKS).min(n);
        if last <= first {
            first = 0;
            last = 1.min(n);
        }
        first..last
    }

    /// 高度表整篇作废（宽度/字号变了，旧实测值不可复用）。
    fn reset_heights(&mut self) {
        for h in self.heights.iter_mut() {
            *h = 0.0;
        }
        self.sum_h = 0.0;
        self.n_measured = 0;
        self.fallback_h = FALLBACK_H;
        self.dirty_from = 0;
    }

    /// 重算前缀和（只在有变化后做，代价是 O(块数) 的加法，实测 24 万块约 0.2ms）。
    fn build(&mut self) {
        if self.dirty_from == usize::MAX {
            return;
        }
        let n = self.heights.len();
        if n + 1 != self.tops.len() {
            self.tops.resize(n + 1, 0.0);
            self.line_tops.resize(n + 1, 0);
        }
        let from = self.dirty_from.min(n);
        for i in from..n {
            let h = self.height(i);
            self.tops[i + 1] = self.tops[i] + h;
            self.line_tops[i + 1] = self.line_tops[i] + self.line(i);
        }
        self.dirty_from = usize::MAX;
    }

    /// 第 i 块的高度：实测值优先，否则兜底。恒 > 0，保证分配到的矩形合法。
    fn height(&self, i: usize) -> f32 {
        let h = self.heights[i];
        if h > 0.0 { h } else { self.fallback_h.max(1.0) }
    }

    fn line(&self, i: usize) -> u32 {
        if self.lines[i] > 0 { self.lines[i] } else { FALLBACK_LINES }
    }

    fn set_lines(&mut self, idx: usize, lines: u32) {
        let lines = lines.max(1);
        if self.lines[idx] == 0 {
            self.lines_unknown = self.lines_unknown.saturating_sub(1);
            self.lines[idx] = lines;
            if idx < self.dirty_from {
                self.dirty_from = idx;
            }
        } else if self.lines[idx] != lines {
            self.lines[idx] = lines;
            if idx < self.dirty_from {
                self.dirty_from = idx;
            }
        }
    }

    fn refresh_fallback(&mut self) {
        if self.n_measured > 0 {
            self.fallback_h = (self.sum_h / self.n_measured as f64) as f32;
        }
    }

    /// 在 `at` 处把 `old_len` 个槽位换成 `new_len` 个未知槽位；
    /// 其余槽位（包括已测量的块高）原样保留，位置自动前后挪动。
    fn splice(&mut self, at: usize, old_len: usize, new_len: usize) {
        let at = at.min(self.heights.len());
        let end = (at + old_len).min(self.heights.len());
        for i in at..end {
            let h = self.heights[i];
            if h > 0.0 {
                self.sum_h -= h as f64;
                self.n_measured = self.n_measured.saturating_sub(1);
            }
            if self.lines[i] == 0 {
                self.lines_unknown = self.lines_unknown.saturating_sub(1);
            }
        }
        self.heights.splice(at..end, std::iter::repeat_n(0.0, new_len));
        self.lines.splice(at..end, std::iter::repeat_n(0, new_len));
        self.lines_unknown += new_len;
        self.refresh_fallback();
        self.dirty_from = self.dirty_from.min(at);
    }

    /// 调试/测试用：读原始表项。
    pub fn raw_height(&self, idx: usize) -> f32 {
        self.heights.get(idx).copied().unwrap_or(0.0)
    }

    pub fn raw_lines(&self, idx: usize) -> u32 {
        self.lines.get(idx).copied().unwrap_or(0)
    }
}

/// 块源码行数：与 `Document::source()` 的 `\n` 连接规则一致，恒 ≥1。
fn count_lines(b: &Block) -> u32 {
    b.source().split('\n').count() as u32
}

/// 第一个 `> y` 的下标（数组必须单调不减）。f32 比较不用 Ord，手写二分。
fn lower_bound(sorted: &[f32], y: f32) -> usize {
    let mut lo = 0usize;
    let mut hi = sorted.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if sorted[mid] <= y {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 高度表算顶边与总高：逐块累加，未测量块用兜底值。
    #[test]
    fn tops_and_total() {
        let mut l = BlockLayout::default();
        l.sync(3);
        assert_eq!(l.len(), 3);
        let fb = FALLBACK_H;
        assert_eq!(l.top(0), 0.0);
        assert_eq!(l.top(1), fb);
        assert_eq!(l.total_height(), fb * 3.0);
        l.measure(0, 10.0, 1);
        l.measure(1, 20.0, 2);
        assert_eq!(l.top(1), 10.0);
        assert_eq!(l.top(2), 30.0);
        // 第 3 块仍未测量：用前两块均值 15 兜底。
        assert_eq!(l.total_height(), 45.0);
    }

    /// 亚像素抖动不触发重算；超过阈值的改动会往后挪顶边。
    #[test]
    fn eps_gates_rebuild() {
        let mut l = BlockLayout::default();
        l.sync(2);
        l.measure(0, 30.0, 1);
        let t2 = l.top(1);
        l.measure(0, 30.0 + 0.4, 1);
        assert_eq!(l.top(1), t2, "0.4px 抖动不应改表");
        l.measure(0, 31.0, 1);
        assert_eq!(l.top(1), 31.0);
    }

    /// 删掉中间的块：后面的块高整体前移，长度对齐，被删槽位的统计要退掉。
    #[test]
    fn splice_remove_shifts() {
        let mut l = BlockLayout::default();
        l.sync(3);
        l.measure(0, 10.0, 1);
        l.measure(1, 20.0, 1);
        l.measure(2, 30.0, 1);
        l.apply(&[Change::Splice {
            at: 1,
            old_len: 1,
            new_len: 0,
        }]);
        assert_eq!(l.len(), 2);
        assert_eq!(l.raw_height(0), 10.0);
        assert_eq!(l.raw_height(1), 30.0, "后一块的实测高度应原样保留");
        assert_eq!(l.top(1), 10.0);
        assert_eq!(l.total_height(), 40.0);
    }

    /// 插入新块：新槽位标记为未测量，行号表随之要求补扫。
    #[test]
    fn splice_insert_marks_unknown() {
        let mut l = BlockLayout::default();
        l.sync(2);
        l.measure(0, 12.0, 3);
        l.measure(1, 12.0, 3);
        assert_eq!(l.lines_missing(), 0);
        l.apply(&[Change::Splice {
            at: 1,
            old_len: 0,
            new_len: 2,
        }]);
        assert_eq!(l.len(), 4);
        assert_eq!(l.raw_height(1), 0.0);
        assert_eq!(l.raw_lines(2), 0);
        assert_eq!(l.lines_missing(), 2);
        // 已知的两块仍保持原位：第 3 块顶边 = 12 + 兜底×2。
        assert_eq!(l.top(3), 12.0 + l.fallback_h * 2.0);
    }

    /// Rebuild（撤销、源码模式落盘等）把整张表清空。
    #[test]
    fn rebuild_clears_everything() {
        let mut l = BlockLayout::default();
        l.sync(2);
        l.measure(0, 12.0, 3);
        l.apply(&[Change::Rebuild]);
        assert!(l.is_empty());
        assert_eq!(l.total_height(), 0.0);
        l.sync(2);
        assert_eq!(l.raw_height(0), 0.0);
        assert_eq!(l.lines_missing(), 2);
    }

    /// 行号前缀和：按各块源码行数累加，供行号栏显示「本块起始行号」。
    #[test]
    fn line_numbers_prefix_sum() {
        let mut l = BlockLayout::default();
        l.sync(3);
        l.measure(0, 10.0, 2);
        l.measure(1, 10.0, 3);
        l.measure(2, 10.0, 1);
        assert_eq!(l.lines_before(0), 0);
        assert_eq!(l.lines_before(1), 2);
        assert_eq!(l.lines_before(2), 5);
    }

    /// 视口范围：落在中间某块内部时，该块也要排进来（它下半截可见）。
    #[test]
    fn visible_range_covers_partial_block() {
        let mut l = BlockLayout::default();
        for _ in 0..100 {
            l.sync(l.len() + 1);
        }
        for i in 0..100 {
            l.measure(i, 10.0, 1);
        }
        // 视口 305..325：块 30 占 300..310（上半截在视口外），块 32 占 320..330。
        let (m, n) = (MARGIN_BLOCKS, 100);
        let r = l.visible_range(305.0, 20.0);
        assert_eq!(r.start, 30usize.saturating_sub(m));
        assert_eq!(r.end, (33 + m).min(n), "下边界含首个越过视口底边的块");
    }

    /// 边距不会越界，空表返回空区间。
    #[test]
    fn visible_range_clamps() {
        let mut l = BlockLayout::default();
        assert_eq!(l.visible_range(0.0, 600.0), 0..0);
        l.sync(3);
        l.measure(0, 10.0, 1);
        l.measure(1, 10.0, 1);
        l.measure(2, 10.0, 1);
        let r = l.visible_range(9.0, 600.0);
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 3);
    }

    /// `sync` 少退多补：长度对齐块数，补出来的槽位一律未知。
    #[test]
    fn sync_shrinks_and_grows() {
        let mut l = BlockLayout::default();
        l.sync(4);
        l.measure(0, 10.0, 1);
        l.measure(3, 11.0, 1);
        l.sync(2);
        assert_eq!(l.len(), 2);
        assert_eq!(l.raw_height(0), 10.0);
        l.sync(5);
        assert_eq!(l.len(), 5);
        assert_eq!(l.raw_height(4), 0.0);
        assert_eq!(l.raw_lines(4), 0);
    }

    /// `fill_lines` 补齐未测量块的行数：按真实解析出的块源码行数，只补缺、不重扫已知的。
    #[test]
    fn fill_lines_uses_real_block_sources() {
        // 空行在 markd 里也是独立块：标题、空块、段落(2 行)、空块、列表 → 5 块。
        let blocks = crate::ast::parse("# a\n\nb\nc\n\n- x");
        assert_eq!(blocks.len(), 5);
        let mut l = BlockLayout::default();
        l.sync(blocks.len());
        assert_eq!(l.lines_missing(), 5);
        // 先「排版测量」过第 1 块（段落 b/c）：行数由实测给出，不应被 fill_lines 覆盖。
        l.measure(2, 20.0, 2);
        assert_eq!(l.lines_missing(), 4);
        l.fill_lines(&blocks);
        assert_eq!(l.lines_missing(), 0);
        assert_eq!(
            (0..5).map(|i| l.raw_lines(i)).collect::<Vec<_>>(),
            vec![1, 1, 2, 1, 1]
        );
        assert_eq!(l.lines_before(4), 5, "第 4 块之前累计 5 行");
    }

    /// 非法高度（NaN / 0）不能污染前缀和，否则整篇顶边全变 NaN。
    #[test]
    fn measure_clamps_bad_height() {
        let mut l = BlockLayout::default();
        l.sync(2);
        l.measure(0, f32::NAN, 1);
        assert_eq!(l.raw_height(0), 1.0);
        l.measure(1, -5.0, 0);
        assert_eq!(l.raw_height(1), 1.0);
        assert!(l.total_height().is_finite());
    }
}
