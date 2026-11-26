//! 图片纹理缓存与图片预览绘制。
//!
//! 本地图片直接从磁盘解码；`http(s)` 图片在后台线程下载后缓存成纹理，
//! 这样文档里写图床 URL 时无需本地文件也能直接阅读。

use super::*;
use crate::video::{VideoInfo, VideoSource};
use std::sync::mpsc::{Receiver, Sender};

/// 后台下载完成的远程图片：URL + 解码后的像素（None 表示失败）。
type RemoteImage = (String, Option<egui::ColorImage>);

/// RaTeX 栅格化得到的公式 PNG 及其尺寸信息（旧路径，仅保留用于缓存测试）。
#[derive(Clone)]
struct PngRenderedMath {
    png: Vec<u8>,
    size: egui::Vec2,
    baseline: f32,
}

/// 一张已经上传到 GPU 的公式图片。
#[derive(Clone)]
pub struct MathImage {
    pub texture: egui::TextureHandle,
    /// 逻辑尺寸（已经除以 device_pixel_ratio）。
    pub size: egui::Vec2,
    /// 基线到图片顶部的逻辑距离，用于行内公式与文字基线对齐。
    pub baseline: f32,
}

const MATH_FONT_SIZE: f32 = 22.0;
const MATH_PADDING: f32 = 4.0;
const MATH_DPR: f32 = 2.0;

/// 用 RaTeX 把 LaTeX 排版并栅格化成透明底 PNG。
/// 黑色字形 + 透明背景，绘制时再按主题 tint，这样浅色/深色可以共用同一份缓存。
fn render_math_png(tex: &str) -> Option<PngRenderedMath> {
    let ast = ratex_parser::parse(tex).ok()?;
    let layout_options = ratex_layout::LayoutOptions {
        color: ratex_types::color::Color::BLACK,
        ..Default::default()
    };
    let layout = ratex_layout::layout(&ast, &layout_options);
    let display_list = ratex_layout::to_display_list(&layout);
    let png = ratex_render::render_to_png(
        &display_list,
        &ratex_render::RenderOptions {
            font_size: MATH_FONT_SIZE,
            padding: MATH_PADDING,
            background_color: ratex_types::color::Color::new(0.0, 0.0, 0.0, 0.0),
            font_dir: String::new(),
            device_pixel_ratio: MATH_DPR,
        },
    )
    .ok()?;

    // RaTeX 的 height 是基线以上高度，depth 是基线以下深度。
    let width_px = (layout.width as f32 * MATH_FONT_SIZE + MATH_PADDING * 2.0) * MATH_DPR;
    let height_px =
        ((layout.height + layout.depth) as f32 * MATH_FONT_SIZE + MATH_PADDING * 2.0) * MATH_DPR;
    let baseline_px = (layout.height as f32 * MATH_FONT_SIZE + MATH_PADDING) * MATH_DPR;
    Some(PngRenderedMath {
        png,
        size: egui::vec2(width_px / MATH_DPR, height_px / MATH_DPR),
        baseline: baseline_px / MATH_DPR,
    })
}

#[cfg(test)]
mod math_tests {
    /// RaTeX + 内嵌字体必须真的产出一张可解码的 PNG。
    #[test]
    fn ratex_renders_png() {
        let math = super::render_math_png("\\frac{a}{b} = c").expect("RaTeX 渲染失败");
        assert!(math.png.starts_with(b"\x89PNG"), "输出不是 PNG");
        let img = image::load_from_memory(&math.png).expect("PNG 解码失败");
        assert!(img.width() > 0 && img.height() > 0);
        assert!(math.size.x > 0.0 && math.size.y > 0.0);
        assert!(math.baseline > 0.0 && math.baseline < math.size.y);
    }

    /// 块级公式提取出的字符串带首尾换行，RaTeX 也必须能渲染。
    #[test]
    fn ratex_accepts_multiline_formula() {
        assert!(
            super::render_math_png("\nE = mc^2\n").is_some(),
            "块级公式渲染失败"
        );
    }

