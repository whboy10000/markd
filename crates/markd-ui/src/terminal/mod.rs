//! Author: aurelianus <aurelianus@126.com>

//! 集成终端：基于 PTY 的交互式 shell（VS Code 风格）。
//!
//! `portable-pty` 启动持久 shell，`vte` 把 ANSI 字节流解析成字符网格，键盘输入
//! 直接写回 PTY。输出经 `mpsc` 回传，UI 帧 `poll()` 时更新，线程用
//! `ctx.request_repaint()` 唤醒界面。
//!
//! 子模块分工：
//! - `screen` 维护字符网格与 ANSI 解析；
//! - `session` 是一个独立 shell（一个 PTY + 一块网格）的渲染与输入；
//! - `layout` 是分屏树与页签的数据结构；
//! - `keys` 把按键映射成写入 PTY 的字节序列；
//! - 本文件里的 `Terminal` 是管理器：持有所有会话、布局树、标题栏工具按钮。

use crate::theme::Theme;
use egui::{self, Color32, FontFamily, FontId, RichText, ScrollArea, Sense};
use portable_pty::{Child, CommandBuilder, MasterPty, PtyPair, PtySize, native_pty_system};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

mod keys;
mod layout;
mod screen;
mod session;
#[cfg(test)]
mod tests;

use self::keys::*;
use self::layout::*;
use self::screen::*;
use self::session::*;

const MAX_SCROLLBACK: usize = 400;

/// 终端管理器：面板开关、所有 shell 会话、分屏布局与右上角工具条。
pub struct Terminal {
    pub open: bool,
    pub height: f32,
    cwd: PathBuf,
    /// 终端专用配色是否切到固定深色（月亮按钮）。
    dark: bool,
    /// 所有会话，按创建顺序存放；布局树只引用它们的 id。
    sessions: Vec<Session>,
    layout: Layout,
    /// 当前获得焦点的会话 id。
    active: u64,
    next_id: u64,
    /// 打开面板时把焦点交给活动会话。
    focus_req: bool,
    /// 图标纹理缓存（UI 图标固定就那几个，自带一份，避免污染外部接口）。
    icons: crate::icons::IconCache,
}

