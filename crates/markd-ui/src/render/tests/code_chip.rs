//! 行内代码片的几何判据。
//!
//! 用户反馈「行内代码和后面的文字不连贯、空格很大」：实测根因是芯片底色
//! 按固定 3px 内边距铺，把源文里那个 4.1px 的空格整个吃掉（芯片边框距相邻
//! 汉字只剩 0.3px），而代码自己的墨迹离边框还有 5.7px——看着就是一个空盒子
//! 贴着正文。竖向又按整行行高铺，标题里 43px 的行高会撑出一个大方框。
//!
//! 这里用真实字体（`fonts::install`）排版，逐条钉住修好后的几何关系。

use super::*;
use crate::ast::Block;
use crate::document::Document;
use crate::theme::Theme;

/// 排版一段含行内代码的文字，返回 (galley, 芯片矩形列表)。
fn chip_layout(ctx: &egui::Context, theme: &Theme, inlines: &[crate::ast::Inline], size: f32) -> (std::sync::Arc<egui::Galley>, Vec<egui::Rect>) {
    let job = crate::render::build_layout(inlines, theme, size, theme.text, false);
    let galley = ctx.fonts_mut(|f| f.layout_job(job));
    let shapes = crate::render::inline_code_bg_shapes(&galley, egui::Pos2::ZERO, theme);
    // 浅色方案还会额外描一圈边（同一个矩形的 Shape::Rect），按底色筛掉重复。
    let bg = theme.inline_code_bg();
    let rects = shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Rect(r) if r.fill == bg => Some(r.rect),
            _ => None,
        })
        .collect();
    (galley, rects)
}

/// 把 Markdown 源码解析成行内节点（只取第一个段落的 content）。
fn inlines_of(src: &str) -> Vec<crate::ast::Inline> {
    let doc = Document::new(src, None);
    let b = doc.blocks.first().expect("首块");
    match b {
        Block::Paragraph { content, .. }
        | Block::Heading { content, .. }
        | Block::Quote { content, .. }
        | Block::ListItem { item: crate::ast::ListItem { content, .. }, .. } => content.clone(),
        other => panic!("用例需要文字块，实际 {other:?}"),
    }
}

/// 某个字符下标所在字形单元的 [左, 右] 边界（推进宽度口径）。
fn cell(g: &egui::Galley, ch: usize) -> (f32, f32) {
    let l = g.pos_from_cursor(egui::text::CCursor::new(ch)).left();
    let r = g.pos_from_cursor(egui::text::CCursor::new(ch + 1)).left();
    (l.min(r), l.max(r))
}

fn ctx_with_fonts() -> std::sync::Arc<egui::Context> {
    let ctx = std::sync::Arc::new(egui::Context::default());
    markd_platform::fonts::install(&ctx);
    let _ = ctx.run_ui(egui::RawInput::default(), |_ui| {});
    ctx
}

/// 源文里有空格时：芯片必须贴着代码自己的单元边界（不吞空格、不放大内边距），
/// 并且与相邻汉字之间留出可见空隙。
#[test]
fn code_chip_hugs_code_and_keeps_visible_gap_from_neighbors() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = ctx_with_fonts();
    let src = "前文 `inline_code` 后文";
    let g = inlines_of(src);
    let (galley, rects) = chip_layout(&ctx, &theme, &g, theme.body);
    assert_eq!(rects.len(), 1, "应只有一个代码片，实际 {rects:?}");
    let chip = rects[0];

    // 代码单元区间：从 plain 里找到 `inline_code` 在所见文本中的起止字符。
    let plain = crate::ast::inline_plain(&g);
    let needle = "inline_code";
    let before: usize = plain[..plain.find(needle).expect("代码在所见文本里")]
        .chars()
        .count();
    let code_l = cell(&galley, before).0;
    let code_r = cell(&galley, before + needle.chars().count() - 1).1;

    // ① 芯片必须完整盖住代码单元，且只多出一小圈内边距（≤2px）。
    assert!(
        chip.left() <= code_l && chip.right() >= code_r,
        "芯片没有盖住代码单元：芯片 {chip:?}，代码单元 {code_l:.1}..{code_r:.1}"
    );
    assert!(
        code_l - chip.left() <= 2.0 && chip.right() - code_r <= 2.0,
        "芯片内边距又变大了：左 {:.1} 右 {:.1}（代码单元 {code_l:.1}..{code_r:.1}）",
        code_l - chip.left(),
        chip.right() - code_r
    );

    // ② 与相邻汉字之间必须看得见空隙（旧实现只剩 0.3px，等于贴着正文）。
    let prev = galley.rows[0]
        .glyphs
        .iter()
        .filter(|gl| gl.pos.x + gl.advance_width <= code_l + 0.01 && !gl.chr.is_whitespace())
        .max_by(|a, b| a.pos.x.total_cmp(&b.pos.x))
        .expect("代码左边有汉字");
    let next = galley.rows[0]
        .glyphs
        .iter()
        .filter(|gl| gl.pos.x >= code_r - 0.01 && !gl.chr.is_whitespace())
        .min_by(|a, b| a.pos.x.total_cmp(&b.pos.x))
        .expect("代码右边有汉字");
    let gap_l = chip.left() - (prev.pos.x + prev.advance_width);
    let gap_r = next.pos.x - chip.right();
    assert!(
        gap_l >= 1.0 && gap_r >= 1.0,
        "芯片与相邻汉字之间没有可见空隙：左 {gap_l:.1} 右 {gap_r:.1}"
    );
}

