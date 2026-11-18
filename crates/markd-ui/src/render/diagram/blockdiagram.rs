//! Mermaid 块图（block-beta）。

use super::*;

pub(crate) struct BNode {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) span: usize,
    pub(crate) columns: usize,
    pub(crate) container: bool,
    pub(crate) children: Vec<BNode>,
    pub(crate) color: usize,
}

impl BNode {
    fn leaf(id: &str, span: usize) -> Self {
        Self {
            id: id.to_string(),
            label: id.to_string(),
            span,
            columns: 1,
            container: false,
            children: Vec::new(),
            color: 0,
        }
    }
    fn container(id: &str, span: usize) -> Self {
        Self {
            id: id.to_string(),
            label: id.to_string(),
            span,
            columns: 1,
            container: true,
            children: Vec::new(),
            color: 0,
        }
    }
}

struct Placed {
    id: String,
    label: String,
    rect: Rect,
    container: bool,
    color: usize,
    children: Vec<Placed>,
}

fn split_tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut quote = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                quote = !quote;
                cur.push(ch);
            }
            '[' | '(' | '{' if !quote => {
                depth += 1;
                cur.push(ch);
            }
            ']' | ')' | '}' if !quote => {
                depth -= 1;
                cur.push(ch);
            }
            c if c.is_whitespace() && depth == 0 && !quote => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn ident(s: &str) -> String {
    s.trim()
        .trim_matches('"')
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

fn parse_arrow(l: &str) -> Option<(String, String, String)> {
    let i = l.find("-->")?;
    let left = l[..i].trim();
    let right = l[i + 3..].trim();
    let (from_part, label) = match left.rfind("--") {
        Some(j) => (
            &left[..j],
            left[j + 2..].trim().trim_matches('"').trim().to_string(),
        ),
        None => (left, String::new()),
    };
    let from = ident(from_part);
    let to = ident(right);
    if from.is_empty() || to.is_empty() {
        return None;
    }
    Some((from, to, label))
}

pub(crate) fn parse_blockdiagram(code: &str) -> (BNode, Vec<(String, String, String)>) {
    let mut stack: Vec<BNode> = vec![BNode::container("root", 1)];
    let mut arrows = Vec::new();
    let mut counter = 0usize;
    for raw in code.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with("%%") || l.to_lowercase().starts_with("block-beta") {
            continue;
        }
        if let Some(a) = parse_arrow(l) {
            arrows.push(a);
            continue;
        }
        if l == "end" {
            if stack.len() > 1 {
                let node = stack.pop().expect("至少两层");
                stack.last_mut().expect("父容器").children.push(node);
            }
            continue;
        }
        if let Some(rest) = l.strip_prefix("columns")
            && let Ok(n) = rest.trim().parse::<usize>()
        {
            stack.last_mut().expect("容器栈非空").columns = n.max(1);
            continue;
        }
        for tok in split_tokens(l) {
            if let Some(rest) = tok.strip_prefix("block:") {
                let parts: Vec<&str> = rest.split(':').collect();
                let id = parts[0].trim();
                let span = parts
                    .get(1)
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(1);
                let mut n = BNode::container(id, span.max(1));
                n.color = counter;
                counter += 1;
                stack.push(n);
                continue;
            }
            if let Some(rest) = tok.strip_prefix("space:") {
                let span = rest.trim().parse::<usize>().unwrap_or(1).max(1);
                let mut n = BNode::leaf("", span);
                n.label.clear();
                n.color = counter;
                counter += 1;
                stack.last_mut().expect("容器栈非空").children.push(n);
                continue;
            }
            let (id, label) = if let Some(open) = tok.find('[') {
                let close = tok.rfind(']').unwrap_or(tok.len());
                (
                    tok[..open].trim().to_string(),
                    tok[open + 1..close].trim().trim_matches('"').to_string(),
                )
            } else {
                (tok.clone(), tok.clone())
            };
            if id.is_empty() {
                continue;
            }
            let mut n = BNode::leaf(&id, 1);
            n.label = label;
            n.color = counter;
            counter += 1;
            stack.last_mut().expect("容器栈非空").children.push(n);
        }
    }
    while stack.len() > 1 {
        let node = stack.pop().expect("至少两层");
        stack.last_mut().expect("父容器").children.push(node);
    }
    (stack.pop().expect("根容器"), arrows)
}

