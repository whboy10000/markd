//! RaTeX 数学公式原生显示列表渲染。
//!
//! 把 RaTeX 的 [`DisplayList`] 直接转换成 epaint 绘制原语
//! （Mesh / RectShape），不走 PNG 栅格化与纹理上传，
//! 追求最高渲染性能与任意缩放清晰度。
//!
//! 坐标说明：RaTeX 显示列表使用屏幕坐标（x 向右、y 向下），
//! 原点在包围盒左上角，基线在 `y = height`。所有坐标以「em」为单位，
//! 乘以 `font_size` 即得逻辑像素。字形轮廓来自字体的 `glyf` 表
//! （y 向上），绘制时翻转 y。

use std::collections::HashMap;
use std::sync::Arc;

use ab_glyph::{Font, FontRef, FontVec, OutlineCurve, Point};
use egui::{Color32, Pos2, Shape, Vec2, epaint};
use lyon::geom::point;
use lyon::lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
};
use lyon::path::Path as LyonPath;
use ratex_font::{FontId, katex_ttf_glyph_char};
use ratex_types::color::Color as RxColor;
use ratex_types::display_item::{DisplayItem, DisplayList};
use ratex_types::math_style::MathStyle;
use ratex_types::path_command::PathCommand;

/// 自定义顶点类型，避免 lyon 几何点的泛型歧义。
#[derive(Copy, Clone)]
struct Vtx(f32, f32);

/// 已排好版的公式：一组 epaint 形状 + 逻辑尺寸 + 基线位置。
/// 坐标以「font_size 为 1em」的逻辑像素表示，绘制时按需平移/缩放。
#[derive(Clone)]
pub struct RenderedMath {
    pub shapes: Vec<Shape>,
    pub size: Vec2,
    /// 基线到公式顶部的逻辑距离，用于行内公式与文字基线对齐。
    pub baseline: f32,
}

/// 公式显示模式：影响上下标、分数等的排版尺寸。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MathDisplay {
    /// 块级 / 独立行：`\displaystyle`，上下标更舒展。
    Display,
    /// 行内：`\textstyle`，上下标更紧凑。
    Inline,
}

/// 把一段 LaTeX 排版并转换成 epaint 形状。
///
/// `font_size` 是 1em 对应的逻辑像素数；`display` 决定排版风格。
/// 返回 `None` 表示解析或字体加载失败。
pub fn render_formula(tex: &str, font_size: f32, display: MathDisplay) -> Option<RenderedMath> {
    let ast = ratex_parser::parse(tex).ok()?;
    let style = match display {
        MathDisplay::Display => MathStyle::Display,
        MathDisplay::Inline => MathStyle::Text,
    };
    let layout_options = ratex_layout::LayoutOptions {
        style,
        color: RxColor::BLACK,
        ..Default::default()
    };
    let layout = ratex_layout::layout(&ast, &layout_options);
    let display_list = ratex_layout::to_display_list(&layout);
    display_list_to_shapes(&display_list, font_size)
}

