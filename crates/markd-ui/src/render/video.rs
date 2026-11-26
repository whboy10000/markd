//! 视频块渲染：把 ```video 围栏画成一张播放器卡片。
//!
//! 应用内是原生绘制，嵌不了 B 站的 web 播放器，所以卡片给封面 + 标题 + 播放键，
//! 点击交给系统浏览器；真正「在页面里播」由导出 HTML 时的 iframe 承担
//! （见 `markd-core::export::video_html`）。

use super::*;
use crate::video::VideoSource;

/// 这个代码块是不是视频围栏：语言标记是 video/bilibili，且内容能解析出 B 站链接。
/// 判据不满足时返回 None，按普通代码块显示——手写的 ```video 说明文字不会被吞掉。
pub(crate) fn video_source_of(lang: &str, code: &str) -> Option<VideoSource> {
    if !crate::video::is_video_lang(lang) {
        return None;
    }
    crate::video::url_from_fence(code)
}

/// 播放键三角形：圆心处一个指向右方的白色三角。
fn play_triangle(rect: Rect, r: f32) -> egui::epaint::PathShape {
    let c = rect.center();
    egui::epaint::PathShape {
        points: vec![
            c + egui::vec2(r * -0.32, r * -0.46),
            c + egui::vec2(r * -0.32, r * 0.46),
            c + egui::vec2(r * 0.52, 0.0),
        ],
        closed: true,
        fill: Color32::WHITE,
        stroke: egui::Stroke::NONE.into(),
    }
}

/// 卡片封面的最大宽度（逻辑点）：正文列宽可以到 900+，16:9 铺满就成了一面墙。
/// 420 宽的封面高 236，约等于常见嵌入播放器的尺寸，读长文时不会把正文挤走。
pub(crate) const VIDEO_CARD_MAX_WIDTH: f32 = 420.0;