impl Terminal {
    pub fn new(cwd: PathBuf) -> Self {
        let first = 0u64;
        Self {
            open: false,
            height: 220.0,
            cwd: cwd.clone(),
            dark: false,
            sessions: vec![Session::new(first, cwd)],
            layout: Layout::new(first),
            active: first,
            next_id: 1,
            focus_req: false,
            icons: crate::icons::IconCache::default(),
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.focus_req = self.open;
    }

    /// shell 启动后自己维护工作目录，只在新建会话时接受外部 cwd。
    pub fn set_cwd(&mut self, path: PathBuf) {
        self.cwd = path.clone();
        for s in &mut self.sessions {
            s.set_cwd(path.clone());
        }
    }

    pub fn has_focus(&self) -> bool {
        self.open && self.sessions.iter().any(|s| s.has_focus())
    }

    /// 主动让出键盘焦点（点回编辑区时调用）。
    pub fn blur(&mut self, ctx: &egui::Context) {
        for s in &mut self.sessions {
            s.blur(ctx);
        }
    }

    /// 每帧让所有会话消化各自 PTY 线程收到的字节。
    pub fn poll(&mut self) {
        for s in &mut self.sessions {
            s.poll();
        }
    }

    fn active_session(&self) -> &Session {
        self.sessions
            .iter()
            .find(|s| s.id == self.active)
            .expect("活动会话一定存在")
    }

    fn active_session_mut(&mut self) -> &mut Session {
        let id = self.active;
        self.sessions
            .iter_mut()
            .find(|s| s.id == id)
            .expect("活动会话一定存在")
    }

    /// 新建一个 shell 作为当前窗格的新页签，并切过去。
    fn add_tab(&mut self) {
        let id = self.next_id;
        self.next_id += 1;
        let mut s = Session::new(id, self.cwd.clone());
        s.focus_req = true;
        if let Some(pane) = self.layout.active_pane_mut(self.active) {
            pane.tabs.push(id);
            pane.active = id;
        }
        self.sessions.push(s);
        self.active = id;
    }

    /// 把当前会话所在窗格一分为二，新窗格里是一个新 shell。
    fn split_pane(&mut self, dir: SplitDir) {
        let id = self.next_id;
        self.next_id += 1;
        let mut s = Session::new(id, self.cwd.clone());
        s.focus_req = true;
        if self.layout.split(self.active, dir, id) {
            self.sessions.push(s);
            self.active = id;
        }
    }

    /// 关闭当前会话（页签）。最后一个会话关掉后收起整个面板。
    fn close_active(&mut self, ctx: &egui::Context) {
        let closing = self.active;
        let pane_before = self.layout.active_pane(closing).cloned();
        self.layout.remove(closing);
        if let Some(s) = self.sessions.iter_mut().find(|s| s.id == closing) {
            s.blur(ctx);
        }
        self.sessions.retain(|s| s.id != closing);
        if self.layout.session_count() == 0 {
            self.open = false;
            return;
        }
        // 优先留在原窗格剩下的页签里，否则退到全树第一个。
        let next = pane_before
            .and_then(|p| p.tabs.into_iter().find(|t| *t != closing))
            .or_else(|| self.layout.first_active());
        if let Some(id) = next {
            if self.layout.set_active(id) {
                self.active = id;
                self.focus_req = true;
            }
        }
    }

    /// 切换页签：只改布局里的 active，并把键盘焦点交给它。
    fn activate(&mut self, id: u64) {
        if self.layout.set_active(id) {
            self.active = id;
            self.active_session_mut().focus_req = true;
        }
    }

    /// 标题栏右上角那个圆图标按钮。`ui` 负责占位与点击，`icons` 负责画。
    fn tool_button(&mut self, ui: &mut egui::Ui, key: &str, tip: &str, tint: Color32) -> bool {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
        let hover_tint = if resp.hovered() {
            tint.lerp_to_gamma(Color32::WHITE, 0.45)
        } else {
            tint
        };
        self.icons.paint_ui(ui, rect, key, 14.0, hover_tint);
        resp.on_hover_cursor(egui::CursorIcon::Default)
            .on_hover_text(tip)
            .clicked()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, theme: &Theme) {
        ui.style_mut().interaction.selectable_labels = false;
        let colors = TermColors::new(theme, self.dark);

        // ---- 标题栏：左标题 + cwd，右侧 5 个工具按钮 ----
        ui.horizontal(|ui| {
            ui.label(RichText::new("终端").strong().color(colors.accent));
            let cwd = self.cwd.to_string_lossy();
            ui.label(RichText::new(cwd).small().color(colors.dim));

            let err = self.active_session().error.clone();
            let exited = self.active_session().exited;
            if let Some(e) = err {
                ui.label(RichText::new(e).small().color(colors.error));
            } else if exited {
                ui.label(RichText::new("shell 已退出").small().color(colors.error));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 顺序按“从右到左”排：关闭 → 暗色 → 新增 → 上下分屏 → 左右分屏。
                let mut close = false;
                let mut dark = false;
                let mut add = false;
                let mut split_v = false;
                let mut split_h = false;

                if self.tool_button(ui, "ui:close", "关闭终端", colors.dim) {
                    close = true;
                }
                let moon_key = if self.dark { "ui:sun" } else { "ui:moon" };
                let moon_tip = if self.dark {
                    "切换为浅色终端"
                } else {
                    "切换为暗色终端"
                };
                if self.tool_button(ui, moon_key, moon_tip, colors.dim) {
                    dark = true;
                }
                if self.tool_button(ui, "ui:add", "新建终端 (页签)", colors.dim) {
                    add = true;
                }
                if self.tool_button(ui, "ui:split-vertical", "上下分屏", colors.dim) {
                    split_v = true;
                }
                if self.tool_button(ui, "ui:split-horizontal", "左右分屏", colors.dim) {
                    split_h = true;
                }

                // 借用已经结束，统一在这里执行动作，避免与 tool_button 争 &mut self。
                if close {
                    self.close_active(ctx);
                } else if dark {
                    self.dark = !self.dark;
                } else if add {
                    self.add_tab();
                } else if split_v {
                    self.split_pane(SplitDir::Vertical);
                } else if split_h {
                    self.split_pane(SplitDir::Horizontal);
                }
            });
        });
        ui.separator();

        // ---- 面板主体：布局树递归绘制 ----
        let body = ui.available_rect_before_wrap();
        if self.focus_req {
            self.active_session_mut().focus_req = true;
            self.focus_req = false;
        }
        let mut action = PaneAction::None;
        let active = self.active;
        let mut split_seq = 0u64;
        paint_layout(
            &mut self.layout,
            &mut self.sessions,
            active,
            body,
            ui,
            ctx,
            theme,
            colors,
            &mut action,
            &mut split_seq,
        );
        match action {
            PaneAction::Switch(id) => self.activate(id),
            PaneAction::Close(id) => {
                if self.active == id {
                    self.close_active(ctx);
                } else {
                    // 关闭非活动页签：先切过去再关，保证 layouts/sessions 一致。
                    self.activate(id);
                    self.close_active(ctx);
                }
            }
            PaneAction::None => {}
        }
    }
}

