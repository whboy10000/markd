//! Author: aurelianus <aurelianus@126.com>

//! 工作区文件系统：目录扫描、Markdown 读写、配置持久化、路径解析。

use crate::theme::ThemeKind;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn default_sidebar_header() -> bool {
    true
}

fn default_sidebar_width() -> f32 {
    240.0
}

fn default_show_line_numbers() -> bool {
    true
}

fn default_show_workspace_bar() -> bool {
    false
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub theme: ThemeKind,
    pub focus_mode: bool,
    pub typewriter: bool,
    pub source_mode: bool,
    pub show_sidebar: bool,
    pub show_outline: bool,
    pub auto_save: bool,
    pub font_size: f32,
    pub workspace: Option<String>,
    #[serde(default = "default_sidebar_header")]
    pub show_sidebar_header: bool,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
    #[serde(default = "default_show_workspace_bar")]
    pub show_workspace_bar: bool,
    /// 编辑区是否显示行号（源码模式整栏行号 + 混合模式逐块行号）。
    #[serde(default = "default_show_line_numbers")]
    pub show_line_numbers: bool,

    // ---- 设置弹窗里的可调项（全部带 serde 默认值，兼容旧配置） ----
    #[serde(default = "default_true")]
    pub auto_pair: bool,
    #[serde(default = "default_tab_size")]
    pub tab_size: usize,
    #[serde(default = "default_true")]
    pub highlight_code: bool,
    #[serde(default = "default_auto_save_interval")]
    pub auto_save_interval: u64,
    #[serde(default = "default_true")]
    pub restore_last_file: bool,
    #[serde(default)]
    pub last_file: Option<String>,
    /// 首页「最近打开」列表，最新的排在最前面。
    #[serde(default)]
    pub recent: Vec<String>,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    #[serde(default = "default_true")]
    pub insert_image_to_assets: bool,
    #[serde(default = "default_assets_dir")]
    pub image_assets_dir: String,
    #[serde(default = "default_true")]
    pub image_relative: bool,
    #[serde(default = "default_true")]
    pub export_embed_images: bool,
    #[serde(default)]
    pub export_open_after: bool,
    #[serde(default = "default_true")]
    pub math_enabled: bool,
    #[serde(default = "default_true")]
    pub mermaid_enabled: bool,
    #[serde(default = "default_true")]
    pub toc_enabled: bool,
    #[serde(default = "default_true")]
    pub footnote_enabled: bool,
    #[serde(default)]
    pub auto_number_heading: bool,
    #[serde(default)]
    pub confirm_on_exit: bool,
}

fn default_true() -> bool {
    true
}
fn default_tab_size() -> usize {
    2
}
fn default_auto_save_interval() -> u64 {
    20
}
fn default_ui_scale() -> f32 {
    1.0
}
fn default_assets_dir() -> String {
    "assets".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: ThemeKind::Light,
            focus_mode: false,
            typewriter: false,
            source_mode: false,
            show_sidebar: true,
            show_outline: true,
            auto_save: true,
            font_size: 16.0,
            workspace: None,
            show_sidebar_header: true,
            sidebar_width: 240.0,
            show_workspace_bar: false,
            show_line_numbers: true,
            auto_pair: true,
            tab_size: 2,
            highlight_code: true,
            auto_save_interval: 20,
            restore_last_file: true,
            last_file: None,
            recent: Vec::new(),
            ui_scale: 1.0,
            insert_image_to_assets: true,
            image_assets_dir: "assets".to_string(),
            image_relative: true,
            export_embed_images: true,
            export_open_after: false,
            math_enabled: true,
            mermaid_enabled: true,
            toc_enabled: true,
            footnote_enabled: true,
            auto_number_heading: false,
            confirm_on_exit: false,
        }
    }
}

impl Config {
    pub fn dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".markd")
    }

    pub fn load() -> Self {
        let p = Self::dir().join("config.json");
        fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = Self::dir();
        let _ = fs::create_dir_all(&dir);
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = fs::write(dir.join("config.json"), s);
        }
    }
}

pub fn snapshots_dir() -> PathBuf {
    let d = Config::dir().join("snapshots");
    let _ = fs::create_dir_all(&d);
    d
}

#[derive(Clone, Debug)]
pub struct FileNode {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub children: Vec<FileNode>,
    pub expanded: bool,
}

fn hidden(name: &str) -> bool {
    name.starts_with('.') || name == "target" || name == "node_modules" || name == "build"
}

fn is_md(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with(".md") || l.ends_with(".markdown")
}

