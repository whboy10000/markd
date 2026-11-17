use super::*;

/// 清除格式：标题 / 引用 / 列表 / 任务列表都变回普通段落，行内加粗也去掉。
#[test]
fn clear_format_turns_blocks_into_paragraph() {
    for (src, want) in [
        ("## **粗体**标题", "粗体标题"),
        ("> 引用文字", "引用文字"),
        ("- 列表项", "列表项"),
        ("1. 有序项", "有序项"),
        ("- [ ] 待办", "待办"),
        ("正文里有 ~~删除线~~ 和 `代码`", "正文里有 删除线 和 代码"),
    ] {
        let mut doc = Document::new(src, None);
        let mut st = EditorState::default();
        assert!(clear_format(&mut doc, &mut st), "{src}");
        assert_eq!(doc.source(), want);
    }
}

/// 代码块擦成段落时保留代码正文，不带围栏；表格等结构块不做处理。
#[test]
fn clear_format_handles_code_and_refuses_structures() {
    let mut doc = Document::new(
        "\u{60}\u{60}\u{60}rust\nfn main() {}\n\u{60}\u{60}\u{60}",
        None,
    );
    let mut st = EditorState::default();
    assert!(clear_format(&mut doc, &mut st));
    assert_eq!(doc.source(), "fn main() {}");

    let mut table = Document::new("| a | b |\n| - | - |\n| 1 | 2 |", None);
    assert!(!clear_format(&mut table, &mut st));
}

/// 点工具条按钮时编辑框已经失焦、`sel` 被清空，要用 last_sel 记住的选区
/// 才能把行内代码/加粗作用在刚选中的文字上。
#[test]
fn wrap_uses_last_selection_after_focus_loss() {
    let mut doc = Document::new("选中这段文字", None);
    let mut st = EditorState::default();
    st.sel = None;
    st.last_sel = Some((0, 0, 2));
    wrap(&mut doc, &mut st, "`", "`", true);
    assert_eq!(doc.source(), "`选中`这段文字");
    assert!(st.last_sel.is_none(), "用过的选区要清掉，避免重复包裹");
}

/// 加粗（工具条按钮）：没有选区时，整行（光标所在块）整体加粗；
/// 再点一次取消——整行已经带着 ** 时，点击就是去掉标记。
#[test]
fn bold_whole_line_toggles() {
    let mut doc = Document::new("正文一段", None);
    let mut st = EditorState::default();
    wrap(&mut doc, &mut st, "**", "**", true);
    assert_eq!(doc.source(), "**正文一段**");
    wrap(&mut doc, &mut st, "**", "**", true);
    assert_eq!(doc.source(), "正文一段");
}

/// 加粗作用在标题块时，偏移按「去掉 # 的可编辑文本」计算，# 不能被裹进标记里。
#[test]
fn bold_on_heading_keeps_marker_outside() {
    let mut doc = Document::new("# 标题内容", None);
    let mut st = EditorState::default();
    wrap(&mut doc, &mut st, "**", "**", true);
    assert_eq!(doc.source(), "# **标题内容**");
}

/// 加标题必须保留正文里的数字编号：如「1.2 核心架构」加 H1 后仍是
/// 「# 1.2 核心架构」。「1.2」是章节号不是有序列表标记，不能被剥掉。
#[test]
fn heading_keeps_section_numbers() {
    for (src, prefix, want) in [
        ("1.2 核心架构", "# ", "# 1.2 核心架构"),
        ("3.1.4 测试章节", "## ", "## 3.1.4 测试章节"),
        ("2024 年度总结", "### ", "### 2024 年度总结"),
    ] {
        let mut doc = Document::new(src, None);
        let mut st = EditorState::default();
        apply_prefix(&mut doc, &mut st, prefix);
        assert_eq!(doc.source(), want, "加标题不应剥掉正文数字：{src}");
    }
}