/// 把显示列表逐项转成 epaint 形状。
fn display_list_to_shapes(dl: &DisplayList, em: f32) -> Option<RenderedMath> {
    let fonts = ratex_font_loader::load_fonts_for_items_parsed("", &dl.items).ok()?;
    let fctx = FontCtx::new(&fonts);

    let mut shapes: Vec<Shape> = Vec::new();
    let mut font_id_cache: HashMap<&str, FontId> = HashMap::new();

    for item in &dl.items {
        match item {
            DisplayItem::GlyphPath {
                x,
                y,
                scale,
                font,
                char_code,
                color,
            } => {
                let font_id = *font_id_cache
                    .entry(font.as_str())
                    .or_insert_with(|| FontId::parse(font).unwrap_or(FontId::MainRegular));
                let px = *x as f32 * em;
                let py = *y as f32 * em;
                let glyph_em = em * *scale as f32;
                if let Some(mesh) = glyph_mesh(
                    &fctx,
                    font_id,
                    *char_code,
                    px,
                    py,
                    glyph_em,
                    rx_to_color32(*color),
                ) {
                    shapes.push(Shape::Mesh(Arc::new(mesh)));
                }
            }
            DisplayItem::Line {
                x,
                y,
                width,
                thickness,
                color,
                dashed,
            } => {
                let px = *x as f32 * em;
                let py = *y as f32 * em;
                let pw = *width as f32 * em;
                let pt = (*thickness as f32 * em).max(0.5);
                let c = rx_to_color32(*color);
                if *dashed {
                    let dash = 4.0 * pt;
                    let period = 2.0 * dash;
                    let mut cx = px;
                    while cx < px + pw {
                        let end = (cx + dash).min(px + pw);
                        if end > cx {
                            shapes.push(Shape::rect_filled(
                                egui::Rect::from_min_max(
                                    Pos2::new(cx, py - pt * 0.5),
                                    Pos2::new(end, py + pt * 0.5),
                                ),
                                0.0,
                                c,
                            ));
                        }
                        cx += period;
                    }
                } else {
                    shapes.push(Shape::rect_filled(
                        egui::Rect::from_min_max(
                            Pos2::new(px, py - pt * 0.5),
                            Pos2::new(px + pw, py + pt * 0.5),
                        ),
                        0.0,
                        c,
                    ));
                }
            }
            DisplayItem::Rect {
                x,
                y,
                width,
                height,
                color,
            } => {
                shapes.push(Shape::rect_filled(
                    egui::Rect::from_min_max(
                        Pos2::new(*x as f32 * em, *y as f32 * em),
                        Pos2::new((*x + *width) as f32 * em, (*y + *height) as f32 * em),
                    ),
                    0.0,
                    rx_to_color32(*color),
                ));
            }
            DisplayItem::Path {
                x,
                y,
                commands,
                fill,
                color,
            } => {
                let px = *x as f32 * em;
                let py = *y as f32 * em;
                if let Some(shape) = path_shape(commands, px, py, em, *fill, rx_to_color32(*color))
                {
                    shapes.push(shape);
                }
            }
        }
    }

    Some(RenderedMath {
        shapes,
        size: Vec2::new(dl.width as f32 * em, (dl.height + dl.depth) as f32 * em),
        baseline: dl.height as f32 * em,
    })
}