/// 播放器卡片：16:9 封面 + 时长角标 + 标题 + 链接，播放键走系统浏览器。
///
/// 卡片限宽 `VIDEO_CARD_MAX_WIDTH` 并在正文列里居中，比代码块窄。
/// 卡片本身的点击不进入编辑态（与代码块不同）：点封面是为了看视频，
/// 想改链接用右上角「编辑」按钮，它走 `Action::Jump` 让本块变成编辑态。
pub(crate) fn video_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    env: &mut RenderEnv<'_>,
    idx: usize,
    src: &VideoSource,
    url_text: &str,
) -> egui::Response {
    let ctx = env.ctx;
    let info = env.images.video_info(ctx, src).cloned();
    let full_w = ui.available_width().min(env.max_width);
    let card_w = full_w.clamp(220.0, VIDEO_CARD_MAX_WIDTH);
    // 顶层用居中布局，卡片就落在正文列中轴上（与插图同一视觉语言）。
    let outer = ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        panel_frame(theme).show(ui, |ui| {
            // 面板宽度定死成 card_w，右侧「删除 / 编辑 / 播放」才贴着卡片而不是整行。
            ui.set_width(card_w);
            // 操作条：左标记，右按钮。与代码块功能栏同一视觉语言。
            let mut play = false;
            let mut edit = false;
            let mut del = false;
            code_header_frame(theme).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("视频 · bilibili")
                            .size(theme.body * 0.9)
                            .color(theme.text),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if ui
                            .button(egui::RichText::new("删除").size(theme.body * 0.85))
                            .clicked()
                        {
                            del = true;
                        }
                        if ui
                            .button(egui::RichText::new("编辑").size(theme.body * 0.85))
                            .clicked()
                        {
                            edit = true;
                        }
                        if ui
                            .button(egui::RichText::new("▶ 播放").size(theme.body * 0.85))
                            .on_hover_text("在系统浏览器中打开 B 站播放页")
                            .clicked()
                        {
                            play = true;
                        }
                    });
                });
            });
            ui.add_space(8.0);
            // 封面区：固定 16:9，指针落在这一片即可播放。
            let media_h = card_w * 9.0 / 16.0;
            let (rect, media) = ui.allocate_exact_size(egui::vec2(card_w, media_h), Sense::click());
            let painter = ui.painter();
            // 黑底：封面未到或比例不符时露出的边角是影厅黑，不是面板灰。
            painter.rect_filled(rect, 4.0, Color32::from_rgb(16, 17, 20));
            let cover = info
                .as_ref()
                .and_then(|i| env.images.remote_texture(ctx, &i.cover));
            if let Some(tex) = &cover {
                // 等比铺满（cover 语义）：短边贴齐，超出部分由矩形边界裁掉。
                let natural = tex.size_vec2();
                let scale =
                    (rect.width() / natural.x.max(1.0)).max(rect.height() / natural.y.max(1.0));
                let drawn = Rect::from_center_size(rect.center(), natural * scale);
                painter.image(
                    tex.id(),
                    drawn,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
                // 轻微压暗，让白色播放键在亮封面上也有对比。
                painter.rect_filled(rect, 4.0, Color32::from_black_alpha(46));
            } else if info.is_none() {
                let text = if env.images.video_loading(src) {
                    "正在获取视频信息…"
                } else {
                    "取不到视频信息，可直接点「播放」"
                };
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_BOTTOM,
                    text,
                    FontId::proportional(theme.body * 0.95),
                    theme.dim,
                );
            }
            let r = (rect.height() * 0.16).clamp(18.0, 34.0);
            painter.circle(
                rect.center(),
                r,
                Color32::from_black_alpha(130),
                egui::Stroke::new(1.4, Color32::from_white_alpha(210)),
            );
            painter.add(play_triangle(rect, r));
            // 时长角标（右下角）。
            let dur = info.as_ref().map(|i| i.duration_text()).unwrap_or_default();
            if !dur.is_empty() {
                let g = painter.layout_no_wrap(
                    dur.clone(),
                    FontId::proportional(theme.body * 0.8),
                    Color32::WHITE,
                );
                let box_rect = Rect::from_min_size(
                    rect.max - g.size() - egui::vec2(16.0, 12.0),
                    g.size() + egui::vec2(10.0, 4.0),
                );
                painter.rect_filled(box_rect, 3.0, Color32::from_black_alpha(180));
                painter.text(
                    box_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    dur,
                    FontId::proportional(theme.body * 0.8),
                    Color32::WHITE,
                );
            }
            ui.add_space(8.0);
            // 标题：元数据没到先用视频标识占位，避免卡片上出现空白行。
            let title = info
                .as_ref()
                .map(|i| i.title.clone())
                .unwrap_or_else(|| format!("Bilibili 视频 {}", src.id()));
            let mut job = LayoutJob::default();
            job.append(
                &title,
                0.0,
                TextFormat::simple(FontId::proportional(theme.body), theme.text),
            );
            let (title_resp, _) = layout_label(ui, job, card_w, Sense::hover());
            // 链接本身也显示出来：断网时它仍是唯一能看到、能选中复制的信息。
            ui.label(
                egui::RichText::new(url_text)
                    .font(FontId::monospace(theme.mono * 0.9))
                    .color(theme.link),
            );
            (
                media.clicked() || play,
                edit || title_resp.double_clicked(),
                del,
            )
        })
    });
    let card = outer.inner;
    let (want_play, want_edit, want_delete) = card.inner;
    if want_play {
        env.actions.push(Action::OpenLink(src.page_url()));
    }
    if want_edit {
        env.actions.push(Action::Jump(idx));
    }
    if want_delete {
        env.actions.push(Action::AskDeleteBlock(idx));
    }
    // 返回给编辑器的是整张卡片的「仅悬停」响应：跨块拖选和选区底色按这块的
    // 真实矩形算，但点击不会被当成「进入编辑」——点封面是去播视频的。
    ui.interact(
        card.response.rect,
        ui.scope_id().with(("video_card", idx)),
        Sense::hover(),
    )
}
