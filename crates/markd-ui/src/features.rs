//! 特色功能：版本快照/时间机器、命令面板与快速打开、全文搜索、
//! 双向链接关系图、写作统计。

use crate::document::Document;
use crate::theme::Theme;
use crate::workspace;
use egui::{self, Align2, Color32, FontId, FontFamily, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------- 命令

// Command 定义在 markd-core，供 UI 与 macOS 原生菜单共用；此处重导出保持
// `crate::features::Command` 这一历史路径继续可用。
pub use markd_core::command::Command;

// ---------------------------------------------------------------- 命令面板

#[derive(Clone, Copy, PartialEq)]
pub enum PaletteMode {
    Commands,
    Files,
}

pub struct Palette {
    pub open: bool,
    pub query: String,
    pub mode: PaletteMode,
    pub cursor: usize,
    /// 快速打开时指定的扫描目录（空串表示使用工作区）。
    pub dir: String,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            mode: PaletteMode::Commands,
            cursor: 0,
            dir: String::new(),
        }
    }
}

impl Palette {
    pub fn open_with(&mut self, mode: PaletteMode) {
        self.open = true;
        self.mode = mode;
        self.query.clear();
        self.cursor = 0;
    }

    pub fn show(&mut self, ctx: &egui::Context, files: &[PathBuf]) -> Option<Command> {
        if !self.open {
            return None;
        }
        let mut result: Option<Command> = None;
        let mut close = false;
        egui::Window::new("命令面板")
            .open(&mut self.open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_width(440.0);
                let out = egui::TextEdit::singleline(&mut self.query)
                    .hint_text("输入命令或文件名，↑↓ 选择，Enter 执行…")
                    .desired_width(f32::INFINITY)
                    .show(ui);
                if out.response.changed() {
                    self.cursor = 0;
                }
                out.response.request_focus();

                if self.mode == PaletteMode::Files {
                    ui.horizontal(|ui| {
                        ui.label("路径");
                        ui.text_edit_singleline(&mut self.dir);
                        if ui
                            .button("选择 MD 文件…")
                            .on_hover_cursor(egui::CursorIcon::Default)
                            .clicked()
                        {
                            result = Some(Command::OpenFileDialog);
                            close = true;
                        }
                    });
                }

                let q = self.query.trim().to_lowercase();
                let mut items: Vec<(String, Command)> = match self.mode {
                    PaletteMode::Commands => Command::all().into_iter().map(|c| (c.label(), c)).collect(),
                    PaletteMode::Files => files
                        .iter()
                        .map(|p| {
                            (
                                p.file_name().unwrap_or_default().to_string_lossy().to_string(),
                                Command::OpenFile(p.clone()),
                            )
                        })
                        .collect(),
                };
                items.retain(|(name, _)| q.is_empty() || name.to_lowercase().contains(&q));
                if items.is_empty() {
                    ui.label(RichText::new("无匹配项").color(Color32::GRAY));
                    return;
                }
                let (up, down, enter) = ui.input(|i| {
                    (
                        i.key_pressed(egui::Key::ArrowUp),
                        i.key_pressed(egui::Key::ArrowDown),
                        i.key_pressed(egui::Key::Enter),
                    )
                });
                if up {
                    self.cursor = self.cursor.saturating_sub(1);
                }
                if down {
                    self.cursor = (self.cursor + 1).min(items.len() - 1);
                }
                self.cursor = self.cursor.min(items.len() - 1);
                if enter {
                    result = Some(items[self.cursor].1.clone());
                    close = true;
                }
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (i, (name, cmd)) in items.iter().enumerate() {
                        let r = ui.selectable_label(i == self.cursor, name);
                        if r.clicked() {
                            result = Some(cmd.clone());
                            close = true;
                        }
                    }
                });
            });
        if close {
            self.open = false;
        }
        result
    }
}

// ---------------------------------------------------------------- 版本快照

#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub label: String,
    pub ts: u64,
    pub path: String,
    pub content: String,
}

pub struct SnapshotStore {
    pub items: Vec<Snapshot>,
    pub open: bool,
}

impl SnapshotStore {
    pub fn load() -> Self {
        let p = workspace::snapshots_dir().join("index.json");
        let items = std::fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { items, open: false }
    }

    fn save(&self) {
        let p = workspace::snapshots_dir().join("index.json");
        if let Ok(s) = serde_json::to_string_pretty(&self.items) {
            let _ = std::fs::write(p, s);
        }
    }

