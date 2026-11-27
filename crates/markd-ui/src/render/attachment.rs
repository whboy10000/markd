//! 附件卡片：把 `[季度报告.xlsx](files/季度报告.xlsx)` 画成一条能点开的文件。
//!
//! 文档里它就是一个普通 Markdown 链接——别的渲染器看到的是链接，本编辑器按后缀
//! 认出是文件，给类型徽标、真实文件名、大小和来源。点文件名 = 下载并预览，
//! 「新标签页」交给系统（远程走浏览器，本地走 `file://`），与视频卡片同一套动作。

use super::*;
use crate::attachment::{self, Kind};

/// 卡片最大宽度（逻辑点）：文件条比视频卡窄，一屏宽的文件名只会更好看，不会更有用。
pub(crate) const FILE_CARD_MAX_WIDTH: f32 = 520.0;

/// 段落是否「只有一个附件链接」。是则返回 (地址, 链接文字, 类型)。
///
/// 句中夹着文件链接的段落不触发，保持普通链接的排版——一句话里塞一张卡片会打断阅读。
pub(crate) fn attachment_of(content: &[Inline]) -> Option<(&str, String, Kind)> {
    if content.len() != 1 {
        return None;
    }
    let Inline::Link { label, url } = &content[0] else {
        return None;
    };
    let kind = attachment::kind_of(url)?;
    Some((url, crate::ast::inline_plain(label), kind))
}

/// 附件此刻在哪儿：本机的原文、已下载的远程副本，还是只有地址。
enum Located {
    /// 文档目录里就有这个文件（路径 + 字节数）。
    Local(PathBuf, u64),
    /// 远程文件已经下到缓存目录（路径 + 字节数）。
    Cached(PathBuf, u64),
    /// 还没拿到实体：远程未下载，或本地文件已挪走。
    Absent,
}

/// 解析附件的可打开路径。只做 `stat`，每帧一次，不做缓存也不留过期状态：
/// 用户把文件下完、或把它挪走，下一帧就该如实显示。
fn locate(base_dir: Option<&Path>, url: &str) -> Located {
    if let Some(p) = attachment::local_path(base_dir, url)
        && let Ok(md) = std::fs::metadata(&p)
    {
        return Located::Local(p, md.len());
    }
    if crate::image_host::is_remote(url)
        && let p = attachment::cached_path(url)
        && let Ok(md) = std::fs::metadata(&p)
    {
        return Located::Cached(p, md.len());
    }
    Located::Absent
}

/// 徽标底色：按类型分色，一眼分得清 PDF / 表格 / 压缩包。
/// 取中间调，深色与浅色面板上都有足够对比。
fn kind_color(kind: Kind) -> Color32 {
    match kind {
        Kind::Pdf => Color32::from_rgb(198, 60, 52),
        Kind::Word => Color32::from_rgb(44, 108, 196),
        Kind::Sheet => Color32::from_rgb(20, 140, 90),
        Kind::Slide => Color32::from_rgb(214, 122, 44),
        Kind::Ebook => Color32::from_rgb(140, 96, 190),
        Kind::Archive => Color32::from_rgb(146, 122, 60),
        Kind::Doc => Color32::from_rgb(110, 118, 129),
        Kind::Data => Color32::from_rgb(0, 140, 150),
        Kind::Audio => Color32::from_rgb(196, 78, 140),
        Kind::Video => Color32::from_rgb(80, 100, 200),
        Kind::Font => Color32::from_rgb(120, 140, 60),
    }
}

/// 「新标签页」要一个能直接交给系统的地址：已下载的副本优先，其次本机原文，
/// 都没有就把原地址交出去（远程链接浏览器自己会下载）。
fn browser_href(located: &Located, url: &str) -> String {
    match located {
        Located::Local(p, _) | Located::Cached(p, _) => attachment::file_href(&p.to_string_lossy()),
        Located::Absent => url.to_string(),
    }
}

/// 画一个类型徽标：色块 + 居中的类型短名（PDF / 表格 / 压缩包）。
fn paint_badge(painter: &egui::Painter, rect: Rect, kind: Kind, theme: &Theme) {
    painter.rect_filled(rect, 6.0, kind_color(kind));
    // 三汉字（「压缩包」）在 44 宽里要收一点才不贴边，拉丁名（PDF）留大字号。
    let label = kind.label();
    let size = if label.is_ascii() {
        theme.body * 0.82
    } else {
        theme.body * 0.72
    };
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(size),
        Color32::WHITE,
    );
}

