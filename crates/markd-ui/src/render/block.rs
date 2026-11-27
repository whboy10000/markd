//! 块级渲染入口：按 AST 块类型分派到行内 / 表格 / 代码块 / 图表 / 图片。

use super::*;

/// YAML Front Matter 悬浮条的尺寸：`(宽, 高)`，宽度按「折叠 + 删除 + 复制」
/// 三个按钮最坏情况留足，超出部分会被按钮自己的右对齐布局吃掉。
const FLOATING_STRIP: (f32, f32) = (140.0, 24.0);

/// 代码面板的操作按钮（折叠 / 删除 / 复制），从右往左排。
/// 普通代码块把它们画在功能栏那一行，YAML Front Matter 画在悬停浮现的悬浮条里，
/// 两处行为完全一致。返回是否点了「复制」。
fn panel_buttons(
    ui: &mut egui::Ui,
    theme: &Theme,
    env: &mut RenderEnv<'_>,
    idx: usize,
    folded: bool,
) -> bool {
    let mut copied = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // 最右：折叠/展开；其次删除（弹确认窗）；再复制
        let fold_label = if folded { "展开" } else { "折叠" };
        if ui
            .button(egui::RichText::new(fold_label).size(theme.body * 0.85))
            .on_hover_cursor(egui::CursorIcon::Default)
            .clicked()
        {
            env.actions.push(Action::FoldToggle(idx));
        }
        if !folded {
            if ui
                .button(egui::RichText::new("删除").size(theme.body * 0.85))
                .on_hover_cursor(egui::CursorIcon::Default)
                .clicked()
            {
                env.actions.push(Action::AskDeleteBlock(idx));
            }
            if ui
                .button(egui::RichText::new("复制").size(theme.body * 0.85))
                .on_hover_cursor(egui::CursorIcon::Default)
                .clicked()
            {
                copied = true;
            }
        }
    });
    copied
}

