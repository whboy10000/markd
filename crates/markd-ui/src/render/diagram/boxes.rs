//! 类图 / ER 图共用的「盒子 + 连线」布局与图表配色。

use super::*;

/// 图表节点配色：浅色主题柔和马卡龙系、深色主题低饱和系，节点按序循环取色。
pub(crate) fn diagram_palette(theme: &Theme) -> Vec<Color32> {
    if theme.kind.is_dark() {
        vec![
            Color32::from_rgb(56, 89, 132),
            Color32::from_rgb(72, 110, 82),
            Color32::from_rgb(116, 84, 130),
            Color32::from_rgb(140, 104, 66),
            Color32::from_rgb(66, 116, 124),
        ]
    } else {
        vec![
            Color32::from_rgb(219, 234, 254),
            Color32::from_rgb(220, 245, 218),
            Color32::from_rgb(243, 226, 250),
            Color32::from_rgb(255, 237, 213),
            Color32::from_rgb(213, 245, 244),
        ]
    }
}

pub(crate) fn palette_color(theme: &Theme, i: usize) -> Color32 {
    let pal = diagram_palette(theme);
    pal[i % pal.len()]
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum EdgeStyle {
    Solid,
    Dashed,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ArrowHead {
    None,
    Triangle,
    HollowTriangle,
    FilledDiamond,
    HollowDiamond,
}

pub(crate) struct BoxNode {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) members: Vec<String>,
}

pub(crate) struct BoxEdge {
    pub(crate) a: String,
    pub(crate) b: String,
    pub(crate) label: String,
    pub(crate) style: EdgeStyle,
    pub(crate) head_a: ArrowHead,
    pub(crate) head_b: ArrowHead,
}

/// 通用「盒子 + 连线」渲染：类图 / ER 图 / 状态图共用。
/// 盒子按网格均布，连线画在盒子下层、从边缘到边缘，装饰端点按样式绘制。
pub(crate) fn draw_boxes_edges(
    ui: &mut egui::Ui,
    theme: &Theme,
    nodes: &[BoxNode],
    edges: &[BoxEdge],
) -> egui::Response {
    let empty = Vec::new();
    let nodes = if nodes.is_empty() { &empty } else { nodes };
    let n = nodes.len();
    if n == 0 {
        let r = panel_frame(theme).show(ui, |ui| {
            ui.label(
                egui::RichText::new("（没有可绘制的内容）")
                    .small()
                    .color(theme.dim),
            );
        });
        return r.response.interact(Sense::click());
    }
    // 盒子尺寸：按各自文字内容算，网格取最大值。
    let measure = |s: &str, size: f32| -> f32 { s.chars().count() as f32 * size };
    let node_sizes: Vec<Vec2> = nodes
        .iter()
        .map(|nd| {
            let inner_w = measure(&nd.label, 12.5).max(
                nd.members
                    .iter()
                    .map(|m| measure(m, 11.0))
                    .fold(0.0, f32::max),
            );
            Vec2::new(
                (inner_w + 28.0).min(220.0),
                30.0 + nd.members.len() as f32 * 16.0,
            )
        })
        .collect();
    let cols = ((n as f32).sqrt().ceil() as usize).max(1);
    let rows = n.div_ceil(cols);
    let mut cell = Vec2::ZERO;
    for r in 0..rows {
        for c in 0..cols {
            let i = r * cols + c;
            if i < n {
                cell = Vec2::new(cell.x.max(node_sizes[i].x), cell.y.max(node_sizes[i].y));
            }
        }
    }
    cell += Vec2::new(30.0, 30.0);
    let w = (cell.x * cols as f32).max(240.0);
    let h = (cell.y * rows as f32).max(120.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let p = ui.painter();

    // 节点位置：网格均布（格子内居中）。
    let mut centers: HashMap<String, Pos2> = HashMap::new();
    let mut sizes: HashMap<String, Vec2> = HashMap::new();
    for (i, nd) in nodes.iter().enumerate() {
        let (r, c) = (i / cols, i % cols);
        let cx = rect.min.x + cell.x * c as f32 + cell.x / 2.0;
        let cy = rect.min.y + cell.y * r as f32 + cell.y / 2.0;
        centers.insert(nd.id.clone(), Pos2::new(cx, cy));
        sizes.insert(nd.id.clone(), node_sizes[i]);
    }

    // 连线先画（垫在盒子下层），装饰箭头/菱形画在端点。
    for e in edges {
        let (Some(pa), Some(pb)) = (centers.get(&e.a), centers.get(&e.b)) else {
            continue;
        };
        let size_a = sizes.get(&e.a).copied().unwrap_or(Vec2::splat(80.0));
        let size_b = sizes.get(&e.b).copied().unwrap_or(Vec2::splat(80.0));
        let dx = pb.x - pa.x;
        let dy = pb.y - pa.y;
        let dist = (dx * dx + dy * dy).sqrt().max(1.0);
        let (ux, uy) = (dx / dist, dy / dist);
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
        let t0 = trim(size_a.x / 2.0, size_a.y / 2.0, ux, uy);
        let t1 = dist - trim(size_b.x / 2.0, size_b.y / 2.0, -ux, -uy);
        if t1 <= t0 + 4.0 {
            continue;
        }
        let s = Pos2::new(pa.x + ux * t0, pa.y + uy * t0);
        let epos = Pos2::new(pa.x + ux * t1, pa.y + uy * t1);
        let stroke = Stroke::new(
            1.3,
            if e.style == EdgeStyle::Dashed {
                theme.dim
            } else {
                theme.text.gamma_multiply(0.8)
            },
        );
        if e.style == EdgeStyle::Dashed {
            paint_dashed_line(p, s, epos, stroke.color, stroke.width);
        } else {
            p.line_segment([s, epos], stroke);
        }
        paint_arrow_head(p, epos, (ux, uy), stroke.color, e.head_b);
        paint_arrow_head(p, s, (-ux, -uy), stroke.color, e.head_a);
        if !e.label.is_empty() {
            let mid = Pos2::new((s.x + epos.x) / 2.0, (s.y + epos.y) / 2.0);
            p.text(
                mid,
                Align2::CENTER_CENTER,
                &e.label,
                FontId::proportional(11.0),
                theme.dim,
            );
        }
    }

    // 盒子后画（盖住连线端点），填充色按调色板循环。
    for (i, nd) in nodes.iter().enumerate() {
        let c = centers[&nd.id];
        let sz = node_sizes[i];
        let r = Rect::from_center_size(c, sz);
        let fill = palette_color(theme, i);
        p.rect_filled(r, 6.0, fill);
        p.rect_stroke(
            r,
            6.0,
            Stroke::new(1.2, theme.border),
            egui::StrokeKind::Outside,
        );
        p.text(
            Pos2::new(r.center().x, r.top() + 4.0),
            Align2::CENTER_TOP,
            &nd.label,
            FontId::proportional(12.5),
            theme.heading,
        );
        for (mi, m) in nd.members.iter().enumerate() {
            p.text(
                Pos2::new(r.left() + 12.0, r.top() + 24.0 + mi as f32 * 16.0),
                Align2::LEFT_CENTER,
                m,
                FontId::monospace(11.0),
                theme.text.gamma_multiply(0.85),
            );
        }
    }
    resp
}

pub(crate) fn paint_arrow_head(
    p: &egui::Painter,
    tip: Pos2,
    dir: (f32, f32),
    color: Color32,
    head: ArrowHead,
) {
    if head == ArrowHead::None {
        return;
    }
    let (ux, uy) = dir;
    let ang = uy.atan2(ux);
    let stroke = Stroke::new(1.3, color);
    match head {
        ArrowHead::None => {}
        ArrowHead::Triangle => {
            for da in [-0.5_f32, 0.5] {
                let a = ang + std::f32::consts::PI + da;
                p.line_segment(
                    [tip, Pos2::new(tip.x + a.cos() * 8.0, tip.y + a.sin() * 8.0)],
                    stroke,
                );
            }
        }
        ArrowHead::HollowTriangle => {
            let back = ang + std::f32::consts::PI;
            let pts = vec![
                tip,
                Pos2::new(
                    tip.x + (back + 0.45).cos() * 11.0,
                    tip.y + (back + 0.45).sin() * 11.0,
                ),
                Pos2::new(
                    tip.x + (back - 0.45).cos() * 11.0,
                    tip.y + (back - 0.45).sin() * 11.0,
                ),
            ];
            let fill = if head == ArrowHead::HollowTriangle {
                Color32::TRANSPARENT
            } else {
                color
            };
            let _ = fill;
            p.add(egui::Shape::convex_polygon(pts, fill, stroke));
        }
        ArrowHead::FilledDiamond | ArrowHead::HollowDiamond => {
            let back = ang + std::f32::consts::PI;
            let mid = Pos2::new(tip.x + ux * 7.0, tip.y + uy * 7.0);
            let pts = vec![
                tip,
                Pos2::new(
                    mid.x + (back + 0.6).cos() * 7.0,
                    mid.y + (back + 0.6).sin() * 7.0,
                ),
                Pos2::new(tip.x + ux * 14.0, tip.y + uy * 14.0),
                Pos2::new(
                    mid.x + (back - 0.6).cos() * 7.0,
                    mid.y + (back - 0.6).sin() * 7.0,
                ),
            ];
            let fill = if head == ArrowHead::FilledDiamond {
                color
            } else {
                Color32::TRANSPARENT
            };
            p.add(egui::Shape::convex_polygon(pts, fill, stroke));
        }
    }
}
