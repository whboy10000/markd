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
//!
//! 像素对齐：公式的平移缩放发生在绘制期（`transform_shapes`），而 epaint 对
//! `Shape::Mesh` 的顶点原样透传——既不 feathering 也不吸附栅格。所以那里必须把
//! 落点吸附到物理像素栅格，并把矩形线条转成栅格对齐的实心网格；否则细如 0.56pt
//! 的分数线会踩到 epaint 的「细线」分支，被摊成 ±1 物理像素的渐隐三角脊
//! （实测：1x 屏上中心只有 66% 墨量、上下各多出一行 11% 的灰边）。

use std::collections::HashMap;
use std::sync::Arc;

use ab_glyph::{Font, FontRef, FontVec, OutlineCurve, Point};
use egui::emath::GuiRounding;
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
    /// 真实墨迹包围盒（与 `shapes` 同一坐标系）。
    /// RaTeX 的 `width` 不含左侧影，墨迹可以从 x=0 以左开始；行内公式要靠它
    /// 才知道自己到底占多宽、该往右挪多少，否则会把相邻正文挤出占位区。
    pub ink: egui::Rect,
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

    // 墨迹包围盒：公式只产出字形网格与实心矩形（分数线、根号横线）。
    let mut ink = egui::Rect::NOTHING;
    for s in &shapes {
        if let Some(b) = shape_bounds(s) {
            ink = ink.union(b);
        }
    }

    Some(RenderedMath {
        shapes,
        size: Vec2::new(dl.width as f32 * em, (dl.height + dl.depth) as f32 * em),
        baseline: dl.height as f32 * em,
        ink,
    })
}