/// 代码块外观的面板：功能栏（语言 + 折叠 / 删除 / 复制）+ 分隔线 + 语法高亮代码区。
/// `lang_editable=false` 时语言位显示固定标签——YAML Front Matter 走这条：外观与普通
/// 代码块一致，但语言恒为 yaml（`Action::SetLang` 也只对 Code 块生效，改它没有意义）。
/// `floating_header=true`（YAML Front Matter）时整条功能栏与分隔线都不画，面板只有代码区
/// 那么高；三个按钮改在指针进入面板时浮现在右上角（[`FLOATING_STRIP`]）。不这么做的话
/// 一 hover 就插进来一行，下面整篇文档跟着往下跳。
/// 返回整块 Response，供上层判定点击激活与跨块拖选的选区矩形。
fn code_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    env: &mut RenderEnv<'_>,
    idx: usize,
    lang: &str,
    code: &str,
    quote: usize,
    lang_editable: bool,
    floating_header: bool,
) -> egui::Response {
    let folded = env.folded.contains(&idx);
    // 折叠动画：1=完全展开、0=完全折叠，中间平滑过渡。
    let openness = env.ctx.animate_value_with_time(
        egui::Id::unique(("code_fold", idx)),
        if folded { 0.0 } else { 1.0 },
        0.18,
    );
    let lines = code.lines().count();
    let mut copied = false;
    let mut new_lang: Option<String> = None;
    // 功能栏矩形：下面的可点击区域从功能栏下缘开始，
    // 避免整块可点后把复制/删除/折叠按钮的点击吞掉。
    let mut header_rect = Rect::NOTHING;
    // 代码 galley 与它的起点：拖选按字符锚定、逐行画选区底色都要用（折叠态没有）。
    let mut galley_at: Option<(std::sync::Arc<egui::Galley>, Pos2)> = None;
    // 引用内的代码块：左侧留出缩进 + 竖线位置，代码面板整体右移，
    // 竖线按面板实际高度画，和引用文字连成一条。
    let hr = ui.horizontal(|ui| {
        if quote > 0 {
            ui.add_space(quote.min(4) as f32 * 8.0 + 11.0);
        }
        // 外层横向布局会传染给面板内容：显式回到纵向，否则功能栏
        // 和代码会被并排放置、代码列被挤成一条很窄的竖条。
        let r = ui
            .vertical(|ui| {
                panel_frame(theme).show(ui, |ui| {
                    if !floating_header {
                        // 功能栏：左语言 + 右操作按钮（code_header 背景色块）
                        header_rect = code_header_frame(theme)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if lang_editable {
                                        if let Some(l) =
                                            language_combo(ui, ("code_lang", idx), lang)
                                        {
                                            new_lang = Some(l);
                                        }
                                    } else {
                                        // Front Matter 的语言恒为 yaml：只作标签显示，
                                        // 免得出现一个改了没反应的下拉（SetLang 只认 Code 块）。
                                        ui.label(
                                            egui::RichText::new(lang)
                                                .size(theme.body * 0.9)
                                                .color(theme.text),
                                        )
                                        .on_hover_text("YAML Front Matter 固定按 yaml 高亮");
                                    }
                                    copied |= panel_buttons(ui, theme, env, idx, folded);
                                });
                            })
                            .response
                            .rect;
                        ui.add_space(6.0);
                        code_divider(ui, theme);
                        ui.add_space(6.0);
                    }
                    // 代码区：完全折叠只留提示；过渡中代码淡出、提示淡入。
                    if openness <= 0.0 {
                        ui.label(
                            egui::RichText::new(format!("已折叠 {lines} 行代码"))
                                .size(theme.body * 0.9)
                                .italics()
                                .color(theme.dim),
                        )
                    } else {
                        let (code_resp, galley) = ui
                            .scope(|ui| {
                                ui.set_opacity(openness);
                                layout_label(
                                    ui,
                                    code_job(code, lang, theme),
                                    env.max_width,
                                    Sense::hover(),
                                )
                            })
                            .inner;
                        galley_at = Some((galley, code_resp.rect.min));
                        if openness < 1.0 {
                            ui.scope(|ui| {
                                ui.set_opacity(1.0 - openness);
                                ui.label(
                                    egui::RichText::new(format!("已折叠 {lines} 行代码"))
                                        .size(theme.body * 0.9)
                                        .italics()
                                        .color(theme.dim),
                                );
                            });
                        }
                        code_resp
                    }
                })
            })
            .inner;
        if quote > 0 {
            let rect = r.response.rect;
            ui.painter().rect_filled(
                Rect::from_min_max(
                    Pos2::new(rect.left() - 11.0, rect.top()),
                    Pos2::new(rect.left() - 8.0, rect.bottom()),
                ),
                1.0,
                theme.accent,
            );
        }
        r
    });
    let r = hr.inner;
    // YAML Front Matter 的悬浮条：指针进入面板才出现，画在代码区右上角内侧。
    // 面板高度一帧之间不变，所以 hover 不会把下面的整篇文档顶下去。
    let floating_strip = if floating_header
        && env
            .ctx
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| r.response.rect.contains(p))
    {
        let panel = r.response.rect;
        Some(Rect::from_min_max(
            Pos2::new(panel.max.x - FLOATING_STRIP.0, panel.min.y + 5.0),
            Pos2::new(panel.max.x - 5.0, panel.min.y + 5.0 + FLOATING_STRIP.1),
        ))
    } else {
        None
    };
    if copied {
        env.ctx.copy_text(code.to_string());
        env.actions.push(Action::Notify("复制成功".to_string()));
    }
    if let Some(l) = new_lang {
        env.actions.push(Action::SetLang(idx, l));
    }
    // 代码区（功能栏以下，含空代码块的留白）整块可点击：
    // 空代码块的 galley 又窄又矮，只把文字 label 当点击区时，
    // 鼠标离开后再点代码区空白处命不中，看起来像「代码块不能编辑」。
    // 面板矩形仍参与 union，保证跨块拖选时为整块叠选区色。
    // 悬浮条模式下没有功能栏那行，命中区从面板顶开始。
    let zone = Rect::from_min_max(
        Pos2::new(
            r.response.rect.left(),
            if floating_header {
                r.response.rect.top()
            } else {
                header_rect.bottom()
            },
        ),
        r.response.rect.right_bottom(),
    );
    let content = ui.interact(
        zone,
        ui.scope_id().with(("code_click", idx)),
        Sense::click(),
    );
    // 拖选与选区高亮精确到代码正文的字符：按下点落在代码区（功能栏以下）时，
    // 锚点/终点都取指针下的那个字符，而不是整块一起选中（用户反馈的
    // 「想选一两行代码，一下子就选中该代码块所有的代码」）。
    // 从别的块拖进来不在这里上报，仍由编辑器按「整块」取端点。
    if let Some((galley, origin)) = &galley_at {
        let len = code.chars().count();
        let band = (zone.left(), zone.right());
        // 选区底色叠在代码之上（半透明）：面板底色不透明，垫在下面会被盖掉。
        if let Some((s0, e0)) = cross_sel_chars(env.cross_sel, idx, len) {
            let shapes = selection_shapes(
                galley,
                *origin,
                (s0, e0),
                selection_overlay(theme),
                Some(band),
            );
            if !shapes.is_empty() {
                ui.painter().add(egui::Shape::Vec(shapes));
            }
        }
        let anchored_here = env.cross_sel.is_some_and(|(a, ..)| a == idx);
        // 命中区：默认只认代码区（功能栏以下）。选区锚点已经落在本块字符上时，
        // 整个面板矩形都算命中——指针暂时飘到功能栏或面板留白时，
        // `cursor_from_pos` 会钳到最近一行的字符，端点继续跟着指针走，
        // 而不是退回「块首/块尾」端点（用户看到的「选一行变成选中整块」）。
        let hit = |p: Pos2| {
            let in_band = p.x >= band.0
                && p.x <= band.1
                && p.y >= zone.top() - 2.0
                && p.y <= zone.bottom() + 2.0;
            // 悬浮条里的按下点不算代码：否则点「删除」会顺手把光标/选区
            // 锚到第一行 yaml 的某个字符上。
            (in_band || (anchored_here && r.response.rect.contains(p)))
                && !floating_strip.is_some_and(|s| s.contains(p))
        };
        let char_at = |p: Pos2| galley.cursor_from_pos(p - *origin).index.0.min(len);
        let pressed_now = env
            .ctx
            .input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
        // 终点只要求「指针当前落在本块代码区」，不要求按下点也在：
        // 按下点在外面时锚点由编辑器的兜底给出，指针一进代码区就该继续延伸选区，
        // 否则选区会停在锚点字符上什么都不选（看起来「选不中」）。
        if pressed_now {
            if let Some(p) = env
                .ctx
                .input(|i| i.pointer.press_origin())
                .filter(|p| hit(*p))
            {
                env.actions.push(Action::CrossDrag(idx, char_at(p), true));
            }
        } else if anchored_here
            && env.ctx.input(|i| i.pointer.primary_down())
            && let Some(q) = env
                .ctx
                .input(|i| i.pointer.interact_pos())
                .filter(|q| hit(*q))
        {
            env.actions.push(Action::CrossDrag(idx, char_at(q), false));
        }
        // 渲染态点代码区：把「相对 galley 的点击位置」交给编辑器，下一帧进入编辑态时
        // 光标落在点到的那个字符。代码块以前不报 Caret（只有文字块报），光标会跳到
        // 整段代码末尾——点第一行行首再按退格，删掉的却是最后一行的字符。
        if content.clicked()
            && !folded
            && let Some(p) = env
                .ctx
                .input(|i| i.pointer.interact_pos())
                .filter(|p| hit(*p))
        {
            env.actions.push(Action::Caret(idx, p - *origin));
        }
    }
    // 悬浮条画在最后：控件也在这一帧才注册，命中测试按注册顺序取最上层，
    // 所以点按钮时上面那个「整块可点击」的 content 不会同时被判定为点击。
    if let Some(strip) = floating_strip {
        ui.painter().rect_filled(strip, 4.0, theme.code_header);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(strip)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        if panel_buttons(&mut child, theme, env, idx, folded) {
            env.ctx.copy_text(code.to_string());
            env.actions.push(Action::Notify("复制成功".to_string()));
        }
    }
    content.union(r.response)
}

