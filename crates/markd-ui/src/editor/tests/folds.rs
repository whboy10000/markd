use super::*;

/// 折叠状态往返：source 键不随下标变化，折叠块之前插入块不会导致错位。
#[test]
fn fold_keys_indices_roundtrip() {
    let src = "# 标题\n\n```rust\nfn a() {}\n```\n\n正文\n\n```rust\nfn b() {}\n```";
    let doc = Document::new(src, None);
    let code_idx: Vec<usize> = doc
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b, Block::Code { .. }))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(code_idx.len(), 2, "应有且仅有两个代码块: {code_idx:?}");

    // 折叠第二个代码块 → 得到它的 source 键。
    let mut folded = HashSet::new();
    folded.insert(code_idx[1]);
    let keys = fold_keys(&doc, &folded);
    assert_eq!(keys.len(), 1, "只有一个折叠代码块: {keys:?}");
    assert!(
        keys[0].contains("fn b()"),
        "键应指向 fn b 的代码块: {keys:?}"
    );

    // 在该代码块之前插入一个段落，其下标后移一位，但 source 键不变，
    // 还原时仍应命中 fn b 的代码块（下标已变）。
    let mut doc2 = Document::new(src, None);
    doc2.insert_block_after(code_idx[1] - 1, "插入的段落");
    let restored = fold_indices(&doc2, &keys);
    assert_eq!(restored.len(), 1, "应恰好还原一个折叠块: {restored:?}");
    let ri = *restored.iter().next().unwrap();
    assert!(
        doc2.blocks[ri].source().contains("fn b()"),
        "source 匹配应抵消下标偏移，实际命中: {:?}",
        doc2.blocks[ri].source()
    );

    // 空键集合应还原成空集。
    assert!(fold_indices(&doc2, &[]).is_empty());
}

/// 图表块编辑往返：编辑缓冲只含图形代码（无围栏），改完能补回围栏写回。
/// 插入流程图后必须还能再次编辑——回归「点击图形无法重新激活块」的修复。
#[test]
fn diagram_edit_roundtrip() {
    let src = "```mermaid\nflowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]\n```";
    let doc = Document::new(src, None);
    let b = &doc.blocks[0];
    assert!(
        matches!(b, Block::Diagram { .. }),
        "应解析出图表块，实际是别的块类型"
    );
    let ek = EditKind::of(b);
    assert!(
        matches!(ek, EditKind::Diagram(_)),
        "图表块应识别为 Diagram 编辑类型"
    );
    assert_eq!(
        block_buffer(b),
        "flowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]",
        "编辑缓冲不应包含 ``` 围栏"
    );
    // 改代码后重建源码：补回围栏，图形随之更新
    let rebuilt = rebuild_source(b, "flowchart TD\n  X[起点] --> Y[终点]");
    assert_eq!(
        rebuilt,
        "```mermaid\nflowchart TD\n  X[起点] --> Y[终点]\n```"
    );
}
