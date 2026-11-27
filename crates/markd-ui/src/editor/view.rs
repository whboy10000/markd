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

/// 编辑区竖向滚动条宽度：0.5 cm（仓库口径 1 cm ≈ 37.8 pt，与 `app` 里标题栏尺寸的注释同一换算）。
///
/// egui 默认是 10pt 的浮动条（`ScrollStyle::floating()`）：不占正文宽度，静止时
/// 底色与滑块的透明度都是 0，要悬停才显形，用户看不出文档有多长、自己滚到哪儿了。
/// 这里换成常显的实条，并在其上加分隔线与光标位置标记。
pub(super) const SCROLLBAR_WIDTH: f32 = 0.5 * 37.8;

/// 正文与滚动条之间的分隔线宽度。
pub(super) const SCROLLBAR_DIVIDER: f32 = 1.0;

/// 光标位置标记的线宽（横穿轨道的一条短横线）。
pub(super) const SCROLLBAR_MARKER: f32 = 2.0;

/// 标记外圈光晕的线宽：比核心粗一档，让短横线在深色轨道与浅色滑块上都看得见。
pub(super) const SCROLLBAR_MARKER_HALO: f32 = SCROLLBAR_MARKER + 2.0;

/// 把编辑区这一层的滚动条样式设成实条：不浮动、不占额外边距，宽度 [`SCROLLBAR_WIDTH`]，
/// 轨道底色跟着主题走。
///
/// 只改这一层 `ui` 的 style，因此设置弹窗、侧栏、终端的滚动条不受影响。
pub(super) fn apply_scrollbar_style(ui: &mut egui::Ui, theme: &Theme) {
    let s = &mut ui.spacing_mut().scroll;
    s.floating = false;
    s.bar_width = SCROLLBAR_WIDTH;
    s.bar_inner_margin = 0.0;
    s.bar_outer_margin = 0.0;

    // 本 app 从不设 `Visuals`（只设 `clear_color`），所以 egui 的滚动条永远是深色那套：
    // 轨道走 `extreme_bg_color`（实测深色为 `#0A0A0A`），浅色主题下就是一根黑条，很突兀。
    // 轨道改成面板色：编辑区子树里读 `extreme_bg_color` 的只有滚动条——段落的 TextEdit 用
    // `Frame::NONE` 或自带 `fill`，没有进度条，也没有 `Frame::canvas`。
    //
    // 滑块**不能**这样改：它的颜色在 `widgets.{inactive,hovered,active}.bg_fill`，而任务列表
    // 的复选框内芯读同一个字段（A/B 实测：改之前 `#3C3C3C`，改之后跟着变成主题色）。
    // 所以滑块留给 `paint_scrollbar_marks` 自己盖。
    ui.visuals_mut().extreme_bg_color = theme.panel;
}

/// 在滚动条那条窄带上画三样东西：滑块的主题色、与正文之间的分隔线、光标在整篇文档里的位置。
///
/// `caret_y` 与 `offset_y` 都是**内容坐标系**里的 y（0 = 文档顶部），由调用方算好交进来：
/// 坐标换算与「光标在哪、滚到哪儿」的判定分开，这里才只是在窄带上描几笔。
pub(super) fn paint_scrollbar_marks(
    ui: &egui::Ui,
    theme: &Theme,
    inner: egui::Rect,
    total_h: f32,
    offset_y: f32,
    caret_y: f32,
) {
    let left = inner.right();
    let right = left + ui.spacing().scroll.bar_width;
    ui.painter().line_segment(
        [
            egui::pos2(left, inner.top()),
            egui::pos2(left, inner.bottom()),
        ],
        egui::Stroke::new(SCROLLBAR_DIVIDER, theme.dim),
    );

    // 滑块：egui 已经按它自己的颜色画了一遍（那个颜色字段不能动，见
    // `apply_scrollbar_style`），这里按同一几何再盖一层主题色。几何照 egui
    // `calculate_handle_rect` 的式子来，`scrollbar::painted_thumb_matches_eguis_geometry`
    // 逐帧比对两者，差一点就红——差一点就会在边上露出一圈 egui 的浅灰。
    let strip = egui::Rect::from_min_max(
        egui::pos2(left, inner.top()),
        egui::pos2(right, inner.bottom()),
    );
    let from_content = |c: f32| {
        egui::remap_clamp(c, 0.0..=total_h, strip.top()..=strip.bottom())
    };
    let max_off = (total_h - inner.height()).max(0.0);
    let len = (from_content(offset_y + inner.height()) - from_content(offset_y))
        .max(ui.spacing().scroll.handle_min_length);
    let start = egui::remap_clamp(
        offset_y,
        0.0..=max_off,
        strip.top()..=(strip.bottom() - len),
    );
    let thumb = egui::Rect::from_min_max(
        egui::pos2(strip.left(), start),
        egui::pos2(strip.right(), start + len),
    );
    // 圆角取 egui 用的同一个值（实测深色主题三态都是 6，与状态无关），否则角上会露底。
    let radius = ui.visuals().widgets.inactive.corner_radius;
    let pressed = ui.input(|i| i.pointer.any_down());
    let hover = ui.rect_contains_pointer(thumb) || (pressed && ui.rect_contains_pointer(strip));
    ui.painter()
        .rect_filled(thumb, radius, if hover { theme.text } else { theme.dim });

    // 标记的 y 与 egui 算滑块用的映射一致：内容 0..总高 线性映到轨道 y 区间。
    // 分母取下限「一屏高」：文档不满一屏时内容坐标就等于屏幕坐标，
    // 标记于是正好落在光标那一行上，而不是在短内容里被除到中间去。
    let denom = total_h.max(inner.height());
    let frac = (caret_y / denom).clamp(0.0, 1.0);
    let y = inner.top() + frac * inner.height();
    // 描两遍：先粗一圈纸色光晕，再画正文色的核心。滑块压在轨道上，两者明度不同，
    // 而光标几乎总落在滑块上——单色总有一侧对比不够，光晕与核心互为深浅就两侧都看得见。
    let seg = [egui::pos2(left + 3.0, y), egui::pos2(right - 3.0, y)];
    ui.painter()
        .line_segment(seg, egui::Stroke::new(SCROLLBAR_MARKER_HALO, theme.bg));
    ui.painter()
        .line_segment(seg, egui::Stroke::new(SCROLLBAR_MARKER, theme.text));
}

