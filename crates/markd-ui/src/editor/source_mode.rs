//! 源码模式：整篇文档用一个多行编辑器显示，带行号与光标行底色。
//!
//! 从 `view::show_editor` 中抽出，行为保持不变。

use super::*;

/// 源码模式的派生数据，按文档修订号缓存。
///
/// 1000 万字实测每帧要付：整篇源码拷贝 19ms + 全文行首表 28ms + 最长行宽度 30ms
/// + 全文行号表 26ms（`MARKD_STRESS=1 cargo test stress_source_parts`）。这四项
/// 都只在「文档真的改过」时才需要重算，静置和滚动时纯属白扫——源码模式一帧
/// 113ms（8 帧/秒）就是这么来的。
#[derive(Debug)]
pub struct SourceCache {
    /// 派生数据对应的文档修订号；`u64::MAX` = 还没算过。
    rev: u64,
    /// 算 `content_w` 时用的正文字号：字号变了宽度就作废。
    mono: f32,
    /// 整篇源码。`TextEdit` 要 `&mut dyn TextBuffer`，每帧把它借出去、帧末放回。
    src: String,
    /// 每个源码行首字符的下标（`CCursor` 按 char 计数）。
    starts: Vec<usize>,
    /// 全文字符数。
    chars: usize,
    /// 最长行的估算宽度，决定编辑框撑多宽、横向滚动能滚多远。
    content_w: f32,
    /// 上一帧实测的「光标行 (行号, 行顶 y, 行高)」。光标行底色必须垫在文字下层，
    /// 只能在 `TextEdit` 之前画，所以只能用上一帧的行几何（滞后一帧，不可见）。
    band: Option<(usize, f32, f32)>,
}

impl Default for SourceCache {
    fn default() -> Self {
        Self {
            rev: u64::MAX,
            mono: 0.0,
            src: String::new(),
            starts: vec![0],
            chars: 0,
            content_w: 0.0,
            band: None,
        }
    }
}

impl SourceCache {
    /// 由整篇源码重建派生数据（全文三趟，只在修订号或字号变化后付一次）。
    fn rebuild(&mut self, rev: u64, mono: f32, src: String) {
        let mut starts = vec![0usize];
        let mut chars = 0usize;
        for c in src.chars() {
            if c == '\n' {
                starts.push(chars + 1);
            }
            chars += 1;
        }
        // 长行宽度估算（ASCII 半宽、CJK 全宽）：编辑框按最长行撑宽，
        // 配合横向滚动；窄于视口时照常占满。
        let content_w = src
            .lines()
            .map(|l| {
                let w: f32 = l
                    .chars()
                    .map(|c| if c.is_ascii() { 0.62 } else { 1.25 })
                    .sum();
                w * mono
            })
            .fold(0.0f32, f32::max);
        // `band` 是上一帧的实测行几何，与文档内容只有「差一帧」的关系，
        // 改文档时留着比清空好：否则每敲一个字，光标行底色就闪没一帧。
        let band = self.band;
        *self = Self {
            rev,
            mono,
            src,
            starts,
            chars,
            content_w,
            band,
        };
    }

    /// 派生数据是否对应当前修订号（测试用）。
    #[cfg(test)]
    fn is_fresh(&self, rev: u64) -> bool {
        self.rev == rev
    }
}

/// 视口内可见的 galley 行下标区间 `[first, last)`。
///
/// 行 `pos.y` 相对 galley 原点且严格递增，所以能二分。不换行的源码里
/// 「一个源码行 = 一个视觉行」，行号就是 `下标 + 1`：32 万行只有屏上那几十行
/// 需要画行号，剩下的连遍历都不该遍历（全量扫一遍实测 26ms）。
fn visible_rows(
    rows: &[egui::epaint::text::PlacedRow],
    clip: egui::Rect,
    at: egui::Pos2,
) -> (usize, usize) {
    let top = clip.top() - at.y;
    let bottom = clip.bottom() - at.y;
    (
        rows.partition_point(|r| r.pos.y + r.row.size.y < top),
        rows.partition_point(|r| r.pos.y <= bottom),
    )
}