/// 附件卡片。点击产生的动作全部推给 `env.actions`，由 app 层执行。
pub(crate) fn file_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    env: &mut RenderEnv<'_>,
    idx: usize,
    url: &str,
    label: &str,
    kind: Kind,
) -> egui::Response {
    let remote = crate::image_host::is_remote(url);
    let located = locate(env.base_dir, url);
    let downloading = remote && matches!(located, Located::Absent) && env.images.is_downloading(url);
    // 显示名：链接文字优先（那是作者自己写的名字），空了才用地址末段。
    let name = {
        let t = label.trim();
        if t.is_empty() {
            attachment::name_of(url)
        } else {
            t.to_string()
        }
    };
    let name = if name.is_empty() { url.to_string() } else { name };
    let status = match &located {
        Located::Local(_, bytes) => {
            format!("{} · {} · 本地文件", kind.describe(), attachment::size_text(*bytes))
        }
        Located::Cached(_, bytes) => {
            format!("{} · {} · 已下载到本机", kind.describe(), attachment::size_text(*bytes))
        }
        Located::Absent if remote => format!("{} · 远程文件，点「下载预览」取到本机", kind.describe()),
        Located::Absent => {
            let shown = attachment::local_path(env.base_dir, url)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| url.to_string());
            format!("{} · 找不到本地文件：{shown}", kind.describe())
        }
    };
    let primary = if remote && matches!(located, Located::Absent) {
        "下载预览"
    } else {
        "打开"
    };
    let primary_hint = match &located {
        Located::Local(p, _) => format!("用系统默认程序打开 {}", p.to_string_lossy()),
        Located::Cached(p, _) => format!("打开已下载的副本 {}", p.to_string_lossy()),
        Located::Absent if remote => "下载到 ~/.markd/attachments 后用程序打开".to_string(),
        Located::Absent => "文件不在文档目录里，先把它放回去".to_string(),
    };
    let href = browser_href(&located, url);

    let full_w = ui.available_width().min(env.max_width);
    let card_w = full_w.clamp(240.0, FILE_CARD_MAX_WIDTH);
    let outer = ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        panel_frame(theme).show(ui, |ui| {
            ui.set_width(card_w);
            let mut open = false;
            let mut newtab = false;
            let mut edit = false;
            let mut del = false;
            code_header_frame(theme).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("附件 · {}", kind.label()))
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
                            .button(egui::RichText::new("新标签页").size(theme.body * 0.85))
                            .on_hover_text("交给系统：远程用浏览器打开，本地用 file:// 地址")
                            .clicked()
                        {
                            newtab = true;
                        }
                        let text = if downloading { "下载中…" } else { primary };
                        let hint = if downloading {
                            "正在后台下载，完成后点「打开」"
                        } else {
                            &primary_hint
                        };
                        let btn = ui.add_enabled(
                            !downloading,
                            egui::Button::new(egui::RichText::new(text).size(theme.body * 0.85)),
                        );
                        if btn.on_hover_text(hint).clicked() {
                            open = true;
                        }
                    });
                });
            });
            ui.add_space(10.0);
            let row = ui.horizontal(|ui| {
                let (rect, badge) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), Sense::hover());
                paint_badge(ui.painter(), rect, kind, theme);
                let name_resp = ui
                    .vertical(|ui| {
                        // 文件名带点击感应：点它就是要看这个文件，不必再去找按钮。
                        let mut job = LayoutJob::default();
                        job.append(
                            &name,
                            0.0,
                            TextFormat::simple(FontId::proportional(theme.body), theme.text),
                        );
                        let (resp, _) = layout_label(ui, job, card_w - 60.0, Sense::click());
                        let _ = ui.add(
                            egui::Label::new(
                                egui::RichText::new(&status)
                                    .size(theme.body * 0.85)
                                    .color(theme.dim),
                            )
                            .truncate(),
                        );
                        resp
                    })
                    .inner;
                name_resp.union(badge)
            });
            ui.add_space(2.0);
            // 链接原文也留在卡片上：文件挪走、下载失败时，它是唯一能看到并复制的信息。
            ui.label(
                egui::RichText::new(url)
                    .font(FontId::monospace(theme.mono * 0.9))
                    .color(theme.link),
            );
            (row.inner.clicked() || open, newtab, edit, del)
        })
    });
    let card = outer.inner;
    let (want_open, want_tab, want_edit, want_delete) = card.inner;
    if want_open {
        env.actions.push(Action::PreviewFile(url.to_string()));
    }
    if want_tab {
        env.actions.push(Action::OpenLink(href));
    }
    if want_edit {
        env.actions.push(Action::Jump(idx));
    }
    if want_delete {
        env.actions.push(Action::AskDeleteBlock(idx));
    }
    // 与其它卡片同一处理：返回整张卡片的仅悬停响应，跨块拖选按真实矩形算，
    // 但落点不会被当成「进入编辑态」——点文件是为了看文件。
    ui.interact(
        card.response.rect,
        ui.scope_id().with(("file_card", idx)),
        Sense::hover(),
    )
}