/// 真正的有序列表标记（数字 + . 或 ) + 空格）仍要被剥掉，换成标题。
#[test]
fn heading_strips_real_ordered_list_marker() {
    for (src, prefix, want) in [
        ("1. 第一项", "# ", "# 第一项"),
        ("12) 第十二项", "## ", "## 第十二项"),
        ("- 列表项", "# ", "# 列表项"),
    ] {
        let mut doc = Document::new(src, None);
        let mut st = EditorState::default();
        apply_prefix(&mut doc, &mut st, prefix);
        assert_eq!(doc.source(), want, "真实块级标记应被剥掉：{src}");
    }
}

/// 鼠标拖选多个段落后再点列表：选区覆盖的每一段都要转成列表，
/// 空行保持原样；有序列表按选区顺序依次编号。
#[test]
fn apply_prefix_to_cross_selection() {
    let mut doc = Document::new("甲\n\n乙\n\n丙", None);
    let mut st = EditorState {
        cross_sel: Some((0, 0, 4, 1)),
        ..Default::default()
    };
    apply_prefix(&mut doc, &mut st, "- ");
    assert_eq!(doc.blocks[0].source(), "- 甲");
    assert!(
        matches!(doc.blocks[1], Block::Blank { .. }),
        "空行不应变成空列表项"
    );
    assert_eq!(doc.blocks[2].source(), "- 乙");
    assert_eq!(doc.blocks[4].source(), "- 丙");
    assert!(st.cross_sel.is_none(), "转换后选区应清除");
    assert_eq!(st.active, 0, "光标应落到第一段");

    // 反向拖选同样有效，有序列表要依次编号。
    let mut doc = Document::new("甲\n\n乙", None);
    let mut st = EditorState {
        cross_sel: Some((2, 0, 0, 1)),
        ..Default::default()
    };
    apply_prefix(&mut doc, &mut st, "1. ");
    assert_eq!(doc.blocks[0].source(), "1. 甲");
    assert_eq!(doc.blocks[2].source(), "2. 乙");
}

/// 链接保持旧行为：没有选区时只在光标处插入一对空标记，不裹整行。
#[test]
fn link_inserts_at_caret_without_selection() {
    let mut doc = Document::new("正文一段", None);
    let mut st = EditorState::default();
    st.last_cursor = 2;
    wrap(&mut doc, &mut st, "[", "](https://)", false);
    assert_eq!(doc.source(), "正文[](https://)一段");
}

/// 表格结构操作：插删行列、对齐、表头保护。
#[test]
fn table_op_grid() {
    use crate::ast::Align;
    let mut grid = vec![
        vec!["A".to_string(), "B".to_string()],
        vec!["1".to_string(), "2".to_string()],
    ];
    let mut align = vec![Align::Left, Align::Left];
    assert!(table_op_on_grid(
        &mut grid,
        &mut align,
        TableOp::InsertRow(0)
    ));
    assert_eq!(grid.len(), 3, "表头下插入空行");
    assert_eq!(grid[1], vec![String::new(), String::new()]);
    assert!(table_op_on_grid(
        &mut grid,
        &mut align,
        TableOp::DeleteRow(1)
    ));
    assert_eq!(grid.len(), 2);
    assert!(table_op_on_grid(
        &mut grid,
        &mut align,
        TableOp::InsertCol(0)
    ));
    assert_eq!(grid[0].len(), 3, "第一列右侧插入空列");
    assert!(table_op_on_grid(
        &mut grid,
        &mut align,
        TableOp::DeleteCol(0)
    ));
    assert_eq!(grid[0].len(), 2);
    assert!(table_op_on_grid(
        &mut grid,
        &mut align,
        TableOp::SetAlign(Align::Center)
    ));
    assert_eq!(align, vec![Align::Center, Align::Center]);
    // 表头行不可删；只剩一列时删列无效
    assert!(!table_op_on_grid(
        &mut grid,
        &mut align,
        TableOp::DeleteRow(0)
    ));
    assert!(table_op_on_grid(
        &mut grid,
        &mut align,
        TableOp::DeleteCol(1)
    ));
    assert_eq!(grid[0].len(), 1, "删掉最后一列后仍保留一列");
    assert!(!table_op_on_grid(
        &mut grid,
        &mut align,
        TableOp::DeleteCol(0)
    ));
    assert_eq!(grid.len(), 2);
}

