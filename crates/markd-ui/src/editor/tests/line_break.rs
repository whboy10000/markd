use super::*;

/// 「换行」按钮：在光标处插入 `<br>`，段落、标题、引用、列表项都要把块级标记留在原处
/// （和加粗一样走 block_buffer + rebuild_source，否则 `#` / `- ` 会被一起写进正文）。
#[test]
fn line_break_inserts_tag_at_caret() {
    let cases = [
        ("第一段第二段", 3, "第一段<br>第二段"),
        ("# 标题文字", 2, "# 标题<br>文字"),
        ("> 引用正文", 0, "> <br>引用正文"),
        ("- 列表项", 2, "- 列表<br>项"),
        ("1. 有序项", 0, "1. <br>有序项"),
    ];
    for (src, caret, want) in cases {
        let mut doc = Document::new(src, None);
        let mut st = EditorState {
            last_cursor: caret,
            ..Default::default()
        };
        assert!(insert_line_break(&mut doc, &mut st), "{src}");
        assert_eq!(doc.source(), want, "{src} 光标 {caret}");
        // 光标要落到标签之后，接着打字不会打进 <br> 中间
        assert_eq!(
            st.caret_req,
            Some((0, caret + LINE_BREAK_TAG.chars().count())),
            "{src} 应要求把光标收敛到标签之后"
        );
    }
}

/// 点工具条按钮时编辑框已失焦：有选区要插在选区起点，并且用完即清，
/// 否则下一次点击会把同一段选区再吃一遍。
#[test]
fn line_break_uses_selection_and_clears_it() {
    let mut doc = Document::new("选中这段文字", None);
    let mut st = EditorState {
        sel: Some((2, 4)),
        ..Default::default()
    };
    assert!(insert_line_break(&mut doc, &mut st));
    assert_eq!(doc.source(), "选中<br>这段文字");
    assert_eq!(st.sel, None);
    assert_eq!(st.last_sel, None);

    // 失焦后 sel 被清空，靠 last_sel 记住的选区起点
    let mut doc2 = Document::new("选中这段文字", None);
    let mut st2 = EditorState {
        last_sel: Some((0, 4, 6)),
        ..Default::default()
    };
    assert!(insert_line_break(&mut doc2, &mut st2));
    assert_eq!(doc2.source(), "选中这段<br>文字");
}

/// 连点两次换行 = 两个标签，不会互相嵌套或吃掉前一个。
#[test]
fn line_break_twice_appends() {
    let mut doc = Document::new("甲乙", None);
    let mut st = EditorState {
        last_cursor: 1,
        ..Default::default()
    };
    assert!(insert_line_break(&mut doc, &mut st));
    assert!(insert_line_break(&mut doc, &mut st));
    assert_eq!(doc.source(), "甲<br><br>乙");
}

/// 插不进换行的块：代码 / 图表 / 公式 / 分隔线 / 目录 / Front Matter ——
/// 标签在这些块里只是字面字符，写进去反而毁掉块语义。必须返回 false 且不动文档。
#[test]
fn line_break_refuses_non_text_blocks() {
    for src in [
        "```rust\nfn main() {}\n```",
        "```mermaid\ngraph LR\nA-->B\n```",
        "$$\nE = mc^2\n$$",
        "---",
        "[TOC]",
        "---\ntitle: T\n---",
    ] {
        let mut doc = Document::new(src, None);
        let before = doc.source();
        let mut st = EditorState {
            last_cursor: 0,
            ..Default::default()
        };
        assert!(!insert_line_break(&mut doc, &mut st), "{src} 不该插进去");
        assert_eq!(doc.source(), before, "{src} 不该被改动");
    }
}

/// 换行要进撤销栈：插完撤销应回到原样。
#[test]
fn line_break_is_undoable() {
    let mut doc = Document::new("正文", None);
    let mut st = EditorState {
        last_cursor: 2,
        ..Default::default()
    };
    assert!(insert_line_break(&mut doc, &mut st));
    assert_eq!(doc.source(), "正文<br>");
    assert!(doc.undo());
    assert_eq!(doc.source(), "正文");
}

