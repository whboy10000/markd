//! 行内文本控件：标签排版、表格单元格对齐、双击选词。

use super::*;

/// 用显式宽度排版成 galley 再交给 Label，并把 galley 一并返回供点击命中测试。
/// `Label` 默认按 Ui 的 wrap 模式决定宽度，横向布局（引用/列表）里会退化成
/// 无限宽导致长段落不换行；这里直接给定宽度，任何布局下都会换行。
pub(crate) fn layout_label(
    ui: &mut egui::Ui,
    mut job: LayoutJob,
    max_w: f32,
    sense: Sense,
) -> (egui::Response, std::sync::Arc<egui::Galley>) {
    let avail = ui.available_width();
    let w = if avail.is_finite() {
        avail.min(max_w)
    } else {
        max_w
    };
    job.wrap.max_width = w.max(80.0);
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let resp = ui.add(egui::Label::new(egui::WidgetText::Galley(galley.clone())).sense(sense));
    (resp, galley)
}

pub(crate) fn wrapped_label(
    ui: &mut egui::Ui,
    job: LayoutJob,
    max_w: f32,
    sense: Sense,
) -> egui::Response {
    layout_label(ui, job, max_w, sense).0
}

/// 单元格内按列对齐方式放置文字：左=默认；居中=vertical_centered；
/// 右=right_to_left 贴右。修复对齐按钮无效的 bug：此前渲染态完全没
/// 使用 align 数组，点了「居中/右对齐」源码变了但显示仍是左对齐。
pub(crate) fn align_cell_label(
    ui: &mut egui::Ui,
    job: LayoutJob,
    max_w: f32,
    a: crate::ast::Align,
    theme: &Theme,
) -> egui::Response {
    // 单元格里的行内代码也要有圆角代码片：占一个绘制槽，排版后填进去。
    let label = |ui: &mut egui::Ui, job: LayoutJob| -> egui::Response {
        let slot = ui.painter().add(egui::Shape::Noop);
        let (resp, galley) = layout_label(ui, job, max_w, Sense::hover());
        let shapes = inline_code_bg_shapes(&galley, resp.rect.min, theme);
        if !shapes.is_empty() {
            ui.painter().set(slot, egui::Shape::Vec(shapes));
        }
        resp
    };
    match a {
        crate::ast::Align::Left => label(ui, job),
        crate::ast::Align::Center => ui.vertical_centered(|ui| label(ui, job)).inner,
        crate::ast::Align::Right => {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                label(ui, job)
            })
            .inner
        }
    }
}

/// 行内代码的字符区间（基于 galley 文本的字符下标）：
/// 等宽字体 + 行内代码文字色 + 透明背景的 section 就是代码片。
fn inline_code_ranges(galley: &egui::Galley, theme: &Theme) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    for sec in &galley.job.sections {
        let bytes = sec.byte_range.start.0..sec.byte_range.end.0;
        let n = galley.job.text[bytes].chars().count();
        let f = &sec.format;
        if f.font_id.family == FontFamily::Monospace
            && f.color == theme.inline_code_fg()
            && f.background == Color32::TRANSPARENT
        {
            out.push((pos, pos + n));
        }
        pos += n;
    }
    out
}

/// 行内代码片：按字符区间映射到每行的矩形，画圆角底色。
///
/// 内边距按实测定：16px 正文里源文的空格推进 4.1px，等宽字符单元 8.5px 而字母
/// 实际墨迹只有约 3–7px（左右各有 ~2.7px 字距）。之前横向各放 3.0px，芯片边框
/// 距相邻汉字只剩 0.3px，等于把空格吞掉、方框直接贴上正文，而代码自己的字离
/// 边框还有 5.7px 空底——看着就是「空盒子挨着正文，字飘在中间」。收到 1.5px 后
/// 两侧各自留出 2.6px 可见空白，芯片也不再吞掉源文空格。
pub const PAD_X: f32 = 1.5;
/// 竖向贴着等宽字形自己的字身框再放 2px：撑满整行会让标题里的代码片变成大方框。
pub const PAD_Y: f32 = 2.0;