pub fn render_block(
    ui: &mut egui::Ui,
    idx: usize,
    block: &Block,
    env: &mut RenderEnv<'_>,
    dimmed: bool,
) -> egui::Response {
    let theme = env.theme;
    let color = if dimmed { theme.dim } else { theme.text };
    let inner = ui.scope(|ui| {
        if dimmed {
            ui.set_opacity(0.5);
        }
        match block {
            Block::Heading { level, content, .. } => {
                // 标题上下留白：尽量紧凑，且与编辑态（标题无额外留白）接近，
                // 否则进出编辑时整块会跳动。之前 8/6px 让 H1–H3 行距明显偏大。
                ui.add_space(if *level <= 2 { 3.0 } else { 2.0 });
                let r = text_response(
                    ui,
                    content,
                    theme,
                    theme.heading_size(*level),
                    theme.heading,
                    true,
                    env,
                    idx,
                );
                ui.add_space(3.0);
                r
            }
            Block::Paragraph { content, .. } => {
                if content.len() == 1 {
                    if let Inline::Image { alt, url, width } = &content[0] {
                        let (resp, resized) = image_preview(
                            ui,
                            alt,
                            url,
                            theme,
                            env.ctx,
                            env.images,
                            env.base_dir,
                            *width,
                            (idx, 0),
                        );
                        if let Some(w) = resized {
                            env.actions.push(Action::ResizeImage(idx, 0, w));
                        }
                        return resp;
                    }
                }
                // 连续多行图片（中间没有空行）会被解析成同一个段落：
                // 逐张渲染成图片预览，否则会退化成一行行「[图片: …]」占位文字。
                if content.iter().any(|i| matches!(i, Inline::Image { .. }))
                    && content
                        .iter()
                        .all(|i| matches!(i, Inline::Image { .. } | Inline::Break))
                {
                    let mut merged: Option<egui::Response> = None;
                    let mut no = 0usize;
                    for inl in content {
                        if let Inline::Image { alt, url, width } = inl {
                            let (r, resized) = image_preview(
                                ui,
                                alt,
                                url,
                                theme,
                                env.ctx,
                                env.images,
                                env.base_dir,
                                *width,
                                (idx, no),
                            );
                            if let Some(w) = resized {
                                env.actions.push(Action::ResizeImage(idx, no, w));
                            }
                            no += 1;
                            merged = Some(match merged.take() {
                                Some(prev) => prev.union(r),
                                None => r,
                            });
                        }
                    }
                    return merged.expect("段落里至少有一张图片");
                }
                // 「整段只有一个附件链接」才升成文件卡片；句中夹的文件链接仍是普通链接。
                if let Some((url, label, kind)) = attachment_of(content) {
                    return file_panel(ui, theme, env, idx, url, &label, kind);
                }
                text_response(ui, content, theme, theme.body, color, false, env, idx)
            }
            Block::Div { tag, content, .. } => {
                // 内嵌 HTML 的盒子：`<div style="background:#eee; padding:10px">提示框</div>`
                // 画成灰底圆角框，没写 background 就只留内边距（不凭空给个框骗人）。
                // 高度由 Frame 自己推进游标，编辑器按 `adv` 量回真实高度，不需要额外表。
                let st = crate::html_style::style_of(tag, theme.body);
                egui::Frame::NONE
                    .fill(
                        st.background
                            .map(|[r, g, b]| egui::Color32::from_rgb(r, g, b))
                            .unwrap_or(egui::Color32::TRANSPARENT),
                    )
                    .corner_radius(6.0)
                    .inner_margin(st.padding_px.unwrap_or(10.0))
                    .show(ui, |ui| {
                        text_response(ui, content, theme, theme.body, color, false, env, idx)
                    })
                    .inner
            }
            Block::Quote { level, content, .. } => {
                ui.horizontal(|ui| {
                    ui.add_space((*level).min(4) as f32 * 8.0);
                    // 竖线不再预先占一个固定高度的矩形：先给正文留出「线宽 + 间距」，
                    // 拿到文字实际高度后再补画竖线，软换行后的每一行都能被竖线贯穿。
                    ui.add_space(11.0);
                    let resp =
                        text_response(ui, content, theme, theme.body, theme.quote, true, env, idx);
                    let r = resp.rect;
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            Pos2::new(r.left() - 11.0, r.top()),
                            Pos2::new(r.left() - 8.0, r.bottom()),
                        ),
                        1.0,
                        theme.accent,
                    );
                    resp
                })
                .inner
            }
            Block::ListItem { item, .. } => {
                let indent = (item.indent.min(12) as f32) * 8.0;
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    // 标记与正文更紧凑：默认 item_spacing 是 8px，列表看着离得太远。
                    ui.spacing_mut().item_spacing.x = 5.0;
                    match item.checked {
                        Some(checked) => {
                            // 任务列表复选框放大一档（默认 14px 偏小），并加深方框描边，
                            // 否则浅色主题下白底白框几乎看不见。
                            ui.spacing_mut().icon_width = 19.0;
                            ui.spacing_mut().icon_width_inner = 11.0;
                            ui.spacing_mut().icon_spacing = 3.0;
                            let v = &mut ui.visuals_mut().widgets;
                            v.inactive.bg_stroke = egui::Stroke::new(1.0, theme.dim);
                            v.hovered.bg_stroke = egui::Stroke::new(1.0, theme.accent);
                            v.active.bg_stroke = egui::Stroke::new(1.0, theme.accent);
                            let mut c = checked;
                            let cb = ui.checkbox(&mut c, "");
                            if cb.clicked() {
                                env.actions.push(Action::ToggleTask(idx));
                            }
                        }
                        None => {
                            let level = list_level(item.indent);
                            if item.marker.ends_with('.') || item.marker.ends_with(')') {
                                // 有序列表：顶层数字，子层依次用 a. / i.（与父列表区分）。
                                let mut f = theme.fmt(theme.body);
                                f.color = theme.dim;
                                let mut job = LayoutJob::default();
                                job.append(&ordered_list_marker(&item.marker, level), 0.0, f);
                                ui.label(job);
                            } else {
                                // 无序列表：标记形状随层级变化（实心圆 → 空心圆 → 小方块）。
                                let d = (theme.body * 0.6).clamp(7.0, 14.0);
                                let color = theme.dim.lerp_to_gamma(theme.text, 0.55);
                                draw_list_bullet(ui, level, color, d);
                            }
                        }
                    }
                    text_response(ui, &item.content, theme, theme.body, color, false, env, idx)
                })
                .inner
            }
            Block::Code {
                lang, code, quote, ..
            } => match if *quote == 0 {
                video_source_of(lang, code)
            } else {
                None
            } {
                // ```video 围栏画成播放器卡片（引用内的仍按代码块走，卡片要整幅宽度）。
                Some(src) => video_panel(ui, theme, env, idx, &src, code.trim()),
                None => code_panel(ui, theme, env, idx, lang, code, *quote, true, false),
            },
            Block::CodeGroup { items, .. } => {
                let folded = env.folded.contains(&idx);
                let openness = env.ctx.animate_value_with_time(
                    egui::Id::unique(("code_group_fold", idx)),
                    if folded { 0.0 } else { 1.0 },
                    0.18,
                );
                let active = env.group_tab.min(items.len().saturating_sub(1));
                let item = &items[active];
                let lines = item.code.lines().count();
                let mut copied = false;
                let mut switch_tab: Option<usize> = None;
                let mut new_lang: Option<String> = None;
                let mut add_tab = false;
                let mut remove_tab = false;
                let mut header_rect = Rect::NOTHING;
                let hr = ui.horizontal(|ui| {
                    let r = ui
                        .vertical(|ui| {
                            panel_frame(theme).show(ui, |ui| {
                                header_rect = code_header_frame(theme)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            // 左侧：先「+ / −」增删标签，再是语言标签。
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            if !folded {
                                                let (a, r) = code_group_tab_buttons(
                                                    ui,
                                                    theme,
                                                    items.len() > 1,
                                                );
                                                add_tab |= a;
                                                remove_tab |= r;
                                            }
                                            for (i, it) in items.iter().enumerate() {
                                                if code_group_tab(ui, theme, i == active, &it.lang)
                                                {
                                                    switch_tab = Some(i);
                                                }
                                            }
                                            // 右侧：与代码块一致的操作按钮。
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.spacing_mut().item_spacing.x = 8.0;
                                                    let fold_label =
                                                        if folded { "展开" } else { "折叠" };
                                                    if ui
                                                        .button(
                                                            egui::RichText::new(fold_label)
                                                                .size(theme.body * 0.85),
                                                        )
                                                        .on_hover_cursor(egui::CursorIcon::Default)
                                                        .clicked()
                                                    {
                                                        env.actions.push(Action::FoldToggle(idx));
                                                    }
                                                    if !folded {
                                                        if ui
                                                            .button(
                                                                egui::RichText::new("删除")
                                                                    .size(theme.body * 0.85),
                                                            )
                                                            .on_hover_cursor(
                                                                egui::CursorIcon::Default,
                                                            )
                                                            .clicked()
                                                        {
                                                            env.actions
                                                                .push(Action::AskDeleteBlock(idx));
                                                        }
                                                        if ui
                                                            .button(
                                                                egui::RichText::new("复制")
                                                                    .size(theme.body * 0.85),
                                                            )
                                                            .on_hover_cursor(
                                                                egui::CursorIcon::Default,
                                                            )
                                                            .clicked()
                                                        {
                                                            copied = true;
                                                        }
                                                        // 语言下拉：改的是当前标签的语言。
                                                        if let Some(l) = language_combo(
                                                            ui,
                                                            ("code_group_lang", idx),
                                                            &item.lang,
                                                        ) {
                                                            new_lang = Some(l);
                                                        }
                                                    }
                                                },
                                            );
                                        });
                                    })
                                    .response
                                    .rect;
                                ui.add_space(6.0);
                                code_divider(ui, theme);
                                ui.add_space(6.0);
                                if openness <= 0.0 {
                                    ui.label(
                                        egui::RichText::new(format!("已折叠 {lines} 行代码"))
                                            .size(theme.body * 0.9)
                                            .italics()
                                            .color(theme.dim),
                                    )
                                } else {
                                    ui.scope(|ui| {
                                        ui.set_opacity(openness);
                                        wrapped_label(
                                            ui,
                                            code_job(&item.code, &item.lang, theme),
                                            env.max_width,
                                            Sense::hover(),
                                        )
                                    })
                                    .inner
                                }
                            })
                        })
                        .inner;
                    r
                });
                let r = hr.inner;
                if copied {
                    env.ctx.copy_text(item.code.clone());
                    env.actions.push(Action::Notify("复制成功".to_string()));
                }
                if let Some(t) = switch_tab {
                    env.actions.push(Action::CodeGroupTab(idx, t));
                }
                if add_tab {
                    env.actions.push(Action::CodeGroupAddTab(idx));
                }
                if remove_tab {
                    env.actions.push(Action::CodeGroupRemoveTab(idx));
                }
                if let Some(l) = new_lang {
                    env.actions.push(Action::CodeGroupLang(idx, l));
                }
                // 代码区（功能栏以下的留白）整块可点击进入编辑，标签栏除外。
                let zone = Rect::from_min_max(
                    Pos2::new(r.response.rect.left(), header_rect.bottom()),
                    r.response.rect.right_bottom(),
                );
                let content = ui.interact(
                    zone,
                    ui.scope_id().with(("code_group_click", idx)),
                    Sense::click(),
                );
                content.union(r.response)
            }
            Block::Table {
                head,
                rows,
                align,
                source,
                ..
            } => {
                // 表格随窗口浮动：列宽平分可用宽度（扣除内边距与列间距），
                // 窗口变宽表就变宽；窄窗口下自动收窄并让单元格内文字换行。
                // 列宽公式与编辑态（editor.rs 表格编辑分支）完全一致，
                // 保证点击进入编辑时列宽不变、表格不跳。
                // 列数按「所有行里最宽的一行」计算。只看表头会在数据行比表头
                // 多列时把整张表按 1 列排版：col_w 拿到整页宽度，第二列之后
                // 全被推到屏幕外，看起来就像「表格只显示一列」。
                let cols = head
                    .len()
                    .max(rows.iter().map(|r| r.len()).max().unwrap_or(0))
                    .max(1);
                // 再按版心宽度兜底：横向布局里 available_width 偶尔会拿到
                // 远超正文宽度的值，若不夹住，col_w 会被放大成整页宽，
                // 第二列之后直接被推出可视区域。
                let avail = (ui.available_width().min(env.max_width) - 12.0).max(60.0);
                let gap = ui.spacing().item_spacing.x * cols.saturating_sub(1) as f32;
                // 列多时允许继续缩小，保证整表落在版心内，而不是把后面的列
                // 直接挤出可视区域。
                let col_w = ((avail - gap) / cols as f32).max(28.0);
                // 记录每个单元格的矩形，用于画「单线」分隔（不是每格一个框）
                let mut row_rects: Vec<Vec<egui::Rect>> = Vec::new();
                let mut copied = false;
                let mut op: Option<crate::editor::TableOp> = None;
                let cell_key = egui::Id::unique(("table_cell_ref", idx));
                // 功能区背景：浅色主题用指定的 #F2F2F4，深色主题沿用 panel
                let tb_fill = if theme.kind == crate::theme::ThemeKind::Light {
                    Color32::from_rgb(0xF2, 0xF2, 0xF4)
                } else {
                    theme.panel
                };
                let r = egui::Frame::NONE
                    .fill(theme.bg)
                    .stroke(Stroke::new(1.0, theme.table_line))
                    .corner_radius(4.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.set_min_width(col_w * cols as f32 + gap);
                        // ---- 第一区：功能区（常驻，不随悬停出现/消失）----
                        // 行/列操作以最后悬停的单元格为基准（Grid 渲染后写入记忆）；
                        // 复制与删除表格固定在最右侧右上角。
                        egui::Frame::NONE
                            .fill(tb_fill)
                            .corner_radius(3.0)
                            .inner_margin(4.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.style_mut().spacing.item_spacing.x = 4.0;
                                    let (hr, hc) = ui
                                        .ctx()
                                        .memory(|m| m.data.get_temp::<(usize, usize)>(cell_key))
                                        .unwrap_or((0, 0));
                                    let btn = |ui: &mut egui::Ui, label: &str, active: bool| {
                                        let txt =
                                            egui::RichText::new(label).size(theme.body * 0.85);
                                        let txt =
                                            if active { txt.color(theme.accent) } else { txt };
                                        ui.button(txt).clicked()
                                    };
                                    let all = |a: crate::ast::Align| align.iter().all(|x| *x == a);
                                    use crate::ast::Align;
                                    use crate::editor::TableOp;
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
                                    // 最右侧右上角：删除表格（最右）+ 复制
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
                                                env.actions.push(Action::AskDeleteTable(idx));
                                            }
                                            if ui
                                                .button(
                                                    egui::RichText::new("复制")
                                                        .size(theme.body * 0.85),
                                                )
                                                .clicked()
                                            {
                                                copied = true;
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(4.0);
                        // ---- 第二区：表头行；第三区：数据行（文字按列对齐）----
                        egui::Grid::new(("md_table", idx))
                            .min_col_width(col_w)
                            .max_col_width(col_w)
                            .show(ui, |ui| {
                                // 表头行：最深一档底色，文字按列对齐
                                let mut rects = Vec::new();
                                for (ci, c) in head.iter().enumerate() {
                                    let fr = egui::Frame::NONE
                                        .fill(theme.table_head)
                                        .inner_margin(3.0)
                                        .show(ui, |ui| {
                                            ui.set_min_width(col_w - 6.0);
                                            align_cell_label(
                                                ui,
                                                build_layout(
                                                    c,
                                                    theme,
                                                    theme.body * 0.95,
                                                    theme.heading,
                                                    true,
                                                ),
                                                col_w - 6.0,
                                                align
                                                    .get(ci)
                                                    .copied()
                                                    .unwrap_or(crate::ast::Align::Left),
                                                theme,
                                            );
                                        });
                                    rects.push(fr.response.rect);
                                }
                                row_rects.push(rects);
                                ui.end_row();
                                // 数据行：正文底色（表头一档颜色就够分了），文字按列对齐
                                for row in rows {
                                    let mut rects = Vec::new();
                                    for (ci, c) in row.iter().enumerate() {
                                        let fr = egui::Frame::NONE
                                            .fill(theme.bg)
                                            .inner_margin(3.0)
                                            .show(ui, |ui| {
                                                ui.set_min_width(col_w - 6.0);
                                                align_cell_label(
                                                    ui,
                                                    build_layout(
                                                        c,
                                                        theme,
                                                        theme.body * 0.95,
                                                        color,
                                                        false,
                                                    ),
                                                    col_w - 6.0,
                                                    align
                                                        .get(ci)
                                                        .copied()
                                                        .unwrap_or(crate::ast::Align::Left),
                                                    theme,
                                                );
                                            });
                                        rects.push(fr.response.rect);
                                    }
                                    row_rects.push(rects);
                                    ui.end_row();
                                }
                            });
                    });
                // 分隔线：行与行、列与列之间只画一条线（不是每个单元格一个盒子）。
                // 注意 shrink2 是「每边减去全额」：frame 内边距 6px，这里必须用 6
                // 才是真正的内容区；之前误用 12 导致所有分隔线整体内移 6px，
                // 列分隔线切进第二列的文字（文字与单元格"融合"的元凶）。
                let content = r.response.rect.shrink2(egui::vec2(6.0, 6.0));
                let stroke = Stroke::new(1.0, theme.table_line);
                let p = ui.painter();
                // 标题行底色铺满整行：单元格之间（item_spacing）和两端露出的缝隙
                // 用标题色补齐，只盖缝隙不盖文字。
                if let Some(row) = row_rects.first()
                    && row.len() == cols
                {
                    let top = row.iter().map(|x| x.top()).fold(f32::MAX, f32::min);
                    let bottom = row.iter().map(|x| x.bottom()).fold(f32::MIN, f32::max);
                    for k in 1..cols {
                        let left = row[k - 1].right();
                        let right = row[k].left();
                        if right > left {
                            p.rect_filled(
                                Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, bottom)),
                                0.0,
                                theme.table_head,
                            );
                        }
                    }
                    // 行两端的缝隙：对齐分隔线用的 content 宽度
                    if content.left() < row[0].left() {
                        p.rect_filled(
                            Rect::from_min_max(
                                Pos2::new(content.left(), top),
                                Pos2::new(row[0].left(), bottom),
                            ),
                            0.0,
                            theme.table_head,
                        );
                    }
                    if content.right() > row[cols - 1].right() {
                        p.rect_filled(
                            Rect::from_min_max(
                                Pos2::new(row[cols - 1].right(), top),
                                Pos2::new(content.right(), bottom),
                            ),
                            0.0,
                            theme.table_head,
                        );
                    }
                }
                for i in 1..row_rects.len() {
                    let next_top = row_rects[i].first().map(|x| x.top());
                    let prev_bottom = row_rects[i - 1]
                        .iter()
                        .map(|x| x.bottom())
                        .fold(f32::MIN, f32::max);
                    if let Some(t) = next_top {
                        let y = (t + prev_bottom) / 2.0;
                        p.line_segment(
                            [Pos2::new(content.left(), y), Pos2::new(content.right(), y)],
                            stroke,
                        );
                    }
                }
                // 列分隔线的纵向范围只覆盖表格网格（第一行顶到最后一行底）：
                // 内容区顶部现在是右上角的复制按钮行，不能让线穿过去。
                let grid_top = row_rects
                    .first()
                    .and_then(|r| r.first())
                    .map(|x| x.top())
                    .unwrap_or(content.top());
                let grid_bottom = row_rects
                    .last()
                    .and_then(|r| r.first())
                    .map(|x| x.bottom())
                    .unwrap_or(content.bottom());
                // 列分隔线与编辑态一致：取相邻单元格实际矩形间隙的中点，
                // 不能按 col_w+gap 估算——gap 是所有列间距之和，列数多时
                // 分隔线会逐列右移，压到左对齐的文字上（文字与边框重合）。
                if let Some(first) = row_rects.first()
                    && first.len() == cols
                {
                    for k in 1..cols {
                        let x = (first[k - 1].right() + first[k].left()) / 2.0;
                        p.line_segment([Pos2::new(x, grid_top), Pos2::new(x, grid_bottom)], stroke);
                    }
                }
                // 点击了哪个单元格：进入编辑态时直接聚焦那一格，光标落在
                // 点击位置附近（点在文字右半边 → 光标到末尾），不再固定跳到第一格。
                if r.response.clicked()
                    && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                {
                    for (ri, rects) in row_rects.iter().enumerate() {
                        if let Some(ci) = rects.iter().position(|x| x.contains(p)) {
                            let end = p.x > rects[ci].center().x;
                            env.actions.push(Action::TableCellClick(idx, ri, ci, end));
                            break;
                        }
                    }
                }
                // 悬停单元格时更新基准 (hr,hc)：功能区的行/列操作按最近悬停定位
                if let Some(hp) = ui.input(|i| i.pointer.hover_pos())
                    && let Some(hit) = hit_cell(&row_rects, hp)
                {
                    ui.ctx().memory_mut(|m| m.data.insert_temp(cell_key, hit));
                }
                // 表格内按住拖动：只把经过的那几格交给编辑器选（拖出表格边界才整块选中，
                // 由 view.rs 的跨块拖选接管）。以前一拖就是整张表，想复制一行都办不到。
                if let Some(range) = table_drag_range(
                    &row_rects,
                    ui.input(|i| i.pointer.press_origin()),
                    ui.input(|i| i.pointer.interact_pos()),
                    ui.input(|i| i.pointer.is_decidedly_dragging()),
                ) {
                    env.actions.push(Action::TableDrag(idx, range));
                }
                // 选中格子的底色：选区由编辑器持有，每帧挂进 ctx（见 view.rs）。
                if let Some(sel) = ui
                    .ctx()
                    .memory(|m| m.data.get_temp::<crate::editor::TableSel>(table_sel_id()))
                    && sel.0 == idx
                {
                    paint_table_sel(ui, theme, &row_rects, crate::editor::table_sel_rect(sel));
                }
                if let Some(op) = op {
                    env.actions.push(Action::TableOp(idx, op));
                }
                if copied {
                    env.ctx.copy_text(source.as_str().to_string());
                    env.actions.push(Action::Notify("复制成功".to_string()));
                }
                r.response.interact(Sense::click())
            }
            Block::Rule { .. } => {
                let w = ui.available_width();
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 18.0), Sense::click());
                ui.painter().line_segment(
                    [
                        Pos2::new(r.min.x, r.center().y),
                        Pos2::new(r.max.x, r.center().y),
                    ],
                    Stroke::new(1.0, theme.border),
                );
                resp
            }
            Block::Math { tex, source } => {
                // ```math 代码块与 `$$...$$` 块级公式都交给 RaTeX 排成原生显示列表，
                // 独占一行、居中显示。
                let latex = crate::ast::math_fence_tex(source.as_str())
                    .or_else(|| crate::ast::display_math_tex(source.as_str()));
                if let Some(latex) = latex {
                    let font_size = 22.0f32;
                    let rendered = env
                        .images
                        .math_shapes(&latex, font_size, MathDisplay::Display);
                    let r = panel_frame(theme).show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            if let Some(m) = &rendered {
                                let max_w = (env.max_width - 16.0).max(40.0);
                                let scale = (max_w / m.size.x.max(1.0)).min(1.0);
                                let draw_size = m.size * scale;
                                let (rect, _) = ui.allocate_exact_size(draw_size, Sense::hover());
                                let painter = ui.painter().clone();
                                for shape in crate::render::math::transform_shapes(
                                    &m.shapes,
                                    rect.min,
                                    scale,
                                    theme.text,
                                    ui.ctx().pixels_per_point(),
                                ) {
                                    painter.add(shape);
                                }
                            } else {
                                // 渲染失败时至少把 LaTeX 原文显示出来，方便排查。
                                let mut f = theme.mono_fmt();
                                f.color = theme.heading;
                                let mut job = LayoutJob::default();
                                job.append(&latex, 0.0, f);
                                wrapped_label(ui, job, env.max_width, Sense::hover());
                            }
                        })
                    });
                    return r.response.interact(Sense::click());
                }
                let r = panel_frame(theme).show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        let mut f = theme.mono_fmt();
                        f.color = theme.heading;
                        let mut job = LayoutJob::default();
                        job.append(&math_pretty(tex), 0.0, f);
                        wrapped_label(ui, job, env.max_width, Sense::hover())
                    })
                });
                r.response.interact(Sense::click())
            }
            Block::Diagram { code, .. } => {
                let folded = env.folded.contains(&idx);
                // 折叠动画：与代码块一致。
                let openness = env.ctx.animate_value_with_time(
                    egui::Id::unique(("diagram_fold", idx)),
                    if folded { 0.0 } else { 1.0 },
                    0.18,
                );
                let lines = code.lines().count();
                let mut copied = false;
                let mut new_lang: Option<String> = None;
                // 图类型：按首行关键字分发到对应的自绘渲染器。
                let kind = code
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .map(|l| l.trim().to_lowercase())
                    .unwrap_or_default();
                let r = panel_frame(theme).show(ui, |ui| {
                    // 功能栏：左语言 + 右操作按钮（默认 mermaid，选其它语言即转为普通代码块）。
                    code_header_frame(theme).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if let Some(l) = language_combo(ui, ("diagram_lang", idx), "mermaid") {
                                new_lang = Some(l);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let fold_label = if folded { "展开" } else { "折叠" };
                                    if ui
                                        .button(
                                            egui::RichText::new(fold_label).size(theme.body * 0.85),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::Default)
                                        .clicked()
                                    {
                                        env.actions.push(Action::FoldToggle(idx));
                                    }
                                    if !folded {
                                        // 与代码块一致：删除按钮弹确认窗，确定才删、取消不删
                                        if ui
                                            .button(
                                                egui::RichText::new("删除").size(theme.body * 0.85),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::Default)
                                            .clicked()
                                        {
                                            env.actions.push(Action::AskDeleteBlock(idx));
                                        }
                                        if ui
                                            .button(
                                                egui::RichText::new("复制").size(theme.body * 0.85),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::Default)
                                            .clicked()
                                        {
                                            copied = true;
                                        }
                                    }
                                },
                            );
                        });
                    });
                    ui.add_space(6.0);
                    code_divider(ui, theme);
                    ui.add_space(6.0);
                    // 图表区：完全折叠只留提示；过渡中图表淡出、提示淡入。
                    if openness <= 0.0 {
                        ui.label(
                            egui::RichText::new(format!("已折叠 {lines} 行代码"))
                                .size(theme.body * 0.9)
                                .italics()
                                .color(theme.dim),
                        )
                    } else {
                        // 渲染器支持流程图/时序图/饼图等；其余类型按代码保真显示，
                        // 绝不乱画。每个分支返回图表「内容区域」的响应——用它做块定位，
                        // 不覆盖右上角的按钮行（否则点「折叠」会连带着把块激活成编辑态）。
                        let body_resp = ui
                            .scope(|ui| {
                                ui.set_opacity(openness);
                                if kind.starts_with("pie") {
                                    draw_pie(ui, code, theme)
                                } else if kind.starts_with("sequencediagram") {
                                    draw_sequence(ui, code, theme)
                                } else if kind.starts_with("flowchart") || kind.starts_with("graph")
                                {
                                    draw_flow(ui, code, theme)
                                } else if kind.starts_with("statediagram") {
                                    // 状态图结构与流程图同构：边直接复用流程图布局绘制
                                    draw_flow(ui, &transform_state_to_flow(code), theme)
                                } else if kind.starts_with("classdiagram") {
                                    let (nodes, edges) = parse_class(code);
                                    draw_boxes_edges(ui, theme, &nodes, &edges)
                                } else if kind.starts_with("erdiagram") {
                                    let (nodes, edges) = parse_er(code);
                                    draw_boxes_edges(ui, theme, &nodes, &edges)
                                } else if kind.starts_with("gantt") {
                                    draw_gantt(ui, code, theme)
                                } else if kind.starts_with("journey") {
                                    draw_journey(ui, code, theme)
                                } else if kind.starts_with("gitgraph") {
                                    draw_git(ui, code, theme)
                                } else if kind.starts_with("timeline") {
                                    draw_timeline(ui, code, theme)
                                } else if kind.starts_with("mindmap") {
                                    draw_mindmap(ui, code, theme)
                                } else if kind.starts_with("quadrantchart") {
                                    draw_quadrant(ui, code, theme)
                                } else if kind.starts_with("xychart") {
                                    draw_xychart(ui, code, theme)
                                } else if kind.starts_with("radar") {
                                    draw_radar(ui, code, theme)
                                } else if kind.starts_with("sankey") {
                                    draw_sankey(ui, code, theme)
                                } else if kind.starts_with("treemap") {
                                    draw_treemap(ui, code, theme)
                                } else if kind.starts_with("c4") {
                                    draw_c4(ui, code, theme)
                                } else if kind.starts_with("block") {
                                    draw_block(ui, code, theme)
                                } else if kind.starts_with("requirement") {
                                    draw_requirement(ui, code, theme)
                                } else if kind.starts_with("packet") {
                                    draw_packet(ui, code, theme)
                                } else if kind.starts_with("kanban") {
                                    draw_kanban(ui, code, theme)
                                } else if kind.starts_with("swimlane") {
                                    draw_swimlane(ui, code, theme)
                                } else if kind.starts_with("cynefin") {
                                    draw_cynefin(ui, code, theme)
                                } else if kind.starts_with("railroad") {
                                    draw_railroad(ui, code, theme)
                                } else {
                                    ui.vertical(|ui| {
                                        ui.set_min_width(env.max_width - 24.0);
                                        for line in code.lines() {
                                            ui.label(
                                                egui::RichText::new(line)
                                                    .monospace()
                                                    .small()
                                                    .color(theme.dim),
                                            );
                                        }
                                    })
                                    .response
                                }
                            })
                            .inner;
                        if openness < 1.0 {
                            ui.scope(|ui| {
                                ui.set_opacity(1.0 - openness);
                                ui.label(
                                    egui::RichText::new(format!("已折叠 {lines} 行代码"))
                                        .size(theme.body * 0.9)
                                        .italics()
                                        .color(theme.dim),
                                );
                            });
                        }
                        body_resp
                    }
                });
                if copied {
                    env.ctx.copy_text(code.clone());
                    env.actions.push(Action::Notify("复制成功".to_string()));
                }
                if let Some(l) = new_lang {
                    env.actions.push(Action::SetLang(idx, l));
                }
                // 只用图表内容区域做块定位，避免覆盖层挡住右上角的折叠/复制按钮。
                // 矩形扩到整个面板，跨块拖选时整块叠一层选区色。
                let content = r.inner.interact(Sense::click());
                content.union(r.response)
            }
            Block::FrontMatter { yaml, .. } => {
                // 与普通代码块同款代码区（yaml 高亮），但**不画功能栏与分隔线**：
                // 文档顶部那一长条灰栏很抢眼，而 Front Matter 基本没人想折叠或复制它。
                // 折叠 / 复制 / 删除改成指针进面板时浮现在右上角，功能一个不少。
                code_panel(ui, theme, env, idx, "yaml", yaml, 0, false, true)
            }
            Block::Footnote { id, content, .. } => {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("[^{}]", id))
                            .small()
                            .color(theme.accent),
                    );
                    text_response(
                        ui,
                        content,
                        theme,
                        theme.body * 0.9,
                        theme.quote,
                        false,
                        env,
                        idx,
                    )
                })
                .inner
            }
            Block::Toc { .. } => {
                let r = panel_frame(theme).show(ui, |ui| {
                    // 盒子宽度 ≈ 最长标题的两倍：目录条目长短不一时，
                    // 收缩到最宽那条的宽度会让整个框看起来又窄又小。
                    let longest_w = env
                        .outline
                        .iter()
                        .map(|item| {
                            ui.fonts_mut(|f| {
                                f.layout_no_wrap(
                                    format!("• {}", item.title),
                                    egui::FontId::proportional(theme.body),
                                    theme.link,
                                )
                                .rect
                                .width()
                            })
                        })
                        .fold(0.0_f32, f32::max);
                    let box_w = (longest_w * 2.0).clamp(300.0, env.max_width);
                    ui.set_min_width(box_w);
                    ui.label(egui::RichText::new("目录").strong().color(theme.heading));
                    for item in env.outline {
                        let indent = ((item.level.saturating_sub(1)) as f32) * 14.0;
                        let resp = ui
                            .horizontal(|ui| {
                                ui.add_space(indent);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(format!("• {}", item.title))
                                            .color(theme.link),
                                    )
                                    .sense(Sense::click()),
                                )
                            })
                            .inner;
                        if resp.clicked() {
                            env.actions.push(Action::Jump(item.block));
                        }
                    }
                });
                r.response.interact(Sense::click())
            }
            Block::Blank { .. } => {
                let (_, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 9.0), Sense::click());
                resp
            }
        }
    });
    inner.inner
}