    pub fn capture(&mut self, doc: &Document) {
        let content = doc.source();
        let ts = now();
        let label = format!(
            "#{}  {} · {} 字",
            self.items.len() + 1,
            doc.title(),
            content.chars().count()
        );
        self.items.push(Snapshot {
            id: format!("{}-{}", ts, self.items.len()),
            label,
            ts,
            path: doc.path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            content,
        });
        if self.items.len() > 60 {
            self.items.remove(0);
        }
        self.save();
    }

    /// 返回用户选择要恢复的内容。
    pub fn show(&mut self, ctx: &egui::Context) -> Option<String> {
        if !self.open {
            return None;
        }
        let mut restore: Option<String> = None;
        let mut remove: Option<usize> = None;
        egui::Window::new("版本快照 · 时间机器")
            .open(&mut self.open)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label("每次保存或手动拍摄都会留下一个可回溯的版本。");
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    let mut i = 0usize;
                    while i < self.items.len() {
                        let s = &self.items[i];
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&s.label).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("恢复").clicked() {
                                    restore = Some(s.content.clone());
                                }
                                if ui.button("删除").clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                        ui.label(
                            RichText::new(format!(
                                "{}  ·  {}",
                                if s.path.is_empty() { "未命名" } else { &s.path },
                                s.content.lines().count()
                            ))
                            .small()
                            .color(Color32::GRAY),
                        );
                        ui.separator();
                        i += 1;
                    }
                });
            });
        if let Some(i) = remove {
            if i < self.items.len() {
                self.items.remove(i);
                self.save();
            }
        }
        restore
    }
}

// ---------------------------------------------------------------- 全文搜索

#[derive(Clone)]
pub struct SearchHit {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

pub struct SearchPanel {
    pub open: bool,
    pub query: String,
    pub hits: Vec<SearchHit>,
}

impl Default for SearchPanel {
    fn default() -> Self {
        Self { open: false, query: String::new(), hits: Vec::new() }
    }
}

impl SearchPanel {
    pub fn run(&mut self, root: Option<&Path>) {
        self.hits.clear();
        let q = self.query.trim().to_lowercase();
        if q.is_empty() {
            return;
        }
        if let Some(root) = root {
            for p in workspace::collect_markdown(root) {
                if let Ok(s) = workspace::read(&p) {
                    for (i, line) in s.lines().enumerate() {
                        if line.to_lowercase().contains(&q) && self.hits.len() < 500 {
                            self.hits.push(SearchHit {
                                path: p.clone(),
                                line: i + 1,
                                text: line.trim().to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, root: Option<&Path>) -> Option<(PathBuf, usize)> {
        if !self.open {
            return None;
        }
        let mut jump: Option<(PathBuf, usize)> = None;
        let mut run = false;
        egui::Window::new("工作区全文搜索")
            .open(&mut self.open)
            .resizable(true)
            .default_width(620.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let out = egui::TextEdit::singleline(&mut self.query)
                        .hint_text("搜索所有 Markdown 文件…")
                        .desired_width(420.0)
                        .show(ui);
                    out.response.request_focus();
                    if ui.button("搜索").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        run = true;
                    }
                });
                ui.label(RichText::new(format!("命中 {} 条", self.hits.len())).small().color(Color32::GRAY));
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for h in &self.hits {
                        let name = h.path.file_name().unwrap_or_default().to_string_lossy();
                        let r = ui.selectable_label(
                            false,
                            format!("{}:{}  {}", name, h.line, h.text),
                        );
                        if r.clicked() {
                            jump = Some((h.path.clone(), h.line.saturating_sub(1)));
                        }
                    }
                });
            });
        if run {
            self.run(root);
        }
        jump
    }
}

// ---------------------------------------------------------------- 双向链接

pub struct GraphPanel {
    pub open: bool,
}

impl Default for GraphPanel {
    fn default() -> Self {
        Self { open: false }
    }
}

impl GraphPanel {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        links: &HashMap<String, Vec<String>>,
        theme: &Theme,
    ) -> Option<String> {
        if !self.open {
            return None;
        }
        let mut open_file: Option<String> = None;
        egui::Window::new("双向链接关系图")
            .open(&mut self.open)
            .resizable(true)
            .default_size([560.0, 460.0])
            .show(ctx, |ui| {
                let mut names: Vec<String> = links.keys().cloned().collect();
                for v in links.values() {
                    for n in v {
                        if !names.contains(n) {
                            names.push(n.clone());
                        }
                    }
                }
                names.sort();
                names.dedup();
                if names.is_empty() {
                    ui.label("用 [[双向链接]] 连接文档后，这里会显示关系图。");
                    return;
                }
                let n = names.len();
                let radius = 160.0_f32.max(n as f32 * 12.0);
                let size = radius * 2.0 + 120.0;
                let (rect, _) = ui.allocate_exact_size(Vec2::new(size, size), Sense::hover());
                let center = rect.center();
                let mut pos: HashMap<String, Pos2> = HashMap::new();
                for (i, name) in names.iter().enumerate() {
                    let a = std::f32::consts::TAU * (i as f32) / (n as f32);
                    pos.insert(
                        name.clone(),
                        Pos2::new(center.x + radius * a.cos(), center.y + radius * a.sin()),
                    );
                }
                let p = ui.painter();
                for (src, targets) in links {
                    if let Some(a) = pos.get(src) {
                        for t in targets {
                            if let Some(b) = pos.get(t) {
                                p.line_segment([*a, *b], Stroke::new(1.0, Color32::from_rgb(120, 140, 170)));
                            }
                        }
                    }
                }
                let node_r = 26.0_f32;
                for name in &names {
                    if let Some(c) = pos.get(name) {
                        let r = Rect::from_center_size(*c, Vec2::splat(node_r * 2.0));
                        p.circle_filled(*c, node_r, theme.code_bg);
                        p.circle_stroke(*c, node_r, Stroke::new(1.5, theme.accent));
                        let short: String = name.chars().take(4).collect();
                        p.text(
                            *c,
                            Align2::CENTER_CENTER,
                            short,
                            FontId::new(11.0, FontFamily::Proportional),
                            theme.text,
                        );
                        let hit = ui.interact(r, ui.scope_id().with(name), Sense::click());
                        if hit.clicked() {
                            open_file = Some(name.clone());
                        }
                        if hit.hovered() {
                            p.circle_stroke(*c, node_r + 2.0, Stroke::new(2.0, theme.link));
                        }
                    }
                }
            });
        open_file
    }
}

// ---------------------------------------------------------------- 写作统计

pub struct WritingStats {
    pub open: bool,
    pub session_start: u64,
    pub start_words: usize,
    pub history: Vec<(u64, usize)>,
}

impl WritingStats {
    pub fn new(doc: &Document) -> Self {
        Self {
            open: false,
            session_start: now(),
            start_words: doc.stats().words,
            history: vec![(now(), doc.stats().words)],
        }
    }