/// 用真实帧跑编辑器：返回绘制出的所有文字（galley 文本）。
fn painted_texts(
    ctx: &egui::Context,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
) -> Vec<String> {
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let c = ui.ctx().clone();
            let _ = show_editor(
                ui,
                doc,
                state,
                theme,
                &c,
                &mut images,
                None,
                &[],
                false,
                false,
                false,
                false,
                1,
                &mut actions,
            );
        });
    });
    out.textures_delta.clear();
    out.shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// 两态一致（渲染态）：非活动块的 `<br>` 真的断行——galley 里出现换行符，
/// 而不是把标签当文字画出来。
#[test]
fn rendered_state_breaks_the_line() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();
    let mut doc = Document::new("上文\n\n第一行<br>第二行", None);
    // 活动块是「上文」，第二段走渲染态
    let mut state = EditorState {
        active: 0,
        ..Default::default()
    };
    let texts = painted_texts(&ctx, &mut doc, &mut state, &theme);
    let broken = texts.iter().find(|t| t.contains("第一行"));
    let broken = broken.expect("应画出含「第一行」的段落");
    assert!(
        broken.contains('\n'),
        "渲染态应把 <br> 断成两行，实际 {broken:?}"
    );
    assert!(
        !broken.contains("<br>"),
        "渲染态不该把标签当文字画出来：{broken:?}"
    );
}

/// 两态一致（编辑态＝部分源码模式）：点进这个块，标签要原样露出来可编辑。
#[test]
fn edit_state_reveals_the_tag() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();
    let mut doc = Document::new("第一行<br>第二行", None);
    // 活动块 = 编辑态
    let mut state = EditorState {
        active: 0,
        ..Default::default()
    };
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let c = ui.ctx().clone();
            let _ = show_editor(
                ui,
                &mut doc,
                &mut state,
                &theme,
                &c,
                &mut images,
                None,
                &[],
                false,
                false,
                false,
                false,
                1,
                &mut actions,
            );
        });
    });
    out.textures_delta.clear();
    let texts: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("第一行<br>第二行")),
        "编辑态应把整段源码（含 <br>）画出来，实际 {texts:?}"
    );
}

/// 表格：渲染态单元格内断行 + 编辑态草稿保留标签。
#[test]
fn table_cell_keeps_break_in_both_states() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();

    // 1) 渲染态：表格不是活动块（前面放一段正文当活动块），单元格里应出现换行
    let mut doc = Document::new(
        "前言\n\n| A | B |\n| --- | --- |\n| 第一行<br>第二行 | x |",
        None,
    );
    let mut state = EditorState::default(); // active=0 → 「前言」段落
    let texts = painted_texts(&ctx, &mut doc, &mut state, &theme);
    let cell = texts.iter().find(|t| t.contains("第一行"));
    let cell = cell.expect("应画出单元格文字");
    assert!(
        cell.contains('\n') && !cell.contains("<br>"),
        "渲染态单元格应断行且不露标签，实际 {cell:?}"
    );

    // 2) 编辑态：草稿要留着 <br>，不能被 inline_plain 摊成空格
    let mut doc2 = Document::new("| A | B |\n| --- | --- |\n| 第一行<br>第二行 | x |", None);
    let mut state2 = EditorState::default(); // active=0 → 表格编辑态
    painted_texts(&ctx, &mut doc2, &mut state2, &theme);
    painted_texts(&ctx, &mut doc2, &mut state2, &theme);
    let grid = state2
        .table_draft
        .as_ref()
        .expect("表格应建立草稿")
        .1
        .clone();
    assert_eq!(
        grid[1][0], "第一行<br>第二行",
        "单元格草稿应保留硬换行标签"
    );

    // 3) 表头那一格同样走 inline_cell_source：漏掉表头，点进含换行的表头再写回就丢标签
    let mut doc3 = Document::new("| 头<br>补充 | B |\n| --- | --- |\n| x | y |", None);
    let mut state3 = EditorState::default();
    painted_texts(&ctx, &mut doc3, &mut state3, &theme);
    painted_texts(&ctx, &mut doc3, &mut state3, &theme);
    let grid3 = state3.table_draft.as_ref().expect("表格应建立草稿").1.clone();
    assert_eq!(grid3[0][0], "头<br>补充", "表头草稿也应保留硬换行标签");
}

