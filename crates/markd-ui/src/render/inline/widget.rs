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

/// 行内代码片：按字符区间映射到每行的矩形，画 4px 圆角底色。
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
            let rect = Rect::from_min_max(
                Pos2::new(x0 - 3.0, row.pos.y + 1.0),
                Pos2::new(x1.max(x0) + 3.0, row.pos.y + row.row.size.y - 1.0),
            )
            .translate(origin.to_vec2());
            shapes.push(egui::Shape::rect_filled(rect, 4.0, bg));
            if let Some(c) = border {
                shapes.push(egui::Shape::rect_stroke(
                    rect,
                    4.0,
                    Stroke::new(1.0, c),
                    egui::StrokeKind::Inside,
                ));
            }
        }
    }
    shapes
}

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
    let mut job = build_layout(inlines, theme, size, color, bold);
    // 先占一个绘制槽：行内代码片与跨块选区底色都要垫在文字下层
    // （在 Label 之前画），否则会盖住文字。
    let bg_slot = ui.painter().add(egui::Shape::Noop);
    let (resp, galley) = layout_label(ui, job, env.max_width, Sense::click_and_drag());
    // 行右侧空白也要可点（Typora 行为）：命中区从文字左缘延伸到本行右缘。
    // 之前响应矩形只有文字 galley 的宽度，点行尾空白毫无反应；扩展后点击
    // 仍走 cursor_from_pos，超出行末的 x 会被钳到该行最后一个字符之后。
    let row_rect = Rect::from_min_max(
        resp.rect.min,
        Pos2::new(ui.max_rect().right(), resp.rect.bottom()),
    );
    let ext = ui
        .interact(
            row_rect,
            egui::Id::new(("row_ext", idx, resp.rect.top().to_bits())),
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
    let press_in_viewport = ui
        .input(|i| i.pointer.press_origin())
        .is_some_and(|p| clip.contains(p));
    let over_this = pointer.is_some_and(|p| resp.rect.contains(p));
    if pressed_now && in_viewport && over_this {
        if let Some(p) = pointer {
            let local = p - resp.rect.min;
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
            let local = p - resp.rect.min;
            let c = galley.cursor_from_pos(local).index.0;
            env.actions.push(Action::CrossDrag(idx, c, false));
        }
    } else if !pressed_now && press_in_viewport && pointer_down && in_viewport && over_this {
        // 拖动起点不在渲染态块上（最常见：起点在活动块的 TextEdit 里，被它
        // 吃掉了按下事件），拖到本块时才建立跨块选区：以活动块的光标为锚点。
        // 没有这条，从编辑态段落往下拖就完全选不中（用户反馈的"断断续续"）。
        if let Some(p) = pointer {
            let local = p - resp.rect.min;
            let c = galley.cursor_from_pos(local).index.0;
            env.actions.push(Action::ShiftClick(idx, c));
        }
    }
    // 跨块选区底色：本块被选区覆盖时，按选中字符区间收集高亮矩形，
    // 最后填进前面预留的槽位（垫在文字下层）。
    let mut sel_shapes: Vec<egui::Shape> = Vec::new();
    if let Some((a, ac, b, bc)) = env.cross_sel {
        let (start_b, start_c, end_b, end_c) = if (a, ac) <= (b, bc) {
            (a, ac, b, bc)
        } else {
            (b, bc, a, ac)
        };
        if idx >= start_b && idx <= end_b {
            let text_len = galley.text().chars().count();
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
            if s0 < e0 {
                // 逐行求选中区间与行的交集：pos_from_cursor 取行内 x 段
                let mut acc = 0usize;
                for row in &galley.rows {
                    let n = row.char_count_including_newline().0;
                    let (rs, re) = (acc, acc + n);
                    acc = re;
                    let lo = s0.clamp(rs, re);
                    let hi = e0.clamp(rs, re);
                    if hi <= lo {
                        // 整行被跨过的空行（只有换行符）：画一小段行首高亮
                        if s0 <= rs && e0 >= re {
                            let rect = Rect::from_min_max(
                                Pos2::new(0.0, row.pos.y),
                                Pos2::new(6.0, row.pos.y + row.row.size.y),
                            )
                            .translate(resp.rect.min.to_vec2());
                            sel_shapes.push(egui::Shape::rect_filled(rect, 0.0, theme.selection));
                        }
                        continue;
                    }
                    let x0 = galley.pos_from_cursor(egui::text::CCursor::new(lo)).left();
                    // egui 的 pos_from_cursor 是「插入点」矩形（零宽）：索引 i
                    // 的 x 是第 i 个字符的左边界。所以选中延伸到行末时要用
                    // 边界 re（最后一字之后的插入点），不能用 re-1 的右缘——
                    // 那样每行最后一个字永远少一截、不被高亮覆盖。
                    let x1 = if e0 >= re {
                        galley.pos_from_cursor(egui::text::CCursor::new(re)).left()
                    } else {
                        galley.pos_from_cursor(egui::text::CCursor::new(hi)).left()
                    };
                    let rect = Rect::from_min_max(
                        Pos2::new(x0, row.pos.y),
                        Pos2::new(x1.max(x0), row.pos.y + row.row.size.y),
                    )
                    .translate(resp.rect.min.to_vec2());
                    sel_shapes.push(egui::Shape::rect_filled(rect, 0.0, theme.selection));
                }
            }
        }
    }
    // 代码片垫底、选区叠在代码片上面（同一槽位里后画的在上层）。
    let mut bg_shapes = inline_code_bg_shapes(&galley, resp.rect.min, theme);
    bg_shapes.extend(sel_shapes);
    if !bg_shapes.is_empty() {
        ui.painter().set(bg_slot, egui::Shape::Vec(bg_shapes));
    }
    // 双击选词 / 三击选整段 / Shift+点击扩选。必须在普通 clicked 之前处理：
    // egui 里双击的第二击也会触发 clicked，命中后直接 return，避免叠加 Caret。
    if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
        let local = p - resp.rect.min;
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
            let local = p - resp.rect.min;
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