/// 行内代码片：按字符区间映射到每行的矩形，画圆角底色。
/// 浅色方案 B 额外描一圈 1px 冷灰边，深色方案 C 不描边。
pub fn inline_code_bg_shapes(
    galley: &egui::Galley,
    origin: egui::Pos2,
    theme: &Theme,
) -> Vec<egui::Shape> {
    let ranges = inline_code_ranges(galley, theme);
    if ranges.is_empty() {
        return Vec::new();
    }
    let bg = theme.inline_code_bg();
    let border = theme.inline_code_border();
    let mut shapes = Vec::new();
    let mut acc = 0usize;
    for row in &galley.rows {
        let n = row.char_count_including_newline().0;
        let (rs, re) = (acc, acc + n);
        acc = re;
        for &(cs, ce) in &ranges {
            let lo = cs.max(rs);
            let hi = ce.min(re);
            if lo >= hi {
                continue;
            }
            let x0 = galley.pos_from_cursor(egui::text::CCursor::new(lo)).left();
            let x1 = galley.pos_from_cursor(egui::text::CCursor::new(hi)).left();
            // 内边距不能越过相邻字形的边界：中文正文里 `文字\`code\`文字` 不留空格
            // 很常见，源文没有空格就没有可放色块的余地，硬放 1.5px 会压到相邻汉字
            // 的墨迹上（实测重叠 1.2–1.5px，看着像错字）。跳过空格找最近的非空白
            // 邻居，用它与代码区间之间的实际空隙钳住内边距。
            let prev_edge = row.glyphs[..lo.saturating_sub(rs).min(row.glyphs.len())]
                .iter()
                .rev()
                .find(|g| !g.chr.is_whitespace())
                .map(|g| g.pos.x + g.advance_width);
            let next_edge = row.glyphs[hi.saturating_sub(rs).min(row.glyphs.len())..]
                .iter()
                .find(|g| !g.chr.is_whitespace())
                .map(|g| g.pos.x);
            let pad_l = PAD_X.min(prev_edge.map(|e| (x0 - e).max(0.0)).unwrap_or(PAD_X));
            let pad_r = PAD_X.min(next_edge.map(|e| (e - x1).max(0.0)).unwrap_or(PAD_X));
            // 竖向贴着等宽字形自己的字身框（基线 ± 该字号的上伸/下延），不再撑满整行。
            // 之前用整行行高（16px 正文那行是 22px），芯片比代码字形本身高出约 4px，
            // 上下各空一截，看起来就是「一个大方框里飘着几个小字母」。
            let mut top = f32::INFINITY;
            let mut bottom = f32::NEG_INFINITY;
            for (gi, gl) in row.glyphs.iter().enumerate() {
                let k = rs + gi;
                if k < lo || k >= hi {
                    continue;
                }
                let desc = (gl.font_height - gl.font_ascent).max(0.0);
                top = top.min(gl.pos.y - gl.font_ascent);
                bottom = bottom.max(gl.pos.y + desc);
            }
            let (ty, by) = if bottom > top {
                (top - PAD_Y, bottom + PAD_Y)
            } else {
                // 区间里一个字形都没有（被折断到别的行等极端情况）：退回行框。
                (1.0, row.row.size.y - 1.0)
            };
            let rect = Rect::from_min_max(
                Pos2::new(x0 - pad_l, row.pos.y + ty),
                Pos2::new(x1.max(x0) + pad_r, row.pos.y + by),
            )
            .translate(origin.to_vec2());
            shapes.push(egui::Shape::rect_filled(rect, 3.0, bg));
            if let Some(c) = border {
                shapes.push(egui::Shape::rect_stroke(
                    rect,
                    3.0,
                    Stroke::new(1.0, c),
                    egui::StrokeKind::Inside,
                ));
            }
        }
    }
    shapes
}

