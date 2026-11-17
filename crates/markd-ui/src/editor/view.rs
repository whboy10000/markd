//! 混合模式（Hybrid View）主绘制流程：活动块源码编辑，其余块渲染。

use super::*;

/// 行号栏（gutter）的固定宽度：70px，三种模式（源码 / 混合 / 专注）共用。
///
/// 设计要点：宽度是常量，不随窗口大小、字体字号、行数位数或模式变化，
/// 也不参与版心居中 —— 行号栏始终锚定在编辑区最左侧，正文紧接其后。
/// 超过 4 位的超大文档行号会挤占行号栏左侧留白，但不会改变栏宽。
pub(super) const GUTTER_WIDTH: f32 = 70.0;

pub(super) fn gutter_width(show: bool) -> f32 {
    if show { GUTTER_WIDTH } else { 0.0 }
}

/// 行号数字右缘相对 gutter 右缘的缩进（行号与正文之间的间距），三种模式共用。
/// 固定 70px 栏宽下留 12px，保证最大字号（26pt）的 4 位行号仍能完整显示。
pub(super) const GUTTER_PAD: f32 = 12.0;

/// 滚动条所在的那条窄带：点这里应该交给滚动条，不能当成空白点击。
pub(super) fn scroll_bar_zone(
    ui: &egui::Ui,
    inner_rect: egui::Rect,
    content_height: f32,
) -> egui::Rect {
    if content_height > inner_rect.height() {
        let w = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin;
        egui::Rect::from_min_max(
            egui::pos2(inner_rect.right() - w - 2.0, inner_rect.top()),
            egui::pos2(inner_rect.right() + 8.0, inner_rect.bottom()),
        )
    } else {
        egui::Rect::NOTHING
    }
}

/// 空白点击调试：`MARKD_TRACE_BLANK=1 cargo run` 时打印判定结果。
pub(super) fn trace_blank(pos: &Option<egui::Pos2>, msg: &str) {
    if std::env::var("MARKD_TRACE_BLANK").is_ok() {
        eprintln!("[blank] pointer={pos:?} {msg}");
    }
}

