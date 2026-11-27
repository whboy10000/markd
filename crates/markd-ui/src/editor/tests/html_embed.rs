use super::*;

/// 内嵌 HTML 在编辑态必须露出原始源码：div 与含 span 的段落都走 `EditKind::Source`，
/// 缓冲就是带标签的整段文字，用户能直接改 style 属性；写回也原样，不会被降级成纯文本。
#[test]
fn html_blocks_edit_as_verbatim_source() {
    for src in [
        "<div style=\"background:#eee; padding:10px;\">灰色背景提示框</div>",
        "<div style=\"background:#eee;\"><span style=\"color:red\">红</span></div>",
        "前<span style=\"color:red; font-size:18px\">红色大号</span>后",
    ] {
        let doc = Document::new(src, None);
        let b = doc.blocks.first().expect("首块");
        assert!(
            block_buffer(b).contains("<span") || block_buffer(b).contains("<div"),
            "编辑态缓冲要露出标签，实际 {:?}",
            block_buffer(b)
        );
        assert_eq!(block_buffer(b), src, "缓冲应等于原始源码");
        assert_eq!(
            rebuild_source(b, &block_buffer(b)),
            src,
            "不改一字时写回应恒等"
        );
    }
}

/// 编辑态把 `#eee` 改成 `#ffe`：写回后的源码仍是合法 div，重新解析还是 div 块，
/// 不会掉成普通段落（那样框和背景色就没了）。
#[test]
fn html_div_survives_editing_roundtrip() {
    let src = "<div style=\"background:#eee;\">提示框</div>";
    let doc = Document::new(src, None);
    let b = &doc.blocks[0];
    let edited = block_buffer(b).replace("#eee", "#ffe");
    assert_ne!(edited, block_buffer(b), "用例要有真实改动");
    let rebuilt = rebuild_source(b, &edited);
    assert_eq!(rebuilt, "<div style=\"background:#ffe;\">提示框</div>");

    let again = Document::new(&rebuilt, None);
    match &again.blocks[0] {
        Block::Div { tag, content, .. } => {
            assert!(tag.contains("#ffe"), "标签要带着改过的样式: {tag}");
            assert_eq!(content.len(), 1, "正文仍是行内节点");
        }
        other => panic!("改完底色应仍是 div 块，实际 {other:?}"),
    }
    assert_eq!(block_buffer(&again.blocks[0]), rebuilt);
}

/// 跨行 div 要用多行编辑框：`is_multiline` 漏掉 Div 时，编辑器给的是单行框，
/// 块内的回车会把 div 劈成两段（源码里的 `</div>` 掉到下一个块）。
#[test]
fn multiline_div_uses_multiline_editor() {
    let src = "<div style=\"background:#eee;\">\n第一行\n第二行\n</div>";
    let doc = Document::new(src, None);
    let b = &doc.blocks[0];
    assert!(matches!(b, Block::Div { .. }), "{b:?}");
    assert!(b.source().contains('\n'), "div 源码本身是多行");
    assert!(b.is_multiline(), "跨行 div 必须走多行编辑框");
}

/// div 块也接受行内片段：`accepts_inline_tag` 漏掉 Div 时，点进提示框再插 span
/// 会直接返回 false，用户看到的现象是「按钮没反应」。
#[test]
fn span_inserts_into_div_block_at_caret() {
    let src = "<div style=\"background:#eee;\">提示框</div>";
    let mut doc = Document::new(src, None);
    // 编辑态缓冲就是整行源码，光标坐标也按源码算（含标签本身），这里落在「提」之后。
    let at = "<div style=\"background:#eee;\">提".chars().count();
    let mut state = EditorState {
        active: 0,
        last_cursor: at,
        ..Default::default()
    };
    assert_eq!(block_buffer(&doc.blocks[0]), src, "div 的编辑缓冲应是源码");
    let tag = "<span style=\"color:red\">红</span>";
    assert!(
        insert_inline_tag(&mut doc, &mut state, tag),
        "div 块应接受行内片段"
    );
    assert_eq!(
        doc.blocks[0].source(),
        "<div style=\"background:#eee;\">提<span style=\"color:red\">红</span>示框</div>",
        "标签要落在光标处"
    );
    assert_eq!(
        state.caret_req,
        Some((0, at + tag.chars().count())),
        "光标应落到片段之后"
    );
}

/// 表格格子：请求必须带上标签原文（只记坐标的话，视图帧只会插 `<br>`），
/// 并在同一帧写进草稿与源码，否则下一次按键会用旧草稿把片段盖掉。
#[test]
fn table_inline_request_carries_the_tag() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut frame = |doc: &mut Document, state: &mut EditorState| {
        frame_warm(
            &ctx,
            800.0,
            600.0,
            doc,
            state,
            &theme,
            &mut images,
            &[],
            &mut actions,
        );
    };
    let mut doc = Document::new("| A | B |\n| --- | --- |\n| 甲乙 | 丙 |", None);
    let mut state = EditorState::default();
    // 先跑两帧建立表格草稿，单元格的编辑目标才是草稿而不是源码
    frame(&mut doc, &mut state);
    frame(&mut doc, &mut state);
    state.table_cell = Some((0, 1, 0, 1));
    let tag = "<span style=\"color:red\">红</span>";
    assert!(
        insert_inline_tag(&mut doc, &mut state, tag),
        "表格应接受片段请求"
    );
    assert_eq!(
        state.table_inline_req,
        Some(((0, 1, 0, 1), tag.to_string())),
        "请求要带标签原文与落点（只记坐标的话帧内只会插 <br>）"
    );
    frame(&mut doc, &mut state);
    assert_eq!(
        state.table_inline_req, None,
        "请求必须一帧内用完，否则会重复插入"
    );
    assert!(
        doc.source().contains(&format!("甲{tag}乙")),
        "标签要写进那一格的源码：{:?}",
        doc.source()
    );
    assert_eq!(
        state.table_draft.as_ref().expect("草稿还在").1[1][0],
        format!("甲{tag}乙"),
        "草稿与源码要同步"
    );
}