/// 把 RaTeX 图片画到占位字符的位置上。
///
/// 两种竖向摆法：
///
/// - 普通短公式（`tall == false`）：用 `run.baseline` 反推图片顶部，让公式内部
///   基线与周围文字的基线重合，读起来才像「在句子里」；
/// - 撑高了行高的多行公式（行列式、分段函数）：在整行里垂直居中。它的行高是
///   专门为它腾出来的，贴基线会把下半截甩到下一行上去。
///
/// 水平方向统一在占位宽度内居中。
pub(crate) fn math_image_shapes(
    galley: &egui::Galley,
    origin: egui::Pos2,
    runs: &[super::job::InlineMathRun],
    tint: Color32,
) -> Vec<egui::Shape> {
    let mut shapes = Vec::new();
    for run in runs {
        let Some(math) = &run.math else { continue };
        'rows: for row in &galley.rows {
            let Some(glyph) = row.glyphs.iter().find(|g| g.chr == run.marker) else {
                continue;
            };
            let row_top = origin.y + row.pos.y;
            let band = row.row.size.y;
            let x = origin.x + row.pos.x + glyph.pos.x + (run.reserved - run.size.x) * 0.5;
            let top = if run.tall {
                row_top + (band - run.size.y) * 0.5
            } else {
                // 占位字符的基线 = 行顶 + 字形 y 偏移；公式顶部 = 基线 − 基线高。
                row_top + glyph.pos.y - run.baseline
            };
            let top_left = Pos2::new(x, top);
            for s in crate::render::math::transform_shapes(&math.shapes, top_left, run.scale, tint)
            {
                shapes.push(s);
            }
            break 'rows;
        }
    }
    shapes
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn text_response(
    ui: &mut egui::Ui,
    inlines: &[Inline],
    theme: &Theme,
    size: f32,
    color: Color32,
    bold: bool,
    env: &mut RenderEnv<'_>,
    idx: usize,
) -> egui::Response {
    let mut math = MathBuild {
        ctx: env.ctx,
        images: &mut *env.images,
        max_width: ui.available_width().min(env.max_width),
        runs: Vec::new(),
    };
    let job = build_layout_with_math(inlines, theme, size, color, bold, &mut math);
    let math_runs = std::mem::take(&mut math.runs);
    drop(math);
    // 先占一个绘制槽：行内代码片与跨块选区底色都要垫在文字下层
    // （在 Label 之前画），否则会盖住文字。
    let bg_slot = ui.painter().add(egui::Shape::Noop);
    let (resp, galley) = layout_label(ui, job, env.max_width, Sense::click_and_drag());
    // galley 的原点：文字真正开始的位置。行带向左扩到圆点/缩进后，
    // resp.rect.min 不再等于它，所有字符下标换算与底色绘制都必须用这个。
    let text_origin = resp.rect.min;
    // 公式图片画在文字上层；占位字符本身是透明的。
    let math_slot = ui.painter().add(egui::Shape::Noop);
    let math_shapes = math_image_shapes(&galley, resp.rect.min, &math_runs, color);
    if !math_shapes.is_empty() {
        ui.painter().set(math_slot, egui::Shape::Vec(math_shapes));
    }
    // 行带（line band）：命中与高亮的横向范围。左端取本行所在内容列的左缘——
    // 列表圆点、任务复选框、有序标记、引用缩进都排在文字左边，指针落在那片
    // 空白时用户认为自己在「这一行」上，必须能锚定/延伸选区，高亮也要铺过去；
    // 右端取可用宽度右缘，行尾空白同样属于这一行。
    // galley 原点（resp.rect.min）单独保留给 cursor_from_pos 与高亮换算，
    // 不能跟着行带左移，否则字符下标整体错位。
    let band_left = ui.min_rect().left().min(resp.rect.min.x);
    let band_right = ui.max_rect().right().max(resp.rect.max.x);
    // 行右侧空白也要可点（Typora 行为）：命中区从行带左缘延伸到本行右缘。
    // 之前响应矩形只有文字 galley 的宽度，点行尾空白毫无反应；扩展后点击
    // 仍走 cursor_from_pos，超出行末的 x 会被钳到该行最后一个字符之后，
    // 小于行带左缘的 x 会被钳到该行第一个字符。
    let row_rect = Rect::from_min_max(
        Pos2::new(band_left, resp.rect.min.y),
        Pos2::new(band_right, resp.rect.bottom()),
    );
    let ext = ui
        .interact(
            row_rect,
            // 不能用 rect.top 参与 Id：悬停/按下时布局可能上下浮动 1–2px，
            // Id 一变 egui 就认为按下和松开不是同一个控件，clicked 永远为 false，
            // 表现就是「第一次点击只让块动一下、第二次才进入编辑」。
            egui::Id::new(("row_ext", idx)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::Text);
    let resp = resp.union(ext);
    // 跨块拖选：在渲染态块上按住拖动，锚定/更新全局选区（show_editor 帧末聚合）。
    // 锚定必须在按下帧立即做（button_pressed）：egui 的 drag_started 晚一帧
    // 触发，那时指针已移到终点，起点字符就丢了。拖进别的块时由
    // 「选区进行中 + 指针压在本块上」的判定接棒上报，选区才能跨块延伸。
    //
    // 终点只由「指针当前压在哪个块上」决定，不能用 resp.dragged()：
    // egui 把 drag 归给按下的起始块，拖动期间它每帧都上报；块按 idx 升序渲染，
    // 向上拖时起始块的上报晚于指针所在块，聚合时会把终点覆盖回起始块——
    // 表现为选区漏掉指针上方的段落（看上去没选中、也复制不到）。
    let raw_pointer = ui.input(|i| i.pointer.interact_pos());
    // 拖选时把越界指针钳进视口：鼠标甩到可视区域外时，越界点往往落在块与块
    // 之间的空档（或文档末尾之后），没有块能命中，选区就不再延伸。钳到视口
    // 边缘后，配合 show_editor 的越界自动滚动，滚到哪儿就选到哪儿。
    //
    // 但「按下」不能钳：点工具条/菜单时指针在视口上方，钳下来会落到最上面的
    // 块上，凭空建立 (0, 7) 这种零长度选区，把用户真正的拖选覆盖掉——表现就是
    // 拖选好文字再点「引用/列表」等按钮，格式加到了错误的位置。
    let clip = ui.clip_rect();
    let in_viewport = raw_pointer.is_some_and(|p| clip.contains(p));
    let pointer = raw_pointer.map(|p| {
        if in_viewport {
            p
        } else {
            Pos2::new(p.x, p.y.clamp(clip.top() + 1.0, clip.bottom() - 1.0))
        }
    });
    let pointer_down = ui.input(|i| i.pointer.primary_down());
    let pressed_now = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
    // 按下点必须在编辑区里：鼠标按在工具条 / 菜单上时，即使指针被钳进视口、
    // 又恰好压在某一行上，也不能修改正文选区。否则点「引用」等按钮时，
    // 选区终点会被改成工具条正下方那一行的字符，看起来选区突然变大/移位。
    // 视口边缘的块可能只露出一半，按在它上面时 press_origin 会落在 clip
    // 之外。这里给编辑区留一点容差，否则从贴着视口顶部的标题起拖会完全
    // 建立不了选区（用户反馈的「有时候怎么拖都选不中」）。
    let press_area = clip.expand2(egui::vec2(48.0, 8.0));
    let press_origin = ui.input(|i| i.pointer.press_origin());
    let press_in_viewport = press_origin.is_some_and(|p| press_area.contains(p));
    let over_this = pointer.is_some_and(|p| resp.rect.contains(p));
    // 锚定必须用「锁定的按下点」press_origin，而不是当前 pointer：按下帧当前指针
    // 常因事件批处理已微移、或落在块间距/视口边缘，导致 over_this 判定失败——
    // 那一帧一旦没锚定，后面就只能用上一次点击的 last_cursor 兜底，选区就跑到
    // 别的块去了（用户反馈的「时出现时不出现、选区乱跑」）。
    // 竖直方向按 ±3px 容差命中（正好铺满块间 6px 空档、彼此不重叠），横向按
    // 行带来：从内容列左缘（列表圆点、任务复选框、缩进）一直铺到可用宽度右缘。
    // 之前横向起点取文字 galley 的左缘，嵌套列表越深、圆点与缩进那片空白越宽，
    // 按在那儿就一个块都不命中，选区既不锚定也不延伸（用户截图的「有些拖不动」）。
    let press_hit = press_origin.is_some_and(|p| {
        p.y >= resp.rect.top() - 3.0
            && p.y <= resp.rect.bottom() + 3.0
            && p.x >= band_left
            && p.x <= band_right
    });
    if pressed_now && press_in_viewport && press_hit {
        if let Some(p) = press_origin {
            let local = p - text_origin;
            let c = galley.cursor_from_pos(local).index.0;
            env.actions.push(Action::CrossDrag(idx, c, true));
        }
    } else if !pressed_now
        && press_in_viewport
        && env.cross_sel.is_some()
        && pointer_down
        && over_this
    {
        if let Some(p) = pointer {
            let local = p - text_origin;
            let c = galley.cursor_from_pos(local).index.0;
            env.actions.push(Action::CrossDrag(idx, c, false));
        }
    } else if !pressed_now && press_in_viewport && pointer_down && in_viewport && over_this {
        // 拖动起点不在渲染态块上（最常见：起点在活动块的 TextEdit 里，被它
        // 吃掉了按下事件），拖到本块时才建立跨块选区：以活动块的光标为锚点。
        // 没有这条，从编辑态段落往下拖就完全选不中（用户反馈的"断断续续"）。
        if let Some(p) = pointer {
            let local = p - text_origin;
            let c = galley.cursor_from_pos(local).index.0;
            env.actions.push(Action::ShiftClick(idx, c));
        }
    }
    // 跨块选区底色：本块被选区覆盖时，按选中字符区间收集高亮矩形，
    // 最后填进前面预留的槽位（垫在文字下层）。
    let mut sel_shapes: Vec<egui::Shape> = Vec::new();
    let text_len = galley.text().chars().count();
    if let Some((s0, e0)) = cross_sel_chars(env.cross_sel, idx, text_len) {
        sel_shapes = selection_shapes(
            &galley,
            text_origin,
            (s0, e0),
            theme.selection,
            Some((band_left, band_right)),
        );
    }
    // 代码片垫底、选区叠在代码片上面（同一槽位里后画的在上层）。
    let mut bg_shapes = inline_code_bg_shapes(&galley, text_origin, theme);
    bg_shapes.extend(sel_shapes);
    if !bg_shapes.is_empty() {
        ui.painter().set(bg_slot, egui::Shape::Vec(bg_shapes));
    }
    // 双击选词 / 三击选整段 / Shift+点击扩选。必须在普通 clicked 之前处理：
    // egui 里双击的第二击也会触发 clicked，命中后直接 return，避免叠加 Caret。
    if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
        let local = p - text_origin;
        let char_idx = galley.cursor_from_pos(local).index.0;
        let text: Vec<char> = galley.text().chars().collect();
        if resp.triple_clicked() {
            // 三击：选中整个块（整行/整段）
            env.actions.push(Action::BlockSel(idx));
            return resp;
        }
        if resp.double_clicked() {
            // 双击：选中点击处的词（连续字母/数字/汉字；标点只选该字符）
            let (a, z) = word_bounds(&text, char_idx);
            env.actions.push(Action::WordSel(idx, a, z));
            return resp;
        }
        if resp.clicked() && ui.input(|i| i.modifiers.shift) {
            // Shift+点击：从光标位置到点击位置之间全部选中（不用按住拖动）
            env.actions.push(Action::ShiftClick(idx, char_idx));
            return resp;
        }
    }
    if resp.clicked() {
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            let local = p - text_origin;
            let cursor = galley.cursor_from_pos(local);
            let char_idx = cursor.index.0;
            // 链接：Cmd/Ctrl+点击才跳转；普通点击只激活编辑并露出该链接的
            // Markdown 源码（见 inline_job），方便直接改地址。
            if let Some(url) = link_at(inlines, char_idx)
                && ui.input(|i| i.modifiers.command || i.modifiers.ctrl)
            {
                env.actions.push(Action::OpenLink(url));
                return resp;
            }
            env.actions.push(Action::Caret(idx, local));
        }
    }
    let _ = idx;
    resp
}