/// 指针落在表格的哪个单元格（行, 列）；不在任何单元格上返回 None。
fn table_cell_at(rows: &[Vec<egui::Rect>], p: egui::Pos2) -> Option<(usize, usize)> {
    rows.iter()
        .enumerate()
        .find_map(|(ri, row)| row.iter().position(|r| r.contains(p)).map(|ci| (ri, ci)))
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
    // 是否显示行号（配置里的 show_line_numbers）。
    show_line_numbers: bool,
    // 标签唯一编号：拼进 TextEdit 的 Id，光标/选中状态才不会被别的标签复用。
    tab_key: u64,
    actions: &mut Vec<Action>,
) -> EditorEvent {
    // ---------------- 源码模式：整篇文档一个多行编辑器 ----------------
    if source_mode {
        return show_source_mode(ui, doc, state, theme, ctx, show_line_numbers, tab_key);
    }

    // 编辑区空白处也要能落光标。先登记一个底层热区作为兜底；真正判定用
    // 本帧指针位置 + 块矩形（见 ScrollArea 之后）——不依赖哪个控件抢到这次
    // 点击，所以即使 ScrollArea/滚动条吃掉了事件，空白点击也不会丢。
    let bg = ui.interact(
        ui.max_rect(),
        Id::new(("editor_blank", tab_key)),
        Sense::click(),
    );

    let active = state.active.min(doc.blocks.len().saturating_sub(1));
    state.active = active;
    let active_multiline = doc
        .blocks
        .get(active)
        .map(|b| b.is_multiline())
        .unwrap_or(false);

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
    // 松开鼠标即清掉按下锚点：只在一次拖拽期间有效。
    if !ctx.input(|i| i.pointer.primary_down()) {
        state.press_caret = None;
        state.table_drag_from = None;
    }
    let typed_pair: Option<String> = ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Text(t) => pair_close(t).map(|s| s.to_string()),
            _ => None,
        })
    });

    // 只维护「活动块」的可编辑缓冲；其它块直接借用 doc.blocks。
    // 之前每帧把所有块的源码 clone 一遍，10 万字时要分配上千个 String。
    // 代码组特殊处理：编辑的是「当前标签的代码」，外层/内层围栏都不进缓冲。
    let edit_kind = match doc.blocks.get(active) {
        Some(Block::CodeGroup { items, .. }) => EditKind::CodeGroup {
            items: items.clone(),
            tab: state.group_tab(active).min(items.len().saturating_sub(1)),
        },
        Some(b) => EditKind::of(b),
        None => EditKind::Source,
    };
    let mut active_buf = match &edit_kind {
        EditKind::CodeGroup { items, tab } => {
            items.get(*tab).map(|i| i.code.clone()).unwrap_or_default()
        }
        _ => doc
            .blocks
            .get(active)
            .map(block_editable_text)
            .unwrap_or_default(),
    };
    // 空块行首退格要并回上一块：这个判断必须基于「按键之前」的缓冲区，
    // 否则删掉最后一个字符的那一帧就会被误判成空块。
    let buf_was_empty = active_buf.is_empty();
    // 按键前的完整缓冲快照：TextEdit 会原地改写 active_buf，「末尾空行再按回车
    // 退出代码块」需要比对「本帧的改动是否恰好是在末尾追加了一个换行」，
    // 以排除同帧既打字又回车的误判。
    let pre_buf = active_buf.clone();
    let buf_ended_nl = pre_buf.ends_with('\n');
    let mut changed = false;
    let mut new_active: Option<usize> = None;
    let mut active_rect: Option<egui::Rect> = None;
    let mut cursor_idx = active_buf.chars().count();
    let mut selection: Option<(usize, usize)> = None;
    let mut pair_insert: Option<(usize, String)> = None;
    let mut table_edit_src: Option<String> = None;
    // 代码块编辑态里更换语言时置位，帧末统一处理
    let mut lang_change: Option<String> = None;
    // 代码组编辑态：切标签 / 改当前标签语言，帧末统一处理
    // （切标签前要先把当前标签的代码写回，否则会丢字）。
    let mut group_tab_switch: Option<usize> = None;
    let mut group_lang_change: Option<String> = None;
    // 代码组增删标签（编辑态，帧末写回当前标签代码后再改标签列表）。
    let mut group_add_tab = false;
    let mut group_remove_tab = false;
    // 本帧每个块实际占用的矩形：(块下标, 矩形)。空白点击靠它区分「点在块上」和「点在留白」。
    let mut block_rects: Vec<(usize, egui::Rect)> = Vec::new();
    // 引用块左侧竖线的矩形：编辑态与渲染态返回的 resp.rect 基准不同
    // （编辑态是整个横向区域、渲染态只是文字），在这里统一换算成竖线本身，
    // 帧末用它把相邻引用块的竖线补接起来。
    let mut quote_bars: Vec<(usize, egui::Rect)> = Vec::new();
    // 列表项编辑态按下 Tab / Shift+Tab：记下方向，帧末统一调整缩进层级。
    let mut list_tab: Option<bool> = None;

    let scroll_out = egui::ScrollArea::vertical()
        .id_salt("editor_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let blocks = &doc.blocks;
            // 每个块起始的源码行号（Document::source 以 \n 连接各块源码）。
            // 用于编辑区左侧的行号显示。
            let mut block_start_lines = Vec::with_capacity(blocks.len());
            let mut line_acc = 1usize;
            for b in blocks.iter() {
                block_start_lines.push(line_acc);
                line_acc += b.source().split('\n').count();
            }
            // 正文宽度随窗口浮动，固定占可用宽度的约 90%；不设死宽度，
            // 所有块都按这个宽度换行，窄窗口下也不会把长段落横向裁掉。
            // 行号栏固定锚定在编辑区最左侧（70px，与源码模式共用），不参与版心
            // 居中，因此全窗口 / 部分窗口、源码 / 混合 / 专注模式下栏宽都恒定。
            let outer_w = ui.available_width();
            let page_w = (outer_w * 0.9).max(200.0);
            let gutter_w = gutter_width(show_line_numbers);
            let viewport_left = ui.clip_rect().left();
            ui.horizontal_top(|ui| {
                // 显示行号时行号栏从最左侧开始，正文紧随其后；关闭行号时保持版心居中。
                let pad = if show_line_numbers {
                    0.0
                } else {
                    ((outer_w - page_w) / 2.0).max(0.0)
                };
                // 扣掉水平 item_spacing，让正文左缘正好落在行号栏右缘。
                let spacing = ui.spacing().item_spacing.x;
                ui.add_space((pad + gutter_w - spacing).max(0.0));
                ui.vertical(|ui| {
                    ui.set_width(page_w - gutter_w);
                    // 拖选时把越界指针钳进视口（与 render.rs 的 text_response 一致）：
                    // 鼠标甩到可视区域外时，越界点可能落在块间空档，钳到边缘才能
                    // 持续命中块、让选区跟着自动滚动一直延伸。但按下的点必须在
                    // 视口内，否则点工具条会被钳成「点了最上面的代码块」。
                    let raw_pos = ctx.input(|i| i.pointer.interact_pos());
                    let clip = ui.clip_rect();
                    let in_viewport = raw_pos.is_some_and(|p| clip.contains(p));
                    // 这次按下是否发生在编辑区里：按在工具条 / 菜单上时，
                    // 指针即使被钳进视口也不能延伸正文选区，否则点一次按钮
                    // 就会把选区终点改成工具条正下方那一行，看起来「选区突然变大」。
                    let press_in_editor = ctx
                        .input(|i| i.pointer.press_origin())
                        .is_some_and(|p| clip.contains(p));
                    let drag_pos = raw_pos.map(|p| {
                        if in_viewport {
                            p
                        } else {
                            egui::Pos2::new(p.x, p.y.clamp(clip.top() + 1.0, clip.bottom() - 1.0))
                        }
                    });
                    // 行号统一画在编辑区左侧固定列里，正文左缘不可用 resp.rect.left()
                    // 推：列表/任务项的 resp 是去掉行首缩进与标记（复选框/圆点/编号）
                    // 后的内容 Label，按它会算出落在正文里的 x，行号就叠进任务列表了。
                    for idx in 0..blocks.len() {
                        let block = &blocks[idx];
                        let compact = matches!(block, Block::ListItem { .. } | Block::Blank { .. });
                        // 连续引用行之间只留最小间距，左侧竖线才能连成一条。
                        // 引用里的代码块同样算作引用行，竖线保持连续。
                        let quote_run = quote_level_of(block).is_some()
                            && blocks
                                .get(idx + 1)
                                .is_some_and(|b| quote_level_of(b).is_some());
                        let is_active = idx == active;
                        // 跨块选区进行中时，活动块也按渲染态绘制：否则它渲染成
                        // TextEdit、既不能作为拖选终点、也不参与选区高亮，
                        // 表现为「选区在这一块断掉」。
                        let editing = is_active && !state.read_only && state.cross_sel.is_none();

                        // 代码块/图表/表格等不经过 text_response，没有自己的文字
                        // 选区高亮。它们多半画在不透明面板里（panel_frame 自带底色），
                        // 垫在下面的蓝底会被面板底色盖住，所以改为在块内容之上叠加
                        // 一层半透明选区色——文字依然清晰，又能看出这块被选中了。
                        let sel_overlay = (!editing
                            && !text_selection_block(block)
                            && atomic_block_selected(state.cross_sel, idx))
                        .then(|| {
                            let c = theme.selection;
                            egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 110)
                        });

                        let resp = if editing && matches!(block, Block::Table { .. }) {
                            // 表格：像 Typora 一样直接在表格单元格里编辑，不看管道源码。
                            if let Block::Table {
                                head, rows, align, ..
                            } = block
                            {
                                let need =
                                    !matches!(&state.table_draft, Some((i, _, _)) if *i == idx);
                                if need {
                                    let mut grid: Vec<Vec<String>> = Vec::new();
                                    grid.push(
                                        head.iter().map(|c| crate::ast::inline_plain(c)).collect(),
                                    );
                                    for r in rows.iter() {
                                        grid.push(
                                            r.iter().map(|c| crate::ast::inline_plain(c)).collect(),
                                        );
                                    }
                                    state.table_draft = Some((idx, grid, align.clone()));
                                    // 首次进入编辑：默认聚焦第一个单元格。若渲染态刚
                                    // 报上来 TableCellClick，app 已把 table_focus 设为
                                    // 点击的格子，这里不要覆盖它。
                                    if state.table_focus.is_none() {
                                        state.table_focus = Some((0, 0));
                                    }
                                }
                            }
                            let mut edited = false;
                            let frame = egui::Frame::NONE
                                .fill(theme.bg)
                                .stroke(egui::Stroke::new(1.0, theme.table_line))
                                .corner_radius(4.0)
                                .inner_margin(6.0);
                            // 功能区背景与渲染态一致：浅色 #F2F2F4 / 深色 panel
                            let tb_fill = if theme.kind == crate::theme::ThemeKind::Light {
                                egui::Color32::from_rgb(0xF2, 0xF2, 0xF4)
                            } else {
                                theme.panel
                            };
                            // 光标所在单元格要在借用 draft 前读出（Copy 类型），
                            // 否则 state 同时被可变借用 + 读取会编译失败
                            let focus = state.table_focus;
                            let inner = frame.show(ui, |ui| {
                                let mut deleted = false;
                                if let Some((_, grid, aligns)) = state.table_draft.as_mut() {
                                    // ---------------- 功能区（常驻） ----------------
                                    // 行/列操作基准 = 光标所在单元格（table_focus），
                                    // 其次最近悬停记忆，最后默认尾部单元格。
                                    let cell_key = Id::new(("table_cell_ref", idx));
                                    let (hr, hc) = focus
                                        .or_else(|| {
                                            ui.ctx().memory(|m| {
                                                m.data.get_temp::<(usize, usize)>(cell_key)
                                            })
                                        })
                                        .unwrap_or((
                                            grid.len().saturating_sub(1),
                                            grid.first()
                                                .map(|r| r.len())
                                                .unwrap_or(1)
                                                .saturating_sub(1),
                                        ));
                                    let mut op: Option<TableOp> = None;
                                    egui::Frame::NONE
                                        .fill(tb_fill)
                                        .corner_radius(3.0)
                                        .inner_margin(4.0)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                        ui.style_mut().spacing.item_spacing.x = 4.0;
                                        let btn = |ui: &mut egui::Ui, label: &str, active: bool| {
                                            let txt =
                                                egui::RichText::new(label).size(theme.body * 0.85);
                                            let txt =
                                                if active { txt.color(theme.accent) } else { txt };
                                            ui.button(txt).clicked()
                                        };
                                        let all =
                                            |a: crate::ast::Align| aligns.iter().all(|x| *x == a);
                                        use crate::ast::Align;
                                        if btn(ui, "+ 行", false) {
                                            op = Some(TableOp::InsertRow(hr));
                                        }
                                        if btn(ui, "− 行", false) {
                                            op = Some(TableOp::DeleteRow(hr));
                                        }
                                        if btn(ui, "+ 列", false) {
                                            op = Some(TableOp::InsertCol(hc));
                                        }
                                        if btn(ui, "− 列", false) {
                                            op = Some(TableOp::DeleteCol(hc));
                                        }
                                        ui.separator();
                                        if btn(ui, "左对齐", all(Align::Left)) {
                                            op = Some(TableOp::SetAlign(Align::Left));
                                        }
                                        if btn(ui, "居中", all(Align::Center)) {
                                            op = Some(TableOp::SetAlign(Align::Center));
                                        }
                                        if btn(ui, "右对齐", all(Align::Right)) {
                                            op = Some(TableOp::SetAlign(Align::Right));
                                        }
                                        // 最右侧右上角：删除表格（最右）+ 复制，与渲染态一致
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .button(
                                                        egui::RichText::new("删除表格")
                                                            .size(theme.body * 0.85),
                                                    )
                                                    .clicked()
                                                {
                                                    // 交给 app 层弹确认窗，避免误点直接丢数据。
                                                    actions.push(
                                                        render::Action::AskDeleteTable(idx),
                                                    );
                                                }
                                                if ui
                                                    .button(
                                                        egui::RichText::new("复制")
                                                            .size(theme.body * 0.85),
                                                    )
                                                    .clicked()
                                                {
                                                    ctx.copy_text(table_source(grid, aligns));
                                                    actions.push(render::Action::Notify(
                                                        "复制成功".to_string(),
                                                    ));
                                                }
                                            },
                                        );
                                        if let Some(op) = op {
                                            match op {
                                                TableOp::Delete => deleted = true,
                                                other => {
                                                    if table_op_on_grid(grid, aligns, other) {
                                                        edited = true;
                                                    }
                                                }
                                            }
                                        }
                                    });
                                        });
                                    ui.add_space(2.0);
                                    // 列宽与渲染态保持一致：平分可用宽度。
                                    // 以前单元格固定 140px，一点进编辑整张表就缩水成一小块。
                                    let cols = grid.first().map(|r| r.len()).unwrap_or(1).max(1);
                                    let gap =
                                        ui.spacing().item_spacing.x * cols.saturating_sub(1) as f32;
                                    let col_w = ((ui.available_width() - gap).max(120.0)
                                        / cols as f32)
                                        .max(56.0);
                                    ui.set_min_width(col_w * cols as f32 + gap);
                                    // 各行单元格矩形：Grid 之后画标题行补缝与行/列分隔线，
                                    // 与渲染态（render.rs 表格分支）同一套画法。
                                    let mut row_rects: Vec<Vec<egui::Rect>> = Vec::new();
                                    egui::Grid::new(("table_edit", idx))
                                        .min_col_width(col_w)
                                        .max_col_width(col_w)
                                        .show(ui, |ui| {
                                            for (ri, row) in grid.iter_mut().enumerate() {
                                                let mut cell_rects: Vec<egui::Rect> = Vec::new();
                                                for (ci, cell) in row.iter_mut().enumerate() {
                                                    // 编辑态与渲染态同一套配色与字号（body*0.95）：
                                                    // 行高一致，点击进入编辑时表格才不会跳。
                                                    let (fill, color) = if ri == 0 {
                                                        (theme.table_head, theme.heading)
                                                    } else {
                                                        (theme.bg, theme.text)
                                                    };
                                                    let r = ui.add(
                                                        egui::TextEdit::singleline(cell)
                                                            .desired_width(col_w)
                                                            // 按表格对齐方式排版单元格文字：之前编辑态的
                                                            // 单元格永远左对齐，点「居中/右对齐」看不到
                                                            // 任何变化，像是按钮失效（实际上源码已改）。
                                                            .align(
                                                                match aligns
                                                                    .get(ci)
                                                                    .copied()
                                                                    .unwrap_or(
                                                                        crate::ast::Align::Left,
                                                                    ) {
                                                                    crate::ast::Align::Left => {
                                                                        egui::Align2::LEFT_TOP
                                                                    }
                                                                    crate::ast::Align::Center => {
                                                                        egui::Align2::CENTER_TOP
                                                                    }
                                                                    crate::ast::Align::Right => {
                                                                        egui::Align2::RIGHT_TOP
                                                                    }
                                                                },
                                                            )
                                                            .font(egui::FontId::new(
                                                                theme.body * 0.95,
                                                                egui::FontFamily::Proportional,
                                                            ))
                                                            .text_color(color)
                                                            .frame(
                                                                egui::Frame::NONE
                                                                    .fill(fill)
                                                                    .inner_margin(3.0),
                                                            ),
                                                    );
                                                    cell_rects.push(r.rect);
                                                    // 渲染态点击了哪个单元格，这里就聚焦哪一格，
                                                    // 光标落在点击位置附近（右半边 → 文字末尾）。
                                                    if state.table_focus == Some((ri, ci)) {
                                                        state.table_focus = None;
                                                        let mut ts = egui::TextEdit::load_state(
                                                            ui.ctx(),
                                                            r.id,
                                                        )
                                                        .unwrap_or_default();
                                                        let pos = if state.table_caret_end {
                                                            cell.chars().count()
                                                        } else {
                                                            0
                                                        };
                                                        ts.cursor.set_char_range(Some(
                                                            egui::text::CCursorRange::one(
                                                                egui::text::CCursor::new(pos),
                                                            ),
                                                        ));
                                                        ts.store(ui.ctx(), r.id);
                                                        r.request_focus();
                                                    }
                                                    if r.changed() {
                                                        edited = true;
                                                    }
                                                }
                                                row_rects.push(cell_rects);
                                                ui.end_row();
                                            }
                                        });
                                    // 单元格里向外拖选：拖到别的单元格（或拖出表格）时，
                                    // 整张表格进入跨块选区，之后可直接 Cmd+C 复制，或点
                                    // 表格工具条的「复制」。单元格内部的小幅拖动仍由
                                    // TextEdit 选择文字，不影响原来的单元格内选词。
                                    let pointer = ui.ctx().input(|i| i.pointer.interact_pos());
                                    let pressed = ui.ctx().input(|i| {
                                        i.pointer.button_pressed(egui::PointerButton::Primary)
                                    });
                                    let down = ui.ctx().input(|i| i.pointer.primary_down());
                                    let dragging =
                                        ui.ctx().input(|i| i.pointer.is_decidedly_dragging());
                                    if pressed {
                                        state.table_drag_from = pointer
                                            .and_then(|p| table_cell_at(&row_rects, p))
                                            .map(|(r, c)| (idx, r, c));
                                    } else if !down {
                                        state.table_drag_from = None;
                                    } else if dragging
                                        && state.cross_sel.is_none()
                                        && let Some((bi, ri, ci)) = state.table_drag_from
                                        && bi == idx
                                        && let Some(p) = pointer
                                    {
                                        let moved_cell =
                                            table_cell_at(&row_rects, p) != Some((ri, ci));
                                        let bounds = row_rects
                                            .iter()
                                            .flatten()
                                            .fold(None::<egui::Rect>, |acc, r| {
                                                Some(acc.map_or(*r, |a| a.union(*r)))
                                            });
                                        let outside = bounds.is_some_and(|b| !b.contains(p));
                                        if moved_cell || outside {
                                            state.table_drag_from = None;
                                            state.cross_sel =
                                                Some((idx, 0, idx, block.plain().chars().count()));
                                            if let Some(fid) = ui.ctx().memory(|m| m.focused()) {
                                                ui.ctx().memory_mut(|m| m.surrender_focus(fid));
                                            }
                                        }
                                    }
                                    // 与渲染态一致：标题行底色铺满整行 + 行/列之间画单线分隔。
                                    // 之前编辑态只有外框描边、单元格又是同色填充，
                                    // 一点进表格格子之间的线就全没了。
                                    let p = ui.painter();
                                    if let Some(head) = row_rects.first()
                                        && head.len() == cols
                                    {
                                        let top =
                                            head.iter().map(|x| x.top()).fold(f32::MAX, f32::min);
                                        let bottom = head
                                            .iter()
                                            .map(|x| x.bottom())
                                            .fold(f32::MIN, f32::max);
                                        for k in 1..cols {
                                            let left = head[k - 1].right();
                                            let right = head[k].left();
                                            if right > left {
                                                p.rect_filled(
                                                    egui::Rect::from_min_max(
                                                        egui::Pos2::new(left, top),
                                                        egui::Pos2::new(right, bottom),
                                                    ),
                                                    0.0,
                                                    theme.table_head,
                                                );
                                            }
                                        }
                                    }
                                    let stroke = egui::Stroke::new(1.0, theme.table_line);
                                    // 行分隔线：相邻两行单元格之间的中线
                                    for i in 1..row_rects.len() {
                                        let next_top = row_rects[i].first().map(|x| x.top());
                                        let prev_bottom = row_rects[i - 1]
                                            .iter()
                                            .map(|x| x.bottom())
                                            .fold(f32::MIN, f32::max);
                                        if let Some(t) = next_top {
                                            let y = (t + prev_bottom) / 2.0;
                                            let left = row_rects[i]
                                                .iter()
                                                .map(|x| x.left())
                                                .fold(f32::MAX, f32::min);
                                            let right = row_rects[i]
                                                .iter()
                                                .map(|x| x.right())
                                                .fold(f32::MIN, f32::max);
                                            p.line_segment(
                                                [
                                                    egui::Pos2::new(left, y),
                                                    egui::Pos2::new(right, y),
                                                ],
                                                stroke,
                                            );
                                        }
                                    }
                                    // 列分隔线：相邻单元格右/左缘的中点，纵贯整个网格
                                    let grid_top =
                                        row_rects.first().and_then(|r| r.first()).map(|x| x.top());
                                    let grid_bottom = row_rects
                                        .last()
                                        .and_then(|r| r.first())
                                        .map(|x| x.bottom());
                                    if let (Some(top), Some(bottom)) = (grid_top, grid_bottom) {
                                        let mut xs: Vec<f32> = Vec::new();
                                        for row in &row_rects {
                                            for k in 1..row.len() {
                                                let x = (row[k - 1].right() + row[k].left()) / 2.0;
                                                if !xs.iter().any(|v| (*v - x).abs() < 0.5) {
                                                    xs.push(x);
                                                }
                                            }
                                        }
                                        for x in xs {
                                            p.line_segment(
                                                [
                                                    egui::Pos2::new(x, top),
                                                    egui::Pos2::new(x, bottom),
                                                ],
                                                stroke,
                                            );
                                        }
                                    }
                                }
                                // 删除表格：清空草稿并把写回源码置空（整块变空段落）
                                if deleted {
                                    state.table_draft = None;
                                    table_edit_src = Some(String::new());
                                    changed = true;
                                }
                            });
                            if edited {
                                if let Some((_, grid, aligns)) = state.table_draft.as_ref() {
                                    table_edit_src = Some(table_source(grid, aligns));
                                }
                                changed = true;
                            }
                            // 编辑态不要给整块再盖一层点击感应：那层会挡住单元格 TextEdit
                            // 的点击聚焦，表现为「点了没反应」。单元格自己处理点击。
                            inner.response
                        } else if editing {
                            let id = Id::new(("block_edit", tab_key, idx));
                            // 标题只编辑文字、代码块只编辑代码（不含围栏），像 Typora 一样直接改。
                            // 代码块的行数 + 1 对齐渲染态：渲染态的 code_job 会在末尾
                            // 补一个换行（多一行视觉空行），编辑态用 desired_rows 预留
                            // 同样高度，点击进入/退出编辑时内容不跳。
                            let rows = if matches!(edit_kind, EditKind::Code { .. }) {
                                active_buf.lines().count().max(1) + 1
                            } else if block.is_multiline() {
                                2
                            } else {
                                1
                            };
                            // 整段只有图片（一张或多张连续图片）时，编辑态在上方
                            // 原样露出 `![文件名](路径)` 源码、下方保留图片预览；
                            // 点击别处就退出编辑，只剩图片。
                            let image_blocks: Option<Vec<(String, String, Option<u32>)>> =
                                match block {
                                    Block::Paragraph { content, .. } => {
                                        let has_image = content
                                            .iter()
                                            .any(|i| matches!(i, crate::ast::Inline::Image { .. }));
                                        let only_images = has_image
                                            && content.iter().all(|i| {
                                                matches!(
                                                    i,
                                                    crate::ast::Inline::Image { .. }
                                                        | crate::ast::Inline::Break
                                                )
                                            });
                                        only_images.then(|| {
                                            content
                                                .iter()
                                                .filter_map(|i| match i {
                                                    crate::ast::Inline::Image {
                                                        alt,
                                                        url,
                                                        width,
                                                    } => Some((alt.clone(), url.clone(), *width)),
                                                    _ => None,
                                                })
                                                .collect()
                                        })
                                    }
                                    _ => None,
                                };
                            // 编辑态行内排版：缓冲区还是原始 Markdown，但标记被隐藏，
                            // 看起来就是渲染后的富文本，而不是源码。
                            // 光标位置传给排版器：紧挨光标的标记要显示出来，
                            // 不然在加粗文字旁边按 Shift+8 打的星号会看不见。
                            let caret = state.last_cursor;
                            // 上一帧的选区：行内代码被选中时用它把代码片底色换成选区色。
                            let sel = state.sel;
                            let mut layouter =
                                |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                                    let job = render::inline_job(
                                        buf.as_str(),
                                        theme,
                                        theme.body,
                                        theme.text,
                                        wrap,
                                        Some(caret),
                                        sel,
                                    );
                                    ui.fonts_mut(|f| f.layout_job(job))
                                };
                            // 代码块编辑时也走高亮排版：缓冲区是纯代码，用同一套词法着色，
                            // 边打字边能看到关键字/字符串的颜色，而不是退化成一片纯文本。
                            let code_lang = match &edit_kind {
                                EditKind::Code { lang: l, .. } | EditKind::Diagram(l) => l.as_str(),
                                EditKind::CodeGroup { items, tab } => items
                                    .get(*tab)
                                    .map(|i| i.lang.as_str())
                                    .unwrap_or_default(),
                                _ => "",
                            };
                            let mut code_layouter =
                                |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                                    let mut job =
                                        render::code_job_edit(buf.as_str(), code_lang, theme);
                                    job.wrap.max_width = wrap;
                                    ui.fonts_mut(|f| f.layout_job(job))
                                };
                            let mut te = TextEdit::multiline(&mut active_buf)
                                .id(id)
                                .desired_rows(rows)
                                .desired_width(page_w);
                            te = match &edit_kind {
                                EditKind::Heading(level) => te
                                    .font(egui::FontId::new(
                                        theme.heading_size(*level),
                                        egui::FontFamily::Proportional,
                                    ))
                                    .text_color(theme.heading)
                                    .frame(egui::Frame::NONE)
                                    .return_key(Option::<egui::KeyboardShortcut>::None),
                                // 代码块：外层 panel_frame 提供盒子（按钮行 + 代码同盒，
                                // 与渲染态一致），TextEdit 自身不再带底色边距。
                                // 宽度对齐渲染态：盒子内可用宽 = page_w - 两侧边距 24。
                                EditKind::Code { .. } => te
                                    .font(egui::FontId::new(
                                        theme.mono,
                                        egui::FontFamily::Monospace,
                                    ))
                                    .text_color(theme.text)
                                    .layouter(&mut code_layouter)
                                    .desired_width((page_w - 24.0).max(80.0))
                                    .frame(egui::Frame::NONE),
                                // 代码组：与代码块同款等宽高亮，只是缓冲是当前标签的代码。
                                EditKind::CodeGroup { .. } => te
                                    .font(egui::FontId::new(
                                        theme.mono,
                                        egui::FontFamily::Monospace,
                                    ))
                                    .text_color(theme.text)
                                    .layouter(&mut code_layouter)
                                    .desired_width((page_w - 24.0).max(80.0))
                                    .frame(egui::Frame::NONE),
                                EditKind::Diagram(_) => te
                                    .font(egui::FontId::new(
                                        theme.mono,
                                        egui::FontFamily::Monospace,
                                    ))
                                    .text_color(theme.text)
                                    .layouter(&mut code_layouter)
                                    .frame(
                                        egui::Frame::NONE
                                            .fill(theme.code_bg)
                                            .inner_margin(12.0)
                                            .corner_radius(6.0),
                                    ),
                                // 段落/列表：TextEdit 主题默认的垂直 margin 会让
                                // 编辑态比渲染态（Label 无 margin）高约 5px，点击进入
                                // 编辑时整块往下跳。归零后两态高度一致，块不再跳动。
                                EditKind::Source
                                | EditKind::List { .. }
                                | EditKind::Quote { .. } => {
                                    if image_blocks.is_some() {
                                        // 图片源码原样显示 `![文件名](路径)`：
                                        // 隐藏标记的 inline_job 会把地址折掉，
                                        // 看不到完整路径，不符合图片源码模式的预期。
                                        te.font(egui::FontId::new(
                                            theme.body * 0.85,
                                            egui::FontFamily::Monospace,
                                        ))
                                        .text_color(theme.dim)
                                        .frame(egui::Frame::NONE)
                                        .margin(egui::Margin::ZERO)
                                        .return_key(Option::<egui::KeyboardShortcut>::None)
                                        .desired_width(page_w)
                                    } else {
                                        te.font(egui::FontId::new(
                                            theme.body,
                                            egui::FontFamily::Proportional,
                                        ))
                                        .frame(egui::Frame::NONE)
                                        .margin(egui::Margin::ZERO)
                                        .return_key(Option::<egui::KeyboardShortcut>::None)
                                        .layouter(&mut layouter)
                                    }
                                }
                            };
                            // 代码块编辑态：按钮行（删除/复制/语言）与代码同在一个
                            // panel_frame 盒子里，与渲染态完全一致——否则点击进入
                            // 编辑后按钮会"跳到"盒子右上方之外。
                            let (out, row_resp) = if let EditKind::Code { lang, quote } = &edit_kind
                            {
                                let quote_level = *quote;
                                let mut copied = false;
                                // 功能栏矩形与「是否点了功能栏控件」：点按钮/下拉时不抢焦点，
                                // 点功能栏空白处仍然把焦点交还给代码编辑框。
                                let mut header_rect = egui::Rect::NOTHING;
                                let mut header_clicked = false;
                                // 引用内的代码块：左侧留出缩进 + 竖线位置，
                                // 编辑时也保持引用竖线，与渲染态一致。
                                let hr = ui.horizontal(|ui| {
                                    if quote_level > 0 {
                                        ui.add_space(quote_level.min(4) as f32 * 8.0 + 11.0);
                                    }
                                    // 外层横向布局会传染给面板内容，显式回到纵向。
                                    let fr = ui
                                        .vertical(|ui| {
                                            render::panel_frame(theme).show(ui, |ui| {
                                                // 功能栏：左语言 + 右操作按钮（与渲染态同款布局）
                                                header_rect = render::code_header_frame(theme)
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            if let Some(l) = render::language_combo(
                                                                ui,
                                                                ("code_lang_edit", tab_key, idx),
                                                                lang,
                                                            ) {
                                                                lang_change = Some(l);
                                                                header_clicked = true;
                                                            }
                                                            ui.with_layout(
                                                                egui::Layout::right_to_left(
                                                                    egui::Align::Center,
                                                                ),
                                                                |ui| {
                                                                    if ui
                                                                    .button(
                                                                        egui::RichText::new("折叠")
                                                                            .size(
                                                                                theme.body * 0.85,
                                                                            ),
                                                                    )
                                                                    .on_hover_cursor(
                                                                        egui::CursorIcon::Default,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    header_clicked = true;
                                                                    actions.push(
                                                                        render::Action::FoldToggle(
                                                                            idx,
                                                                        ),
                                                                    );
                                                                    // 折叠的是活动块：把它让给相邻块，立即退出编辑态看到折叠效果。
                                                                    new_active = Some(
                                                                        if idx + 1
                                                                            < doc.blocks.len()
                                                                        {
                                                                            idx + 1
                                                                        } else {
                                                                            idx.saturating_sub(1)
                                                                        },
                                                                    );
                                                                }
                                                                    if ui
                                                                    .button(
                                                                        egui::RichText::new("删除")
                                                                            .size(
                                                                                theme.body * 0.85,
                                                                            ),
                                                                    )
                                                                    .on_hover_cursor(
                                                                        egui::CursorIcon::Default,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    header_clicked = true;
                                                                    actions.push(
                                                                render::Action::AskDeleteBlock(idx),
                                                            );
                                                                }
                                                                    if ui
                                                                    .button(
                                                                        egui::RichText::new("复制")
                                                                            .size(
                                                                                theme.body * 0.85,
                                                                            ),
                                                                    )
                                                                    .on_hover_cursor(
                                                                        egui::CursorIcon::Default,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    copied = true;
                                                                    header_clicked = true;
                                                                }
                                                                },
                                                            );
                                                        });
                                                    })
                                                    .response
                                                    .rect;
                                                ui.add_space(6.0);
                                                render::code_divider(ui, theme);
                                                ui.add_space(6.0);
                                                te.show(ui)
                                            })
                                        })
                                        .inner;
                                    if quote_level > 0 {
                                        // 竖线高度跟随整个代码面板（含功能栏），
                                        // 连续引用时用帧末的补接逻辑连成一条。
                                        let rect = fr.response.rect;
                                        ui.painter().rect_filled(
                                            egui::Rect::from_min_max(
                                                egui::pos2(rect.left() - 11.0, rect.top()),
                                                egui::pos2(rect.left() - 8.0, rect.bottom()),
                                            ),
                                            1.0,
                                            theme.accent,
                                        );
                                    }
                                    fr
                                });
                                let fr = hr.inner;
                                if copied {
                                    ctx.copy_text(active_buf.clone());
                                    actions.push(Action::Notify("复制成功".to_string()));
                                }
                                // 整块矩形（左侧引用缩进 + 功能栏 + 代码区）：
                                // 1) 用作 block_rects，点在这些留白上不算「点空白」，
                                //    否则空白点会把光标甩到文档末尾（看起来像代码块不能编辑）；
                                // 2) 点在代码区（功能栏以下）时聚焦代码编辑框，并把光标放到点击处。
                                let code_row = hr.response.union(fr.response);
                                let popup_open = egui::Popup::is_any_open(ui.ctx());
                                let te_rect = fr.inner.response.response.rect;
                                if let Some(p) = ui.input(|i| i.pointer.interact_pos())
                                    && ui.input(|i| i.pointer.primary_clicked())
                                    && code_row.rect.contains(p)
                                    // TextEdit 自己的点击交给它处理；只接管它没覆盖到的留白。
                                    && !te_rect.contains(p)
                                    && !(header_rect.contains(p) && (header_clicked || popup_open))
                                {
                                    let had_focus = fr.inner.response.response.has_focus();
                                    fr.inner.response.response.request_focus();
                                    // 点击处在代码区才按点击位置放光标；功能栏空白保持原光标。
                                    if !had_focus && !header_rect.contains(p) {
                                        let ci = fr
                                            .inner
                                            .galley
                                            .cursor_from_pos(p - fr.inner.galley_pos)
                                            .index
                                            .0;
                                        let pos = ci.min(active_buf.chars().count());
                                        let mut ts = fr.inner.state.clone();
                                        ts.cursor.set_char_range(Some(
                                            egui::text::CCursorRange::one(
                                                egui::text::CCursor::new(pos),
                                            ),
                                        ));
                                        ts.store(ui.ctx(), id);
                                        cursor_idx = pos;
                                        selection = None;
                                    }
                                }
                                (fr.inner, code_row)
                            } else if let EditKind::CodeGroup { items, tab } = &edit_kind {
                                let tab = (*tab).min(items.len().saturating_sub(1));
                                let mut header_rect = egui::Rect::NOTHING;
                                let mut copied = false;
                                let hr = ui.horizontal(|ui| {
                                    let fr = ui
                                        .vertical(|ui| {
                                            render::panel_frame(theme).show(ui, |ui| {
                                                // 功能栏：左侧语言标签，右侧与代码块同款按钮。
                                                header_rect = render::code_header_frame(theme)
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            // 先「+ / −」增删标签，再是语言标签。
                                                            ui.spacing_mut().item_spacing.x = 4.0;
                                                            let (a, r) = render::code_group_tab_buttons(
                                                                ui,
                                                                theme,
                                                                items.len() > 1,
                                                            );
                                                            group_add_tab |= a;
                                                            group_remove_tab |= r;
                                                            for (i, it) in items.iter().enumerate() {
                                                                if render::code_group_tab(
                                                                    ui,
                                                                    theme,
                                                                    i == tab,
                                                                    &it.lang,
                                                                ) {
                                                                    group_tab_switch = Some(i);
                                                                }
                                                            }
                                                            ui.with_layout(
                                                                egui::Layout::right_to_left(
                                                                    egui::Align::Center,
                                                                ),
                                                                |ui| {
                                                                    ui.spacing_mut().item_spacing.x =
                                                                        8.0;
                                                                    if ui
                                                                        .button(
                                                                            egui::RichText::new(
                                                                                "折叠",
                                                                            )
                                                                            .size(
                                                                                theme.body * 0.85,
                                                                            ),
                                                                        )
                                                                        .on_hover_cursor(
                                                                            egui::CursorIcon::Default,
                                                                        )
                                                                        .clicked()
                                                                    {
                                                                        group_tab_switch = None;
                                                                        actions.push(
                                                                            render::Action::FoldToggle(
                                                                                idx,
                                                                            ),
                                                                        );
                                                                        new_active = Some(
                                                                            if idx + 1
                                                                                < doc.blocks.len()
                                                                            {
                                                                                idx + 1
                                                                            } else {
                                                                                idx.saturating_sub(1)
                                                                            },
                                                                        );
                                                                    }
                                                                    if ui
                                                                        .button(
                                                                            egui::RichText::new("删除")
                                                                                .size(
                                                                                    theme.body
                                                                                        * 0.85,
                                                                                ),
                                                                        )
                                                                        .on_hover_cursor(
                                                                            egui::CursorIcon::Default,
                                                                        )
                                                                        .clicked()
                                                                    {
                                                                        actions.push(
                                                                        render::Action::AskDeleteBlock(idx),
                                                                    );
                                                                    }
                                                                    if ui
                                                                        .button(
                                                                            egui::RichText::new("复制")
                                                                                .size(
                                                                                    theme.body
                                                                                        * 0.85,
                                                                                ),
                                                                        )
                                                                        .on_hover_cursor(
                                                                            egui::CursorIcon::Default,
                                                                        )
                                                                        .clicked()
                                                                    {
                                                                        copied = true;
                                                                    }
                                                                    if let Some(l) = render::language_combo(
                                                                        ui,
                                                                        (
                                                                            "code_group_lang_edit",
                                                                            tab_key,
                                                                            idx,
                                                                        ),
                                                                        &items[tab].lang,
                                                                    ) {
                                                                        group_lang_change = Some(l);
                                                                    }
                                                                },
                                                            );
                                                        });
                                                    })
                                                    .response
                                                    .rect;
                                                ui.add_space(6.0);
                                                render::code_divider(ui, theme);
                                                ui.add_space(6.0);
                                                te.show(ui)
                                            })
                                        })
                                        .inner;
                                    fr
                                });
                                let fr = hr.inner;
                                if copied {
                                    ctx.copy_text(items[tab].code.clone());
                                    actions.push(render::Action::Notify("复制成功".to_string()));
                                }
                                // 点代码区留白聚焦编辑框；点标签/按钮不抢焦点。
                                let code_row = hr.response.union(fr.response);
                                let popup_open = egui::Popup::is_any_open(ui.ctx());
                                let te_rect = fr.inner.response.response.rect;
                                let header_clicked = group_tab_switch.is_some()
                                    || group_lang_change.is_some()
                                    || group_add_tab
                                    || group_remove_tab;
                                if let Some(p) = ui.input(|i| i.pointer.interact_pos())
                                    && ui.input(|i| i.pointer.primary_clicked())
                                    && code_row.rect.contains(p)
                                    && !te_rect.contains(p)
                                    && !(header_rect.contains(p)
                                        && (header_clicked || popup_open))
                                {
                                    let had_focus = fr.inner.response.response.has_focus();
                                    fr.inner.response.response.request_focus();
                                    if !had_focus && !header_rect.contains(p) {
                                        let ci = fr
                                            .inner
                                            .galley
                                            .cursor_from_pos(p - fr.inner.galley_pos)
                                            .index
                                            .0;
                                        let pos = ci.min(active_buf.chars().count());
                                        let mut ts = fr.inner.state.clone();
                                        ts.cursor.set_char_range(Some(
                                            egui::text::CCursorRange::one(
                                                egui::text::CCursor::new(pos),
                                            ),
                                        ));
                                        ts.store(ui.ctx(), id);
                                        cursor_idx = pos;
                                        selection = None;
                                    }
                                }
                                (fr.inner, code_row)
                            } else if let EditKind::List {
                                indent,
                                marker,
                                checked,
                            } = &edit_kind
                            {
                                let indent_px = (*indent).min(12) as f32 * 8.0;
                                let hr = ui.horizontal(|ui| {
                                    ui.add_space(indent_px);
                                    // 与渲染态一致：标记贴近正文、复选框与圆点放大。
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    match checked {
                                        Some(c) => {
                                            ui.spacing_mut().icon_width = 19.0;
                                            ui.spacing_mut().icon_width_inner = 11.0;
                                            ui.spacing_mut().icon_spacing = 3.0;
                                            let v = &mut ui.visuals_mut().widgets;
                                            v.inactive.bg_stroke =
                                                egui::Stroke::new(1.0, theme.dim);
                                            v.hovered.bg_stroke =
                                                egui::Stroke::new(1.0, theme.accent);
                                            v.active.bg_stroke =
                                                egui::Stroke::new(1.0, theme.accent);
                                            let mut cb_state = *c;
                                            let cb = ui.checkbox(&mut cb_state, "");
                                            if cb.clicked() {
                                                actions.push(render::Action::ToggleTask(idx));
                                            }
                                        }
                                        None => {
                                            // 与渲染态一致：层级的标记样式随缩进变化。
                                            let level = render::list_level(*indent);
                                            if marker.ends_with('.') || marker.ends_with(')') {
                                                let mut f = theme.fmt(theme.body);
                                                f.color = theme.dim;
                                                let mut job = egui::text::LayoutJob::default();
                                                job.append(
                                                    &render::ordered_list_marker(marker, level),
                                                    0.0,
                                                    f,
                                                );
                                                ui.label(job);
                                            } else {
                                                let d = (theme.body * 0.6).clamp(7.0, 14.0);
                                                let color =
                                                    theme.dim.lerp_to_gamma(theme.text, 0.55);
                                                render::draw_list_bullet(ui, level, color, d);
                                            }
                                        }
                                    }
                                    // Tab / Shift+Tab 在列表项里表示调整缩进层级，
                                    // 不能让它变成 \t 插进正文，也不能把焦点移走。
                                    let shift_tab = ui.input(|i| {
                                        i.events.iter().find_map(|e| match e {
                                            egui::Event::Key {
                                                key: Key::Tab,
                                                pressed: true,
                                                modifiers,
                                                ..
                                            } => Some(modifiers.shift),
                                            _ => None,
                                        })
                                    });
                                    if let Some(shift_tab) = shift_tab {
                                        list_tab = Some(shift_tab);
                                        ui.ctx().input_mut(|i| {
                                            i.events.retain(|e| {
                                                !matches!(
                                                    e,
                                                    egui::Event::Key {
                                                        key: Key::Tab,
                                                        pressed: true,
                                                        ..
                                                    }
                                                )
                                            });
                                        });
                                    }
                                    let te = te
                                        .event_filter(egui::EventFilter {
                                            tab: true,
                                            ..Default::default()
                                        })
                                        .desired_width((ui.available_width() - 8.0).max(80.0));
                                    // 行内代码的圆角底色垫在文字下层。
                                    let code_bg = ui.painter().add(egui::Shape::Noop);
                                    let out = te.show(ui);
                                    let shapes = render::inline_code_bg_shapes(
                                        &out.galley,
                                        out.galley_pos,
                                        theme,
                                    );
                                    if !shapes.is_empty() {
                                        ui.painter().set(code_bg, egui::Shape::Vec(shapes));
                                    }
                                    out
                                });
                                (hr.inner, hr.response)
                            } else if let EditKind::Quote { level } = &edit_kind {
                                // 引用编辑态与渲染态一致：左侧竖线 + 缩进，正文加粗、
                                // 引用配色，不显示 `>` 源码标记。
                                let indent_px = (*level).min(4) as f32 * 8.0;
                                let bold_family = egui::FontFamily::Name("markd-bold".into());
                                let mut quote_layouter =
                                    |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                                        let mut job = render::inline_job(
                                            buf.as_str(),
                                            theme,
                                            theme.body,
                                            theme.quote,
                                            wrap,
                                            Some(caret),
                                            sel,
                                        );
                                        // 只把引用正文（普通文字，颜色为引用色）换成粗体；
                                        // 透明标记、链接、行内代码等保持原样式。
                                        for section in &mut job.sections {
                                            if section.format.font_id.family
                                                == egui::FontFamily::Proportional
                                                && section.format.color == theme.quote
                                            {
                                                section.format.font_id = egui::FontId::new(
                                                    theme.body,
                                                    bold_family.clone(),
                                                );
                                            }
                                        }
                                        ui.fonts_mut(|f| f.layout_job(job))
                                    };
                                let hr = ui.horizontal(|ui| {
                                    ui.add_space(indent_px);
                                    // 竖线先不占固定高度：给正文留出「线宽 + 间距」，
                                    // 等 TextEdit 排版完成后再按它的实际高度画，
                                    // 软换行/多行时竖线会一直延伸到最底部。
                                    ui.add_space(11.0);
                                    let te = te
                                        .text_color(theme.quote)
                                        .layouter(&mut quote_layouter)
                                        .desired_width((ui.available_width() - 8.0).max(80.0));
                                    // 行内代码的圆角底色垫在文字下层。
                                    let code_bg = ui.painter().add(egui::Shape::Noop);
                                    let out = te.show(ui);
                                    let shapes = render::inline_code_bg_shapes(
                                        &out.galley,
                                        out.galley_pos,
                                        theme,
                                    );
                                    if !shapes.is_empty() {
                                        ui.painter().set(code_bg, egui::Shape::Vec(shapes));
                                    }
                                    let r = out.response.rect;
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_max(
                                            egui::pos2(r.left() - 11.0, r.top()),
                                            egui::pos2(r.left() - 8.0, r.bottom()),
                                        ),
                                        1.0,
                                        theme.accent,
                                    );
                                    out
                                });
                                (hr.inner, hr.response)
                            } else {
                                // 图片块：上方露出 `![文件名](路径)` 源码，
                                // 下方保留图片预览（可拖动右下角缩放）；
                                // 点别处即退出这种模式。
                                let (out, preview) = if let Some(imgs) = &image_blocks {
                                    let mut prev: Option<egui::Response> = None;
                                    let out = ui
                                        .vertical(|ui| {
                                            let out = te.show(ui);
                                            ui.add_space(2.0);
                                            for (no, (alt, url, width)) in imgs.iter().enumerate() {
                                                let (r, resized) = render::image_preview(
                                                    ui,
                                                    alt,
                                                    url,
                                                    theme,
                                                    ctx,
                                                    images,
                                                    base_dir,
                                                    *width,
                                                    (idx, no),
                                                );
                                                if let Some(w) = resized {
                                                    actions.push(render::Action::ResizeImage(
                                                        idx, no, w,
                                                    ));
                                                }
                                                prev = Some(match prev.take() {
                                                    Some(p) => p.union(r),
                                                    None => r,
                                                });
                                            }
                                            out
                                        })
                                        .inner;
                                    (out, prev)
                                } else {
                                    // 行内代码的圆角底色垫在文字下层。
                                    let code_bg = ui.painter().add(egui::Shape::Noop);
                                    let out = te.show(ui);
                                    let shapes = render::inline_code_bg_shapes(
                                        &out.galley,
                                        out.galley_pos,
                                        theme,
                                    );
                                    if !shapes.is_empty() {
                                        ui.painter().set(code_bg, egui::Shape::Vec(shapes));
                                    }
                                    (out, None)
                                };
                                let mut row = out.response.response.clone();
                                if let Some(p) = preview {
                                    row = row.union(p);
                                }
                                (out, row)
                            };
                            // 编辑态里 Cmd/Ctrl+单击链接才打开；普通点击只把光标
                            // 放进链接，并露出 `[标签](地址)` 源码供修改。
                            if out.response.response.clicked()
                                && ui.input(|i| i.modifiers.command || i.modifiers.ctrl)
                            {
                                if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                                    let ci = out.galley.cursor_from_pos(p - out.galley_pos).index.0;
                                    if let Some(url) = render::link_at_markdown(&active_buf, ci) {
                                        actions.push(Action::OpenLink(url));
                                    }
                                }
                            }
                            // 点击进入编辑：把光标放到刚才点击的位置，而不是跳到末尾/居中
                            if let Some(off) = state.pending_caret.take() {
                                let ccursor = out.galley.cursor_from_pos(off);
                                let mut ts = out.state.clone();
                                ts.cursor
                                    .set_char_range(Some(egui::text::CCursorRange::one(ccursor)));
                                ts.store(ui.ctx(), id);
                                cursor_idx = ccursor.index.0.min(active_buf.chars().count());
                            }
                            // 查找/替换跳转：把命中的文字选中（字符下标按当前缓冲算）。
                            if let Some((bi, a, b)) = state.select_req
                                && bi == idx
                            {
                                state.select_req = None;
                                let len = active_buf.chars().count();
                                let (a, b) = (a.min(len), b.min(len));
                                let mut ts = out.state.clone();
                                ts.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(a),
                                    egui::text::CCursor::new(b),
                                )));
                                ts.store(ui.ctx(), id);
                                out.response.request_focus();
                                cursor_idx = a;
                            }
                            let cr = out.cursor_range;
                            let resp = out.response;
                            // 活动块（含空行）起拖由 text_response 的兜底分支接管：
                            // 拖出本块落到别的渲染态块时以光标为锚点建立跨块选区。
                            // 这里不再在按下帧预置选区，否则普通点击会残留。
                            if resp.changed() {
                                changed = true;
                            }
                            let len = active_buf.chars().count();
                            if let Some(r) = cr {
                                let a = r.primary.index.0;
                                let b = r.secondary.index.0;
                                cursor_idx = a.min(len);
                                selection = if a == b {
                                    None
                                } else {
                                    Some((a.min(b).min(len), a.max(b).min(len)))
                                };
                            }
                            // 在编辑态块内按下鼠标 → 记下按下位置，作为从本块
                            // 拖出时的跨块选区锚点（否则锚点会变成拖出时已到行尾的光标）。
                            let pressed_now = ui
                                .input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
                            if pressed_now
                                && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                                && resp.response.rect.contains(p)
                            {
                                // 用点击位置直接在 galley 上映射字符：按下那一帧
                                // TextEdit 可能还没返回 cursor_range，cursor_idx
                                // 仍是初始值（行尾），会把锚点记错。
                                let ci = out.galley.cursor_from_pos(p - out.galley_pos).index.0;
                                state.press_caret = Some((idx, ci.min(len)));
                            }
                            // 格式按钮（加粗/斜体等）用完选区后，把文本框里存的
                            // 旧选区收敛成单点光标，不然那段文字会一直保持选中。
                            if let Some((bi, pos)) = state.caret_req
                                && bi == idx
                            {
                                state.caret_req = None;
                                let pos = pos.min(len);
                                let mut ts = out.state.clone();
                                ts.cursor.set_char_range(Some(egui::text::CCursorRange::one(
                                    egui::text::CCursor::new(pos),
                                )));
                                ts.store(ui.ctx(), id);
                                cursor_idx = pos;
                                selection = None;
                                resp.request_focus();
                            }
                            if let Some(f) = state.focus_req {
                                if f == idx {
                                    resp.request_focus();
                                }
                            }
                            // 整行响应 = TextEdit ∪ 行首标记区：点复选框/圆点不算
                            // 「点在空白」，否则空白点击会把活动块切到文档末尾。
                            resp.response.union(row_resp)
                        } else {
                            let mut env = RenderEnv {
                                theme,
                                ctx,
                                images,
                                base_dir,
                                outline,
                                actions,
                                max_width: page_w,
                                cross_sel: state.cross_sel,
                                folded: &state.folded,
                                group_tab: state.group_tab(idx),
                            };
                            render::render_block(ui, idx, block, &mut env, focus_mode)
                        };
                        // 非文本块的选区叠加层：必须画在块内容之后（面板底色会盖住
                        // 画在前面的形状），半透明所以不影响阅读。
                        if let Some(color) = sel_overlay {
                            if resp.rect.is_positive() {
                                ui.painter().rect_filled(resp.rect.expand(1.0), 3.0, color);
                            }
                        }
                        // 非文本块也参与跨块拖选：指针拖到它上面时把选区端点延伸进来。
                        // 这类块没有可映射的字符位置，按「整块」计——向下拖取块尾、
                        // 向上拖取块首，保证选区能跨过代码块/图表继续延伸。
                        if !editing && !text_selection_block(block) && resp.rect.is_positive() {
                            let over = drag_pos.is_some_and(|p| resp.rect.contains(p));
                            let down = ctx.input(|i| i.pointer.primary_down());
                            let dragging = ctx.input(|i| i.pointer.is_decidedly_dragging());
                            let len = block.plain().chars().count();
                            if over && down && press_in_editor {
                                // 已有选区：把端点延伸进本块（向下取块尾、向上取块首）。
                                if let Some((a, _, _, _)) = state.cross_sel {
                                    actions.push(Action::CrossDrag(
                                        idx,
                                        if idx >= a { len } else { 0 },
                                        false,
                                    ));
                                } else if in_viewport
                                    && dragging
                                    && matches!(block, Block::Code { .. } | Block::Diagram { .. })
                                {
                                    // 只有「真的拖动」才从代码块/图表开启选区：
                                    // 单纯按下-松开必须保留「点击进入编辑」，否则空代码块
                                    // 点一下会停在零长度选区里，输入不进代码。
                                    actions.push(Action::CrossDrag(idx, 0, true));
                                } else if in_viewport && dragging {
                                    // 起点在活动块的 TextEdit 里：以那里为锚点。
                                    actions.push(Action::ShiftClick(
                                        idx,
                                        if idx >= state.active { len } else { 0 },
                                    ));
                                }
                            }
                        }
                        // 行号：画在编辑区最左侧固定 70px 行号栏内，右缘固定缩进，
                        // 不随窗口宽度（版心居中）或模式移动。
                        if show_line_numbers {
                            let num_x = viewport_left + gutter_w - GUTTER_PAD;
                            if num_x - theme.body > ui.clip_rect().left() {
                                ui.painter().text(
                                    egui::Pos2::new(num_x, resp.rect.top() + theme.body * 0.5),
                                    egui::Align2::RIGHT_CENTER,
                                    &block_start_lines[idx].to_string(),
                                    egui::FontId::proportional(theme.body),
                                    theme.dim.gamma_multiply(0.75),
                                );
                            }
                        }
                        if resp.rect.is_positive() {
                            block_rects.push((idx, resp.rect));
                            if let Some(level) = quote_level_of(block) {
                                // 渲染态 resp 是文字/面板矩形（竖线在左侧 11px 处）；
                                // 编辑态 resp 是整条横向区域（文字前留了缩进 + 11px）。
                                let x = if editing {
                                    resp.rect.left() + level.min(4) as f32 * 8.0
                                } else {
                                    resp.rect.left() - 11.0
                                };
                                quote_bars.push((
                                    idx,
                                    egui::Rect::from_min_max(
                                        egui::pos2(x, resp.rect.top()),
                                        egui::pos2(x + 3.0, resp.rect.bottom()),
                                    ),
                                ));
                            }
                        }
                        if resp.clicked() {
                            new_active = Some(idx);
                            state.clicked = true;
                            // 点击进入编辑：清除跨块选区（拖选才产生它）
                            if state.cross_sel.is_some() {
                                state.cross_sel = None;
                            }
                        }
                        if is_active {
                            active_rect = Some(resp.rect);
                            if !editing {
                                state.sel = None;
                            }
                        }
                        if quote_run {
                            ui.add_space(1.0);
                        } else {
                            ui.add_space(if compact { 1.0 } else { 6.0 });
                        }
                    }
                    // 连续引用的竖线要连成一条：egui 会在相邻块之间自动留
                    // item_spacing，单块各自的竖线在块间距处会断开。这里用同色
                    // 小矩形把相邻两个引用块竖线之间的空隙补上。
                    for pair in quote_bars.windows(2) {
                        let ((i, a), (j, b)) = (pair[0], pair[1]);
                        if j != i + 1 {
                            continue;
                        }
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(a.left(), a.bottom()),
                                egui::pos2(a.left() + 3.0, b.top() + 1.0),
                            ),
                            0.0,
                            theme.accent,
                        );
                    }
                });
            });
            // 只在「本帧真的在输入」时跟随滚动。若每帧都检查，用户用滚轮滚开
            // 会被立刻拉回活动块，表现就是滚轮失效、来回跳。
            if !state.read_only && changed {
                if let Some(r) = active_rect {
                    if !ui.clip_rect().contains_rect(r) {
                        ui.scroll_to_rect(r, None);
                    }
                }
            }
            // 打字机模式只在光标移动时居中。以前每帧都强制 scroll_to_rect，
            // 会把用户用滚轮翻到的位置立刻拉回去，表现就是「滚轮滚不动」。
            if typewriter && !state.read_only && cursor_idx != state.last_cursor {
                if let Some(r) = active_rect {
                    ui.scroll_to_rect(r, Some(egui::Align::Center));
                }
            }
            // 大纲 / TOC / 新建块等显式跳转请求。必须 take()：
            // 之前只写不读，所以点大纲标题完全不会滚动。
            if state.scroll_req.take().is_some() {
                if let Some(r) = active_rect {
                    // 换行/新建块只做「最小滚动」：上面的文字保持不动，只有超出视口才滚。
                    let align = if state.scroll_top {
                        Some(egui::Align::TOP)
                    } else {
                        None
                    };
                    ui.scroll_to_rect(r, align);
                }
                state.scroll_top = false;
            }

            // 跨块拖选聚合：渲染态块上报的 CrossDrag 在这里聚成全局选区，
            // 并从 actions 中移除（app 层无需关心）。拖选开始时放弃编辑态
            // 焦点，否则活动块的 TextEdit 会抢先消费键盘输入。
            // 双击选词 / 三击选整段 / Shift+点击扩选 也在此聚成 cross_sel。
            let mut cross_drag_reported = false;
            actions.retain(|a| match a {
                Action::CrossDrag(i, c, start) => {
                    cross_drag_reported = true;
                    if *start {
                        state.cross_sel = Some((*i, *c, *i, *c));
                        if let Some(fid) = ctx.memory(|m| m.focused()) {
                            ctx.memory_mut(|m| m.surrender_focus(fid));
                        }
                    } else if let Some((a, ac, _, _)) = state.cross_sel {
                        state.cross_sel = Some((a, ac, *i, *c));
                    }
                    false
                }
                // 双击选词：把跨块选区直接设成词区间
                Action::WordSel(i, a, z) => {
                    state.cross_sel = Some((*i, *a, *i, *z));
                    false
                }
                // 三击选中整块（整行/整段）
                Action::BlockSel(i) => {
                    let len = doc
                        .blocks
                        .get(*i)
                        .map(|b| b.plain().chars().count())
                        .unwrap_or(0);
                    state.cross_sel = Some((*i, 0, *i, len));
                    false
                }
                // Shift+点击：有选区时保持锚点、把终点移到点击处；
                // 无选区时从光标（活动块 + 光标字符）选到点击处
                Action::ShiftClick(i, c) => {
                    if let Some((a, ac, _, _)) = state.cross_sel {
                        state.cross_sel = Some((a, ac, *i, *c));
                    } else {
                        let buf_len = doc
                            .blocks
                            .get(active)
                            .map(|b| b.plain().chars().count())
                            .unwrap_or(0);
                        // 优先用「按下鼠标时的位置」当锚点（从编辑态段落拖出的场景），
                        // 没有记录时退回当前光标。
                        let anchor = state
                            .press_caret
                            .filter(|(b, _)| *b == active)
                            .map(|(_, c)| c.min(buf_len))
                            .unwrap_or_else(|| state.last_cursor.min(buf_len));
                        state.cross_sel = Some((active, anchor, *i, *c));
                        // 从编辑态的 TextEdit 里拖出来时，放弃它的焦点，
                        // 否则 TextEdit 会继续吃键盘/拖拽、和跨块选区打架。
                        if let Some(fid) = ctx.memory(|m| m.focused()) {
                            ctx.memory_mut(|m| m.surrender_focus(fid));
                        }
                    }
                    false
                }
                _ => true,
            });
            // 拖选端点兜底：指针落在块与块之间的空档、正文上下方的空白，
            // 或者列表圆点/复选框这类块内留白时，没有任何块上报 CrossDrag，
            // 选区就会停在最后一个命中块（用户反馈的「拖到底选不全」）。
            // 这里按指针的竖直位置补一个端点：向下拖取块尾、向上拖取块首，
            // 拖到正文下方即选中到文档末尾。
            let pointer_down = ctx.input(|i| i.pointer.primary_down());
            let dragging = ctx.input(|i| i.pointer.is_decidedly_dragging());
            // 按下点必须在编辑区里：按在工具条 / 菜单上时不能延伸正文选区。
            let press_in_editor = ctx
                .input(|i| i.pointer.press_origin())
                .is_some_and(|p| ui.clip_rect().contains(p));
            // 与块内拖选一致：指针甩出视口时钳到边缘，越界也能持续延伸选区。
            let clip = ui.clip_rect();
            let drag_pointer = ctx.input(|i| i.pointer.interact_pos()).map(|p| {
                if clip.contains(p) {
                    p
                } else {
                    egui::Pos2::new(p.x, p.y.clamp(clip.top() + 1.0, clip.bottom() - 1.0))
                }
            });
            let over_active_edit = block_rects
                .iter()
                .any(|(i, r)| *i == active && drag_pointer.is_some_and(|p| r.contains(p)));
            if pointer_down
                && dragging
                && press_in_editor
                && !cross_drag_reported
                && (state.cross_sel.is_some() || state.press_caret.is_some())
                && !(state.cross_sel.is_none() && over_active_edit)
                && let Some(p) = drag_pointer
            {
                let buf_len = doc
                    .blocks
                    .get(active)
                    .map(|b| b.plain().chars().count())
                    .unwrap_or(0);
                let anchor = state
                    .cross_sel
                    .map(|(a, ac, _, _)| (a, ac))
                    .unwrap_or_else(|| {
                        // 从编辑态的 TextEdit 里拖出来：用按下点当锚点，没记录时退回光标。
                        let c = state
                            .press_caret
                            .filter(|(b, _)| *b == active)
                            .map(|(_, c)| c.min(buf_len))
                            .unwrap_or_else(|| state.last_cursor.min(buf_len));
                        (active, c)
                    });
                let plain_len = |i: usize| {
                    doc.blocks
                        .get(i)
                        .map(|b| b.plain().chars().count())
                        .unwrap_or(0)
                };
                // 指针竖直方向压在某个块上（哪怕 x 在标记/留白区）→ 取该块端点。
                let mut endpoint = block_rects
                    .iter()
                    .find(|(_, r)| p.y >= r.top() && p.y <= r.bottom())
                    .map(|(i, _)| (*i, if *i >= anchor.0 { plain_len(*i) } else { 0 }));
                // 在块与块的空档里：取上方最近的块；在正文上方则取第一块块首。
                if endpoint.is_none() {
                    endpoint = block_rects
                        .iter()
                        .rev()
                        .find(|(_, r)| r.bottom() <= p.y)
                        .map(|(i, _)| (*i, if *i >= anchor.0 { plain_len(*i) } else { 0 }))
                        .or_else(|| block_rects.first().map(|(i, _)| (*i, 0)));
                }
                if let Some((ei, ec)) = endpoint {
                    if state.cross_sel.is_none() {
                        // 和 ShiftClick 分支一致：开始跨块选区时放弃 TextEdit 焦点。
                        if let Some(fid) = ctx.memory(|m| m.focused()) {
                            ctx.memory_mut(|m| m.surrender_focus(fid));
                        }
                    }
                    state.cross_sel = Some((anchor.0, anchor.1, ei, ec));
                }
            }
            // 拖选越界自动滚动：按住左键拖到编辑区可视范围之外时，
            // 按超出距离持续滚动，选区随之延伸（Typora / VS Code 行为）。
            // 指针停住不动时系统不再送鼠标事件，必须主动请求重绘，
            // 否则只会滚一帧就停住（用户反馈的「拖到底不会继续滚」）。
            // 用 scroll_to_rect 而不是 scroll_with_delta：后者的位移在
            // ScrollArea 里不会生效（实测 offset 始终为 0）。
            // press_caret 也要算进来：在活动段落（TextEdit）里一直往下拖、
            // 段落本身比视口高时还没有跨块选区，但同样应该自动滚动。
            if (state.cross_sel.is_some() || state.press_caret.is_some())
                && ctx.input(|i| i.pointer.primary_down())
                && let Some(p) = ctx.input(|i| i.pointer.interact_pos())
            {
                let view = ui.clip_rect();
                let (overflow, below) = if p.y > view.bottom() {
                    (p.y - view.bottom(), true)
                } else if p.y < view.top() {
                    (view.top() - p.y, false)
                } else {
                    (0.0, false)
                };
                if overflow > 0.0 {
                    // 越界越远滚得越快。
                    let step = (overflow * 0.6).clamp(3.0, 40.0);
                    let y = if below {
                        view.bottom() + step
                    } else {
                        view.top() - step
                    };
                    // 在视口边缘外放一个 1px 的虚拟目标，让 ScrollArea 把内容滚过去。
                    let target =
                        egui::Rect::from_min_size(egui::pos2(view.left(), y), egui::vec2(1.0, 1.0));
                    ui.scroll_to_rect_animation(target, None, egui::style::ScrollAnimation::none());
                    ctx.request_repaint();
                }
            }
        });

    // 空白点击判定：不依赖哪个控件抢到这次点击，直接看指针落点。
    // 条件：在编辑区范围内、不在任何块（含块里的链接/按钮）上、且事件归属
    // 编辑区这一层（弹窗/对话框盖在上面时不算）。滚动条那一条也要排除。
    let pointer = ctx.input(|i| i.pointer.interact_pos());
    let primary_clicked = ctx.input(|i| i.pointer.primary_clicked());
    let bar_zone = scroll_bar_zone(ui, scroll_out.inner_rect, scroll_out.content_size.y);
    let blank_pos = pointer.filter(|p| {
        ui.max_rect().contains(*p)
            && !bar_zone.contains(*p)
            && !block_rects.iter().any(|(_, r)| r.contains(*p))
            && ctx
                .layer_id_at(*p)
                .is_none_or(|layer| layer == ui.layer_id())
    });
    // bg 是兜底：极端情况下拿不到指针坐标，但底层热区仍然命中了。
    // bg 是整块编辑区的兜底热区：点在代码块功能栏/分隔线/列表圆点等块内留白时，
    // 没有具体控件消费点击，bg 就会命中。这类点击必须排除，否则会被当成
    // 「点空白」把光标甩到文档末尾（用户反馈的「代码块里没法写代码」）。
    let bg_blank =
        bg.clicked() && pointer.is_none_or(|p| !block_rects.iter().any(|(_, r)| r.contains(p)));
    let blank_clicked = (primary_clicked && blank_pos.is_some()) || bg_blank;

    if !state.read_only {
        state.last_cursor = cursor_idx;
        state.sel = selection;
        if let Some((a, z)) = selection {
            state.last_sel = Some((active, a, z));
        }
    }
    if std::env::var("MARKD_TRACE_FOCUS").is_ok() {
        eprintln!(
            "[focus] req={:?} focused={:?} editing_block={} active={}",
            state.focus_req,
            ctx.memory(|m| m.focused()),
            !state.read_only,
            active
        );
    }
    state.focus_req = None;

    // 点在编辑区空白处：重新聚焦并把光标放回去。
    // - 新建文档只有一个块：无论点多少次，光标都回到第一行闪烁（caret 0）。
    // - 多块文档：聚焦离落点最近的块；点在正文下方时放到最后一块末尾，方便继续写。
    // 这一步要放在 `focus_req = None` 之后，否则请求会被同一帧清掉，
    // 下一帧 TextEdit 拿不到焦点，光标依旧不闪。
    if blank_clicked {
        // 空白点击统一落到「可以继续输入的最后一行」：最后一块的末尾。
        // 新建空文档只有一块，末尾就是第一行行首——所以无论点多少次、点哪里，
        // 光标都在第一行闪烁。
        let target = doc.blocks.len().saturating_sub(1);
        let caret = doc
            .blocks
            .get(target)
            .map(|b| block_buffer(b).chars().count())
            .unwrap_or(0);
        state.active = target;
        state.focus_req = Some(target);
        state.caret_req = Some((target, caret));
        state.last_cursor = caret;
        state.sel = None;
        state.last_sel = None;
        state.pending_caret = None;
        // 立即把焦点设过去，不等下一帧的 TextEdit 重新请求：
        // 中间任何一帧都不会出现「没有光标」的空窗。
        ctx.memory_mut(|m| m.request_focus(Id::new(("block_edit", tab_key, target))));
        state.clicked = true;
        // 空白点击：清除跨块选区（拖选才产生它）
        state.cross_sel = None;
        ctx.request_repaint();
        trace_blank(
            &blank_pos,
            &format!("block {target} caret {caret} (blocks={})", doc.blocks.len()),
        );
    } else {
        trace_blank(
            &blank_pos,
            &format!(
                "miss: primary_clicked={primary_clicked} blocks={} bg_clicked={}",
                block_rects.len(),
                bg.clicked()
            ),
        );
    }

    // ---------------- 结构性按键（回车 / 退格 / Tab）----------------
    // 终端持有键盘焦点时（read_only = true），按键全部归终端：
    // 编辑区不再处理回车/退格/Tab，否则在终端里按回车，编辑区的段落也跟着换行。
    if state.read_only {
        return EditorEvent::None;
    }

    // ---------------- 跨块选区：输入替换 / 退格删除 ----------------
    // 拖选出的跨块选区存在时，键盘输入优先作用于选区（Typora 行为）：
    // 打字 → 删除选中内容并插入新文字；退格 → 删除选中内容；
    // 选区由拖选建立时编辑框已失焦，不会与 TextEdit 的输入处理冲突。
    if state.cross_sel.is_some() {
        let typed: Option<String> = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Text(t) => Some(t.clone()),
                _ => None,
            })
        });
        if backspace || typed.is_some() {
            apply_cross_sel_edit(doc, state, typed.as_deref());
            ctx.request_repaint();
            return EditorEvent::Changed;
        }
    }

    // ---------------- 代码组：增删标签 / 切换标签 / 改当前标签语言 ----------------
    // 三种操作前都要先把当前标签的最新代码写回，否则刚输入的内容会丢。
    if group_add_tab || group_remove_tab {
        if let EditKind::CodeGroup { items, tab } = &edit_kind {
            let mut items = items.clone();
            if let Some(it) = items.get_mut(*tab) {
                it.code = active_buf.clone();
            }
            let next_tab = if group_add_tab {
                items.push(crate::ast::CodeGroupItem {
                    lang: String::new(),
                    code: String::new(),
                });
                items.len() - 1
            } else if items.len() > 1 {
                items.remove(*tab);
                (*tab).min(items.len() - 1)
            } else {
                // 只剩一个标签：按钮已禁用，这里兜底不改文档。
                return EditorEvent::None;
            };
            doc.record();
            doc.edit_block(active, &crate::ast::code_group_source(&items));
            state.code_group_tab.insert(active, next_tab);
            state.focus_req = Some(active);
            state.caret_req = Some((active, 0));
            state.last_cursor = 0;
            state.sel = None;
            state.last_sel = None;
            ctx.request_repaint();
            return EditorEvent::Changed;
        }
    }
    // 切标签前必须先写回当前标签的代码，否则刚输入的内容会丢。
    if let Some(next) = group_tab_switch {
        if changed {
            doc.maybe_record();
            doc.edit_block(active, &edit_kind.rebuild(&active_buf));
        }
        state.code_group_tab.insert(active, next);
        state.focus_req = Some(active);
        state.caret_req = Some((active, 0));
        state.last_cursor = 0;
        state.sel = None;
        state.last_sel = None;
        ctx.request_repaint();
        return if changed {
            EditorEvent::Changed
        } else {
            EditorEvent::None
        };
    }
    if let (EditKind::CodeGroup { items, tab }, Some(lang)) = (&edit_kind, group_lang_change) {
        let mut items = items.clone();
        if let Some(it) = items.get_mut(*tab) {
            // 缓冲里是本帧最新的代码，一起写回，避免换语言时丢字。
            it.code = active_buf.clone();
            it.lang = lang;
        }
        doc.record();
        doc.edit_block(active, &crate::ast::code_group_source(&items));
        state.focus_req = Some(active);
        state.caret_req = Some((active, state.last_cursor.min(active_buf.chars().count())));
        state.sel = None;
        ctx.request_repaint();
        return EditorEvent::Changed;
    }

    // ---------------- 代码块换语言 ----------------
    // 先把本帧可能发生的文本修改按旧语言写回，再整体重建源码换语言，
    // 避免旧的 edit_kind 把新语言覆盖回去。
    if let Some(new_lang) = lang_change.take() {
        if changed {
            doc.maybe_record();
            let src = edit_kind.rebuild(&active_buf);
            doc.edit_block(active, &src);
        }
        let mut code = active_buf.trim_end_matches('\n').to_string();
        code.push('\n');
        doc.record();
        doc.edit_block(active, &format!("```{new_lang}\n{code}```"));
        return EditorEvent::Changed;
    }

    // ---------------- 行首退格：并回上一块 ----------------
    // 放在写回之前：以前这段在 `if changed { return }` 之后，删空那块文本的那一帧
    // 会直接提前返回，导致「行首退格」永远执行不到。
    if backspace && active > 0 && state.sel.is_none() && (buf_was_empty || state.last_cursor == 0) {
        merge_into_previous_block(doc, state, &active_buf);
        return EditorEvent::Changed;
    }

    // ---------------- 代码块末尾空行回车：退出代码块 ----------------
    // 光标在末尾空行上再按一次回车，退出代码块编辑（Typora 行为）：
    // 去掉末尾空行写回，在下方插入普通段落块并聚焦。判定条件：
    // 1) 本帧按了回车且 TextEdit 真的处理了它（changed 为真要求持有焦点）；
    // 2) 按键前缓冲已以换行结尾（末行是空行）——第一次回车只是新建行，不会命中；
    // 3) 本帧的改动恰好是在末尾追加一个换行且光标仍在缓冲末尾，
    //    排除「同帧打字 + 回车」「行中间回车」的误判。
    if enter
        && changed
        && matches!(edit_kind, EditKind::Code { .. } | EditKind::Diagram(_))
        && buf_ended_nl
        && state.sel.is_none()
        && cursor_idx == active_buf.chars().count()
        && active_buf == format!("{pre_buf}\n")
    {
        let code = active_buf.trim_end_matches('\n').to_string();
        doc.record();
        if code.is_empty() {
            // 空代码块退出：整块直接变成普通段落，不再插入新块
            doc.edit_block(active, "");
        } else {
            doc.edit_block(active, &edit_kind.rebuild(&code));
            doc.insert_block_after(active, "");
            state.active = active + 1;
        }
        state.focus_req = Some(state.active);
        state.scroll_req = Some(state.active);
        state.scroll_top = false;
        state.last_cursor = 0;
        state.sel = None;
        return EditorEvent::Changed;
    }

    // ---------------- 自动配对（括号与引号） ----------------
    if changed && state.auto_pair {
        if let Some(close) = &typed_pair {
            let pos = cursor_idx.min(active_buf.chars().count());
            pair_insert = Some((pos, close.clone()));
        }
    }
    if let Some((pos, close)) = pair_insert {
        active_buf = insert_at_char(&active_buf, pos, &close);
        changed = true;
    }

    // ---------------- 普通输入写回 ----------------
    if changed {
        doc.maybe_record();
        let src = table_edit_src.clone().unwrap_or_else(|| {
            // 删空了就落成空段落：不要把标题的 #、代码块的围栏这些残留标记
            // 当正文写回去，否则下一次退格只会在标记里删字符，永远合并不回上一行。
            if active_buf.trim().is_empty() {
                String::new()
            } else {
                edit_kind.rebuild(&active_buf)
            }
        });
        doc.edit_block(active, &src);
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
        let buf = active_buf.clone();
        let pos = state.last_cursor.min(buf.chars().count());
        let (left, right) = split_at_char(&buf, pos);
        // 列表项（含任务项）/ 引用块：编辑缓冲已去掉行首标记，不能用
        // inherited_marker(buf)，要从 EditKind 拿到当前标记来继承。
        // Typora 行为：非空项回车继承同类标记；空项再回车退出该块级格式。
        let (marker, exit_prefix) = match &edit_kind {
            EditKind::List {
                indent,
                marker,
                checked,
                ..
            } => {
                // 已勾选的任务项回车，新项从「未勾选」开始（Typora 行为）；
                // 有序列表回车时序号自动 +1。
                let inherited = if checked == &Some(true) {
                    replace_task_checkbox(marker)
                } else if checked.is_none() {
                    next_ordered_marker(marker)
                } else {
                    marker.clone()
                };
                let prefix = format!("{}{} ", " ".repeat(*indent), inherited);
                if buf.trim().is_empty() {
                    // 空列表项回车：当前块去掉标记变普通段落，不再插入新块
                    (String::new(), true)
                } else {
                    (prefix, false)
                }
            }
            EditKind::Quote { level } => {
                if buf.trim().is_empty() {
                    // 空引用回车：当前块去掉 `>` 变普通段落，不再插入新块
                    (String::new(), true)
                } else {
                    // 新行继承相同引用层级（连续 `>`）。
                    (format!("{} ", ">".repeat((*level).max(1))), false)
                }
            }
            EditKind::Source => (inherited_marker(&buf), false),
            _ => (String::new(), false),
        };
        doc.record();
        if exit_prefix {
            // 退出列表 / 引用：当前空块直接写回正文（无标记），光标留在原块
            doc.edit_block(active, buf.trim_end());
            state.focus_req = Some(active);
            state.last_cursor = 0;
            return EditorEvent::Changed;
        }
        doc.edit_block(active, &edit_kind.rebuild(&left));
        doc.insert_block_after(active, &format!("{}{}", marker, right));
        state.active = active + 1;
        state.focus_req = Some(state.active);
        state.scroll_req = Some(state.active);
        state.scroll_top = false;
        state.last_cursor = marker.chars().count();
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
        if let Some(shift_tab) = list_tab {
            // 列表项：Tab 增加一级缩进（成为子列表），Shift+Tab 退回上一级。
            if let EditKind::List {
                indent,
                marker,
                checked,
            } = &edit_kind
            {
                let step = state.tab_size.max(1);
                let new_indent = if shift_tab {
                    indent.saturating_sub(step)
                } else {
                    indent + step
                };
                if new_indent != *indent {
                    doc.record();
                    let ek = EditKind::List {
                        indent: new_indent,
                        marker: marker.clone(),
                        checked: *checked,
                    };
                    doc.edit_block(active, &ek.rebuild(&active_buf));
                    // 保持焦点与光标：缩进只改源码前缀，正文缓冲不变。
                    state.focus_req = Some(active);
                    return EditorEvent::Changed;
                }
            }
        } else if !matches!(edit_kind, EditKind::List { .. }) {
            let cur = active_buf.clone();
            let indent = " ".repeat(state.tab_size.max(1));
            let updated = if shift {
                cur.strip_prefix(indent.as_str())
                    .unwrap_or(&cur)
                    .to_string()
            } else {
                format!("{indent}{cur}")
            };
            if updated != cur {
                doc.record();
                doc.edit_block(active, &updated);
                return EditorEvent::Changed;
            }
        }
    }

    EditorEvent::None
}
