//! Mermaid 需求图（requirementDiagram）。

use super::*;

pub(crate) struct ReqNode {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) label: String,
    pub(crate) members: Vec<String>,
}

pub(crate) struct ReqEdge {
    pub(crate) a: String,
    pub(crate) b: String,
    pub(crate) label: String,
}

/// 解析需求图：`requirement 名 { 字段 }` / `element 名 { 字段 }` / `A - 关系 -> B`。
pub(crate) fn parse_requirement(code: &str) -> (Vec<ReqNode>, Vec<ReqEdge>) {
    let kinds = [
        "functionalRequirement",
        "performanceRequirement",
        "interfaceRequirement",
        "physicalRequirement",
        "designConstraint",
        "requirement",
        "element",
    ];
    let mut nodes: Vec<ReqNode> = Vec::new();
    let mut edges: Vec<ReqEdge> = Vec::new();
    let mut current: Option<usize> = None;
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") {
            continue;
        }
        if l.eq_ignore_ascii_case("requirementDiagram") {
            continue;
        }
        let mut matched: Option<&str> = None;
        for k in kinds {
            if let Some(prefix) = l.get(..k.len())
                && prefix.eq_ignore_ascii_case(k)
            {
                let rest = &l[k.len()..];
                if rest.chars().next().map_or(true, |c| !c.is_alphanumeric()) {
                    matched = Some(k);
                    break;
                }
            }
        }
        if let Some(kind) = matched {
            let head = &l[kind.len()..];
            let name = head.split('{').next().unwrap_or("").trim().to_string();
            if name.is_empty() {
                continue;
            }
            nodes.push(ReqNode {
                id: name.clone(),
                kind: (*kind).to_string(),
                label: name,
                members: Vec::new(),
            });
            current = Some(nodes.len() - 1);
            if !head.contains('{') {
                current = None;
            }
            continue;
        }
        if l == "}" {
            current = None;
            continue;
        }
        if let Some(cur) = current
            && let Some((k, v)) = l.split_once(':')
        {
            nodes[cur]
                .members
                .push(format!("{}: {}", k.trim(), v.trim().trim_matches('"')));
            continue;
        }
        // 关系：`a - satisfies -> b`
        if let Some((left, right)) = l.split_once("->")
            && let Some((a, rel)) = left.split_once(" - ")
        {
            let a = a.trim().to_string();
            let b = right.trim().to_string();
            if !a.is_empty() && !b.is_empty() {
                edges.push(ReqEdge {
                    a,
                    b,
                    label: rel.trim().to_string(),
                });
            }
        }
    }
    (nodes, edges)
}

pub(crate) fn draw_requirement(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (nodes, rels) = parse_requirement(code);
    if nodes.is_empty() {
        return raw_code(ui, code, theme);
    }
    let box_nodes: Vec<BoxNode> = nodes
        .iter()
        .map(|n| BoxNode {
            id: n.id.clone(),
            label: if n.kind.eq_ignore_ascii_case("element") {
                format!("{} · element", n.label)
            } else {
                n.label.clone()
            },
            members: n.members.clone(),
        })
        .collect();
    let box_edges: Vec<BoxEdge> = rels
        .iter()
        .map(|e| BoxEdge {
            a: e.a.clone(),
            b: e.b.clone(),
            label: e.label.clone(),
            style: EdgeStyle::Solid,
            head_a: ArrowHead::None,
            head_b: ArrowHead::Triangle,
        })
        .collect();
    draw_boxes_edges(ui, theme, &box_nodes, &box_edges)
}