/// 指针命中哪一行哪一列的单元格（表格工具栏的行列基准）。
pub(super) fn hit_cell(row_rects: &[Vec<Rect>], p: Pos2) -> Option<(usize, usize)> {
    for (ri, row) in row_rects.iter().enumerate() {
        for (ci, r) in row.iter().enumerate() {
            if r.contains(p) {
                return Some((ri, ci));
            }
        }
    }
    None
}

/// 本帧的表格单元格选区：编辑器在块循环之前挂进来，渲染层拿它给选中的格子叠底色
/// （只有它知道每格的矩形）。一个键装整份选区，按下标自己比对，避免逐块留键。
pub(crate) const TABLE_SEL_KEY: &str = "markd_table_sel_frame";

/// 上面那个键对应的 `Id`（编辑器与渲染层必须用同一个）。
pub(crate) fn table_sel_id() -> egui::Id {
    #[allow(deprecated)]
    {
        egui::Id::unique(TABLE_SEL_KEY)
    }
}

/// 指针落在哪一格：缝隙里（行列之间的间距）按最近的行、最近的列归位。
fn table_cell_near(row_rects: &[Vec<Rect>], p: Pos2) -> (usize, usize) {
    /// 到一维区间 [lo, hi] 的距离（落在区间内为 0）。
    fn axis_dist(v: f32, lo: f32, hi: f32) -> f32 {
        if v < lo {
            lo - v
        } else if v > hi {
            v - hi
        } else {
            0.0
        }
    }
    let mut rows: Vec<(usize, f32)> = Vec::new();
    for (ri, row) in row_rects.iter().enumerate() {
        let band = row.iter().fold(None::<(f32, f32)>, |acc, r| {
            Some(match acc {
                Some((lo, hi)) => (lo.min(r.top()), hi.max(r.bottom())),
                None => (r.top(), r.bottom()),
            })
        });
        if let Some((lo, hi)) = band {
            rows.push((ri, axis_dist(p.y, lo, hi)));
        }
    }
    let ri = rows
        .iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| *i)
        .unwrap_or(0);
    let cols = row_rects.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut cs: Vec<(usize, f32)> = Vec::new();
    for ci in 0..cols {
        let mut band: Option<(f32, f32)> = None;
        for row in row_rects {
            if let Some(r) = row.get(ci) {
                band = Some(match band {
                    Some((lo, hi)) => (lo.min(r.left()), hi.max(r.right())),
                    None => (r.left(), r.right()),
                });
            }
        }
        if let Some((lo, hi)) = band {
            cs.push((ci, axis_dist(p.x, lo, hi)));
        }
    }
    let ci = cs
        .iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| *i)
        .unwrap_or(0);
    (ri, ci)
}