/// 中文习惯常写作 `文字\`code\`文字`（不带空格）。这时没有余地放内边距，
/// 芯片必须收在相邻字形边界之内，不能压到汉字笔画上。
#[test]
fn code_chip_does_not_overlap_neighbors_when_no_space() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = ctx_with_fonts();
    let (galley, rects) = chip_layout(&ctx, &theme, &inlines_of("紧凑前文`x`紧凑后文"), theme.body);
    assert_eq!(rects.len(), 1, "应只有一个代码片，实际 {rects:?}");
    let chip = rects[0];
    let plain = crate::ast::inline_plain(&inlines_of("紧凑前文`x`紧凑后文"));
    let before = plain[..plain.find('x').expect("代码在所见文本里")].chars().count();
    // 相邻汉字自己的单元边界（按字形推进宽度算，和产品内的口径一致；
    // pos_from_cursor 有 0.3px 取整差，不能混用）。
    let row = &galley.rows[0];
    let gi = row
        .glyphs
        .iter()
        .position(|g| g.chr == 'x')
        .expect("代码字形");
    let code_l = row.glyphs[gi].pos.x;
    let code_r = row.glyphs[gi].pos.x + row.glyphs[gi].advance_width;
    let prev_right = row.glyphs[gi - 1].pos.x + row.glyphs[gi - 1].advance_width;
    let next_left = row.glyphs[gi + 1].pos.x;
    // 等宽单元与相邻汉字单元本身就有 ~0.5px 的推进宽度取整重叠，
    // 所以判据是：芯片绝不越过相邻单元边界，同时把代码单元（容 0.6px 取整）
    // 盖住——代码字母的墨迹在单元内居中，0.5px 的单元边缘不会切到笔画。
    assert!(
        chip.left() >= prev_right - 0.01 && chip.right() <= next_left + 0.01,
        "无空格时芯片越过相邻字形边界：芯片 [{:.1}, {:.1}]，可用区间 [{prev_right:.1}, {next_left:.1}]",
        chip.left(),
        chip.right()
    );
    assert!(
        chip.left() <= code_l + 0.6 && chip.right() >= code_r - 0.6,
        "避让把代码切掉太多了：芯片 {chip:?}，代码单元 {code_l:.1}..{code_r:.1}"
    );
    let _ = before;
}

/// 标题里正文行高明显更大（43px），芯片必须贴着等宽字形自己的字身框，
/// 不能再按整行行高铺出一个大方框。
#[test]
fn code_chip_in_heading_hugs_glyph_box_not_row_height() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = ctx_with_fonts();
    let size = theme.heading_size(1);
    let (galley, rects) = chip_layout(&ctx, &theme, &inlines_of("# 标题里带 `code` 的标题"), size);
    let chip = *rects.first().expect("标题里的代码片");
    let row_h = galley.rows[0].row.size.y;
    // 等宽字形自己的字身框高度。
    let mono = galley.rows[0]
        .glyphs
        .iter()
        .find(|gl| gl.chr == 'c')
        .expect("代码字形");
    let box_h = mono.font_height;
    assert!(
        chip.height() <= box_h + 4.5,
        "芯片高度没有贴着等宽字身框：芯片 {:.1}，字身框 {box_h:.1}",
        chip.height()
    );
    assert!(
        chip.height() < row_h - 4.0,
        "芯片仍然撑满整行：芯片 {:.1}，行高 {row_h:.1}",
        chip.height()
    );
    // 也不能矮到切掉字形（上伸 + 下延都要在里面）。
    let ink_h = mono.font_ascent + (mono.font_height - mono.font_ascent);
    assert!(
        chip.height() >= ink_h,
        "芯片比字形字身框还矮，会切掉笔画：芯片 {:.1}，字身框 {ink_h:.1}",
        chip.height()
    );
}