/// 列表项的所见即所得编辑：编辑缓冲只含正文（不含 "- [ ]" 等行首标记），
/// 写回时由 rebuild 补回标记，源码无损往返。
#[test]
fn list_item_edit_roundtrip() {
    for src in [
        "- [ ] 未完成任务",
        "- [x] 已完成任务",
        "* 普通列表项",
        "1. 编号一项",
        "  - 缩进列表项",
    ] {
        let doc = Document::new(src, None);
        match doc.blocks.first() {
            Some(b) => {
                assert!(
                    matches!(EditKind::of(b), EditKind::List { .. }),
                    "{src} 应识别为 List 编辑类型"
                );
                let buf = block_buffer(b);
                assert!(
                    !buf.contains("- [") && !buf.starts_with('-') && !buf.starts_with('1'),
                    "编辑缓冲不应含行首标记: {src} -> {buf:?}"
                );
                assert_eq!(rebuild_source(b, &buf), src, "往返必须无损: {src}");
            }
            other => panic!("{src} 应解析为列表块，实际 {other:?}"),
        }
    }
}

/// 选中文字后打开链接弹窗：确认时用记住的选区写回，选中文字必须保留。
#[test]
fn insert_link_keeps_selected_text() {
    let mut doc = Document::new("看这段文字", None);
    let mut st = EditorState::default();
    assert!(insert_link(
        &mut doc,
        &mut st,
        Some((0, 1, 3)),
        "这段",
        "https://example.com",
    ));
    assert_eq!(doc.source(), "看[这段](https://example.com)文字");
    assert_eq!(st.focus_req, Some(0));
}

/// 没有选区时用 URL 当显示文字，链接可见、可点。
#[test]
fn insert_link_without_selection_uses_url_label() {
    let mut doc = Document::new("正文", None);
    let mut st = EditorState::default();
    st.last_cursor = 2;
    assert!(insert_link(&mut doc, &mut st, None, "", "https://e.com"));
    assert_eq!(doc.source(), "正文[https://e.com](https://e.com)");
}

/// selection_text 在失焦后会回退到 last_sel，供链接弹窗取选中文字。
#[test]
fn selection_text_falls_back_to_last_sel() {
    let doc = Document::new("选中这段文字", None);
    let mut st = EditorState::default();
    st.last_sel = Some((0, 2, 4));
    assert_eq!(
        selection_text(&doc, &st),
        Some((0, 2, 4, "这段".to_string()))
    );
}

/// 删除线（工具条按钮）：选中文字后两侧各补一个 ~~，渲染成一条横线。
#[test]
fn strike_wraps_selection() {
    let mut doc = Document::new("普通文字", None);
    let mut st = EditorState {
        sel: Some((0, 2)),
        ..Default::default()
    };
    wrap(&mut doc, &mut st, "~~", "~~", true);
    assert_eq!(doc.source(), "~~普通~~文字");
    assert!(doc.dirty);
}

/// 图床上传完成后：把文档里的本地图片地址替换成公网 URL。
#[test]
fn replace_image_url_rewrites_link() {
    let mut doc = Document::new("文字\n\n![图片](imgs/a.png)", None);
    assert!(replace_image_url(
        &mut doc,
        "imgs/a.png",
        "https://gitee.com/u/r/raw/master/images/a.png"
    ));
    assert!(
        doc.source()
            .contains("https://gitee.com/u/r/raw/master/images/a.png"),
        "{}",
        doc.source()
    );
    // 找不到旧地址时不改文档。
    assert!(!replace_image_url(&mut doc, "imgs/missing.png", "x"));
}