/// 绘制阶段收集到的页签操作；借用结束后由 `Terminal::ui` 统一执行。
enum PaneAction {
    None,
    Switch(u64),
    Close(u64),
}

/// 把布局树画进 `rect`：Split 先按比例切两块（中间留分隔条），
/// Pane 画一条页签栏 + 当前会话的字符网格。
fn paint_layout(
    node: &mut Layout,
    sessions: &mut [Session],
    active: u64,
    rect: egui::Rect,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    colors: TermColors,
    action: &mut PaneAction,
    split_seq: &mut u64,
) {
    match node {
        Layout::Pane(pane) => {
            // 页签栏（页签多于一个才占高度，单会话时把空间全给终端）。
            let tabs = &mut pane.tabs;
            let show_tabs = tabs.len() > 1;
            let tab_h = if show_tabs { 22.0 } else { 0.0 };
            let (tab_rect, body_rect) = split_off_top(rect, tab_h);

            if show_tabs {
                let mut clicked: Option<u64> = None;
                let mut closed: Option<u64> = None;
                let mut x = tab_rect.min.x;
                for &id in tabs.iter() {
                    let title = sessions
                        .iter()
                        .find(|s| s.id == id)
                        .map(|s| s.title.clone())
                        .unwrap_or_else(|| "shell".into());
                    let is_active = id == pane.active;
                    let w = (title.chars().count() as f32 * 7.0 + 34.0).clamp(56.0, 132.0);
                    let r = egui::Rect::from_min_size(
                        egui::pos2(x, tab_rect.min.y),
                        egui::vec2(w, tab_h),
                    );
                    let resp = ui.interact(r, ui.id().with(("term_tab", id)), Sense::click());
                    let bg = if is_active {
                        colors.bg.lerp_to_gamma(Color32::WHITE, 0.10)
                    } else {
                        colors.bg.gamma_multiply(0.86)
                    };
                    ui.painter().rect_filled(r, egui::CornerRadius::ZERO, bg);
                    if is_active {
                        ui.painter().rect_stroke(
                            r,
                            egui::CornerRadius::ZERO,
                            egui::Stroke::new(1.0, colors.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let tcol = if is_active { colors.text } else { colors.dim };
                    ui.painter().text(
                        egui::pos2(r.min.x + 7.0, r.min.y + 3.0),
                        egui::Align2::LEFT_TOP,
                        title,
                        egui::FontId::new(12.0, egui::FontFamily::Proportional),
                        tcol,
                    );
                    // 页签上的小 ×：关闭该页签。
                    let xr = egui::Rect::from_min_size(
                        egui::pos2(r.max.x - 16.0, r.min.y + 3.0),
                        egui::vec2(13.0, 13.0),
                    );
                    let xresp =
                        ui.interact(xr, ui.id().with(("term_tab_close", id)), Sense::click());
                    ui.painter().text(
                        egui::pos2(xr.min.x + 2.0, xr.min.y - 1.0),
                        egui::Align2::LEFT_TOP,
                        "×",
                        egui::FontId::new(14.0, egui::FontFamily::Proportional),
                        if xresp.hovered() {
                            colors.text
                        } else {
                            colors.dim
                        },
                    );
                    if resp.clicked() {
                        clicked = Some(id);
                    }
                    if xresp.clicked() {
                        closed = Some(id);
                    }
                    x += w + 1.0;
                }
                // 借用 tabs 结束前不碰 pane.active，交给调用方统一处理。
                drop(tabs);
                if let Some(id) = closed {
                    *action = PaneAction::Close(id);
                } else if let Some(id) = clicked {
                    *action = PaneAction::Switch(id);
                }
            }

            let id = pane.active;
            if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(body_rect)
                        .layout(egui::Layout::top_down(egui::Align::LEFT)),
                    |ui| s.ui(ui, ctx, colors),
                );
            }
            let _ = (active, theme);
        }
        Layout::Split { dir, ratio, a, b } => {
            let gap = 3.0;
            let (ra, rb, handle) = split_rect(*dir, *ratio, rect, gap);

            // 拖动分隔条：直接在当前 Split 节点上改 ratio。
            // 用确定性的遍历序号做 id，保证每个分隔条唯一且跨帧稳定
            // （对称布局下 pane_count 组合会重复，不能拿来当 id）。
            let seq = *split_seq;
            *split_seq += 1;
            let sep_id = ui.id().with(("term_sep", seq));
            let sep = ui.interact(handle, sep_id, Sense::drag() | Sense::hover());
            if sep.hovered() || sep.dragged() {
                ctx.set_cursor_icon(match dir {
                    SplitDir::Horizontal => egui::CursorIcon::ResizeHorizontal,
                    SplitDir::Vertical => egui::CursorIcon::ResizeVertical,
                });
            }
            if sep.dragged() {
                let d = sep.drag_delta();
                let delta = match dir {
                    SplitDir::Horizontal => d.x,
                    SplitDir::Vertical => d.y,
                };
                let extent = match dir {
                    SplitDir::Horizontal => rect.width(),
                    SplitDir::Vertical => rect.height(),
                };
                if extent > 1.0 {
                    *ratio = (*ratio + delta / extent).clamp(0.15, 0.85);
                }
            }

            paint_layout(
                a, sessions, active, ra, ui, ctx, theme, colors, action, split_seq,
            );
            paint_layout(
                b, sessions, active, rb, ui, ctx, theme, colors, action, split_seq,
            );

            ui.painter().rect_filled(
                handle,
                egui::CornerRadius::ZERO,
                colors.bg.lerp_to_gamma(Color32::WHITE, 0.16),
            );
        }
    }
}

/// 从 `rect` 顶部切掉 `h` 高度，返回（顶部条, 剩余）。
fn split_off_top(rect: egui::Rect, h: f32) -> (egui::Rect, egui::Rect) {
    if h <= 0.0 {
        return (
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 0.0)),
            rect,
        );
    }
    let top = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + h));
    let rest = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.min.y + h), rect.max);
    (top, rest)
}

/// 按方向切分矩形，返回（a, b, 分隔条）。
fn split_rect(
    dir: SplitDir,
    ratio: f32,
    rect: egui::Rect,
    gap: f32,
) -> (egui::Rect, egui::Rect, egui::Rect) {
    let r = ratio.clamp(0.15, 0.85);
    match dir {
        SplitDir::Horizontal => {
            let mid = rect.min.x + rect.width() * r;
            (
                egui::Rect::from_min_max(rect.min, egui::pos2(mid - gap / 2.0, rect.max.y)),
                egui::Rect::from_min_max(egui::pos2(mid + gap / 2.0, rect.min.y), rect.max),
                egui::Rect::from_min_size(
                    egui::pos2(mid - gap / 2.0, rect.min.y),
                    egui::vec2(gap, rect.height()),
                ),
            )
        }
        SplitDir::Vertical => {
            let mid = rect.min.y + rect.height() * r;
            (
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, mid - gap / 2.0)),
                egui::Rect::from_min_max(egui::pos2(rect.min.x, mid + gap / 2.0), rect.max),
                egui::Rect::from_min_size(
                    egui::pos2(rect.min.x, mid - gap / 2.0),
                    egui::vec2(rect.width(), gap),
                ),
            )
        }
    }
}