    /// 同一个公式第二次取纹理必须命中缓存，而不是重新渲染。
    #[test]
    fn math_texture_is_cached_by_formula() {
        let ctx = egui::Context::default();
        let mut cache = super::ImageCache::new();
        let first = cache.math_texture(&ctx, "x^2 + y^2").expect("首次渲染失败");
        let second = cache.math_texture(&ctx, "x^2 + y^2").expect("缓存未命中");
        assert_eq!(
            first.texture.id(),
            second.texture.id(),
            "同一公式不应生成第二张纹理"
        );
    }
}

pub struct ImageCache {
    local: HashMap<PathBuf, Option<egui::TextureHandle>>,
    remote: HashMap<String, Option<egui::TextureHandle>>,
    /// `math` 代码块按 LaTeX 源字符串缓存纹理；None 表示渲染失败，避免每帧重试。
    math: HashMap<String, Option<MathImage>>,
    /// 原生显示列表公式缓存：key = (公式, 字号×100取整, 显示模式)。
    /// 避免每帧重新解析 RaTeX 与细分字形轮廓。
    math_native: HashMap<(String, u32, MathDisplay), Option<RenderedMath>>,
    /// 正在下载的 URL，避免同一张图重复起线程。
    pending: HashSet<String>,
    rx: Receiver<RemoteImage>,
    tx: Sender<RemoteImage>,
    /// B 站视频元数据缓存：键为视频标识，None 表示已失败（不再重试，避免每帧发请求）。
    videos: HashMap<String, Option<VideoInfo>>,
    /// 正在取元数据的视频标识。
    video_pending: HashSet<String>,
    vrx: Receiver<(String, Option<VideoInfo>)>,
    vtx: Sender<(String, Option<VideoInfo>)>,
}