/// 递归测量并放置：返回 (本层高度, 已放置节点)。
fn layout_nodes(nodes: &[BNode], columns: usize, origin: Pos2, width: f32) -> (Vec<Placed>, f32) {
    if nodes.is_empty() {
        return (Vec::new(), 0.0);
    }
    let columns = columns.max(1);
    let gap = 8.0;
    let cell_w = ((width - gap * (columns.saturating_sub(1)) as f32) / columns as f32).max(24.0);
    // 按 span 分行。
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut row: Vec<usize> = Vec::new();
    let mut used = 0usize;
    for (i, n) in nodes.iter().enumerate() {
        let span = n.span.clamp(1, columns);
        if used + span > columns && !row.is_empty() {
            rows.push(std::mem::take(&mut row));
            used = 0;
        }
        row.push(i);
        used += span;
    }
    if !row.is_empty() {
        rows.push(row);
    }
    let mut row_hs = Vec::new();
    let mut row_items: Vec<Vec<(usize, f32, f32)>> = Vec::new();
    for r in &rows {
        let mut items = Vec::new();
        let mut rh = 0.0_f32;
        for &i in r {
            let n = &nodes[i];
            let span = n.span.clamp(1, columns);
            let w = cell_w * span as f32 + gap * (span.saturating_sub(1)) as f32;
            let h = if n.container {
                let (_, ih) =
                    layout_nodes(&n.children, n.columns, Pos2::ZERO, (w - 16.0).max(20.0));
                34.0 + ih + 8.0
            } else {
                32.0
            };
            rh = rh.max(h);
            items.push((i, w, h));
        }
        row_hs.push(rh);
        row_items.push(items);
    }
    let total_h = row_hs.iter().sum::<f32>() + gap * (rows.len().saturating_sub(1)) as f32;
    let mut placed = Vec::new();
    let mut y = origin.y;
    for (ri, items) in row_items.iter().enumerate() {
        let mut x = origin.x;
        for &(i, w, h) in items {
            let n = &nodes[i];
            let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h));
            let children = if n.container {
                let inner = Rect::from_min_max(
                    Pos2::new(r.left() + 8.0, r.top() + 24.0),
                    Pos2::new(r.right() - 8.0, r.bottom() - 8.0),
                );
                let (placed_children, _) =
                    layout_nodes(&n.children, n.columns, inner.min, inner.width());
                placed_children
            } else {
                Vec::new()
            };
            placed.push(Placed {
                id: n.id.clone(),
                label: n.label.clone(),
                rect: r,
                container: n.container,
                color: n.color,
                children,
            });
            x += w + gap;
        }
        y += row_hs[ri] + gap;
    }
    (placed, total_h)
}

pub(crate) fn draw_block(ui: &mut egui::Ui, code: &str, theme: &Theme) -> egui::Response {
    let (root, arrows) = parse_blockdiagram(code);
    if root.children.is_empty() {
        return raw_code(ui, code, theme);
    }
    let avail = ui.available_width().max(320.0).min(900.0);
    let (placed, h) = layout_nodes(&root.children, root.columns, Pos2::ZERO, avail - 8.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail, h + 12.0), Sense::click());
    let p = ui.painter();
    let offset = Vec2::new(rect.left() + 4.0, rect.top() + 6.0);
    let mut rects: HashMap<String, Rect> = HashMap::new();
    draw_placed(p, theme, &placed, offset, &mut rects);
    for (from, to, label) in &arrows {
        let (Some(ra), Some(rb)) = (rects.get(from), rects.get(to)) else {
            continue;
        };
        let a = ra.center();
        let b = rb.center();
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dist = (dx * dx + dy * dy).sqrt().max(1.0);
        let (ux, uy) = (dx / dist, dy / dist);
        let trim = |r: &Rect, dx: f32, dy: f32| -> f32 {
            let tx = if dx.abs() > 1e-4 {
                r.width() / 2.0 / dx.abs()
            } else {
                f32::INFINITY
            };
            let ty = if dy.abs() > 1e-4 {
                r.height() / 2.0 / dy.abs()
            } else {
                f32::INFINITY
            };
            tx.min(ty)
        };
        let t0 = trim(ra, ux, uy);
        let t1 = dist - trim(rb, -ux, -uy);
        if t1 <= t0 + 2.0 {
            continue;
        }
        let s = Pos2::new(a.x + ux * t0, a.y + uy * t0);
        let e = Pos2::new(a.x + ux * t1, a.y + uy * t1);
        p.line_segment([s, e], Stroke::new(1.3, theme.dim));
        paint_arrow_head(p, e, (ux, uy), theme.dim, ArrowHead::Triangle);
        if !label.is_empty() {
            p.text(
                Pos2::new((s.x + e.x) / 2.0, (s.y + e.y) / 2.0 - 7.0),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(10.0),
                theme.dim,
            );
        }
    }
    resp
}

fn draw_placed(
    p: &egui::Painter,
    theme: &Theme,
    placed: &[Placed],
    offset: Vec2,
    rects: &mut HashMap<String, Rect>,
) {
    for n in placed {
        let r = n.rect.translate(offset);
        if !n.id.is_empty() {
            rects.insert(n.id.clone(), r);
        }
        let base = palette_color(theme, n.color);
        if n.container {
            p.rect_filled(r, 6.0, base.gamma_multiply(0.45));
            p.rect_stroke(
                r,
                6.0,
                Stroke::new(1.2, theme.border),
                egui::StrokeKind::Inside,
            );
            p.text(
                Pos2::new(r.left() + 6.0, r.top() + 4.0),
                Align2::LEFT_TOP,
                &n.label,
                FontId::proportional(11.0),
                theme.heading,
            );
        } else if !n.label.is_empty() {
            p.rect_filled(r, 5.0, base);
            p.rect_stroke(
                r,
                5.0,
                Stroke::new(1.0, theme.border),
                egui::StrokeKind::Inside,
            );
            p.text(
                r.center(),
                Align2::CENTER_CENTER,
                &n.label,
                FontId::proportional(11.5),
                theme.heading,
            );
        }
        let child_offset = offset;
        draw_placed(p, theme, &n.children, child_offset, rects);
    }
}