/// 跨块选区在某个块上的字符区间 [start, end)（相对该块的所见文本）。
/// 块不在选区范围内返回 None。
pub fn cross_sel_chars(
    sel: Option<(usize, usize, usize, usize)>,
    idx: usize,
    text_len: usize,
) -> Option<(usize, usize)> {
    let (a, ac, b, bc) = sel?;
    let (start_b, start_c, end_b, end_c) = if (a, ac) <= (b, bc) {
        (a, ac, b, bc)
    } else {
        (b, bc, a, ac)
    };
    if idx < start_b || idx > end_b {
        return None;
    }
    let s0 = if idx == start_b {
        start_c.min(text_len)
    } else {
        0
    };
    let e0 = if idx == end_b {
        end_c.min(text_len)
    } else {
        text_len
    };
    (s0 < e0).then_some((s0, e0))
}

/// 按字符区间 [start, end) 收集 galley 每一行的选区矩形。
/// 渲染态 text_response 与编辑态活动块（自己画选区高亮）共用。
///
/// `band = Some((左, 右))` 时按「行带」铺色：某行的开头被选中就从行带左缘画起
/// （覆盖列表圆点、任务复选框、缩进那片空白），行末被选中就画到行带右缘
/// （覆盖行尾空白与行内代码/公式两侧的空隙）。不传就只按字形宽度画。
/// 逐行都铺满行带，选区看起来才是一整块连续的区域，而不是断成一截一截。
pub fn selection_shapes(
    galley: &egui::Galley,
    origin: Pos2,
    (start, end): (usize, usize),
    color: Color32,
    band: Option<(f32, f32)>,
) -> Vec<egui::Shape> {
    let text_len = galley.text().chars().count();
    let s0 = start.min(text_len);
    let e0 = end.min(text_len);
    if s0 >= e0 {
        return Vec::new();
    }
    // 行带坐标换到 galley 局部系（下面所有矩形都按 origin 平移）。
    let band_local = band.map(|(l, r)| (l - origin.x, r - origin.x));
    let mut shapes = Vec::new();
    let mut acc = 0usize;
    for row in &galley.rows {
        let n = row.char_count_including_newline().0;
        let (rs, re) = (acc, acc + n);
        acc = re;
        let lo = s0.clamp(rs, re);
        let hi = e0.clamp(rs, re);
        if hi <= lo {
            // 整行被跨过的空行（只有换行符）：有行带就铺满整带，否则画一小段行首。
            if s0 <= rs && e0 >= re {
                let (x0, x1) = match band_local {
                    Some((l, r)) => (l, r),
                    None => (0.0, 6.0),
                };
                let rect = Rect::from_min_max(
                    Pos2::new(x0, row.pos.y),
                    Pos2::new(x1, row.pos.y + row.row.size.y.max(1.0)),
                )
                .translate(origin.to_vec2());
                shapes.push(egui::Shape::rect_filled(rect, 0.0, color));
            }
            continue;
        }
        // egui 的 pos_from_cursor 是「插入点」矩形（零宽）：索引 i
        // 的 x 是第 i 个字符的左边界。所以选中延伸到行末时要用
        // 边界 re（最后一字之后的插入点），不能用 re-1 的右缘——
        // 那样每行最后一个字永远少一截、不被高亮覆盖。
        let x0 = if lo == rs {
            band_local.map(|(l, _)| l)
        } else {
            None
        }
        .unwrap_or_else(|| galley.pos_from_cursor(egui::text::CCursor::new(lo)).left());
        let x1 = if e0 >= re {
            band_local.map(|(_, r)| r)
        } else {
            None
        }
        .unwrap_or_else(|| galley.pos_from_cursor(egui::text::CCursor::new(hi)).left());
        let rect = Rect::from_min_max(
            Pos2::new(x0, row.pos.y),
            Pos2::new(x1.max(x0), row.pos.y + row.row.size.y),
        )
        .translate(origin.to_vec2());
        shapes.push(egui::Shape::rect_filled(rect, 0.0, color));
    }
    shapes
}