impl ImageCache {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let (vtx, vrx) = std::sync::mpsc::channel();
        Self {
            local: HashMap::new(),
            remote: HashMap::new(),
            math: HashMap::new(),
            math_native: HashMap::new(),
            pending: HashSet::new(),
            rx,
            tx,
            videos: HashMap::new(),
            video_pending: HashSet::new(),
            vrx,
            vtx,
        }
    }

    pub fn texture(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
        if let Some(v) = self.local.get(path) {
            return v.clone();
        }
        let tex = (|| {
            let img = image::open(path).ok()?.to_rgba8();
            let (w, h) = (img.width() as usize, img.height() as usize);
            if w == 0 || h == 0 {
                return None;
            }
            let ci = egui::ColorImage::from_rgba_unmultiplied([w, h], img.as_raw());
            Some(ctx.load_texture(
                path.to_string_lossy().into_owned(),
                ci,
                egui::TextureOptions::LINEAR,
            ))
        })();
        self.local.insert(path.to_path_buf(), tex.clone());
        tex
    }

    /// 数学代码块纹理：RaTeX 把 LaTeX 排成 PNG，再上传成 egui 纹理。
    /// 以公式源字符串为 key，同一公式只渲染一次。
    pub fn math_texture(&mut self, ctx: &egui::Context, tex: &str) -> Option<MathImage> {
        if let Some(v) = self.math.get(tex) {
            return v.clone();
        }
        let image = render_math_png(tex).and_then(|rendered| {
            let img = image::load_from_memory(&rendered.png).ok()?.to_rgba8();
            let (w, h) = (img.width() as usize, img.height() as usize);
            if w == 0 || h == 0 {
                return None;
            }
            let ci = egui::ColorImage::from_rgba_unmultiplied([w, h], img.as_raw());
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            use std::hash::{Hash, Hasher};
            tex.hash(&mut hasher);
            Some(MathImage {
                texture: ctx.load_texture(
                    format!("markd-math:{:x}", hasher.finish()),
                    ci,
                    egui::TextureOptions::LINEAR,
                ),
                size: rendered.size,
                baseline: rendered.baseline,
            })
        });
        // 新纹理在本帧末尾才上传；请求下一帧，避免公式首帧空白。
        if image.is_some() {
            ctx.request_repaint();
        }
        self.math.insert(tex.to_string(), image.clone());
        image
    }

    /// 原生显示列表公式：直接把 RaTeX 显示列表转成 epaint 形状，不走 PNG。
    /// 按 (公式, 字号, 显示模式) 缓存，解析与字形细分只做一次。
    pub fn math_shapes(
        &mut self,
        tex: &str,
        font_size: f32,
        display: MathDisplay,
    ) -> Option<RenderedMath> {
        let key = (tex.to_string(), (font_size * 100.0).round() as u32, display);
        if let Some(v) = self.math_native.get(&key) {
            return v.clone();
        }
        let rendered = crate::render::math::render_formula(tex, font_size, display);
        self.math_native.insert(key, rendered.clone());
        rendered
    }

    /// 远程图片纹理：命中缓存直接返回；未命中时启动后台下载（每张图只下一次）。
    pub fn remote_texture(
        &mut self,
        ctx: &egui::Context,
        url: &str,
    ) -> Option<egui::TextureHandle> {
        while let Ok((u, img)) = self.rx.try_recv() {
            self.pending.remove(&u);
            let tex = img.map(|ci| {
                ctx.load_texture(
                    format!("markd-remote:{u}"),
                    ci,
                    egui::TextureOptions::LINEAR,
                )
            });
            self.remote.insert(u, tex);
        }
        if let Some(t) = self.remote.get(url) {
            return t.clone();
        }
        // 已失败过（缓存了 None）就不再自动重试，避免每帧重新发请求。
        if !self.remote.contains_key(url) && !self.pending.contains(url) {
            self.pending.insert(url.to_string());
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            let url = url.to_string();
            std::thread::spawn(move || {
                let img = crate::image_host::fetch_remote_image(&url);
                let _ = tx.send((url, img));
                ctx.request_repaint();
            });
        } else if self.pending.contains(url) {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        None
    }

    /// 该远程图片是否仍在下载中（用于显示「正在加载」占位框）。
    pub fn remote_loading(&self, url: &str) -> bool {
        self.pending.contains(url)
    }

    /// B 站视频元数据（标题/封面/时长）：命中缓存直接返回；
    /// 未命中时起后台线程调接口——渲染每帧都跑，网络请求只做一次。
    pub fn video_info(&mut self, ctx: &egui::Context, src: &VideoSource) -> Option<&VideoInfo> {
        while let Ok((id, info)) = self.vrx.try_recv() {
            self.video_pending.remove(&id);
            self.videos.insert(id, info);
        }
        let key = src.id();
        if let Some(v) = self.videos.get(&key) {
            return v.as_ref();
        }
        if !self.video_pending.contains(&key) {
            self.video_pending.insert(key.clone());
            let tx = self.vtx.clone();
            let ctx = ctx.clone();
            let src = src.clone();
            std::thread::spawn(move || {
                let info = crate::video::fetch_video_info(&src);
                let _ = tx.send((src.id(), info));
                ctx.request_repaint();
            });
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        None
    }

    /// 该视频是否仍在取元数据（卡片据此显示「正在获取…」而不是「取不到」）。
    pub fn video_loading(&self, src: &VideoSource) -> bool {
        self.video_pending.contains(&src.id())
    }

    /// 测试专用：预置元数据，免得单测真去访问 B 站接口。
    #[cfg(test)]
    pub fn prime_video(&mut self, src: &VideoSource, info: Option<VideoInfo>) {
        self.videos.insert(src.id(), info);
    }
}

/// 图片预览：限宽限高、居中、带细边框；右下角可拖动缩放。
///
/// 返回 `(响应, 拖动产生的新宽度)`：调用方把新宽度写回文档的
/// `{width=N}` 属性，下一帧就按新宽度显示。
pub fn image_preview(
    ui: &mut egui::Ui,
    alt: &str,
    url: &str,
    theme: &Theme,
    ctx: &egui::Context,
    images: &mut ImageCache,
    base_dir: Option<&Path>,
    width_attr: Option<u32>,
    grip: (usize, usize),
) -> (egui::Response, Option<u32>) {
    // 远程图片（图床 URL）后台下载后缓存；本地图片走磁盘解码。
    let texture = if crate::image_host::is_remote(url) {
        images.remote_texture(ctx, url)
    } else {
        workspace::resolve(base_dir, url).and_then(|path| images.texture(ctx, &path))
    };
    if let Some(tex) = texture {
        let natural = tex.size_vec2();
        let max_w = ui.available_width().max(80.0);
        // 预览不铺满整屏：宽度不超正文，高度最多占视口的六成。
        let max_h = (ui.clip_rect().height() * 0.6).max(160.0);
        let aspect = (natural.x / natural.y.max(1.0)).max(0.01);
        // 默认按原始尺寸；有 width 属性（拖动缩放写回的）就按属性显示。
        let target_w = width_attr.map(|w| w as f32).unwrap_or(natural.x);
        let mut scale = (target_w / natural.x.max(1.0))
            .min(max_w / natural.x.max(1.0))
            .min(max_h / natural.y.max(1.0));
        // 最小宽度 48，但不要为了这个下限突破视口限制。
        let min_scale = (48.0 / natural.x.max(1.0))
            .min(max_w / natural.x.max(1.0))
            .min(max_h / natural.y.max(1.0));
        scale = scale.max(min_scale);
        let size = Vec2::new((natural.x * scale).max(16.0), (natural.y * scale).max(16.0));
        // 整行占满正文宽度，图片在行内居中：插图后不会孤零零贴在左边。
        let (row, resp) = ui.allocate_exact_size(Vec2::new(max_w, size.y), Sense::click());
        let rect = Rect::from_center_size(row.center(), size);
        let painter = ui.painter();
        painter.image(
            tex.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        // 细边框：白色/浅色截图在浅色主题下不会和背景糊在一起。
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, theme.border),
            egui::StrokeKind::Inside,
        );
        // 右下角缩放手柄：只在悬停（或正在拖）时显示，不打扰正常阅读。
        let grip_center = rect.max - Vec2::splat(9.0);
        let grip_rect = Rect::from_center_size(grip_center, Vec2::splat(18.0));
        let grip_resp = ui
            .interact(
                grip_rect,
                egui::Id::unique(("img_grip", grip.0, grip.1)),
                Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::ResizeNwSe);
        if resp.hovered() || grip_resp.hovered() || grip_resp.dragged() {
            let p = ui.painter();
            for k in 0..3 {
                let off = 2.0 + k as f32 * 3.5;
                p.line_segment(
                    [
                        Pos2::new(grip_center.x - 6.0 + off, grip_center.y + 6.0),
                        Pos2::new(grip_center.x + 6.0, grip_center.y + 6.0 - off),
                    ],
                    Stroke::new(1.4, theme.dim),
                );
            }
        }
        let mut resized = None;
        if grip_resp.dragged() {
            if let Some(p) = grip_resp.interact_pointer_pos() {
                // 图片水平居中：以中心为轴，指针到中心的距离 ×2 就是目标宽度。
                let want = (p.x - row.center().x) * 2.0;
                let limit = max_w.min(max_h * aspect).max(48.0);
                let w = want.clamp(48.0, limit).round() as u32;
                if width_attr != Some(w) {
                    resized = Some(w);
                }
            }
        }
        return (resp.on_hover_text(url), resized);
    }
    // 远程图片还在下载时显示「正在加载」；本地图片找不到（路径写错/文件被删）
    // 或下载失败时显示占位框，比一行灰字更容易认出这里原本是图片。
    let loading = crate::image_host::is_remote(url) && images.remote_loading(url);
    let name = if alt.is_empty() { url } else { alt };
    let label = if loading {
        format!("正在加载图片：{name}")
    } else {
        format!("图片未找到：{name}")
    };
    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 56.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(row, 4.0, theme.code_bg);
    painter.rect_stroke(
        row,
        4.0,
        egui::Stroke::new(1.0, theme.border),
        egui::StrokeKind::Inside,
    );
    let f = theme.fmt(theme.body * 0.9);
    painter.text(
        row.center(),
        egui::Align2::CENTER_CENTER,
        label,
        f.font_id,
        theme.dim,
    );
    (resp.on_hover_text(url), None)
}