pub fn scan_tree(root: &Path, expanded: &HashSet<PathBuf>) -> Vec<FileNode> {
    fn walk(dir: &Path, depth: usize, expanded: &HashSet<PathBuf>) -> Vec<FileNode> {
        if depth > 6 {
            return Vec::new();
        }
        let mut entries: Vec<_> = match fs::read_dir(dir) {
            Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
            Err(_) => return Vec::new(),
        };
        entries.sort_by(|a, b| {
            let ad = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let bd = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
            bd.cmp(&ad).then_with(|| a.file_name().cmp(&b.file_name()))
        });
        let mut out = Vec::new();
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if hidden(&name) {
                continue;
            }
            let path = e.path();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                let is_open = expanded.contains(&path);
                let children = if is_open {
                    walk(&path, depth + 1, expanded)
                } else {
                    Vec::new()
                };
                out.push(FileNode {
                    name,
                    path,
                    is_dir: true,
                    children,
                    expanded: is_open,
                });
            } else if is_md(&name) {
                out.push(FileNode {
                    name,
                    path,
                    is_dir: false,
                    children: Vec::new(),
                    expanded: false,
                });
            }
        }
        out
    }
    walk(root, 0, expanded)
}

/// 链接图扫描的上限：文件数与遍历条目数都要封顶。
/// 把桌面、家目录甚至整盘当成工作区时，无上限遍历会让启动卡死。
const MAX_MARKDOWN_FILES: usize = 4000;
const MAX_SCAN_ENTRIES: usize = 60_000;

pub fn collect_markdown(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize, seen: &mut usize) {
        if depth > 8 || out.len() >= MAX_MARKDOWN_FILES || *seen >= MAX_SCAN_ENTRIES {
            return;
        }
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.filter_map(|e| e.ok()) {
                *seen += 1;
                if out.len() >= MAX_MARKDOWN_FILES || *seen >= MAX_SCAN_ENTRIES {
                    return;
                }
                let name = e.file_name().to_string_lossy().to_string();
                if hidden(&name) {
                    continue;
                }
                let p = e.path();
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    walk(&p, out, depth + 1, seen);
                } else if is_md(&name) {
                    out.push(p);
                }
            }
        }
    }
    let mut v = Vec::new();
    let mut seen = 0usize;
    walk(root, &mut v, 0, &mut seen);
    v.sort();
    v
}

pub fn read(path: &Path) -> std::io::Result<String> {
    fs::read_to_string(path)
}

pub fn write(path: &Path, s: &str) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(path, s)
}

/// 解析 Markdown 中的本地图片/链接路径；http(s) 与 data: 返回 None。
pub fn resolve(base: Option<&Path>, url: &str) -> Option<PathBuf> {
    let u = url.trim();
    if u.starts_with("http://")
        || u.starts_with("https://")
        || u.starts_with("data:")
        || u.is_empty()
    {
        return None;
    }
    let p = PathBuf::from(u);
    if p.is_absolute() {
        return Some(p);
    }
    let root = base
        .map(|b| b.to_path_buf())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    Some(root.join(p))
}

pub fn is_image(url: &str) -> bool {
    let l = url.to_lowercase();
    l.ends_with(".png")
        || l.ends_with(".jpg")
        || l.ends_with(".jpeg")
        || l.ends_with(".gif")
        || l.ends_with(".webp")
        || l.ends_with(".bmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_backward_compatible() {
        // 模拟旧版 config.json（缺少两个新增字段），反序列化不能失败，
        // 也不能把用户已保存的设置回退成全默认值。
        let json = r#"{"theme":"Dark","focus_mode":true,"typewriter":false,"source_mode":false,"show_sidebar":true,"show_outline":false,"auto_save":true,"font_size":18.0,"workspace":"/tmp/notes"}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert_eq!(c.theme, ThemeKind::Dark);
        assert!(c.focus_mode);
        assert!(!c.show_outline);
        assert_eq!(c.font_size, 18.0);
        assert!(c.show_sidebar_header);
        assert_eq!(c.sidebar_width, 240.0);
        assert!(!c.show_workspace_bar);
    }

    #[test]
    fn config_roundtrip_with_new_fields() {
        let mut c = Config::default();
        c.show_sidebar_header = false;
        c.show_outline = false;
        c.sidebar_width = 320.0;
        c.show_workspace_bar = true;
        let s = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&s).unwrap();
        assert!(!back.show_sidebar_header);
        assert!(!back.show_outline);
        assert_eq!(back.sidebar_width, 320.0);
        assert!(back.show_workspace_bar);
    }
}