    pub fn tick(&mut self, words: usize) {
        let w = words;
        if self.history.last().map(|(_, x)| *x) != Some(w) {
            self.history.push((now(), w));
            if self.history.len() > 120 {
                self.history.remove(0);
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, stats: &crate::document::Stats, theme: &Theme) {
        if !self.open {
            return;
        }
        let st = stats;
        let gained = st.words.saturating_sub(self.start_words);
        let minutes = now().saturating_sub(self.session_start) / 60;
        egui::Window::new("写作统计")
            .open(&mut self.open)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.columns(2, |cols| {
                    cols[0].label(RichText::new("本次新增字数").small().color(Color32::GRAY));
                    cols[0].label(RichText::new(format!("{} 字", gained)).heading());
                    cols[1].label(RichText::new("专注时长").small().color(Color32::GRAY));
                    cols[1].label(RichText::new(format!("{} 分钟", minutes)).heading());
                });
                ui.separator();
                ui.label(format!(
                    "总字数 {} · 字符 {} · 中文字符 {} · 行 {} · 预计阅读 {} 分钟",
                    st.words, st.chars, st.cjk, st.lines, st.reading_min
                ));
                ui.add_space(8.0);
                ui.label(RichText::new("字数变化曲线").small().color(Color32::GRAY));
                let (rect, _) = ui.allocate_exact_size(Vec2::new(380.0, 110.0), Sense::hover());
                if self.history.len() > 1 {
                    let min = self.history.iter().map(|(_, w)| *w).min().unwrap_or(0);
                    let max = self.history.iter().map(|(_, w)| *w).max().unwrap_or(1).max(min + 1);
                    let p = ui.painter();
                    let mut prev: Option<Pos2> = None;
                    for (i, (_, w)) in self.history.iter().enumerate() {
                        let x = rect.min.x + (i as f32 / (self.history.len() - 1) as f32) * rect.width();
                        let y = rect.max.y - ((w - min) as f32 / (max - min) as f32) * rect.height();
                        let cur = Pos2::new(x, y);
                        if let Some(pr) = prev {
                            p.line_segment([pr, cur], Stroke::new(2.0, theme.accent));
                        }
                        prev = Some(cur);
                    }
                }
            });
    }
}
