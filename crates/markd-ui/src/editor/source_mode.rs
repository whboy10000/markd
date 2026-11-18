//! 源码模式：整篇文档用一个多行编辑器显示，带行号与光标行底色。
//!
//! 从 `view::show_editor` 中抽出，行为保持不变。

use super::*;

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
    let mut src = doc.source();
    // TextEdit 会随内容自动撑高，直接放在父 Ui 上会被裁剪、滚轮无处生效，
    // 表现就是源码模式下鼠标滚不动；必须像下方混合模式一样包一层 ScrollArea。
    let mut changed = false;
    let mut editor_rect = egui::Rect::NOTHING;
    // 行号栏宽度固定（三种模式共用），不随行数位数、窗口、模式变化。
    let gutter_w = gutter_width(show_line_numbers);
    // 每个源码行首字符的下标（CCursor 按 char 计数）。
    let line_starts: Vec<usize> = {
        let mut v = vec![0usize];
        let mut cnt = 0usize;
        for c in src.chars() {
            if c == '\n' {
                v.push(cnt + 1);
            }
            cnt += 1;
        }
        v
    };
    // 上一帧的视觉行映射 (源码行号, 行顶 y, 行高)：光标行底色要垫在文字
    // 下层，必须在 TextEdit 之前画，只能用上一帧的数据（误差一帧，不可见）。
    let rows_state: std::cell::RefCell<Vec<(usize, f32, f32)>> =
        std::cell::RefCell::new(Vec::new());
    // 当前光标所在的源码行（1 起）。
    let cur_line = {
        let cur = state.last_cursor.min(src.chars().count());
        match line_starts.binary_search(&cur) {
            Ok(i) => i + 1,
            Err(i) => i.min(line_starts.len()).max(1),
        }
    };
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
                // 行距放大到字号的 1.4 倍：行与行（含空行）之间的间隔
                // 更舒展，行号数字不再显得挤在一起。
                let mut fmt = theme.mono_fmt();
                fmt.line_height = Some(theme.mono * 1.4);
                job.append(buf.as_str(), 0.0, fmt);
                job.wrap.max_width = f32::INFINITY;
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                rows_state.borrow_mut().clear();
                let mut acc = 0usize;
                for row in &galley.rows {
                    let start_char = acc;
                    acc += row.char_count_including_newline().0;
                    let line_no = match line_starts.binary_search(&start_char) {
                        Ok(i) => i + 1,
                        Err(i) => i.min(line_starts.len()).max(1),
                    };
                    rows_state
                        .borrow_mut()
                        .push((line_no, row.pos.y, row.row.size.y));
                    let _ = start_char;
                }
                galley
            };
            // 长行宽度估算（ASCII 半宽、CJK 全宽）：编辑框按最长行撑宽，
            // 配合横向滚动；窄于视口时照常占满。
            let content_w = src
                .lines()
                .map(|l| {
                    let w: f32 = l
                        .chars()
                        .map(|c| if c.is_ascii() { 0.62 } else { 1.25 })
                        .sum();
                    w * theme.mono
                })
                .fold(0.0f32, f32::max);
            let edit_w = (ui.available_width() - gutter_w).max(content_w);

            // 当前光标行的整行淡色底：必须在 TextEdit 之前画才能垫在文字下层。
            // 从行号栏右侧延伸到编辑区右缘；未显示行号时从正文左缘开始。
            {
                let band = rows_state
                    .borrow()
                    .iter()
                    .find(|(l, _, _)| *l == cur_line)
                    .map(|(_, y, h)| (*y, *h));
                if let Some((y, h)) = band {
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
            // 行号：当帧 galley 单遍扫描，固定画在视口左缘（不随横向滚动移动）；
            // 光标所在行的行号用主题色高亮。
            if show_line_numbers {
                let mut per_line: Vec<Option<f32>> = vec![None; line_starts.len() + 1];
                let mut acc = 0usize;
                for row in &out.galley.rows {
                    let start_char = acc;
                    acc += row.char_count_including_newline().0;
                    let line_no = match line_starts.binary_search(&start_char) {
                        Ok(i) => i + 1,
                        Err(i) => i.min(line_starts.len()),
                    };
                    if per_line.get(line_no).map_or(false, |s| s.is_none()) {
                        per_line[line_no] = Some(row.pos.y + row.row.size.y * 0.5);
                    }
                }
                let p = ui.painter();
                // 数字右对齐到 gutter 右缘内固定间距，与混合模式完全一致。
                let num_right = vpl + gutter_w - GUTTER_PAD;
                for (line_no, y) in per_line.iter().enumerate().skip(1) {
                    if let Some(y) = y {
                        let color = if line_no == cur_line {
                            theme.accent
                        } else {
                            theme.dim.gamma_multiply(0.7)
                        };
                        p.text(
                            egui::Pos2::new(num_right, out.galley_pos.y + y),
                            egui::Align2::RIGHT_CENTER,
                            &line_no.to_string(),
                            egui::FontId::monospace(theme.mono),
                            color,
                        );
                    }
                }
            }
            changed = out.response.changed();
            editor_rect = out.response.rect;
            out
        });
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
        if let Some(mut ts) = egui::TextEdit::load_state(ui.ctx(), id) {
            ts.cursor.set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(src.chars().count()),
            )));
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
        return EditorEvent::Changed;
    }
    return EditorEvent::None;
}
