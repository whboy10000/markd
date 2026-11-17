//! Mermaid 流程图（flowchart / graph）与状态图。

use super::*;

/// 从一行里提取节点定义。返回 (id, 标签, 形状)：0=矩形[]、1=圆角()、2=菱形{}。
pub(crate) fn extract_nodes(line: &str) -> Vec<(String, String, u8)> {
    let c: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < c.len() {
        if c[i].is_alphanumeric() || c[i] == '_' {
            let s = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let id: String = c[s..i].iter().collect();
            if i < c.len() && matches!(c[i], '[' | '(' | '{') {
                let (close, shape) = match c[i] {
                    '[' => (']', 0_u8),
                    '(' => (')', 1),
                    _ => ('}', 2),
                };
                let start = i + 1;
                let mut j = start;
                while j < c.len() && c[j] != close {
                    j += 1;
                }
                let label: String = c[start..j].iter().collect();
                out.push((id, label, shape));
                i = j.saturating_add(1);
                continue;
            }
        }
        i += 1;
    }
    out
}

pub(crate) fn first_id(s: &str) -> String {
    s.trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

pub(crate) fn parse_flow(
    code: &str,
) -> (
    Vec<(String, String, u8)>,
    Vec<(String, String, String)>,
    bool,
) {
    let mut nodes: Vec<(String, String, u8)> = Vec::new();
    let mut edges: Vec<(String, String, String)> = Vec::new();
    let mut horizontal = false;
    for raw in code.lines() {
        let l = raw.trim();
        if l.starts_with("graph LR")
            || l.starts_with("flowchart LR")
            || l.starts_with("graph RL")
            || l.starts_with("flowchart RL")
        {
            horizontal = true;
        }
        if l.starts_with("graph")
            || l.starts_with("flowchart")
            || l.starts_with("sequenceDiagram")
            || l.starts_with("gantt")
            || l.is_empty()
            || l.starts_with("%%")
        {
            continue;
        }
        for (id, label, shape) in extract_nodes(l) {
            if !nodes.iter().any(|(n, _, _)| n == &id) {
                nodes.push((id, label, shape));
            }
        }
        if let Some(arrow) = l.find("-->").or_else(|| l.find("---")) {
            let left = &l[..arrow];
            let mut after = &l[arrow + 3..];
            let mut label = String::new();
            if after.starts_with('|') {
                if let Some(e) = after[1..].find('|') {
                    label = after[1..1 + e].to_string();
                    after = &after[2 + e..];
                }
            }
            let a = first_id(left);
            let b = first_id(after);
            if !a.is_empty() && !b.is_empty() {
                edges.push((a, b, label));
            }
        }
    }
    (nodes, edges, horizontal)
}

pub(crate) fn draw_flow(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (nodes, edges, horizontal) = parse_flow(code);
    if nodes.is_empty() {
        let r = panel_frame(theme).show(ui, |ui| {
            for line in code.lines() {
                ui.label(
                    egui::RichText::new(line)
                        .monospace()
                        .small()
                        .color(theme.dim),
                );
            }
        });
        return r.response.interact(Sense::click());
    }

    let nw = 116.0_f32;
    let nh = 34.0_f32;
    let gap = 26.0_f32;

    // 分层布局：没有入边的节点在第 0 层，其余节点深度 = 前驱最大深度 + 1。
    // 有限次迭代求解最长路径，环状引用靠迭代上限截断，不会死循环。
    // TD/TB 自上而下逐层排（行 = 层），LR/RL 自左向右逐层排（列 = 层），
    // 不再按固定每行 3 个摆格子——那样 TD 图会被横着摆成一排。
    let mut layer: HashMap<String, usize> =
        nodes.iter().map(|(id, _, _)| (id.clone(), 0)).collect();
    // 环（如状态图 Running ↔ Paused）会让最长路径每轮继续加深、画布高度暴涨。
    // 先用 DFS 找出回边，分层时跳过回边，图退化成 DAG 后再求最长路径。
    let idx_of: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.0.as_str(), i))
        .collect();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (ei, (a, _, _)) in edges.iter().enumerate() {
        if let Some(&ai) = idx_of.get(a.as_str()) {
            adj[ai].push(ei);
        }
    }
    let mut back = vec![false; edges.len()];
    let mut state = vec![0_u8; nodes.len()];
    fn dfs(
        u: usize,
        adj: &[Vec<usize>],
        edges: &[(String, String, String)],
        idx_of: &HashMap<&str, usize>,
        state: &mut [u8],
        back: &mut [bool],
    ) {
        state[u] = 1;
        for &ei in &adj[u] {
            let Some(&v) = idx_of.get(edges[ei].1.as_str()) else {
                continue;
            };
            match state[v] {
                0 => dfs(v, adj, edges, idx_of, state, back),
                1 => back[ei] = true,
                _ => {}
            }
        }
        state[u] = 2;
    }
    for i in 0..nodes.len() {
        if state[i] == 0 {
            dfs(i, &adj, &edges, &idx_of, &mut state, &mut back);
        }
    }
    for _ in 0..=nodes.len() {
        let mut changed = false;
        for (ei, (a, b, _)) in edges.iter().enumerate() {
            if back[ei] {
                continue;
            }
            let la = layer.get(a.as_str()).copied();
            if let Some(lb) = layer.get_mut(b.as_str()) {
                let la = la.unwrap_or(0);
                if la + 1 > *lb {
                    *lb = la + 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let depth = layer.values().copied().max().unwrap_or(0) + 1;
    let mut layers: Vec<Vec<(String, String, u8)>> = vec![Vec::new(); depth];
    for n in &nodes {
        layers[layer[&n.0]].push(n.clone());
    }
    // 单层最多多少个节点决定画布宽/高；孤立节点都落在第 0 层。
    let widest = layers.iter().map(|g| g.len()).max().unwrap_or(1).max(1);
    let (w, h) = if horizontal {
        (
            (depth as f32) * (nw + gap) + gap,
            (widest as f32) * (nh + gap) + gap,
        )
    } else {
        (
            (widest as f32) * (nw + gap) + gap,
            (depth as f32) * (nh + gap) + gap,
        )
    };

    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w.max(160.0), h.max(60.0)), Sense::click());
    let p = ui.painter();
    let mut pos: HashMap<String, Pos2> = HashMap::new();
    for (li, group) in layers.iter().enumerate() {
        for (gi, (id, _, _)) in group.iter().enumerate() {
            let (x, y) = if horizontal { (li, gi) } else { (gi, li) };
            pos.insert(
                id.clone(),
                Pos2::new(
                    rect.min.x + gap + (x as f32) * (nw + gap) + nw / 2.0,
                    rect.min.y + gap + (y as f32) * (nh + gap) + nh / 2.0,
                ),
            );
        }
    }
    let edge_stroke = Stroke::new(1.5, theme.dim);
    for (a, b, label) in &edges {
        if let (Some(pa), Some(pb)) = (pos.get(a), pos.get(b)) {
            let dx = pb.x - pa.x;
            let dy = pb.y - pa.y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < 1.0 {
                continue;
            }
            let (ux, uy) = (dx / dist, dy / dist);
            // 线段只画在两个节点框之间（从边缘到边缘），箭头顶在目标框边上，
            // 不再从中心到中心、整条线都被节点盖住。
            let trim = |hw: f32, hh: f32, dx: f32, dy: f32| -> f32 {
                let tx = if dx.abs() > 1e-4 {
                    hw / dx.abs()
                } else {
                    f32::INFINITY
                };
                let ty = if dy.abs() > 1e-4 {
                    hh / dy.abs()
                } else {
                    f32::INFINITY
                };
                tx.min(ty)
            };
            let t0 = trim(nw / 2.0 + 2.0, nh / 2.0 + 2.0, ux, uy);
            let t1 = dist - trim(nw / 2.0 + 4.0, nh / 2.0 + 4.0, -ux, -uy);
            if t1 <= t0 {
                continue;
            }
            let s = Pos2::new(pa.x + ux * t0, pa.y + uy * t0);
            let e = Pos2::new(pa.x + ux * t1, pa.y + uy * t1);
            p.line_segment([s, e], edge_stroke);
            // 箭头：从终点往回画两条短斜线组成的 V 形
            let a0 = (e.y - s.y).atan2(e.x - s.x);
            for da in [-0.5_f32, 0.5] {
                let ang = a0 + std::f32::consts::PI + da;
                let wing = Pos2::new(e.x + ang.cos() * 7.0, e.y + ang.sin() * 7.0);
                p.line_segment([e, wing], edge_stroke);
            }
            if !label.is_empty() {
                let mid = Pos2::new((s.x + e.x) / 2.0, (s.y + e.y) / 2.0);
                p.text(
                    mid,
                    Align2::CENTER_CENTER,
                    label,
                    FontId::new(11.0, FontFamily::Proportional),
                    theme.dim,
                );
            }
        }
    }
    let node_i = |id: &str| -> usize { nodes.iter().position(|n| &n.0 == id).unwrap_or(0) };
    for (id, label, shape) in &nodes {
        if let Some(c) = pos.get(id) {
            let r = Rect::from_center_size(*c, Vec2::new(nw, nh));
            let fill = palette_color(theme, node_i(id));
            let stroke_c = Stroke::new(1.2, theme.border);
            match shape {
                // {判断} —— 决策节点画菱形
                2 => {
                    let pts = vec![
                        Pos2::new(c.x, r.top()),
                        Pos2::new(r.right(), c.y),
                        Pos2::new(c.x, r.bottom()),
                        Pos2::new(r.left(), c.y),
                    ];
                    p.add(egui::Shape::convex_polygon(pts, fill, stroke_c));
                }
                // (圆角节点) 画胶囊
                1 => {
                    p.rect_filled(r, nh / 2.0, fill);
                    p.rect_stroke(r, nh / 2.0, stroke_c, egui::StrokeKind::Outside);
                }
                // [普通节点] 画矩形
                _ => {
                    p.rect_filled(r, 5.0, fill);
                    p.rect_stroke(r, 5.0, stroke_c, egui::StrokeKind::Outside);
                }
            }
            p.text(
                r.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::new(12.0, FontFamily::Proportional),
                theme.text,
            );
        }
    }
    resp
}

/// 状态图 → 流程图语法转换：stateDiagram 的 `A --> B: 标签` 改写成
/// flowchart 的 `A -->|标签| B`；`[*]` 起止标记换成圆角节点（起点/终点），
/// 否则 first_id 取不到标识符、含 [*] 的边会被整条丢弃。note/state 声明跳过。
pub(crate) fn transform_state_to_flow(code: &str) -> String {
    let mut out = String::from("flowchart TD\n");
    // 状态图里 `Idle --> Running : 启动` 的状态是裸标识符，流程图解析器
    // 只会给带形状的节点建标签；这里补成 `Idle[Idle]`，否则状态名全丢。
    fn state_node(s: &str) -> String {
        let s = s.trim();
        if s.is_empty() || s.contains('[') || s.contains('(') || s.contains('{') {
            s.to_string()
        } else {
            format!("{s}[{s}]")
        }
    }
    for raw in code.lines() {
        let l = raw.trim();
        let lower = l.to_lowercase();
        if l.is_empty()
            || lower.starts_with("statediagram")
            || lower.starts_with("note ")
            || lower.starts_with("state ")
        {
            continue;
        }
        // [*] 必须换成带方括号标签的普通节点：裸 id 会被 first_id
        // （只取字母数字）判成空串，整条边连带节点一起被丢弃。
        let l2 = l
            .replace("[*] -->", "ST0[●开始] -->")
            .replace("--> [*]", "--> ST9[●结束]");
        let l: &str = &l2;
        // `A --> B: 标签` → `A[A] -->|标签| B[B]`
        if let Some(ci) = l.find(':').filter(|_| l.contains("-->")) {
            let left = &l[..ci];
            let label = l[ci + 1..].trim();
            if let Some((a, b)) = left.split_once("-->")
                && !label.is_empty()
                && !left.contains('|')
            {
                out.push_str(&format!(
                    "{} -->|{}| {}\n",
                    state_node(a),
                    label,
                    state_node(b)
                ));
                continue;
            }
        }
        if let Some((a, b)) = l.split_once("-->") {
            out.push_str(&format!("{} --> {}\n", state_node(a), state_node(b)));
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    out
}
