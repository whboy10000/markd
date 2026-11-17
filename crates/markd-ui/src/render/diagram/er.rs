//! Mermaid ER 图（erDiagram）。

use super::*;

/// 解析 mermaid ER 图：`USER ||--o{ ORDER : 拥有` 与实体属性块。
pub(crate) fn parse_er(code: &str) -> (Vec<BoxNode>, Vec<BoxEdge>) {
    let mut nodes: Vec<BoxNode> = Vec::new();
    let mut edges: Vec<BoxEdge> = Vec::new();
    let mut cur_members: Option<(String, Vec<String>)> = None;
    let mut ensure = |nodes: &mut Vec<BoxNode>, id: &str| -> usize {
        if let Some(i) = nodes.iter().position(|n| n.id == id) {
            return i;
        }
        nodes.push(BoxNode {
            id: id.to_string(),
            label: id.to_string(),
            members: Vec::new(),
        });
        nodes.len() - 1
    };
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.starts_with("erDiagram") {
            continue;
        }
        if l == "}" {
            if let Some((id, members)) = cur_members.take() {
                if let Some(i) = nodes.iter().position(|n| n.id == id) {
                    nodes[i].members = members;
                }
            }
            continue;
        }
        // 关系行必须先于实体块判断：`USER ||--o{ ORDER : 下单` 里也有 '{'，
        // 会被误当成实体块的开始，整行变成一个节点。
        if l.contains("--") {
            parse_er_relation(l, &mut nodes, &mut edges, &mut ensure);
            continue;
        }
        if l.contains('{') {
            let name = l.trim_end_matches('{').trim();
            if !name.is_empty() {
                ensure(&mut nodes, name);
                cur_members = Some((name.to_string(), Vec::new()));
            }
            continue;
        }
        if cur_members.is_some() {
            if let Some((_, members)) = cur_members.as_mut() {
                members.push(l.to_string());
            }
            continue;
        }
        // （关系解析已上移到 parse_er_relation）
    }
    (nodes, edges)
}

/// ER 关系行：`A ||--o{ B : 标签`。实体名 = "--" 两侧剥掉基数记号
/// （记号字符只有 | o } { 这几种）之后的单词。
pub(crate) fn parse_er_relation(
    l: &str,
    nodes: &mut Vec<BoxNode>,
    edges: &mut Vec<BoxEdge>,
    ensure: &mut dyn FnMut(&mut Vec<BoxNode>, &str) -> usize,
) {
    let Some(i) = l.find("--") else { return };
    let a_part = l[..i].trim();
    let after = &l[i..];
    if after.len() < 6 {
        return;
    }
    let dash_pos = after.find("--").unwrap_or(0);
    let left_mark = &after[..dash_pos];
    let right_raw = &after[dash_pos + 2..];
    let (b, label) = match right_raw.find(':') {
        Some(ci) => (
            right_raw[..ci].trim(),
            right_raw[ci + 1..].trim().to_string(),
        ),
        None => (right_raw.trim(), String::new()),
    };
    let strip_mark = |s: &str| -> String {
        s.trim()
            .trim_start_matches(|c: char| " |o}{".contains(c))
            .trim_end_matches(|c: char| " |o}{".contains(c))
            .to_string()
    };
    let a = strip_mark(a_part);
    let b = strip_mark(b);
    if a.is_empty() || b.is_empty() {
        return;
    }
    ensure(nodes, &a);
    ensure(nodes, &b);
    let many = left_mark.contains('o') || left_mark.contains('{');
    edges.push(BoxEdge {
        a,
        b,
        label: if label.is_empty() {
            String::new()
        } else {
            format!("{label} (1 : N)")
        },
        style: EdgeStyle::Solid,
        head_a: ArrowHead::None,
        head_b: ArrowHead::None,
    });
    let _ = many;
}