/// 已加载字体的查找上下文：解析字体（FontVec）与原始大字体（FontRef）。
struct FontCtx<'a> {
    parsed: HashMap<FontId, (&'a FontVec, ratex_font_loader::OutlineSourceId)>,
    raw: HashMap<FontId, (&'a [u8], ratex_font_loader::OutlineSourceId)>,
}

impl<'a> FontCtx<'a> {
    fn new(fonts: &'a ratex_font_loader::ParsedFontSet) -> Self {
        let mut parsed = HashMap::new();
        for (id, font, source_id) in fonts.iter_with_source() {
            parsed.insert(*id, (font, source_id));
        }
        let mut raw = HashMap::new();
        for (id, bytes, source_id) in fonts.iter_raw_with_source() {
            raw.insert(*id, (bytes, source_id));
        }
        Self { parsed, raw }
    }

    /// 解析字形轮廓：优先用 FontVec，失败回退原始字体，再回退 Main-Regular。
    fn outline(&self, font_id: FontId, ch: char) -> Option<(Arc<[OutlineCurve]>, f32)> {
        if let Some(r) = self.outline_in(font_id, ch) {
            return Some(r);
        }
        if font_id != FontId::MainRegular {
            if let Some(r) = self.outline_in(FontId::MainRegular, ch) {
                return Some(r);
            }
        }
        None
    }

    fn outline_in(&self, font_id: FontId, ch: char) -> Option<(Arc<[OutlineCurve]>, f32)> {
        if let Some((font, source_id)) = self.parsed.get(&font_id) {
            let glyph_id = font.glyph_id(ch);
            if glyph_id.0 != 0 {
                if let Some(curves) =
                    ratex_font_loader::outline_cache::get_or_compute_outline_fontvec(
                        font_id, font, *source_id, glyph_id,
                    )
                {
                    if !curves.is_empty() {
                        return Some((curves, font.units_per_em().unwrap_or(1000.0) as f32));
                    }
                }
            }
        }
        if let Some((bytes, source_id)) = self.raw.get(&font_id) {
            let face_index = ratex_font_loader::font_face_index(font_id);
            if let Ok(font) = FontRef::try_from_slice_and_index(bytes, face_index) {
                let glyph_id = font.glyph_id(ch);
                if glyph_id.0 != 0 {
                    if let Some(curves) =
                        ratex_font_loader::outline_cache::get_or_compute_outline_with_source_id(
                            font_id, &font, *source_id, glyph_id,
                        )
                    {
                        if !curves.is_empty() {
                            return Some((curves, font.units_per_em().unwrap_or(1000.0) as f32));
                        }
                    }
                }
            }
        }
        None
    }
}

/// 把一个字形的轮廓细分成实心三角形网格。
fn glyph_mesh(
    fctx: &FontCtx<'_>,
    font_id: FontId,
    char_code: u32,
    px: f32,
    py: f32,
    em: f32,
    color: Color32,
) -> Option<epaint::Mesh> {
    let ch = katex_ttf_glyph_char(font_id, char_code);
    let (curves, units_per_em) = fctx.outline(font_id, ch)?;
    let scale = em / units_per_em;

    let mut builder = LyonPath::builder();
    // 轮廓连续性判断必须在「缩放后的像素坐标」下做，与 RaTeX 官方
    // build_outline_path 一致。此前用未缩放的字体单位比较、阈值 0.01，
    // 在 units_per_em≈1000 下相当于 0.00001em，远严于浮点误差：相邻曲线
    // 端点本应相等却因误差被判为断点，插入多余的 close()+begin()，把字形
    // 轮廓拆成多段——表现就是「字母笔画断断续续」。改在像素坐标下比较可
    // 容忍这类误差。
    let mut last_end: Option<(f32, f32)> = None;
    for curve in curves.iter() {
        let (start, end) = curve_endpoints(curve);
        let (sx, sy) = (px + start.x * scale, py - start.y * scale);
        let (ex, ey) = (px + end.x * scale, py - end.y * scale);
        let need_move = match last_end {
            None => true,
            Some((lx, ly)) => (lx - sx).abs() > 0.01 || (ly - sy).abs() > 0.01,
        };
        if need_move {
            if last_end.is_some() {
                builder.close();
            }
            builder.begin(point(sx, sy));
        }
        match curve {
            OutlineCurve::Line(_, p1) => {
                builder.line_to(point(px + p1.x * scale, py - p1.y * scale));
            }
            OutlineCurve::Quad(_, p1, p2) => {
                builder.quadratic_bezier_to(
                    point(px + p1.x * scale, py - p1.y * scale),
                    point(px + p2.x * scale, py - p2.y * scale),
                );
            }
            OutlineCurve::Cubic(_, p1, p2, p3) => {
                builder.cubic_bezier_to(
                    point(px + p1.x * scale, py - p1.y * scale),
                    point(px + p2.x * scale, py - p2.y * scale),
                    point(px + p3.x * scale, py - p3.y * scale),
                );
            }
        }
        last_end = Some((ex, ey));
    }
    if last_end.is_some() {
        builder.close();
    }
    let path = builder.build();
    fill_path_to_mesh(&path, FillRule::NonZero, color)
}

/// 把 RaTeX 的 `PathCommand` 序列转成形状：实心填充或描边。
fn path_shape(
    commands: &[PathCommand],
    px: f32,
    py: f32,
    em: f32,
    fill: bool,
    color: Color32,
) -> Option<Shape> {
    let mut builder = LyonPath::builder();
    for cmd in commands {
        match cmd {
            PathCommand::MoveTo { x: cx, y: cy } => {
                builder.begin(point(px + *cx as f32 * em, py + *cy as f32 * em));
            }
            PathCommand::LineTo { x: cx, y: cy } => {
                builder.line_to(point(px + *cx as f32 * em, py + *cy as f32 * em));
            }
            PathCommand::CubicTo {
                x1,
                y1,
                x2,
                y2,
                x: cx,
                y: cy,
            } => {
                builder.cubic_bezier_to(
                    point(px + *x1 as f32 * em, py + *y1 as f32 * em),
                    point(px + *x2 as f32 * em, py + *y2 as f32 * em),
                    point(px + *cx as f32 * em, py + *cy as f32 * em),
                );
            }
            PathCommand::QuadTo {
                x1,
                y1,
                x: cx,
                y: cy,
            } => {
                builder.quadratic_bezier_to(
                    point(px + *x1 as f32 * em, py + *y1 as f32 * em),
                    point(px + *cx as f32 * em, py + *cy as f32 * em),
                );
            }
            PathCommand::Close => {
                builder.close();
            }
        }
    }
    let path = builder.build();
    if fill {
        // 与 RaTeX PNG 渲染器一致：填充路径用 EvenOdd，避免 tallDelim 双分量自交。
        fill_path_to_mesh(&path, FillRule::EvenOdd, color).map(|m| Shape::Mesh(Arc::new(m)))
    } else {
        stroke_path_to_shape(&path, color)
    }
}

fn fill_path_to_mesh(path: &LyonPath, rule: FillRule, color: Color32) -> Option<epaint::Mesh> {
    let mut geometry: VertexBuffers<Vtx, u32> = VertexBuffers::new();
    let mut tessellator = FillTessellator::new();
    // 轮廓已在像素坐标下；公式字形（尤其行内小字号）曲线很细，0.5px 容差会把
    // 弧线折成明显的多边形，配合无 MSAA 时看起来像笔画断续。收紧到 0.1px 让
    // 曲线更贴合字形轮廓；公式字形数很少，多出的三角形开销可忽略。
    let options = FillOptions::tolerance(0.1).with_fill_rule(rule);
    tessellator
        .tessellate_path(
            path,
            &options,
            &mut BuffersBuilder::new(&mut geometry, |v: FillVertex| {
                let p = v.position();
                Vtx(p.x, p.y)
            }),
        )
        .ok()?;
    if geometry.vertices.is_empty() || geometry.indices.is_empty() {
        return None;
    }
    let mut mesh = epaint::Mesh::default();
    for v in &geometry.vertices {
        mesh.vertices.push(epaint::Vertex {
            pos: Pos2::new(v.0, v.1),
            uv: egui::pos2(0.0, 0.0),
            color,
        });
    }
    mesh.indices = geometry.indices.clone();
    Some(mesh)
}

fn stroke_path_to_shape(path: &LyonPath, color: Color32) -> Option<Shape> {
    let mut geometry: VertexBuffers<Vtx, u32> = VertexBuffers::new();
    let mut tessellator = StrokeTessellator::new();
    let options = StrokeOptions::tolerance(0.1).with_line_width(1.5);
    tessellator
        .tessellate_path(
            path,
            &options,
            &mut BuffersBuilder::new(&mut geometry, |v: StrokeVertex| {
                let p = v.position();
                Vtx(p.x, p.y)
            }),
        )
        .ok()?;
    if geometry.vertices.is_empty() || geometry.indices.is_empty() {
        return None;
    }
    let mut mesh = epaint::Mesh::default();
    for v in &geometry.vertices {
        mesh.vertices.push(epaint::Vertex {
            pos: Pos2::new(v.0, v.1),
            uv: egui::pos2(0.0, 0.0),
            color,
        });
    }
    mesh.indices = geometry.indices.clone();
    Some(Shape::Mesh(Arc::new(mesh)))
}

/// 返回轮廓曲线的起点与终点（用于判断是否需要 MoveTo）。
fn curve_endpoints(curve: &OutlineCurve) -> (Point, Point) {
    match curve {
        OutlineCurve::Line(p0, p1) => (*p0, *p1),
        OutlineCurve::Quad(p0, _, p2) => (*p0, *p2),
        OutlineCurve::Cubic(p0, _, _, p3) => (*p0, *p3),
    }
}

fn rx_to_color32(c: RxColor) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.a.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// 把一组公式形状平移到目标位置（左上角），按 `scale` 缩放，并整体染色为 `tint`。
/// RaTeX 输出统一为黑色，这里在绘制时按主题文字色重染色，浅色/深色主题共用同一份缓存。
pub fn transform_shapes(shapes: &[Shape], origin: Pos2, scale: f32, tint: Color32) -> Vec<Shape> {
    shapes
        .iter()
        .map(|s| transform_shape(s, origin, scale, tint))
        .collect()
}

fn transform_shape(shape: &Shape, origin: Pos2, scale: f32, tint: Color32) -> Shape {
    match shape {
        Shape::Mesh(mesh) => {
            let mut m = (**mesh).clone();
            for v in m.vertices.iter_mut() {
                v.pos = origin + v.pos.to_vec2() * scale;
                v.color = tint;
            }
            Shape::Mesh(Arc::new(m))
        }
        Shape::Rect(r) => {
            let mut r = r.clone();
            r.rect = egui::Rect::from_min_size(
                origin + r.rect.min.to_vec2() * scale,
                r.rect.size() * scale,
            );
            r.fill = tint;
            r.stroke.color = tint;
            Shape::Rect(r)
        }
        Shape::Path(p) => {
            let mut p = p.clone();
            for pt in p.points.iter_mut() {
                *pt = origin + pt.to_vec2() * scale;
            }
            p.fill = tint;
            p.stroke.color = egui::epaint::ColorMode::Solid(tint);
            Shape::Path(p)
        }
        Shape::LineSegment { points, stroke } => {
            let mut s = *stroke;
            s.color = tint;
            Shape::LineSegment {
                points: [
                    origin + points[0].to_vec2() * scale,
                    origin + points[1].to_vec2() * scale,
                ],
                stroke: s,
            }
        }
        other => other.clone(),
    }
}

/// 把一组公式形状平移到目标位置（左上角）。
#[allow(dead_code)]
pub fn translate_shapes(shapes: &[Shape], origin: Pos2) -> Vec<Shape> {
    shapes.iter().map(|s| translate_shape(s, origin)).collect()
}

fn translate_shape(shape: &Shape, origin: Pos2) -> Shape {
    let t = origin.to_vec2();
    match shape {
        Shape::Mesh(mesh) => {
            let mut m = (**mesh).clone();
            for v in m.vertices.iter_mut() {
                v.pos += t;
            }
            Shape::Mesh(Arc::new(m))
        }
        Shape::Rect(r) => {
            let mut r = r.clone();
            r.rect = r.rect.translate(t);
            Shape::Rect(r)
        }
        Shape::Path(p) => {
            let mut p = p.clone();
            for pt in p.points.iter_mut() {
                *pt += t;
            }
            Shape::Path(p)
        }
        Shape::LineSegment { points, stroke } => Shape::LineSegment {
            points: [points[0] + t, points[1] + t],
            stroke: *stroke,
        },
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 基础公式必须能产出非空形状与正尺寸。
    #[test]
    fn renders_fraction() {
        let r = render_formula("\\frac{a}{b} = c", 22.0, MathDisplay::Display).expect("应成功渲染");
        assert!(!r.shapes.is_empty(), "应产出形状");
        assert!(r.size.x > 0.0 && r.size.y > 0.0);
        assert!(r.baseline > 0.0 && r.baseline < r.size.y);
    }

    /// 行内公式尺寸应与块级不同（上下标更紧凑）。
    #[test]
    fn inline_vs_display_differ() {
        let inline = render_formula("x^2 + y^2", 22.0, MathDisplay::Inline).expect("inline");
        let display = render_formula("x^2 + y^2", 22.0, MathDisplay::Display).expect("display");
        // 块级上下标间距更大，高度一般不小于行内。
        assert!(display.size.y >= inline.size.y - 0.5);
    }

    /// 多个字形与符号都必须各自产出三角形网格，不能因轮廓连续性误判而漏掉
    /// 笔画（回归：此前用字体单位做 0.01 阈值，曲线端点容差被放大导致轮廓
    /// 被拆段，字母看起来断断续续）。
    #[test]
    fn glyphs_produce_triangles_at_inline_size() {
        // 行内小字号最容易触发轮廓断裂；覆盖普通字母、斜体数学字母、数字、
        // 运算符与分数（含横线 Path）。
        let formula = "E = mc^2, \\frac{a}{b} + \\alpha \\leq \\sqrt{x}";
        let r = render_formula(formula, 14.0, MathDisplay::Inline).expect("应成功渲染");
        let mesh_count = r
            .shapes
            .iter()
            .filter(|s| matches!(s, Shape::Mesh(_)))
            .count();
        let triangle_count: usize = r
            .shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Mesh(m) => Some(m.indices.len() / 3),
                _ => None,
            })
            .sum();
        assert!(mesh_count >= 10, "应产出多个字形网格，实际 {mesh_count}");
        assert!(triangle_count >= 100, "字形网格应有足够三角形，实际 {triangle_count}");
    }

    /// 空 / 非法公式不应崩溃，返回 None 即可。
    #[test]
    fn invalid_formula_returns_none() {
        // parse 对明显不完整的输入返回 Err
        assert!(render_formula("\\frac{", 22.0, MathDisplay::Display).is_none());
    }
}