/// 编辑态 galley 的逐字符标记：(是否被隐藏, 字符)。
/// inline_job 把 Markdown 标记（`**`、`[](…)` 等）用 0.1px 透明字渲染，
/// 这些字符占着缓冲区下标但用户看不见。
fn glyph_flags(galley: &egui::Galley) -> impl Iterator<Item = (bool, char)> + '_ {
    galley.job.sections.iter().flat_map(|sec| {
        let hidden = sec.format.font_id.size <= 0.5;
        galley.job.text[sec.byte_range.start.0..sec.byte_range.end.0]
            .chars()
            .map(move |ch| (hidden, ch))
    })
}

/// 编辑态缓冲区下标 → 所见文本（plain）下标。
/// 跨块选区的字符坐标以「所见文本」计，按下点/终点必须先换算，
/// 否则 `**粗体**` 这类块会把锚点算到标记上（多选/漏选）。
pub fn plain_char_index(galley: &egui::Galley, buffer_idx: usize) -> usize {
    let mut visible = 0usize;
    let mut seen = 0usize;
    for (hidden, ch) in glyph_flags(galley) {
        if seen == buffer_idx {
            break;
        }
        seen += 1;
        if !hidden && ch != '\n' {
            visible += 1;
        }
    }
    visible
}

/// 所见文本区间 [start, end) → 编辑态缓冲区区间。
pub fn buffer_char_range(galley: &egui::Galley, (start, end): (usize, usize)) -> (usize, usize) {
    let mut visible = 0usize;
    let mut bstart: Option<usize> = None;
    let mut bend = galley.text().chars().count();
    for (i, (hidden, ch)) in glyph_flags(galley).enumerate() {
        if hidden || ch == '\n' {
            continue;
        }
        if bstart.is_none() && visible == start {
            bstart = Some(i);
        }
        visible += 1;
        if visible == end {
            bend = i + 1;
        }
    }
    let bstart = bstart.unwrap_or(bend);
    (bstart.min(bend), bend)
}

/// 双击选词的词边界：从字符下标 pos 向两侧扩展。
/// 连续的字母/数字/汉字（含下划线）算一个词；空白/标点只选单个字符。
pub(crate) fn word_bounds(text: &[char], pos: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let pos = pos.min(text.len() - 1);
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if is_word(text[pos]) {
        let mut a = pos;
        while a > 0 && is_word(text[a - 1]) {
            a -= 1;
        }
        let mut z = pos + 1;
        while z < text.len() && is_word(text[z]) {
            z += 1;
        }
        (a, z)
    } else {
        (pos, pos + 1)
    }
}