/// 滚动条所在的那条窄带：点这里应该交给滚动条，不能当成空白点击。
///
/// 实条从第一帧起就占着这条带子（见 `show_editor` 里的 `AlwaysVisible`），
/// 所以不必等内容真的超出一屏才排除。
pub(super) fn scroll_bar_zone(ui: &egui::Ui, inner_rect: egui::Rect) -> egui::Rect {
    let s = &ui.spacing().scroll;
    egui::Rect::from_min_max(
        egui::pos2(inner_rect.right() - 2.0, inner_rect.top()),
        egui::pos2(
            inner_rect.right() + s.bar_width + s.bar_inner_margin + s.bar_outer_margin,
            inner_rect.bottom(),
        ),
    )
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

    // 拖选进行中冻结「标记露出」：光标一进 `**`/反引号内部，成对标记从 0.1px
    // 变成正常字号，本行文字当场变宽、后面的字整体位移，指针下的字符映射跟着
    // 漂 1–2 个字符，表现为「段落里有行内代码时只选中半截」。静态编辑（点击、
    // 打字）仍按光标露出标记，行为不变。
    let drag_in_progress =
        ctx.input(|i| i.pointer.primary_down() && i.pointer.is_decidedly_dragging());

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
    // 跨块选区与表格单元格选区互斥：一旦有跨块选区（含从表格拖出去得到的整块选区），
    // 单元格选区就让位，避免两套高亮同时亮着、复制口径不一致。
    if state.cross_sel.is_some() {
        state.table_sel = None;
    }
    // 新按下（非 Shift、且按在编辑区内）时清掉上一次残留的跨块选区。否则若这次
    // 按下的命中判定失败（点在标记区/块间空档/布局切换中），锚点没被重新锁定，
    // 兜底逻辑会沿用旧 cross_sel 的起点，把新选区锚到上一次拖拽/点击的块——
    // 表现为「间歇性选不中、选区跑到别处」。Shift+点击要扩选、工具条/视口外的
    // 点击不能动正文选区，故排除。
    let press_in_editor = ctx
        .input(|i| i.pointer.press_origin())
        .is_some_and(|p| ui.clip_rect().contains(p));
    if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
        && !shift
        && press_in_editor
    {
        state.cross_sel = None;
        // 表格单元格选区同理：在格子里重新按下就清掉上一轮，拖动会重新建立。
        state.table_sel = None;
    }
    // 渲染层才有表格每格的矩形，本帧的单元格选区挂到 ctx，由它给选中的格子叠底色
    // （块循环之前写入，同帧生效）。
    ctx.memory_mut(|m| match state.table_sel {
        Some(sel) => {
            m.data.insert_temp(render::table_sel_id(), sel);
        }
        None => {
            m.data.remove_temp::<TableSel>(render::table_sel_id());
        }
    });
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

    // 滚动条样式只改编辑区这一层：设置弹窗、侧栏、终端仍用 egui 的浮动细条。
    apply_scrollbar_style(ui, theme);

    let scroll_out = egui::ScrollArea::vertical()
        .id_salt("editor_scroll")
        .auto_shrink([false, false])
        // 实条宽度从第一帧就要占满：默认 VisibleWhenNeeded 下 egui 会把滑块出现
        // 做成动画（show_bars_factor 0→1），正文可用宽度逐帧变窄，而排版表按宽度
        // 指纹整篇作废重测——滚动请求恰好落在那几帧就再也不会到位。
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        // `show_viewport` 而不是 `show`：回调里能拿到「内容坐标系中的视口矩形」，
        // 于是可以只排版看得见的那几十块。帧成本原先与块数成正比
        // （实测每块约 23µs：1000 万字 24 万块 → 静置一帧 5.7s、首帧 13.9s），
        // 裁剪后只与视口内块数有关。
        .show_viewport(ui, |ui, view| {
            // ---------------- 排版表：本帧到底要排哪几块 ----------------
            let n = doc.blocks.len();
            // 模型的块级变更先落到表上：只作废受影响的区间，其余块高继续复用。
            state.layout.apply(&doc.take_changes());
            state.layout.sync(n);
            // 正文宽度随窗口浮动，固定占可用宽度的约 90%；不设死宽度，
            // 所有块都按这个宽度换行，窄窗口下也不会把长段落横向裁掉。
            // 行号栏固定锚定在编辑区最左侧（70px，与源码模式共用），不参与版心
            // 居中，因此全窗口 / 部分窗口、源码 / 混合 / 专注模式下栏宽都恒定。
            let outer_w = ui.available_width();
            let page_w = (outer_w * 0.9).max(200.0);
            let gutter_w = gutter_width(show_line_numbers);
            let viewport_left = ui.clip_rect().left();
            // 版心宽度、正文字号、专注模式、行号栏都会改变块高：任一变了就整篇重测。
            state.layout.rebase([
                page_w as i32,
                theme.body as i32,
                focus_mode as i32,
                show_line_numbers as i32,
            ]);
            if show_line_numbers && state.layout.lines_missing() > 0 {
                // 行数只在「表里还缺」时扫源码：打开文档扫一次，之后只补新增/改动块。
                state.layout.fill_lines(&doc.blocks);
            }
            // 内容原点（绝对坐标）：未滚动时等于视口上缘，向下滚动后会往上移出视口。
            // `view.min.y` 是内容坐标系里的滚动量，两者相加才是屏幕上的实际位置。
            let content_top = ui.max_rect().top();
            // 内容总高必须按整篇给，否则滚动条长度与可滚范围会缩水成一屏。
            let total_h = state.layout.total_height();
            ui.set_height(total_h);
            let range = state.layout.visible_range(view.min.y, view.height());
            let mut order: Vec<usize> = if n == 0 {
                Vec::new()
            } else {
                (range.start..range.end).collect()
            };
            // 活动块即使被滚出视口也要排版：它的 TextEdit 是本帧唯一的键盘入口，
            // 少排一帧就等于吞掉一个按键。
            if n > 0 && active < n && (active < range.start || active >= range.end) {
                order.push(active);
                order.sort_unstable();
            }
            // 顶边 / 估算高度 / 起始行号都在排版借用语义之外先取好，
            // 循环体内就不再碰 `state.layout`（体内要改 state 的其它字段）。
            // 行号（Document::source 以 \n 连接各块）：画在编辑区左侧固定行号栏里。
            let mut anchors: Vec<f32> = Vec::with_capacity(order.len());
            let mut ests: Vec<f32> = Vec::with_capacity(order.len());
            let mut line_nos: Vec<u32> = Vec::with_capacity(order.len());
            for &idx in order.iter() {
                anchors.push(content_top + state.layout.top(idx));
                ests.push(state.layout.height_of(idx));
                line_nos.push(state.layout.lines_before(idx) + 1);
            }
            // 快开文档的块共享原文、行内树为空：只给本帧真正可见的块补行内树，
            // 代价与视口大小有关，与全文块数无关。
            for &idx in order.iter() {
                doc.ensure_inline(idx);
            }
            let blocks = &doc.blocks;
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
                    // 列左缘与列宽由外层 horizontal/vertical 定好；块与块之间不再靠
                    // 流式推进，而是各自按绝对矩形排版，这样才允许中间「跳」一块。
                    let col_x = ui.cursor().min.x;
                    let col_w = ui.available_width();
                    let mut y = 0.0f32;
                    let mut measured: Vec<(usize, f32)> = Vec::with_capacity(order.len());
                    for (k, &idx) in order.iter().enumerate() {
                        // 连续的块接着上一块排版后的位置往下走，与整篇流式排版逐像素一致；
                        // 只有「视口外的活动块」那一处会跳，跳回高度表里它自己的顶边。
                        if k == 0 || idx != order[k - 1] + 1 {
                            y = anchors[k];
                        }
                        let band = egui::Rect::from_min_max(
                            egui::pos2(col_x, y),
                            egui::pos2(col_x + col_w, y + ests[k].max(1.0)),
                        );
                        // 子 Ui 的裁剪矩形继承父级（视口），所以估算矩形比内容矮
                        // 也不会把这一块裁掉，只是下一帧会用实测值修正。
                        let adv = ui
                            .scope_builder(egui::UiBuilder::new().max_rect(band), |ui| {
                            let block = &blocks[idx];
                        let compact = matches!(block, Block::ListItem { .. } | Block::Blank { .. });
                        // 连续引用行之间只留最小间距，左侧竖线才能连成一条。
                        // 引用里的代码块同样算作引用行，竖线保持连续。
                        let quote_run = quote_level_of(block).is_some()
                            && blocks
                                .get(idx + 1)
                                .is_some_and(|b| quote_level_of(b).is_some());
                        let is_active = idx == active;
                        // 活动块始终保持编辑态（TextEdit），哪怕正在跨块拖选：
                        // 一旦切回渲染态，块高/间距会变化，下方内容整体位移，
                        // 指针会突然落到块间空档里，选区就此断掉（用户反馈的
                        // 「从标题拖到段落，有时候只选中一部分」）。
                        // 活动块自己的选区高亮由下面统一补画（selection_shapes）。
                        let editing = is_active && !state.read_only;

                        // 代码块/图表/表格/图片等不经过 text_response，没有自己的文字
                        // 选区高亮。它们多半画在不透明面板里（panel_frame 自带底色），
                        // 垫在下面的蓝底会被面板底色盖住，所以改为在块内容之上叠加
                        // 一层半透明选区色——文字依然清晰，又能看出这块被选中了。
                        // 活动块也照常叠加，拖选扫过它时同样有反馈。
                        // 代码块（展开态）除外：它有逐行的精确高亮（render::code_panel），
                        // 整块叠色会变成「选一行亮一块」。折叠态没有文字可画，仍用整块底色。
                        let sel_overlay = (!text_selection_block(block)
                            && !(line_selectable_block(block) && !state.folded.contains(&idx))
                            && atomic_block_selected(state.cross_sel, idx))
                        .then(|| render::selection_overlay(theme));

                        // 活动块（TextEdit）的跨块选区底色：先占绘制槽，排版完成后
                        // 回填，保证底色垫在文字下层（选区色是不透明的）。
                        let active_sel_slot =
                            editing.then(|| ui.painter().add(egui::Shape::Noop));

                        // 本块这一帧是否已经按「精确位置」上报过拖选（代码块代码区、
                        // 表格单元格）。报了就不再走下面的「整块」端点兜底，
                        // 否则精确端点会被块尾覆盖，表现成「一拖就选中整块」。
                        let action_start = actions.len();
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
                                                    let src = table_source(grid, aligns);
                                                    let html = crate::export::source_html(&src);
                                                    crate::clipboard::copy_rich(ctx, &src, &html);
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
                                    // 与渲染态一致：列数取所有行里最宽的一行，
                                    // 避免表头列数偏少时整张表被按 1 列排版。
                                    let cols = grid
                                        .iter()
                                        .map(|r| r.len())
                                        .max()
                                        .unwrap_or(1)
                                        .max(1);
                                    let gap =
                                        ui.spacing().item_spacing.x * cols.saturating_sub(1) as f32;
                                    let col_w = ((ui.available_width().min(page_w) - gap)
                                        .max(60.0)
                                        / cols as f32)
                                        .max(28.0);
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
                                    // 单元格里拖选：只选中经过的那几格（与渲染态同一套规则），
                                    // 拖出表格边界才把整张表交给跨块选区，好连着上下文一起选走。
                                    // 留在同一格里的小幅拖动仍由 TextEdit 选单元格文字。
                                    let pointer = ui.ctx().input(|i| i.pointer.interact_pos());
                                    let pressed = ui.ctx().input(|i| {
                                        i.pointer.button_pressed(egui::PointerButton::Primary)
                                    });
                                    let down = ui.ctx().input(|i| i.pointer.primary_down());
                                    let dragging =
                                        ui.ctx().input(|i| i.pointer.is_decidedly_dragging());
                                    let press_pt =
                                        ui.ctx().input(|i| i.pointer.press_origin()).or(pointer);
                                    let surrender = |ui: &egui::Ui| {
                                        if let Some(fid) = ui.ctx().memory(|m| m.focused()) {
                                            ui.ctx().memory_mut(|m| m.surrender_focus(fid));
                                        }
                                    };
                                    if pressed {
                                        state.table_drag_from = pointer
                                            .and_then(|p| table_cell_at(&row_rects, p))
                                            .map(|(r, c)| (idx, r, c));
                                    } else if !down {
                                        state.table_drag_from = None;
                                    } else if let Some(range) = render::table_drag_range(
                                        &row_rects,
                                        press_pt,
                                        pointer,
                                        dragging,
                                    ) {
                                        match range {
                                            Some((from, to)) => {
                                                if to != from {
                                                    state.table_sel = Some((idx, from, to));
                                                    surrender(ui);
                                                }
                                            }
                                            None => {
                                                state.table_drag_from = None;
                                                state.table_sel = None;
                                                state.cross_sel = Some((
                                                    idx,
                                                    0,
                                                    idx,
                                                    block.plain().chars().count(),
                                                ));
                                                surrender(ui);
                                            }
                                        }
                                    }
                                    // 已选中的单元格叠底色（半透明，盖在格子文字上）。
                                    if let Some(sel) = state.table_sel.filter(|s| s.0 == idx) {
                                        let sel = table_sel_rect(sel);
                                        render::paint_table_sel(ui, theme, &row_rects, sel);
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
                                        if drag_in_progress {
                                            None
                                        } else {
                                            Some(caret)
                                        },
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
                                    let html = crate::export::source_html(&active_buf);
                                    crate::clipboard::copy_rich(ctx, &active_buf, &html);
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
                                    let code = items[tab].code.clone();
                                    let html = crate::export::source_html(&code);
                                    crate::clipboard::copy_rich(ctx, &code, &html);
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
                                            if drag_in_progress {
                                                None
                                            } else {
                                                Some(caret)
                                            },
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
                                    // 竖线按文字的 galley 矩形画，不用 TextEdit 的响应矩形：
                                    // 响应矩形比文字高 1px、上缘还高 0.5px，点击进入编辑时
                                    // 引用竖线会当场抖一下（渲染态 block.rs 用的就是文字矩形，
                                    // 两态必须同口径才能「点进去边框不动」）。
                                    let g = egui::Rect::from_min_size(
                                        out.galley_pos,
                                        out.galley.rect.size(),
                                    );
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_max(
                                            egui::pos2(g.left() - 11.0, g.top()),
                                            egui::pos2(g.left() - 8.0, g.bottom()),
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
                            // VS Code 风格的当前行高亮：从行号栏右侧铺到编辑区右缘，
                            // 半透明垫在文字之上也不会影响阅读；编辑器失焦时仍然保留。
                            // 失焦后 TextEdit 不再返回 cursor_range，用上次记录的光标位置兜底。
                            {
                                let caret_idx = cr
                                    .map(|r| r.primary.index.0)
                                    .unwrap_or_else(|| {
                                        state.last_cursor.min(active_buf.chars().count())
                                    });
                                let mut acc = 0usize;
                                let mut band: Option<(f32, f32)> = None;
                                for row in &out.galley.rows {
                                    let n = row.char_count_including_newline().0;
                                    if caret_idx < acc + n {
                                        band =
                                            Some((out.galley_pos.y + row.pos.y, row.row.size.y));
                                        break;
                                    }
                                    acc += n;
                                }
                                if band.is_none() {
                                    if let Some(row) = out.galley.rows.last() {
                                        band =
                                            Some((out.galley_pos.y + row.pos.y, row.row.size.y));
                                    }
                                }
                                // 空块的 galley 只有一条高度为 0 的空 row，
                                // 用编辑框实际高度兜底，光标停在空行时也能看到整行高亮。
                                let band = band.filter(|(_, h)| *h > 0.5).or_else(|| {
                                    let r = out.response.rect;
                                    (r.height() > 0.0).then_some((r.top(), r.height()))
                                });
                                if let Some((top, h)) = band {
                                    let left = clip.left() + gutter_w + pad;
                                    let right = (clip.right() - 4.0).max(left + 1.0);
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_max(
                                            egui::Pos2::new(left, top),
                                            egui::Pos2::new(right, top + h),
                                        ),
                                        0.0,
                                        theme.line_highlight(),
                                    );
                                }
                            }
                            let resp = out.response;
                            // 活动块（含空行）起拖由 text_response 的兜底分支接管：
                            // 拖出本块落到别的渲染态块时以光标为锚点建立跨块选区。
                            // 这里不再在按下帧预置选区，否则普通点击会残留。
                            if resp.changed() {
                                changed = true;
                            }
                            let len = active_buf.chars().count();
                            // 跨块选区的字符坐标以「所见文本」计，编辑态 galley 用的是
                            // 缓冲区原文（可能含 **、[](…) 等隐藏标记），需要换算。
                            let plain_len = block.plain().chars().count();
                            let selected_text_block = text_selection_block(block);
                            // 代码块同样能按字符定位（缓冲 == 所见文本 == 代码本身），
                            // 只是它不走 text_response，所以单独放行；它的下标换算必须
                            // 恒等：plain_char_index/buffer_char_range 会跳过换行符，
                            // 用在多行代码上会把锚点整体往上挪几行。
                            let code_block = line_selectable_block(block);
                            let char_addressable = selected_text_block || code_block;
                            let galley_to_plain =
                                |gi: usize| -> usize {
                                    if code_block {
                                        gi
                                    } else {
                                        render::plain_char_index(&out.galley, gi)
                                    }
                                };
                            // 跨块选区扫过活动块：补画选中底色（垫在文字下层，
                            // 槽位在 TextEdit 之前已预留），并按指针位置上报精确
                            // 终点字符。没有这一段，选区拖回活动块时就会断掉。
                            // 活动块的行带：横向从内容列左缘铺到可用宽度右缘，
                            // 纵向给 ±8px 容差（块间距里的按下也算落在本行）。
                            // 与渲染态 text_response 的行带口径一致，否则同一个
                            // 选区在活动块/渲染态块交界处会出现一边能拖一边不能拖、
                            // 高宽窄也不一样的断层。
                            let band_left = ui.min_rect().left().min(out.galley_pos.x);
                            let band_right = ui.max_rect().right().max(out.galley_pos.x + out.galley.size().x);
                            let band_hit = egui::Rect::from_min_max(
                                egui::pos2(band_left - 8.0, resp.response.rect.top() - 8.0),
                                egui::pos2(band_right, resp.response.rect.bottom() + 8.0),
                            );
                            if char_addressable
                                && let Some((s0, e0)) = render::cross_sel_chars(
                                    state.cross_sel,
                                    idx,
                                    plain_len,
                                )
                            {
                                let range = if code_block {
                                    (s0, e0)
                                } else {
                                    render::buffer_char_range(&out.galley, (s0, e0))
                                };
                                // 活动块同样按行带铺色：左到内容列左缘、右到可用宽度
                                // 右缘，和渲染态块对齐，选区跨块时不会出现宽窄不一的断层。
                                // 代码编辑框画在不透明 panel 里，垫底的选区色会被面板盖住：
                                // 改成半透明叠在文字之上（与渲染态代码块同一口径）。
                                let (color, slot) = if code_block {
                                    (render::selection_overlay(theme), None)
                                } else {
                                    (theme.selection, active_sel_slot)
                                };
                                let shapes = render::selection_shapes(
                                    &out.galley,
                                    out.galley_pos,
                                    range,
                                    color,
                                    Some((band_left, band_right)),
                                );
                                if !shapes.is_empty() {
                                    match slot {
                                        Some(slot) => {
                                            ui.painter().set(slot, egui::Shape::Vec(shapes));
                                        }
                                        None => {
                                            ui.painter().add(egui::Shape::Vec(shapes));
                                        }
                                    }
                                }
                            }
                            if char_addressable
                                && state.cross_sel.is_some()
                                && ui.input(|i| i.pointer.primary_down())
                                && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                                && band_hit.contains(p)
                            {
                                let ci = out.galley.cursor_from_pos(p - out.galley_pos).index.0;
                                let ci = galley_to_plain(ci);
                                actions.push(Action::CrossDrag(idx, ci.min(plain_len), false));
                            }
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
                                // 按在行首标记/上下留白里也要记录锚点，否则会退回
                                // 上一次光标位置，拖出的选区就只覆盖「一部分」。
                                && band_hit.contains(p)
                            {
                                // 用点击位置直接在 galley 上映射字符：按下那一帧
                                // TextEdit 可能还没返回 cursor_range，cursor_idx
                                // 仍是初始值（行尾），会把锚点记错。
                                // 缓冲区下标 → 所见文本下标（跳过 ** 等隐藏标记）。
                                let ci = out.galley.cursor_from_pos(p - out.galley_pos).index.0;
                                let ci = galley_to_plain(ci);
                                state.press_caret = Some((idx, ci.min(plain_len)));
                            }
                            // 编辑态文本块内直接拖动也要建立跨块选区。此前活动块
                            // 完全依赖 TextEdit 原生选区，而标题（大字号、无自定义
                            // layouter）和含行内代码/加粗等隐藏标记的段落，原生选区
                            // 在这类 galley 上常常只定位光标、拖不出高亮（用户反馈
                            // 「点标题能动光标，拖却选不中」）。这里在按下锚点已记录、
                            // 且确认为拖动后，用所见文本坐标主动建立 cross_sel，
                            // 终点由上面的分支逐帧延伸，高亮由 selection_shapes 统一绘制。
                            if selected_text_block
                                && state.cross_sel.is_none()
                                && ui.input(|i| i.pointer.primary_down())
                                && ui.input(|i| i.pointer.is_decidedly_dragging())
                                && let Some((pa, pc)) = state.press_caret
                                && pa == idx
                                && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                                && band_hit.contains(p)
                            {
                                let buf_ci = out.galley.cursor_from_pos(p - out.galley_pos).index.0;
                                let ci = render::plain_char_index(&out.galley, buf_ci);
                                // 起点用按下锚点、终点用当前指针；CrossDrag(start) 会
                                // 让帧末聚合以锚点建选区，随后再延伸终点。
                                actions.push(Action::CrossDrag(idx, pc.min(plain_len), true));
                                actions.push(Action::CrossDrag(idx, ci.min(plain_len), false));
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
                        // 向上拖取块首，保证选区能跨过代码块/图表/图片继续延伸。
                        // 本块自己已经上报过精确端点时（代码块里拖代码、表格里拖单元格）跳过。
                        let drag_reported = actions[action_start..].iter().any(|a| {
                            matches!(a,
                                Action::CrossDrag(i, _, _) | Action::TableDrag(i, Some(_)) if *i == idx
                            )
                        });
                        if !text_selection_block(block)
                            && !drag_reported
                            && resp.rect.is_positive()
                            // 展开的代码块且锚点就在本块的字符上：指针暂时飘到功能栏
                            // 或面板里没有文字的位置时，宁可让端点停在最后一次落点，
                            // 也不能改取「块首/块尾」——那正是「选一行变成选中整块」。
                            // 真正拖出本块矩形时由后面的按行兜底继续跨块延伸。
                            && !(line_selectable_block(block)
                                && !state.folded.contains(&idx)
                                && state.cross_sel.is_some_and(|(a, ..)| a == idx))
                        {
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
                                } else if let Some((pa, _)) = state.press_caret {
                                    // 从活动块（TextEdit）里拖出来：以按下点为锚点，
                                    // 终点按本块在锚点下方/上方取块首/块尾。
                                    // 按下点就在本块内时不处理，避免抢走 TextEdit
                                    // 自己的拖选（代码块里选一段代码）。
                                    if pa != idx {
                                        actions.push(Action::ShiftClick(
                                            idx,
                                            if idx >= pa { len } else { 0 },
                                        ));
                                    }
                                } else if !editing && in_viewport && dragging {
                                    // 在渲染态非文本块上起拖：整块作为锚点。
                                    actions.push(Action::CrossDrag(idx, 0, true));
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
                                    &line_nos[k].to_string(),
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
                            // 本块这一帧实际推进了多少（含块后间距），回写排版表；
                            // 下一帧的可见区间与滚动条长度都靠它。
                            ui.cursor().min.y - band.min.y
                        })
                        .inner;
                        measured.push((idx, adv));
                        y += adv;
                    }
                    // 实测高度统一回写：放在循环之外，避免和循环体内对 state 的借用打架。
                    // 行数顺带算准（只算可见块），行号表因此无需整篇重扫。
                    for (idx, adv) in measured {
                        let lines = blocks
                            .get(idx)
                            .map(|b| b.source().split('\n').count() as u32)
                            .unwrap_or(1);
                        state.layout.measure(idx, adv, lines);
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

            #[cfg(test)]
            {
                // 测试基准：本帧各块真实矩形（含表格面板、代码块功能栏这类
                // 比文字大了一圈的块），供「拖拽网格」用例判定归属块。
                state.last_block_rects = block_rects
                    .iter()
                    .map(|(i, r)| (*i, (r.left(), r.top(), r.right(), r.bottom())))
                    .collect();
            }
            let pointer_down = ctx.input(|i| i.pointer.primary_down());
            let dragging = ctx.input(|i| i.pointer.is_decidedly_dragging());
            // 按下点必须在编辑区里：按在工具条 / 菜单上时不能延伸正文选区。
            let press_in_editor = ctx
                .input(|i| i.pointer.press_origin())
                .is_some_and(|p| ui.clip_rect().expand2(egui::vec2(48.0, 8.0)).contains(p));
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
            let plain_len = |i: usize| -> usize {
                doc.blocks
                    .get(i)
                    .map(|b| b.plain().chars().count())
                    .unwrap_or(0)
            };
            // 两张矩形表只在真正拖拽的这一帧才构造：静置、滚动、敲字帧不分配
            // （1000 万字实测静置一帧约 1.8 ms，不能被每帧的向量化挤掉）。
            let drag_happening = pointer_down && dragging && press_in_editor;
            // 矩形表含空白行：指针停在空行上时，选区落到该空行（锚点仍由文字块/
            // 按下块决定，不会缩成零长度），与逐行选择的用户预期一致。
            // 注意不要把空白行从候选里剔掉——旧做法因此把端点停在上一个块，
            // 表现为「拖到一半只选中一部分」。
            let all_rects: Vec<(usize, egui::Rect)> = if drag_happening {
                block_rects.iter().copied().collect()
            } else {
                Vec::new()
            };
            // 指针压在哪个块上：先按矩形精确命中，再放宽到「同一行带内、左右各 24px」。
            // 表格单元、代码块功能栏、图片预览这些子控件自己不接拖拽事件，
            // 只能在这里按块归属兜住，否则按下它们时选区会退回上一次光标所在块。
            let owner_of = |p: egui::Pos2| -> Option<usize> {
                all_rects
                    .iter()
                    .find(|(_, r)| r.contains(p))
                    .map(|(i, _)| *i)
                    .or_else(|| {
                        all_rects
                            .iter()
                            .filter(|(_, r)| p.y >= r.top() - 3.0 && p.y <= r.bottom() + 3.0)
                            .filter(|(_, r)| p.x >= r.left() - 24.0 && p.x <= r.right() + 24.0)
                            .min_by_key(|(_, r)| {
                                ((p.x - (r.left() + r.right()) * 0.5).abs() * 1000.0) as i32
                            })
                            .map(|(i, _)| *i)
                    })
            };
            // 按下点落在「块内子控件不吃拖拽事件」的原子块（表格单元、代码块功能栏、图片预览）上时，
            // 渲染层这一帧不上报锚点；不先补锚，聚合里 ShiftClick 的「无选区」分支会拿上一次光标当锚点，
            // 表现为「从表格起拖，表格和它前面的内容整块漏掉」。
            // 展开的代码块不在其列：它自己按字符报锚点（渲染态走代码区、编辑态走
            // press_caret），这里再补一个「块首」锚点会把精确落点抹成 0。
            if drag_happening && state.cross_sel.is_none() {
                let press_atomic_owner = ctx
                    .input(|i| i.pointer.press_origin())
                    .and_then(owner_of)
                    .filter(|o| {
                        doc.blocks.get(*o).is_some_and(|b| {
                            !matches!(b, Block::Blank { .. })
                                && !text_selection_block(b)
                                && (!line_selectable_block(b) || state.folded.contains(o))
                        })
                    })
                    // 按下点落在表格单元格里（编辑态）：这一帧起就在按单元格选，
                    // 不能再补「块首」锚点，否则下一帧的互斥判定会把单元格选区清掉。
                    .filter(|o| state.table_drag_from.is_none_or(|(b, ..)| b != *o));
                if let Some(o) = press_atomic_owner {
                    state.cross_sel = Some((o, 0, o, 0));
                    // 与 CrossDrag(start) 一致：开始跨块选区时放弃 TextEdit 焦点。
                    if let Some(fid) = ctx.memory(|m| m.focused()) {
                        ctx.memory_mut(|m| m.surrender_focus(fid));
                    }
                }
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
                // 表格内拖选：Some = 只选中经过的单元格（跨块选区让位）；
                // None = 指针已拖出表格，清除单元格选区，整块端点由同帧的
                // CrossDrag / 越界兜底负责。
                Action::TableDrag(i, range) => {
                    match range {
                        Some((a, b)) => {
                            state.table_sel = Some((*i, *a, *b));
                            state.cross_sel = None;
                            if let Some(fid) = ctx.memory(|m| m.focused()) {
                                ctx.memory_mut(|m| m.surrender_focus(fid));
                            }
                        }
                        None => state.table_sel = None,
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
                let endpoint_at = |i: usize| (i, if i >= anchor.0 { plain_len(i) } else { 0 });
                // 指针竖直方向压在某个块上（哪怕 x 在标记/留白区）→ 取该块端点。
                // 候选含空白行：指针停在空行上时选区就落到那一行（锚点仍是文字块，
                // 不会缩成零长度），与逐行选择的用户预期一致。
                let mut endpoint = all_rects
                    .iter()
                    .find(|(_, r)| p.y >= r.top() && p.y <= r.bottom())
                    .map(|(i, _)| endpoint_at(*i));
                // 在块与块的空档里：取竖直距离最近的一块，上方取块尾、下方取块首；
                // 正文上方没有任何块时取第一块块首。
                if endpoint.is_none() {
                    let above = all_rects
                        .iter()
                        .filter(|(_, r)| r.bottom() <= p.y)
                        .max_by(|a, b| a.1.bottom().total_cmp(&b.1.bottom()));
                    let below = all_rects
                        .iter()
                        .filter(|(_, r)| r.top() >= p.y)
                        .min_by(|a, b| a.1.top().total_cmp(&b.1.top()));
                    endpoint = match (above, below) {
                        (Some(a), Some(b)) => {
                            if p.y - a.1.bottom() <= b.1.top() - p.y {
                                Some(endpoint_at(a.0))
                            } else {
                                Some((b.0, 0))
                            }
                        }
                        (Some(a), None) => Some(endpoint_at(a.0)),
                        (None, Some(b)) => Some((b.0, 0)),
                        (None, None) => None,
                    };
                }
                // 端点又落回锚点所在的展开代码块、且指针还在它的矩形里（功能栏、
                // 面板留白这些没有字符的位置）：这一步不做改动，保留上一次的精确端点。
                // 拖出该块矩形时指针落在邻块或空档，仍然照常跨块延伸。
                if endpoint.is_some_and(|(ei, _)| {
                    ei == anchor.0
                        && doc.blocks.get(ei).is_some_and(|b| {
                            line_selectable_block(b) && !state.folded.contains(&ei)
                        })
                        && all_rects.iter().any(|(i, r)| *i == ei && r.contains(p))
                }) {
                    endpoint = None;
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

    // ---------------- 滚动条上的分隔线与光标位置标记 ----------------
    // 位置取自排版表，不取本帧画出来的块矩形：锚点本来就是表算的，两者一致，
    // 而活动块被滚出视口时表是唯一还说得清它在哪儿的东西（它仍为键盘而排版）。
    if active < doc.blocks.len() {
        let caret_y = state.layout.top(active) + state.layout.height_of(active) * 0.5;
        paint_scrollbar_marks(
            ui,
            theme,
            scroll_out.inner_rect,
            scroll_out.content_size.y,
            scroll_out.state.offset().y,
            caret_y,
        );
    }

    // 空白点击判定：不依赖哪个控件抢到这次点击，直接看指针落点。
    // 条件：在编辑区范围内、不在任何块（含块里的链接/按钮）上、且事件归属
    // 编辑区这一层（弹窗/对话框盖在上面时不算）。滚动条那一条也要排除。
    let pointer = ctx.input(|i| i.pointer.interact_pos());
    let primary_clicked = ctx.input(|i| i.pointer.primary_clicked());
    let bar_zone = scroll_bar_zone(ui, scroll_out.inner_rect);
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

    // 编辑区右键：记录弹出位置，由 app 层统一绘制上下文菜单。
    //
    // 不再要求落点必须「绕开块矩形」。块矩形通常是整行宽度，段落右侧的
    // 视觉留白也在矩形内，之前的严格判断会表现为「有时能弹、有时不能弹」。
    // 现在只要右键落在编辑区可见范围、不压在滚动条上、且事件归属编辑区
    // 这一层，就打开菜单；块内按钮/链接仍会优先处理自己的右键。
    let right_click_pos = pointer.filter(|p| {
        (ui.max_rect().contains(*p) || ui.clip_rect().contains(*p))
            && !bar_zone.contains(*p)
            && ctx
                .layer_id_at(*p)
                .is_none_or(|layer| layer == ui.layer_id())
    });
    if !state.read_only
        && ctx.input(|i| i.pointer.secondary_clicked())
        && let Some(p) = right_click_pos
    {
        state.context_menu = Some(p);
    }

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
    // 围栏块（代码块/图表/代码组）例外：并块会把 ` ``` ` 连同代码一起塞进上一段，
    // 用户只想删第一行行首前那一个字符，结果整块代码掉成普通文字。
    // 这类块的退格保持光标不动，退出走空块连按两次回车或块头「删除」。
    if backspace
        && active > 0
        && state.sel.is_none()
        && (buf_was_empty || state.last_cursor == 0)
        && !edit_kind.is_fenced()
    {
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
    // 只在本帧真的改到了字时写回：egui 对「空代码块里按一次退格」这类没删掉
    // 任何东西的按键也报 changed，照写会把 "```rust\n```" 重排成带空行的等价
    // 源码，白白把文档标脏、撤销栈多塞一步空操作。
    let text_differs = doc
        .blocks
        .get(active)
        .map(|b| block_editable_text(b) != active_buf)
        .unwrap_or(true);
    if changed && text_differs {
        doc.maybe_record();
        let src = table_edit_src.clone().unwrap_or_else(|| {
            // 删空了就落成空段落：不要把标题的 #、列表的 - 这些残留标记
            // 当正文写回去，否则下一次退格只会在标记里删字符，永远合并不回上一行。
            // 围栏块反过来：` ``` ` 是块语义本身，正文删空（或只剩空格/缩进）
            // 也要把围栏写回去，否则「删掉最后一个字符」那一帧就把整块代码打回段落。
            if active_buf.trim().is_empty() && !edit_kind.is_fenced() {
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
