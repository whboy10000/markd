//! Mermaid 类图（classDiagram）。

use super::*;

/// 解析 mermaid 类图：`class X { 成员 }` 与 `A <|-- B : 标签` 关系。
pub(crate) fn parse_class(code: &str) -> (Vec<BoxNode>, Vec<BoxEdge>) {
    let mut nodes: Vec<BoxNode> = Vec::new();
    let mut edges: Vec<BoxEdge> = Vec::new();
    let mut cur_members: Option<(String, Vec<String>)> = None;
    let ensure = |nodes: &mut Vec<BoxNode>, id: &str| -> usize {
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
        if l.is_empty()
            || l.starts_with("%%")
            || l.starts_with("classDiagram")
            || l.starts_with("note ")
        {
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
        if let Some(rest) = l
            .strip_prefix("class ")
            .or_else(|| l.strip_prefix("interface "))
        {
            // `class X` 或 `class X { ... }`（单行成员用 ~ 分隔的少见，忽略）
            let name = rest.trim().trim_end_matches('{').trim();
            if name.is_empty() {
                continue;
            }
            ensure(&mut nodes, name);
            if rest.contains('{') {
                cur_members = Some((name.to_string(), Vec::new()));
            }
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
        // 关系行：A <|-- B / A *-- B / A o-- B / A ..> B / A --> B
        const RELS: [(&str, ArrowHead, ArrowHead, EdgeStyle); 6] = [
            (
                "<|--",
                ArrowHead::HollowTriangle,
                ArrowHead::None,
                EdgeStyle::Solid,
            ),
            (
                "*--",
                ArrowHead::FilledDiamond,
                ArrowHead::None,
                EdgeStyle::Solid,
            ),
            (
                "o--",
                ArrowHead::HollowDiamond,
                ArrowHead::None,
                EdgeStyle::Solid,
            ),
            (
                "..>",
                ArrowHead::None,
                ArrowHead::Triangle,
                EdgeStyle::Dashed,
            ),
            (
                "<..",
                ArrowHead::Triangle,
                ArrowHead::None,
                EdgeStyle::Dashed,
            ),
            (
                "-->",
                ArrowHead::None,
                ArrowHead::Triangle,
                EdgeStyle::Solid,
            ),
        ];
        let mut matched = false;
        for (sym, ha, hb, st) in RELS {
            if let Some(i) = l.find(sym) {
                let a = l[..i].trim();
                let mut b = l[i + sym.len()..].trim();
                let mut label = String::new();
                if let Some(ci) = b.find(':') {
                    label = b[ci + 1..].trim().to_string();
                    b = b[..ci].trim();
                }
                if a.is_empty() || b.is_empty() {
                    break;
                }
                ensure(&mut nodes, a);
                ensure(&mut nodes, b);
                edges.push(BoxEdge {
                    a: a.to_string(),
                    b: b.to_string(),
                    label,
                    style: st,
                    head_a: ha,
                    head_b: hb,
                });
                matched = true;
                break;
            }
        }
        let _ = matched;
    }
    (nodes, edges)
}