#[allow(deprecated)]
pub(super) fn show_source_mode(
    ui: &mut egui::Ui,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    ctx: &egui::Context,
    show_line_numbers: bool,
    tab_key: u64,
) -> EditorEvent {
    // 派生数据只在文档改过（或字号改过）之后重算：静置、滚动时一帧都不扫全文。
    if state.source.rev != doc.rev || state.source.mono != theme.mono {
        state.source.rebuild(doc.rev, theme.mono, doc.source());
    }
    // TextEdit 会随内容自动撑高，直接放在父 Ui 上会被裁剪、滚轮无处生效，
    // 表现就是源码模式下鼠标滚不动；必须像下方混合模式一样包一层 ScrollArea。
    let mut changed = false;
    let mut editor_rect = egui::Rect::NOTHING;
    // 行号栏宽度固定（三种模式共用），不随行数位数、窗口、模式变化。
    let gutter_w = gutter_width(show_line_numbers);
    // 当前光标所在的源码行（1 起）：在行首表里二分，O(log 行数)。
    let cur_line = {
        let cur = state.last_cursor.min(state.source.chars);
        match state.source.starts.binary_search(&cur) {
            Ok(i) => i + 1,
            Err(i) => i.min(state.source.starts.len()).max(1),
        }
    };
    let content_w = state.source.content_w;
    let band = state.source.band;
    // 把源码字符串从缓存里借出来给 TextEdit 改写，帧末放回（见函数尾）。
    let mut src = std::mem::take(&mut state.source.src);
    let mut band_next: Option<(usize, f32, f32)> = None;
    // 纵向 + 横向都要滚：长行不换行，超出视口的部分靠横向滚动露出
    //（光标移动时 TextEdit 会调 scroll_to_rect，ScrollArea 跟随平移）。
    let scroll = egui::ScrollArea::both()
        .id_salt("source_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // 行号列与光标行底色的 x 边界：必须用 ScrollArea 自己的裁剪区。
            // 之前用外层 ui 的 clip_rect，其左缘在 ScrollArea 内容裁剪区之左，
            // 数字几乎全被裁掉，只剩最右侧一个"点"。
            let vpl = ui.clip_rect().left();
            let vpr = ui.clip_rect().right();
            // 不换行：一行源码 = 一个视觉行。换行会让一个源码行占多行，
            // 行号随之忽上忽下（用户反馈的"跳动"）。
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                let mut job = egui::text::LayoutJob::default();
                // 行距 = 字号 × 配置的「源码模式行距」（默认 1.4）：行与行（含空行）
                // 之间的间隔更舒展，行号数字不再显得挤在一起。
                let mut fmt = theme.mono_fmt();
                fmt.line_height = Some(theme.mono * theme.line_height);
                job.append(buf.as_str(), 0.0, fmt);
                job.wrap.max_width = f32::INFINITY;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let edit_w = (ui.available_width() - gutter_w).max(content_w);

            // 当前光标行的整行淡色底：必须在 TextEdit 之前画才能垫在文字下层。
            // 从行号栏右侧延伸到编辑区右缘；未显示行号时从正文左缘开始。
            // 行几何取上一帧的实测值——请求的行高是 `mono * 1.4`（18.9），
            // 排版出来是 19.0，按「行号 × 行高」算到 30 万行会漂出上万像素。
            if let Some((_, y, h)) = band {
                let left = if show_line_numbers {
                    vpl + gutter_w
                } else {
                    vpl
                };
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::Pos2::new(left, ui.cursor().top() + y),
                        egui::Pos2::new(vpr, ui.cursor().top() + y + h),
                    ),
                    0.0,
                    theme.line_highlight(),
                );
            }
            let out = ui.horizontal(|ui| {
                if show_line_numbers {
                    // 固定 70px 行号栏：扣掉水平 item_spacing，正文左缘正好落在
                    // 行号栏右缘；数字本身在下面按视口固定 x 补画。
                    let spacing = ui.spacing().item_spacing.x;
                    ui.add_space((gutter_w - spacing).max(0.0));
                }
                let te = TextEdit::multiline(&mut src)
                    .id(Id::new(("source_editor", tab_key)))
                    .font(egui::TextStyle::Monospace)
                    .desired_width(edit_w)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .layouter(&mut layouter)
                    .code_editor();
                te.show(ui)
            });
            let out = out.inner;
            // 记下本帧真实行几何，下一帧拿它垫光标行底色。
            if let Some(row) = out.galley.rows.get(cur_line - 1) {
                band_next = Some((cur_line, row.pos.y, row.row.size.y));
            }
            // 行号：只画视口内那几十行，固定画在视口左缘（不随横向滚动移动）；
            // 光标所在行的行号用主题色高亮。
            if show_line_numbers {
                let (first, last) =
                    visible_rows(&out.galley.rows, ui.clip_rect(), out.galley_pos);
                let p = ui.painter();
                // 数字右对齐到 gutter 右缘内固定间距，与混合模式完全一致。
                let num_right = vpl + gutter_w - GUTTER_PAD;
                for (k, row) in out.galley.rows[first..last].iter().enumerate() {
                    let line_no = first + k + 1;
                    let color = if line_no == cur_line {
                        theme.accent
                    } else {
                        theme.dim.gamma_multiply(0.7)
                    };
                    p.text(
                        egui::Pos2::new(
                            num_right,
                            out.galley_pos.y + row.pos.y + row.row.size.y * 0.5,
                        ),
                        egui::Align2::RIGHT_CENTER,
                        line_no.to_string(),
                        egui::FontId::monospace(theme.mono),
                        color,
                    );
                }
            }
            changed = out.response.changed();
            editor_rect = out.response.rect;
            out
        });
    state.source.band = band_next;
    // 源码模式同样支持「点空白落光标」：点在编辑框外（页边距/正文下方）时，
    // 把光标移到文档末尾（最后一行）并立刻聚焦，行为与混合模式一致。
    let pointer = ctx.input(|i| i.pointer.interact_pos());
    let clicked = ctx.input(|i| i.pointer.primary_clicked());
    let bar_zone = scroll_bar_zone(ui, scroll.inner_rect, scroll.content_size.y);
    let blank = pointer.filter(|p| {
        clicked
            && ui.max_rect().contains(*p)
            && !bar_zone.contains(*p)
            && !editor_rect.contains(*p)
            && ctx
                .layer_id_at(*p)
                .is_none_or(|layer| layer == ui.layer_id())
    });
    // 源码模式也弹出同一个上下文菜单。与混合模式保持一致：只要在编辑区
    // 可见范围内右键即可，不再要求必须点在 TextEdit 矩形之外，避免
    // 「有时能弹、有时不能弹」。
    if ctx.input(|i| i.pointer.secondary_clicked())
        && let Some(p) = pointer.filter(|p| {
            (ui.max_rect().contains(*p) || ui.clip_rect().contains(*p))
                && !bar_zone.contains(*p)
                && ctx
                    .layer_id_at(*p)
                    .is_none_or(|layer| layer == ui.layer_id())
        })
    {
        state.context_menu = Some(p);
    }
    if let Some(p) = blank {
        let id = Id::new(("source_editor", tab_key));
        // 字符数取缓存值（它是本帧开始时整篇源码的长度）。同一帧里既改文本
        // 又点空白不可能发生，所以这点滞后不会落到用户身上。
        let end = egui::text::CCursor::new(state.source.chars);
        if let Some(mut ts) = egui::TextEdit::load_state(ui.ctx(), id) {
            ts.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
            ts.store(ui.ctx(), id);
        }
        ctx.memory_mut(|m| m.request_focus(id));
        ctx.request_repaint();
        trace_blank(&pointer, &format!("source -> 文档末尾 (点在 {p:?})"));
    } else {
        trace_blank(&pointer, "source miss");
    }
    if changed {
        doc.maybe_record();
        doc.set_source(&src);
    }
    // 放回缓存。若这一帧改过文本，`doc.rev` 已经变了，下一帧自动重建。
    state.source.src = src;
    if changed {
        EditorEvent::Changed
    } else {
        EditorEvent::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 朴素算法：逐字符扫一遍求行首下标、字符数与最长行宽度，用来对照缓存。
    fn naive(src: &str, mono: f32) -> (Vec<usize>, usize, f32) {
        let mut starts = vec![0usize];
        let mut chars = 0usize;
        for c in src.chars() {
            if c == '\n' {
                starts.push(chars + 1);
            }
            chars += 1;
        }
        let content_w = src
            .lines()
            .map(|l| {
                let w: f32 = l
                    .chars()
                    .map(|c| if c.is_ascii() { 0.62 } else { 1.25 })
                    .sum();
                w * mono
            })
            .fold(0.0f32, f32::max);
        (starts, chars, content_w)
    }

    /// 缓存重建必须和朴素扫描逐项一致：尤其中文按 char 计、行首下标要落在
    /// `\n` 之后一位、空行与无尾换行的边界。
    #[test]
    fn source_cache_matches_naive_scan() {
        let mono = 13.5;
        for src in [
            "",
            "a",
            "一行中文\nsecond line\n",
            "# 标题\n\n段落 with ASCII and 中文 混排\n> 引用\n- 列表\n",
            "\n\n\n",
            "尾部无换行\n最后一行",
        ] {
            let mut cache = SourceCache::default();
            cache.rebuild(7, mono, src.to_string());
            let (starts, chars, content_w) = naive(src, mono);
            assert_eq!(cache.starts, starts, "行首表 {src:?}");
            assert_eq!(cache.chars, chars, "字符数 {src:?}");
            assert_eq!(cache.content_w, content_w, "最长行宽度 {src:?}");
            assert!(cache.is_fresh(7));
            assert!(!cache.is_fresh(8));
        }
    }

    /// 默认缓存不能被判成「已是最新」：`rev` 初值必须落在任何真实修订号之外，
    /// 否则新建文档（rev = 0）第一次进源码模式会拿空行首表去排版。
    #[test]
    fn empty_cache_is_never_fresh() {
        let cache = SourceCache::default();
        assert!(!cache.is_fresh(0), "文档首个修订号就是 0");
        assert_eq!(cache.starts, vec![0]);
    }

    /// 改过文本后重建不能顺手清掉底色行几何：否则每敲一个字，
    /// 光标行底色就闪没一帧。
    #[test]
    fn rebuild_keeps_band_from_previous_frame() {
        let mut cache = SourceCache::default();
        cache.rebuild(1, 13.0, "a\nb\n".into());
        cache.band = Some((2, 19.0, 19.0));
        cache.rebuild(2, 13.0, "a\nb\nc\n".into());
        assert_eq!(cache.band, Some((2, 19.0, 19.0)));
    }

    /// 用真实排版结果构造 galley：每行一段不同长度的文本。
    fn galley_of(lines: &[&str]) -> std::sync::Arc<egui::Galley> {
        let ctx = egui::Context::default();
        // 字体要到第一次 run 之后才可用。
        let mut warm = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |_| {},
        );
        warm.textures_delta.clear();
        let mut job = egui::text::LayoutJob::default();
        let fmt = egui::TextFormat {
            font_id: egui::FontId::monospace(13.0),
            line_height: Some(19.0),
            ..Default::default()
        };
        job.append(&lines.join("\n"), 0.0, fmt);
        job.wrap.max_width = f32::INFINITY;
        ctx.fonts_mut(|f| f.layout_job(job))
    }

    /// 「一个源码行 = 一个视觉行」是「行号取自行下标」的前提：
    /// 排版结果必须逐行对齐，字符数累加等于全文。
    #[test]
    fn one_source_line_is_one_galley_row() {
        let lines: Vec<String> = (0..200).map(|i| format!("第 {i} 行 内容 {i}")).collect();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let g = galley_of(&refs);
        assert_eq!(g.rows.len(), refs.len(), "不换行时视觉行数 = 源码行数");
        let mut acc = 0usize;
        for (i, row) in g.rows.iter().enumerate() {
            assert_eq!(
                row.text().trim_end_matches('\n'),
                lines[i].as_str(),
                "第 {i} 视觉行的内容错位"
            );
            acc += row.char_count_including_newline().0;
        }
        assert_eq!(acc, lines.join("\n").chars().count(), "各行字符数累加 = 全文");
    }

    /// 二分出可见区间（新实现）与逐行线性过滤（旧实现等价）必须给出同一批行：
    /// 少一行 = 行号缺失，多一行 = 白画。视口位置/高度取多种组合。
    #[test]
    fn visible_rows_covers_exactly_the_clip_rect() {
        let lines: Vec<String> = (0..200).map(|i| format!("第 {i} 行 内容 {i}")).collect();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let g = galley_of(&refs);
        for at_y in [0.0f32, 3.0, -18.0, 250.0, 3000.0] {
            for h in [100.0f32, 1.0, 600.0] {
                let clip =
                    egui::Rect::from_min_max(egui::pos2(0.0, at_y), egui::pos2(600.0, at_y + h));
                for at_y2 in [0.0f32, -120.0, 500.0] {
                    let at = egui::pos2(0.0, at_y2);
                    let (first, last) = visible_rows(&g.rows, clip, at);
                    let top = clip.top() - at.y;
                    let bottom = clip.bottom() - at.y;
                    let brute: Vec<usize> = (0..g.rows.len())
                        .filter(|&i| {
                            let r = &g.rows[i];
                            r.pos.y + r.row.size.y >= top && r.pos.y <= bottom
                        })
                        .collect();
                    assert_eq!(
                        (first..last).collect::<Vec<usize>>(),
                        brute,
                        "二分区间与线性扫描不符 (clip={clip:?} at={at:?})"
                    );
                }
            }
        }
    }
}