/// 单个形状的包围盒（只处理公式会产出的两类）。
fn shape_bounds(s: &Shape) -> Option<egui::Rect> {
    match s {
        Shape::Mesh(m) => Some(m.calc_bounds()),
        Shape::Rect(r) => Some(r.rect),
        _ => None,
    }
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
///
/// `ppp` 是 `pixels_per_point`（一个逻辑点合多少个物理像素）。公式的平移缩放发生在
/// 绘制期，而 epaint 对 `Shape::Mesh` 的顶点是原样透传的（既不 feathering 也不吸附
/// 栅格），原点差半个像素就会让每根笔画一起被 MSAA 摊出一圈半透明边，和已经吸附过
/// 栅格的正文文字形成两套清晰度。所以落点先吸附到物理像素栅格，矩形笔画另行处理。
pub fn transform_shapes(
    shapes: &[Shape],
    origin: Pos2,
    scale: f32,
    tint: Color32,
    ppp: f32,
) -> Vec<Shape> {
    let origin = origin.round_to_pixels(ppp);
    shapes
        .iter()
        .map(|s| transform_shape(s, origin, scale, tint, ppp))
        .collect()
}

/// 把矩形吸附到物理像素栅格：四条边落在整像素边界上，每个方向至少一个物理像素，
/// 中心尽量不动。
fn snap_to_pixel_grid(rect: egui::Rect, ppp: f32) -> egui::Rect {
    let ppp = if ppp.is_finite() { ppp.max(0.01) } else { 1.0 };
    let w = (rect.width() * ppp).round().max(1.0);
    let h = (rect.height() * ppp).round().max(1.0);
    let left = (rect.center().x * ppp - w * 0.5).round();
    let top = (rect.center().y * ppp - h * 0.5).round();
    egui::Rect::from_min_size(
        Pos2::new(left / ppp, top / ppp),
        Vec2::new(w / ppp, h / ppp),
    )
}

/// 实心矩形网格（与字形网格同一种顶点格式）。
fn solid_rect_mesh(rect: egui::Rect, color: Color32) -> epaint::Mesh {
    let mut mesh = epaint::Mesh::default();
    for pos in [
        rect.min,
        Pos2::new(rect.max.x, rect.min.y),
        rect.max,
        Pos2::new(rect.min.x, rect.max.y),
    ] {
        mesh.vertices.push(epaint::Vertex {
            pos,
            uv: egui::pos2(0.0, 0.0),
            color,
        });
    }
    mesh.indices = vec![0, 1, 2, 0, 2, 3];
    mesh
}

fn transform_shape(shape: &Shape, origin: Pos2, scale: f32, tint: Color32, ppp: f32) -> Shape {
    match shape {
        Shape::Mesh(mesh) => {
            let mut m = (**mesh).clone();
            for v in m.vertices.iter_mut() {
                v.pos = origin + v.pos.to_vec2() * scale;
                v.color = tint;
            }
            Shape::Mesh(Arc::new(m))
        }
        // 分数线、根号横线、`\colorbox`：直角实心矩形。
        Shape::Rect(r) if r.stroke.width <= 0.0 && r.corner_radius == egui::CornerRadius::ZERO => {
            // 不交给 epaint 画矩形：14px 字号的分数线只有 0.56 逻辑像素厚，
            // `tessellator.rs` 对「厚度 ≤ 2×feathering」的矩形退化成画一条线，
            // 线宽再 ≤ 0.9×feathering 时索性把它摊成 ±1 物理像素的三角脊并按比例
            // 压低不透明度——这就是公式边缘向外突出的那圈半透明像素。
            // 栅格对齐的实心网格没有 feathering，边落在整像素上，MSAA 的覆盖率
            // 非 0 即 1，一个像素都不外溢。
            let rect = egui::Rect::from_min_max(
                origin + r.rect.min.to_vec2() * scale,
                origin + r.rect.max.to_vec2() * scale,
            );
            Shape::Mesh(Arc::new(solid_rect_mesh(
                snap_to_pixel_grid(rect, ppp),
                tint,
            )))
        }
        Shape::Rect(r) => {
            let mut r = r.clone();
            r.rect = egui::Rect::from_min_size(
                origin + r.rect.min.to_vec2() * scale,
                r.rect.size() * scale,
            );
            r.fill = tint;
            r.stroke.color = tint;
            r.round_to_pixels = Some(true);
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
        assert!(
            triangle_count >= 100,
            "字形网格应有足够三角形，实际 {triangle_count}"
        );
    }

    /// 吸附到物理像素栅格：边落在整像素上、厚度至少一个物理像素、中心不漂出半个像素。
    /// 分数线在 14px 字号下只有 0.56 逻辑像素厚——1x 屏上连一个物理像素都不到。
    #[test]
    fn snap_to_pixel_grid_keeps_thin_bars_visible() {
        let bar = egui::Rect::from_min_max(egui::pos2(1.68, 7.424), egui::pos2(6.86, 7.984));
        for ppp in [1.0f32, 1.5, 2.0, 3.0] {
            let s = snap_to_pixel_grid(bar, ppp);
            for (name, v) in [
                ("min.x", s.min.x),
                ("min.y", s.min.y),
                ("max.x", s.max.x),
                ("max.y", s.max.y),
            ] {
                let dev = v * ppp;
                assert!(
                    (dev - dev.round()).abs() < 1e-3,
                    "ppp={ppp}：{name}={v} 没落在物理像素栅格上（{dev}）"
                );
            }
            assert!(
                s.height() * ppp >= 1.0 - 1e-3 && s.width() * ppp >= 1.0 - 1e-3,
                "ppp={ppp}：吸附后线条不足一个物理像素（{s:?}）"
            );
            let drift = (s.center() - bar.center()).length();
            assert!(
                drift <= 1.0 / ppp,
                "ppp={ppp}：中心漂了 {drift:.3}，超出一个物理像素"
            );
        }
    }

    /// 回归「公式边缘长出半透明绒毛」：
    /// ① 分数线 / 根号横线不再以 `Shape::Rect` 交给 epaint——细矩形会被退化成
    ///    ±1 物理像素的三角脊并按厚度比例压低不透明度，那就是向外突出的那圈像素——
    ///    改成栅格对齐的实心网格；
    /// ② 字形网格与线条共用同一个已吸附的原点，整条公式不会整体错半个像素。
    #[test]
    fn bars_are_painted_as_grid_aligned_meshes() {
        for ppp in [1.0f32, 2.0] {
            let r = render_formula("\\frac{a}{b}+\\sqrt{x}", 14.0, MathDisplay::Inline)
                .expect("render");
            assert!(
                r.shapes.iter().any(|s| matches!(s, Shape::Rect(_))),
                "排版阶段仍应产出矩形线条（绘制期才转换）"
            );
            let shapes = transform_shapes(
                &r.shapes,
                egui::pos2(100.37, 50.61),
                0.97,
                Color32::BLACK,
                ppp,
            );
            assert!(
                shapes.iter().all(|s| !matches!(s, Shape::Rect(_))),
                "ppp={ppp}：公式里不该再有走 epaint feathering 的矩形"
            );
            // 四顶点 + 两个三角形 = 本模块生成的实心矩形线条。
            let mut n = 0;
            for m in shapes.iter().filter_map(|s| match s {
                Shape::Mesh(m) if m.vertices.len() == 4 => Some(m.clone()),
                _ => None,
            }) {
                n += 1;
                for v in &m.vertices {
                    for (name, c) in [("x", v.pos.x), ("y", v.pos.y)] {
                        let dev = c * ppp;
                        assert!(
                            (dev - dev.round()).abs() < 1e-2,
                            "ppp={ppp}：线条顶点 {name}={c} 偏离物理像素栅格（{dev}）"
                        );
                    }
                }
                let b = m.calc_bounds();
                assert!(
                    b.height() * ppp >= 1.0 - 1e-2 && b.width() * ppp >= 1.0 - 1e-2,
                    "ppp={ppp}：线条厚度不足一个物理像素（{b:?}）"
                );
                assert!(
                    m.vertices.iter().all(|v| v.color == Color32::BLACK),
                    "线条应整体染成主题文字色"
                );
            }
            assert!(
                n >= 2,
                "ppp={ppp}：分数线与根号横线都应转成网格，实际 {n} 条"
            );
        }
    }

    /// 修复前的等价画法：矩形线条直接交给 epaint（不吸附、不转网格）。
    fn legacy_rect_bars(r: &RenderedMath, origin: egui::Pos2, scale: f32) -> Vec<Shape> {
        r.shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Rect(rect) => Some(Shape::rect_filled(
                    egui::Rect::from_min_max(
                        origin + rect.rect.min.to_vec2() * scale,
                        origin + rect.rect.max.to_vec2() * scale,
                    ),
                    0.0,
                    Color32::BLACK,
                )),
                _ => None,
            })
            .collect()
    }

    /// 把形状交给 epaint 真实细分，数「线条外扩 1.5 物理像素的环带里、落在线条之外」
    /// 的半透明顶点——那一圈渐隐顶点就是用户看到的「蒲公英绒毛」。
    fn translucent_ring_vertices(shapes: &[Shape], band: egui::Rect, ppp: f32) -> usize {
        let ring = band.expand(1.5 / ppp);
        let inside = band.expand(0.001);
        let mut tess = epaint::Tessellator::new(
            ppp,
            epaint::TessellationOptions::default(),
            [512, 512],
            Vec::new(),
        );
        let mut count = 0;
        for s in shapes {
            let mut out = epaint::Mesh::default();
            tess.tessellate_shape(s.clone(), &mut out);
            count += out
                .vertices
                .iter()
                .filter(|v| v.color.a() < 255 && ring.contains(v.pos) && !inside.contains(v.pos))
                .count();
        }
        count
    }

    /// 用 epaint 的真实细分结果量化「向外突出的半透明像素」：修复后线条周围一个
    /// 都不该有；同一个量在修复前的画法（矩形）上必须数得出来，否则这个度量本身
    /// 就是假的。
    #[test]
    fn bars_produce_no_translucent_feather_ring() {
        let origin = egui::pos2(100.37, 50.61);
        let scale = 0.97;
        for ppp in [1.0f32, 2.0] {
            let r = render_formula("\\frac{a}{b}+\\sqrt{x}", 14.0, MathDisplay::Inline)
                .expect("render");
            let shapes = transform_shapes(&r.shapes, origin, scale, Color32::BLACK, ppp);
            let bars: Vec<egui::Rect> = shapes
                .iter()
                .filter_map(|s| match s {
                    Shape::Mesh(m) if m.vertices.len() == 4 => Some(m.calc_bounds()),
                    _ => None,
                })
                .collect();
            assert_eq!(bars.len(), 2, "ppp={ppp}：应有分数线 + 根号横线两条");
            for band in &bars {
                assert_eq!(
                    translucent_ring_vertices(&shapes, *band, ppp),
                    0,
                    "ppp={ppp}：线条 {band:?} 外围出现了 feathering 渐隐顶点"
                );
            }
            let legacy: Vec<egui::Rect> = legacy_rect_bars(&r, origin, scale)
                .iter()
                .map(|s| match s {
                    Shape::Rect(rect) => rect.rect,
                    _ => unreachable!(),
                })
                .collect();
            let fuzz = legacy
                .iter()
                .map(|band| {
                    translucent_ring_vertices(&legacy_rect_bars(&r, origin, scale), *band, ppp)
                })
                .sum::<usize>();
            assert!(
                fuzz > 0,
                "ppp={ppp}：对照组（矩形线条）本该数出渐隐顶点，度量失效"
            );
        }
    }

    /// 逐物理像素量化墨量：把形状交给 epaint 细分后，按每像素 4×4 采样点做
    /// src-over 叠加（黑色墨迹），返回 `y0..y0+rows` 每一行的平均墨量
    /// （0.0 = 纯白，1.0 = 实色）。参数一律用物理像素坐标。
    ///
    /// 轴对齐且落在栅格上的边在这个采样网面上非 0 即 1，与 GPU MSAA 的结果一致，
    /// 所以这一列数字就是屏幕上的灰度剖面。
    fn row_ink_profile(
        shape: &Shape,
        x0: f32,
        x1: f32,
        y0: f32,
        rows: usize,
        ppp: f32,
    ) -> Vec<f32> {
        let mut mesh = epaint::Mesh::default();
        epaint::Tessellator::new(
            ppp,
            epaint::TessellationOptions::default(),
            [512, 512],
            Vec::new(),
        )
        .tessellate_shape(shape.clone(), &mut mesh);
        // 顶点位置换算到物理像素，避免后面每个采样点重复乘。
        let tri: Vec<([egui::Pos2; 3], [f32; 3])> = (0..mesh.indices.len() / 3)
            .map(|i| {
                let v: Vec<&epaint::Vertex> = (0..3)
                    .map(|k| &mesh.vertices[mesh.indices[i * 3 + k] as usize])
                    .collect();
                (
                    [v[0].pos, v[1].pos, v[2].pos].map(|p| egui::pos2(p.x * ppp, p.y * ppp)),
                    [0, 1, 2].map(|k| v[k].color.a() as f32 / 255.0),
                )
            })
            .collect();
        let cross = |a: egui::Pos2, b: egui::Pos2, c: egui::Pos2| {
            (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
        };
        (0..rows)
            .map(|row| {
                let mut total = 0.0f32;
                let mut n = 0.0f32;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let px = x0 + (sx as f32 + 0.5) * (x1 - x0) / 4.0;
                        let p = egui::pos2(px, y0 + row as f32 + (sy as f32 + 0.5) / 4.0);
                        let mut clear = 1.0f32;
                        for (t, a) in &tri {
                            let area = cross(t[0], t[1], t[2]);
                            if area == 0.0 {
                                continue;
                            }
                            let w = [
                                cross(t[1], t[2], p) / area,
                                cross(t[2], t[0], p) / area,
                                cross(t[0], t[1], p) / area,
                            ];
                            if w.iter().any(|v| *v < -1e-6) || w.iter().any(|v| *v > 1.0 + 1e-6) {
                                continue;
                            }
                            let alpha = w[0] * a[0] + w[1] * a[1] + w[2] * a[2];
                            clear *= 1.0 - alpha.clamp(0.0, 1.0);
                        }
                        total += 1.0 - clear;
                        n += 1.0;
                    }
                }
                total / n
            })
            .collect()
    }

    /// 屏幕上的灰度剖面：修复后分数线正好占满一个物理像素行、上下全白；
    /// 修复前同一条线被摊成好几行半明半暗的像素，中心还达不到实色——
    /// 那就是「向外突出的半透明像素 / 蒲公英绒毛」。
    #[test]
    fn fraction_bar_inks_exactly_one_row() {
        let r = render_formula("\\frac{1}{2}", 22.0, MathDisplay::Display).expect("render");
        let src = r
            .shapes
            .iter()
            .find_map(|s| match s {
                Shape::Rect(rect) if rect.rect.width() > 4.0 => Some(rect.rect),
                _ => None,
            })
            .expect("应有分数线矩形");
        let origin = egui::pos2(100.0, 50.0);
        for ppp in [1.0f32, 2.0] {
            let fixed = transform_shapes(&r.shapes, origin, 1.0, Color32::BLACK, ppp)
                .into_iter()
                .find(|s| match s {
                    Shape::Mesh(m) => m.vertices.len() == 4 && m.calc_bounds().width() * ppp > 4.0,
                    _ => false,
                })
                .expect("修复后分数线应是四顶点网格");
            let bar = match &fixed {
                Shape::Mesh(m) => m.calc_bounds(),
                _ => unreachable!(),
            };
            // 修复前的等价画法：同一条线以矩形提交。
            let legacy = Shape::rect_filled(
                egui::Rect::from_min_max(origin + src.min.to_vec2(), origin + src.max.to_vec2()),
                0.0,
                Color32::BLACK,
            );
            let (x0, x1) = (bar.min.x * ppp + 2.0, bar.max.x * ppp - 2.0);
            let top = (bar.min.y * ppp).floor() - 2.0;
            let rows = ((bar.height() * ppp).floor() as usize) + 5;
            let after = row_ink_profile(&fixed, x0, x1, top, rows, ppp);
            let before = row_ink_profile(&legacy, x0, x1, top, rows, ppp);
            let solid = after.iter().filter(|v| **v > 0.999).count();
            let spill = after.iter().filter(|v| 0.001 < **v && **v < 0.999).count();
            assert_eq!(
                solid,
                (bar.height() * ppp).round() as usize,
                "ppp={ppp}：修复后剖面应为 {after:?}",
            );
            assert_eq!(spill, 0, "ppp={ppp}：修复后仍有非整像素的过渡行：{after:?}");
            assert!(
                before.iter().copied().fold(f32::MIN, f32::max) < 0.999,
                "度量失效：修复前的矩形画法本不该出现实色行 {before:?}"
            );
            assert!(
                before.iter().filter(|v| 0.05 < **v).count() > solid,
                "度量失效：修复前本该把墨量摊到更多行上 {before:?}"
            );
        }
    }

    /// 同一个物理像素内的任意落点必须画出完全一样的形状——落点已吸附到栅格，
    /// 不会出现「同一个公式在行内比在块级糊一点」的相位差。
    #[test]
    fn sub_pixel_placement_is_quantized() {
        let ppp = 2.0;
        let r = render_formula("\\frac{a}{b}", 14.0, MathDisplay::Inline).expect("render");
        let positions = |origin: egui::Pos2| -> Vec<(f32, f32)> {
            transform_shapes(&r.shapes, origin, 1.0, Color32::BLACK, ppp)
                .iter()
                .flat_map(|s| match s {
                    Shape::Mesh(m) => m.vertices.iter().map(|v| (v.pos.x, v.pos.y)).collect(),
                    _ => Vec::new(),
                })
                .collect()
        };
        let base = positions(egui::pos2(100.0, 50.0));
        assert!(!base.is_empty());
        assert_eq!(
            base,
            positions(egui::pos2(100.2, 50.2)),
            "同一个物理像素内的位移不该改变任何顶点"
        );
        assert_ne!(
            base,
            positions(egui::pos2(100.5, 50.0)),
            "整一个物理像素的位移必须真的挪动"
        );
    }

    /// 空 / 非法公式不应崩溃，返回 None 即可。
    #[test]
    fn invalid_formula_returns_none() {
        // parse 对明显不完整的输入返回 Err
        assert!(render_formula("\\frac{", 22.0, MathDisplay::Display).is_none());
    }
}