/// 换行请求（`table_inline_req`）要在同一帧改草稿并写回源码：
/// 只改草稿，下一次按键就会用旧草稿把标签盖掉。
#[test]
fn table_break_request_writes_back_to_source() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();
    let mut doc = Document::new("| A | B |\n| --- | --- |\n| 甲乙 | 丙 |", None);
    let mut state = EditorState::default();
    // 先跑两帧建立草稿
    painted_texts(&ctx, &mut doc, &mut state, &theme);
    painted_texts(&ctx, &mut doc, &mut state, &theme);
    assert!(state.table_draft.is_some(), "应已建立表格草稿");

    // 模拟「光标在第 1 行第 0 列、格内两字之间」时点了换行按钮：
    // 落点必须在光标处，追加到格尾是另一种实现，这里要能分辨。
    state.table_cell = Some((0, 1, 0, 1));
    assert!(
        insert_line_break(&mut doc, &mut state),
        "表格里应接受换行请求"
    );
    assert_eq!(
        state.table_inline_req,
        Some(((0, 1, 0, 1), "<br>".to_string())),
        "应先挂上一次性请求（含标签原文），交给视图帧内消费"
    );
    painted_texts(&ctx, &mut doc, &mut state, &theme);
    assert_eq!(
        state.table_inline_req, None,
        "请求必须一帧内用完，否则会重复插入"
    );
    assert_eq!(
        doc.source(),
        // 写回走 table_source：分隔行按对齐方式规范化成 `:---`（编辑表格本来就会重排源码）
        "| A | B |\n| :--- | :--- |\n| 甲<br>乙 | 丙 |",
        "标签应写回表格源码"
    );
    let grid = state.table_draft.as_ref().expect("草稿还在").1.clone();
    assert_eq!(grid[1][0], "甲<br>乙", "草稿与源码要同步");
    assert_eq!(
        state.table_cell,
        Some((0, 1, 0, 5)),
        "格内光标要落到标签之后"
    );
}

/// 草稿属于别的块时不能认草稿不看活动块：活动块是段落就要往段落里插，
/// 否则「表格建过草稿」这件事会把段落的换行请求偷走，段落一个字都不变。
#[test]
fn line_break_follows_active_block_not_any_draft() {
    let mut doc = Document::new(
        "段落文字\n\n| A | B |\n| --- | --- |\n| 甲 | 乙 |",
        None,
    );
    let mut state = EditorState {
        active: 0,
        table_draft: Some((
            1,
            vec![vec!["A".into(), "B".into()], vec!["甲".into(), "乙".into()]],
            vec![crate::ast::Align::Left, crate::ast::Align::Left],
        )),
        table_cell: Some((1, 1, 0, 1)),
        last_cursor: 2,
        ..Default::default()
    };
    assert!(
        insert_line_break(&mut doc, &mut state),
        "活动块是段落，应当接受换行"
    );
    assert_eq!(state.table_inline_req, None, "不该把请求挂给别的块");
    assert!(
        doc.blocks[0].source().contains("<br>"),
        "标签要写进活动段落：{:?}",
        doc.blocks[0].source()
    );
}

/// 没点进任何单元格（不知道往哪格插）时不能瞎插，也不能改文档。
#[test]
fn table_break_without_focused_cell_is_refused() {
    let mut doc = Document::new("| A | B |\n| --- | --- |\n| 甲 | 乙 |", None);
    let mut state = EditorState {
        table_draft: Some((
            0,
            vec![vec!["A".into(), "B".into()], vec!["甲".into(), "乙".into()]],
            vec![crate::ast::Align::Left, crate::ast::Align::Left],
        )),
        table_cell: None,
        ..Default::default()
    };
    assert!(
        !insert_line_break(&mut doc, &mut state),
        "不知道单元格位置时不该插"
    );
    assert_eq!(state.table_inline_req, None);
    assert_eq!(doc.source(), "| A | B |\n| --- | --- |\n| 甲 | 乙 |");
}

/// 请求记录的块下标必须和当前草稿对得上：删表/换表后留下的旧格子坐标不能乱插。
#[test]
fn stale_table_cell_is_not_used() {
    let mut doc = Document::new("| A | B |\n| --- | --- |\n| 甲 | 乙 |", None);
    // 另一张表留下的坐标
    let mut state = EditorState {
        table_draft: Some((
            0,
            vec![vec!["A".into(), "B".into()], vec!["甲".into(), "乙".into()]],
            vec![crate::ast::Align::Left, crate::ast::Align::Left],
        )),
        table_cell: Some((7, 1, 0, 1)),
        ..Default::default()
    };
    assert!(!insert_line_break(&mut doc, &mut state));
    assert_eq!(state.table_inline_req, None);
}