/// 表格单元格矩形选区的两端格子：`((起行, 起列), (止行, 止列))`，行列含表头行。
pub type TableCellRect = ((usize, usize), (usize, usize));

/// 表格内拖选的判定（渲染态与编辑态共用同一套规则）：
/// - 按下点不在单元格上，或还没拖起来 → `None`：本帧不表态，按「整块」跨块选区走；
/// - 按下点与指针都在表内 → `Some(Some((起格, 止格)))`：只选中经过的那几格；
/// - 指针拖出表格边界 → `Some(None)`：整张表交给跨块选区，好把它和上下文一起选走。
pub fn table_drag_range(
    row_rects: &[Vec<Rect>],
    press: Option<Pos2>,
    pointer: Option<Pos2>,
    dragging: bool,
) -> Option<Option<TableCellRect>> {
    let from = hit_cell(row_rects, press?)?;
    if !dragging {
        return None;
    }
    let p = pointer?;
    let bounds = row_rects
        .iter()
        .flatten()
        .fold(None::<Rect>, |acc, r| Some(acc.map_or(*r, |a| a.union(*r))))?;
    if !bounds.expand(2.0).contains(p) {
        return Some(None);
    }
    Some(Some((from, table_cell_near(row_rects, p))))
}

/// 选中格子的底色：叠在单元格内容之上（半透明，文字依然清晰），
/// 与代码块/图表那套选区叠加层同一口径。
pub(crate) fn paint_table_sel(
    ui: &egui::Ui,
    theme: &Theme,
    row_rects: &[Vec<Rect>],
    sel: TableCellRect,
) {
    let ((r0, c0), (r1, c1)) = sel;
    let color = selection_overlay(theme);
    for (ri, row) in row_rects.iter().enumerate() {
        if ri < r0 || ri > r1 {
            continue;
        }
        for (ci, r) in row.iter().enumerate() {
            if ci >= c0 && ci <= c1 {
                ui.painter().rect_filled(*r, 0.0, color);
            }
        }
    }
}

// ---------------- 图表主题配色与通用「盒子+连线」布局 ----------------
