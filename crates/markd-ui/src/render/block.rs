//! 块级渲染入口：按 AST 块类型分派到行内 / 表格 / 代码块 / 图表 / 图片。

use super::*;

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
                text_response(ui, content, theme, theme.body, color, false, env, idx)
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
            } => {
                let folded = env.folded.contains(&idx);
                // 折叠动画：1=完全展开、0=完全折叠，中间平滑过渡。
                let openness = env.ctx.animate_value_with_time(
                    egui::Id::new(("code_fold", idx)),
                    if folded { 0.0 } else { 1.0 },
                    0.18,
                );
                let lines = code.lines().count();
                let mut copied = false;
                let mut new_lang: Option<String> = None;
                // 功能栏矩形：下面的可点击区域从功能栏下缘开始，
                // 避免整块可点后把复制/删除/折叠按钮的点击吞掉。
                let mut header_rect = Rect::NOTHING;
                // 引用内的代码块：左侧留出缩进 + 竖线位置，代码面板整体右移，
                // 竖线按面板实际高度画，和引用文字连成一条。
                let hr = ui.horizontal(|ui| {
                    if *quote > 0 {
                        ui.add_space((*quote).min(4) as f32 * 8.0 + 11.0);
                    }
                    // 外层横向布局会传染给面板内容：显式回到纵向，否则功能栏
                    // 和代码会被并排放置、代码列被挤成一条很窄的竖条。
                    let r = ui
                        .vertical(|ui| {
                            panel_frame(theme).show(ui, |ui| {
                                // 功能栏：左语言 + 右操作按钮（code_header 背景色块）
                                header_rect = code_header_frame(theme)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            if let Some(l) =
                                                language_combo(ui, ("code_lang", idx), lang)
                                            {
                                                new_lang = Some(l);
                                            }
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    // 最右：折叠/展开；其次删除（弹确认窗）；再复制
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
                                // 代码区：完全折叠只留提示；过渡中代码淡出、提示淡入。
                                if openness <= 0.0 {
                                    ui.label(
                                        egui::RichText::new(format!("已折叠 {lines} 行代码"))
                                            .size(theme.body * 0.9)
                                            .italics()
                                            .color(theme.dim),
                                    )
                                } else {
                                    let code_resp = ui
                                        .scope(|ui| {
                                            ui.set_opacity(openness);
                                            wrapped_label(
                                                ui,
                                                code_job(code, lang, theme),
                                                env.max_width,
                                                Sense::hover(),
                                            )
                                        })
                                        .inner;
                                    if openness < 1.0 {
                                        ui.scope(|ui| {
                                            ui.set_opacity(1.0 - openness);
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "已折叠 {lines} 行代码"
                                                ))
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
                    if *quote > 0 {
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
                if copied {
                    env.ctx.copy_text(code.clone());
                    env.actions.push(Action::Notify("复制成功".to_string()));
                }
                if let Some(l) = new_lang {
                    env.actions.push(Action::SetLang(idx, l));
                }
                // 代码区（功能栏以下，含空代码块的留白）整块可点击：
                // 空代码块的 galley 又窄又矮，只把文字 label 当点击区时，
                // 鼠标离开后再点代码区空白处命不中，看起来像「代码块不能编辑」。
                // 面板矩形仍参与 union，保证跨块拖选时为整块叠选区色。
                let zone = Rect::from_min_max(
                    Pos2::new(r.response.rect.left(), header_rect.bottom()),
                    r.response.rect.right_bottom(),
                );
                let content = ui.interact(zone, ui.id().with(("code_click", idx)), Sense::click());
                content.union(r.response)
            }
            Block::CodeGroup { items, .. } => {
                let folded = env.folded.contains(&idx);
                let openness = env.ctx.animate_value_with_time(
                    egui::Id::new(("code_group_fold", idx)),
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
                    ui.id().with(("code_group_click", idx)),
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
                let cols = head.len().max(1);
                let avail = (ui.available_width() - 12.0).max(120.0);
                let gap = ui.spacing().item_spacing.x * cols.saturating_sub(1) as f32;
                let col_w = ((avail - gap) / cols as f32).max(56.0);
                // 记录每个单元格的矩形，用于画「单线」分隔（不是每格一个框）
                let mut row_rects: Vec<Vec<egui::Rect>> = Vec::new();
                let mut copied = false;
                let mut op: Option<crate::editor::TableOp> = None;
                let cell_key = egui::Id::new(("table_cell_ref", idx));
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
                if let Some(op) = op {
                    env.actions.push(Action::TableOp(idx, op));
                }
                if copied {
                    env.ctx.copy_text(source.clone());
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
            Block::Math { tex, .. } => {
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
                    egui::Id::new(("diagram_fold", idx)),
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
                let r = panel_frame(theme).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("YAML Front Matter")
                            .small()
                            .color(theme.dim),
                    );
                    for line in yaml.lines() {
                        ui.label(
                            egui::RichText::new(line)
                                .monospace()
                                .small()
                                .color(theme.quote),
                        );
                    }
                });
                r.response.interact(Sense::click())
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

// ---------------- 图表主题配色与通用「盒子+连线」布局 ----------------
