//! Author: aurelianus <aurelianus@126.com>

//! 工作区文件系统：目录扫描、Markdown 读写、配置持久化、路径解析。

use crate::theme::{ColorOverrides, ThemeKind};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn default_sidebar_header() -> bool {
    true
}

fn default_line_height() -> f32 {
    1.4
}

/// 界面语言。目前只写入配置文件，界面文字仍是中文——设置页的下拉也这么标注，
/// 等真接上多语言再改读取端。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    #[default]
    #[serde(alias = "zh-CN")]
    ZhCn,
    En,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::ZhCn, Language::En];

    pub fn name(self) -> &'static str {
        match self {
            Language::ZhCn => "简体中文",
            Language::En => "English",
        }
    }
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
#[serde(default)]
pub struct Config {
    pub theme: ThemeKind,
    /// 主题跟着系统的深色/浅色走；为真时 `theme` 里的档位只当作手动档记录。
    #[serde(default)]
    pub theme_follow_system: bool,
    /// 界面语言，详见 `Language`：当前只存配置，不参与渲染。
    #[serde(default)]
    pub language: Language,
    pub focus_mode: bool,
    pub typewriter: bool,
    pub source_mode: bool,
    pub show_sidebar: bool,
    pub show_outline: bool,
    pub auto_save: bool,
    pub font_size: f32,
    /// 源码模式的行距，按字号的倍数算（1.0 = 贴着排）。
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    pub workspace: Option<String>,
    #[serde(default = "default_sidebar_header")]
    pub show_sidebar_header: bool,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
    #[serde(default = "default_show_workspace_bar")]
    pub show_workspace_bar: bool,
    /// 编辑区上方的格式工具条；首次启动默认隐藏，可在偏好设置中打开。
    #[serde(default = "default_true")]
    pub show_toolbar: bool,
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

    // ---- 图床：把图片上传到远端仓库，文档里直接写公网 URL ----
    /// 图床平台；`Disabled` 时图片仍写本地路径。
    #[serde(default)]
    pub image_host: crate::image_host::ImageHostKind,
    #[serde(default = "default_image_host_owner")]
    pub image_host_owner: String,
    #[serde(default = "default_image_host_repo")]
    pub image_host_repo: String,
    #[serde(default = "default_image_host_branch")]
    pub image_host_branch: String,
    #[serde(default = "default_image_host_dir")]
    pub image_host_dir: String,
    /// 仓库根目录下的保存路径模板，支持
    /// `{year}` `{month}` `{day}` `{hour}` `{minute}` `{random}` `{filename}`
    /// `{suffix}` `{.suffix}` 占位符。
    #[serde(default = "default_image_host_path")]
    pub image_host_path: String,
    /// 私人令牌：只保存在本地配置文件里，不会写进文档。
    #[serde(default)]
    pub image_host_token: String,
    /// 可选的自定义访问前缀（CDN）；留空按平台默认规则生成 URL。
    #[serde(default)]
    pub image_host_public_base: String,
    /// 阿里云 OSS 地域，写成 `oss-cn-shanghai` 或控制台的「华东2（上海）」都行。
    #[serde(default)]
    pub image_host_oss_region: String,
    /// 阿里云 OSS 存储空间名（Bucket）。
    #[serde(default)]
    pub image_host_oss_bucket: String,
    #[serde(default)]
    pub image_host_oss_key_id: String,
    /// AccessKey Secret：只保存在本地配置文件里，不会写进文档。
    #[serde(default)]
    pub image_host_oss_key_secret: String,
    /// 七牛云区域，写成 `z2` 或控制台的「华南-广东」都行。
    #[serde(default)]
    pub image_host_qiniu_region: String,
    /// 七牛云空间名称（Bucket）。
    #[serde(default)]
    pub image_host_qiniu_bucket: String,
    #[serde(default)]
    pub image_host_qiniu_access_key: String,
    /// SecretKey：只保存在本地配置文件里，不会写进文档。
    #[serde(default)]
    pub image_host_qiniu_secret_key: String,
    /// 七牛空间的访问域名；写进文档的 URL 前缀，七牛没法从空间名推出，必须填。
    /// 可以直接带 `http://` 或 `https://` 前缀。
    #[serde(default)]
    pub image_host_qiniu_domain: String,
    /// 火山引擎 TOS 地域，写成 `cn-beijing` 或控制台的「华北2（北京）」都行。
    #[serde(default)]
    pub image_host_volc_region: String,
    /// 火山引擎 TOS 存储桶名。
    #[serde(default)]
    pub image_host_volc_bucket: String,
    #[serde(default)]
    pub image_host_volc_access_key: String,
    /// Secret Access Key：只保存在本地配置文件里，不会写进文档。
    #[serde(default)]
    pub image_host_volc_secret_key: String,
    /// 百度智能云 BOS 地域，写成 `bj` 或控制台的「华北-北京」都行。
    #[serde(default)]
    pub image_host_bos_region: String,
    /// 百度智能云 BOS 存储桶名。
    #[serde(default)]
    pub image_host_bos_bucket: String,
    #[serde(default)]
    pub image_host_bos_access_key: String,
    /// Secret Access Key：只保存在本地配置文件里，不会写进文档。
    #[serde(default)]
    pub image_host_bos_secret_key: String,
    /// 华为云 OBS 区域，写成 `cn-east-3` 或控制台的「华东-上海一」都行。
    #[serde(default)]
    pub image_host_obs_region: String,
    /// 华为云 OBS 桶名。
    #[serde(default)]
    pub image_host_obs_bucket: String,
    #[serde(default)]
    pub image_host_obs_access_key: String,
    /// 秘密访问密钥：只保存在本地配置文件里，不会写进文档。
    #[serde(default)]
    pub image_host_obs_secret_key: String,
    /// 腾讯云 COS 地域，写成 `ap-nanjing` 或控制台的「南京」都行。
    #[serde(default)]
    pub image_host_cos_region: String,
    /// 腾讯云 COS 存储桶名，必须带 `-APPID` 后缀。
    #[serde(default)]
    pub image_host_cos_bucket: String,
    /// SecretId：访问密钥 ID。
    #[serde(default)]
    pub image_host_cos_secret_id: String,
    /// SecretKey：只保存在本地配置文件里，不会写进文档。
    #[serde(default)]
    pub image_host_cos_secret_key: String,
    /// 用户对主题颜色的覆盖；每项 None 表示跟随当前主题默认色。
    #[serde(default)]
    pub color_overrides: ColorOverrides,
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
    "imgs".to_string()
}
fn default_image_host_owner() -> String {
    "domitius".to_string()
}
fn default_image_host_repo() -> String {
    "markd_images".to_string()
}
fn default_image_host_branch() -> String {
    "master".to_string()
}
fn default_image_host_dir() -> String {
    "images".to_string()
}
fn default_image_host_path() -> String {
    "{year}/{month}/{day}".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: ThemeKind::Light,
            theme_follow_system: false,
            language: Language::ZhCn,
            focus_mode: false,
            typewriter: false,
            source_mode: false,
            show_sidebar: false,
            show_outline: false,
            auto_save: true,
            font_size: 16.0,
            line_height: 1.4,
            workspace: None,
            show_sidebar_header: true,
            sidebar_width: 240.0,
            show_workspace_bar: false,
            show_toolbar: false,
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
            image_assets_dir: "imgs".to_string(),
            image_relative: true,
            export_embed_images: true,
            export_open_after: false,
            math_enabled: true,
            mermaid_enabled: true,
            toc_enabled: true,
            footnote_enabled: true,
            auto_number_heading: false,
            confirm_on_exit: false,
            image_host: crate::image_host::ImageHostKind::Disabled,
            image_host_owner: default_image_host_owner(),
            image_host_repo: default_image_host_repo(),
            image_host_branch: default_image_host_branch(),
            image_host_dir: default_image_host_dir(),
            image_host_path: default_image_host_path(),
            image_host_token: String::new(),
            image_host_public_base: String::new(),
            image_host_oss_region: String::new(),
            image_host_oss_bucket: String::new(),
            image_host_oss_key_id: String::new(),
            image_host_oss_key_secret: String::new(),
            image_host_qiniu_region: String::new(),
            image_host_qiniu_bucket: String::new(),
            image_host_qiniu_access_key: String::new(),
            image_host_qiniu_secret_key: String::new(),
            image_host_qiniu_domain: String::new(),
            image_host_volc_region: String::new(),
            image_host_volc_bucket: String::new(),
            image_host_volc_access_key: String::new(),
            image_host_volc_secret_key: String::new(),
            image_host_bos_region: String::new(),
            image_host_bos_bucket: String::new(),
            image_host_bos_access_key: String::new(),
            image_host_bos_secret_key: String::new(),
            image_host_obs_region: String::new(),
            image_host_obs_bucket: String::new(),
            image_host_obs_access_key: String::new(),
            image_host_obs_secret_key: String::new(),
            image_host_cos_region: String::new(),
            image_host_cos_bucket: String::new(),
            image_host_cos_secret_id: String::new(),
            image_host_cos_secret_key: String::new(),
            color_overrides: ColorOverrides::default(),
        }
    }
}

impl Config {
    /// 手动选主题档位的唯一入口：菜单、命令面板、设置页都走这里。
    /// 选了具体档位就不可能还在「跟随系统」，一起清掉，避免两个字段互相盖。
    pub fn set_theme(&mut self, kind: ThemeKind) {
        self.theme = kind;
        self.theme_follow_system = false;
    }

    pub fn dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".markd")
    }

    pub fn load() -> Self {
        let p = Self::dir().join("config.json");
        let Ok(text) = fs::read_to_string(p) else {
            return Self::default();
        };
        if let Ok(c) = serde_json::from_str::<Config>(&text) {
            return c;
        }
        Self::load_lenient(&text)
    }

    /// 逐字段回退。整体解析失败多半是**某一个字段**的值这个版本读不懂——
    /// 例如旧版本的程序碰到新版本写进配置的平台名——此时只该丢那一项，
    /// 而不是像直接 `unwrap_or_default()` 那样把用户的全部偏好清零。
    fn load_lenient(text: &str) -> Self {
        let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(text)
        else {
            return Self::default();
        };
        let mut kept = serde_json::Map::new();
        for (key, value) in map {
            let mut trial = kept.clone();
            trial.insert(key, value);
            let probe = serde_json::Value::Object(trial.clone());
            if serde_json::from_value::<Config>(probe).is_ok() {
                kept = trial;
            }
        }
        serde_json::from_value::<Config>(serde_json::Value::Object(kept)).unwrap_or_default()
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
        // 本轮新增的三项在老配置里没有键：必须落默认值，既不能解析失败也不能乱填。
        assert_eq!(c.language, Language::ZhCn, "老配置的语言应落默认值");
        assert_eq!(c.line_height, 1.4, "老配置的源码行距应落 1.4");
        assert!(!c.theme_follow_system, "老配置不该凭空开启跟随系统");
        assert!(c.show_sidebar_header);
        assert_eq!(c.sidebar_width, 240.0);
        assert!(!c.show_workspace_bar);
        // 旧配置缺字段时保留工具条，避免老用户升级后功能栏突然消失。
        assert!(c.show_toolbar);
        // 全新配置则默认隐藏工具条（首次启动只看编辑区）。
        assert!(!Config::default().show_toolbar);
        // 旧配置没有颜色覆盖字段，应安全地得到空覆盖（跟随主题默认色）。
        assert!(c.color_overrides.is_empty());
    }

    #[test]
    fn config_color_overrides_roundtrip() {
        use crate::theme::ThemeColor;
        let mut c = Config::default();
        c.color_overrides.set(ThemeColor::Heading, Some([1, 2, 3]));
        c.color_overrides.set(ThemeColor::Bg, Some([250, 251, 252]));
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.color_overrides.get(ThemeColor::Heading), Some([1, 2, 3]));
        assert_eq!(back.color_overrides.get(ThemeColor::Bg), Some([250, 251, 252]));
        assert_eq!(back.color_overrides.get(ThemeColor::Link), None);
        // 空覆盖也能 round-trip。
        let empty = Config::default();
        let json = serde_json::to_string(&empty).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert!(back.color_overrides.is_empty());
    }

    #[test]
    fn config_roundtrip_with_new_fields() {
        let mut c = Config::default();
        c.show_sidebar_header = false;
        c.show_outline = false;
        c.sidebar_width = 320.0;
        c.show_workspace_bar = true;
        c.show_toolbar = false;
        let s = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&s).unwrap();
        assert!(!back.show_sidebar_header);
        assert!(!back.show_outline);
        assert_eq!(back.sidebar_width, 320.0);
        assert!(back.show_workspace_bar);
        assert!(!back.show_toolbar);
    }

    /// 单个字段读不懂时，只丢那一项，不许把用户全部偏好清零。
    /// （旧版本程序碰到新版本写的平台名就是这种情形：实测旧实例把整份
    /// config.json 回写成了默认值，用户名/仓库/令牌一起没了。）
    #[test]
    fn one_unreadable_field_keeps_the_rest() {
        let json = r#"{"theme":"Dark","font_size":19.0,"focus_mode":true,"show_sidebar":false,
            "recent":["/tmp/a.md","/tmp/b.md"],"image_host":"brand-new-host",
            "image_host_owner":"me","image_host_oss_bucket":"my-bucket"}"#;
        let c = Config::load_lenient(json);
        assert_eq!(c.theme, ThemeKind::Dark, "主题要保住");
        assert_eq!(c.font_size, 19.0);
        assert!(c.focus_mode);
        assert!(!c.show_sidebar);
        assert_eq!(
            c.recent,
            vec!["/tmp/a.md".to_string(), "/tmp/b.md".to_string()]
        );
        assert_eq!(c.image_host_owner, "me", "同族里能读的字段要留下");
        assert_eq!(c.image_host_oss_bucket, "my-bucket");
        assert_eq!(
            c.image_host,
            crate::image_host::ImageHostKind::Disabled,
            "认不出的平台名退回默认"
        );
        // 整份不是 JSON：安静回退默认，不 panic。
        let bad = Config::load_lenient("{ 这不是 json");
        assert_eq!(bad.theme, ThemeKind::Light);
        assert!(bad.recent.is_empty());
        // 每项都坏：结果仍是默认值。
        let all_bad = Config::load_lenient(r#"{"theme":"紫","font_size":"很大"}"#);
        assert_eq!(all_bad.theme, ThemeKind::Light);
        assert_eq!(all_bad.font_size, 16.0);
        // 好文件走原来的路径也必须照样通过（别让回退逻辑掩盖主路径的回归）。
        let good: Config = serde_json::from_str(
            r#"{"theme":"Dark","font_size":19.0,"image_host":"oss","image_host_oss_bucket":"b"}"#,
        )
        .unwrap();
        assert_eq!(good.image_host, crate::image_host::ImageHostKind::Oss);
        assert_eq!(good.image_host_oss_bucket, "b");
    }

    /// 旧配置没有图床字段：应回退到「关闭」+ 预置仓库信息，不能反序列化失败。
    #[test]
    fn old_config_gets_image_host_defaults() {
        let json = r#"{"theme":"Dark","focus_mode":false,"typewriter":false,"source_mode":false,"show_sidebar":true,"show_outline":false,"auto_save":true,"font_size":16.0,"workspace":null}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert_eq!(c.image_host, crate::image_host::ImageHostKind::Disabled);
        assert_eq!(c.image_host_owner, "domitius");
        assert_eq!(c.image_host_repo, "markd_images");
        assert_eq!(c.image_host_branch, "master");
        assert_eq!(c.image_host_dir, "images");
        assert_eq!(c.image_host_path, "{year}/{month}/{day}");
        assert!(c.image_host_token.is_empty());
        // 七牛那几项也一样：缺字段回退空值，不报错。
        assert_eq!(c.image_host_qiniu_region, "");
        assert!(c.image_host_qiniu_bucket.is_empty());
        assert!(c.image_host_qiniu_domain.is_empty());
        assert!(c.image_host_qiniu_access_key.is_empty());
        assert!(c.image_host_qiniu_secret_key.is_empty());
        // 百度那几项也一样：缺字段回退空值，不报错。
        assert_eq!(c.image_host_bos_region, "");
        assert!(c.image_host_bos_bucket.is_empty());
        assert!(c.image_host_bos_access_key.is_empty());
        assert!(c.image_host_bos_secret_key.is_empty());
        // 华为那几项也一样：缺字段回退空值，不报错。
        assert_eq!(c.image_host_obs_region, "");
        assert!(c.image_host_obs_bucket.is_empty());
        assert!(c.image_host_obs_access_key.is_empty());
        assert!(c.image_host_obs_secret_key.is_empty());
    }

    /// 七牛字段要能存能取；「平台=七牛」在配置里写的是小写 `qiniu`。
    #[test]
    fn qiniu_fields_roundtrip() {
        let mut c = Config::default();
        c.image_host = crate::image_host::ImageHostKind::Qiniu;
        c.image_host_qiniu_region = "华南-广东".to_string();
        c.image_host_qiniu_bucket = "markd-markd".to_string();
        c.image_host_qiniu_access_key = "AKID".to_string();
        c.image_host_qiniu_secret_key = "SECRET".to_string();
        c.image_host_qiniu_domain = "img.example.com".to_string();
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.image_host, crate::image_host::ImageHostKind::Qiniu);
        assert_eq!(back.image_host_qiniu_region, "华南-广东");
        assert_eq!(back.image_host_qiniu_bucket, "markd-markd");
        assert_eq!(back.image_host_qiniu_access_key, "AKID");
        assert_eq!(back.image_host_qiniu_secret_key, "SECRET");
        assert_eq!(back.image_host_qiniu_domain, "img.example.com");
        // 密钥不落在文档里，只在这份 JSON 里；确认没被别的字段串台。
        assert!(back.image_host_oss_key_secret.is_empty());
    }

    /// 旧实例读不懂新平台名时逐字段回退，七牛那几项要原样保住。
    #[test]
    fn lenient_load_keeps_qiniu_fields() {
        let json = r#"{"theme":"Dark","image_host":"qiniu","image_host_qiniu_bucket":"markd-markd",
            "image_host_qiniu_domain":"touadcd4p.hn-bkt.clouddn.com","font_size":"很大"}"#;
        let c = Config::load_lenient(json);
        assert_eq!(c.theme, ThemeKind::Dark);
        assert_eq!(c.image_host, crate::image_host::ImageHostKind::Qiniu);
        assert_eq!(c.image_host_qiniu_bucket, "markd-markd");
        assert_eq!(c.image_host_qiniu_domain, "touadcd4p.hn-bkt.clouddn.com");
        // 只有那一项坏：字号退回默认。
        assert_eq!(c.font_size, 16.0);
    }

    /// 火山引擎 TOS 字段要能存能取；旧配置缺这几项时回退空值不报错。
    #[test]
    fn volc_fields_roundtrip_and_default_to_empty() {
        let mut c = Config::default();
        c.image_host = crate::image_host::ImageHostKind::Volcengine;
        c.image_host_volc_region = "华北2（北京）".to_string();
        c.image_host_volc_bucket = "markd-markd".to_string();
        c.image_host_volc_access_key = "AKID".to_string();
        c.image_host_volc_secret_key = "SECRET".to_string();
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.image_host,
            crate::image_host::ImageHostKind::Volcengine
        );
        assert_eq!(back.image_host_volc_region, "华北2（北京）");
        assert_eq!(back.image_host_volc_bucket, "markd-markd");
        assert_eq!(back.image_host_volc_access_key, "AKID");
        assert_eq!(back.image_host_volc_secret_key, "SECRET");
        // 不与七牛/OSS 的字段串台。
        assert!(back.image_host_qiniu_bucket.is_empty());
        assert!(back.image_host_oss_bucket.is_empty());
        // 旧配置（还没有火山字段）照读，火山各项为空。
        let old_json = r#"{"theme":"Dark","image_host":"oss","image_host_oss_bucket":"b"}"#;
        let old: Config = serde_json::from_str(old_json).unwrap();
        assert!(old.image_host_volc_bucket.is_empty());
        assert!(old.image_host_volc_region.is_empty());
        assert!(old.image_host_volc_access_key.is_empty());
        assert!(old.image_host_volc_secret_key.is_empty());
        // 逐字段回退路径也要保住火山那几项。
        let lenient = Config::load_lenient(
            r#"{"theme":"Dark","image_host":"volcengine","image_host_volc_bucket":"markd-markd","font_size":"很大"}"#,
        );
        assert_eq!(
            lenient.image_host,
            crate::image_host::ImageHostKind::Volcengine
        );
        assert_eq!(lenient.image_host_volc_bucket, "markd-markd");
        assert_eq!(lenient.font_size, 16.0);
    }

    /// 百度 BOS 四个字段要能存能取；平台在配置里写的是小写 `baidu`；
    /// 缺字段的旧配置与逐字段回退路径都不能把它们连带清零。
    #[test]
    fn bos_fields_roundtrip_and_default_to_empty() {
        let mut c = Config::default();
        c.image_host = crate::image_host::ImageHostKind::Baidu;
        c.image_host_bos_region = "华北-北京".to_string();
        c.image_host_bos_bucket = "markd-markd".to_string();
        c.image_host_bos_access_key = "ALTAKid".to_string();
        c.image_host_bos_secret_key = "SECRET".to_string();
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.image_host, crate::image_host::ImageHostKind::Baidu);
        assert_eq!(back.image_host_bos_region, "华北-北京");
        assert_eq!(back.image_host_bos_bucket, "markd-markd");
        assert_eq!(back.image_host_bos_access_key, "ALTAKid");
        assert_eq!(back.image_host_bos_secret_key, "SECRET");
        // 不与火山/七牛/OSS 的字段串台。
        assert!(back.image_host_volc_bucket.is_empty());
        assert!(back.image_host_qiniu_bucket.is_empty());
        assert!(back.image_host_oss_bucket.is_empty());
        // 旧配置（还没有百度字段）照读，百度各项为空。
        let old_json = r#"{"theme":"Dark","image_host":"baidu","image_host_bos_bucket":"b"}"#;
        let old: Config = serde_json::from_str(old_json).unwrap();
        assert_eq!(old.image_host_bos_bucket, "b");
        assert!(old.image_host_bos_region.is_empty());
        assert!(old.image_host_bos_access_key.is_empty());
        assert!(old.image_host_bos_secret_key.is_empty());
        // 逐字段回退路径（有个字段类型写错时）也要保住百度那几项。
        let lenient = Config::load_lenient(
            r#"{"theme":"Dark","image_host":"baidu","image_host_bos_bucket":"markd-markd","font_size":"很大"}"#,
        );
        assert_eq!(lenient.image_host, crate::image_host::ImageHostKind::Baidu);
        assert_eq!(lenient.image_host_bos_bucket, "markd-markd");
        assert!(lenient.image_host_bos_region.is_empty());
    }

    /// 华为云 OBS 四个字段要能存能取；平台在配置里写的是小写 `huawei`；
    /// 缺字段的旧配置与逐字段回退路径都不能把它们连带清零。
    #[test]
    fn obs_fields_roundtrip_and_default_to_empty() {
        let mut c = Config::default();
        c.image_host = crate::image_host::ImageHostKind::Huawei;
        c.image_host_obs_region = "华东-上海一".to_string();
        c.image_host_obs_bucket = "markd-makrd".to_string();
        c.image_host_obs_access_key = "HWAKID".to_string();
        c.image_host_obs_secret_key = "SECRET".to_string();
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.image_host, crate::image_host::ImageHostKind::Huawei);
        assert_eq!(back.image_host_obs_region, "华东-上海一");
        assert_eq!(back.image_host_obs_bucket, "markd-makrd");
        assert_eq!(back.image_host_obs_access_key, "HWAKID");
        assert_eq!(back.image_host_obs_secret_key, "SECRET");
        // 不与百度/火山/阿里云的字段串台（阿里云那几项同样叫 bucket，但是另一个键名）。
        assert!(back.image_host_bos_bucket.is_empty());
        assert!(back.image_host_volc_bucket.is_empty());
        assert!(back.image_host_oss_bucket.is_empty());
        // 旧配置（还没有华为字段）照读，华为各项为空。
        let old_json = r#"{"theme":"Dark","image_host":"huawei","image_host_obs_bucket":"b"}"#;
        let old: Config = serde_json::from_str(old_json).unwrap();
        assert_eq!(old.image_host_obs_bucket, "b");
        assert!(old.image_host_obs_region.is_empty());
        assert!(old.image_host_obs_access_key.is_empty());
        assert!(old.image_host_obs_secret_key.is_empty());
        // 逐字段回退路径也要保住华为那几项。
        let lenient = Config::load_lenient(
            r#"{"theme":"Dark","image_host":"huawei","image_host_obs_bucket":"markd-makrd","font_size":"很大"}"#,
        );
        assert_eq!(lenient.image_host, crate::image_host::ImageHostKind::Huawei);
        assert_eq!(lenient.image_host_obs_bucket, "markd-makrd");
        assert!(lenient.image_host_obs_region.is_empty());
    }

    /// 腾讯云 COS 四个字段要能存能取；平台在配置里写的是小写 `tencent`；
    /// 缺字段的旧配置与逐字段回退路径都不能把它们连带清零。
    #[test]
    fn cos_fields_roundtrip_and_default_to_empty() {
        let mut c = Config::default();
        c.image_host = crate::image_host::ImageHostKind::Tencent;
        c.image_host_cos_region = "公有云园区-中国-南京".to_string();
        c.image_host_cos_bucket = "markd-1253518225".to_string();
        c.image_host_cos_secret_id = "AKIDX".to_string();
        c.image_host_cos_secret_key = "SECKEYX".to_string();
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.image_host, crate::image_host::ImageHostKind::Tencent);
        assert_eq!(back.image_host_cos_region, "公有云园区-中国-南京");
        assert_eq!(back.image_host_cos_bucket, "markd-1253518225");
        assert_eq!(back.image_host_cos_secret_id, "AKIDX");
        assert_eq!(back.image_host_cos_secret_key, "SECKEYX");
        // 与另外五家对象存储的字段互不串台（华为那几项同样存桶名，但是另一个键名）。
        assert!(back.image_host_obs_bucket.is_empty());
        assert!(back.image_host_bos_bucket.is_empty());
        assert!(back.image_host_volc_bucket.is_empty());
        assert!(back.image_host_oss_bucket.is_empty());
        assert!(back.image_host_qiniu_bucket.is_empty());
        // 默认值与旧配置（还没有腾讯字段）照读，腾讯各项为空。
        let d = Config::default();
        assert_eq!(d.image_host_cos_region, "");
        assert!(d.image_host_cos_bucket.is_empty());
        assert!(d.image_host_cos_secret_id.is_empty());
        assert!(d.image_host_cos_secret_key.is_empty());
        let old: Config = serde_json::from_str(
            r#"{"theme":"Dark","image_host":"tencent","image_host_cos_bucket":"b-1"}"#,
        )
        .unwrap();
        assert_eq!(old.image_host_cos_bucket, "b-1");
        assert!(old.image_host_cos_region.is_empty());
        assert!(old.image_host_cos_secret_key.is_empty());
        // 逐字段回退路径也要保住腾讯那几项。
        let lenient = Config::load_lenient(
            r#"{"theme":"Dark","image_host":"tencent","image_host_cos_bucket":"markd-1253518225","font_size":"很大"}"#,
        );
        assert_eq!(
            lenient.image_host,
            crate::image_host::ImageHostKind::Tencent
        );
        assert_eq!(lenient.image_host_cos_bucket, "markd-1253518225");
        assert!(lenient.image_host_cos_region.is_empty());
    }

    /// 读到不认识的 `image_host` 时会怎样：逐字段回退会**丢掉这一项**（落回 `disabled`），
    /// 其余字段原样保住。这正是「装不上新平台的旧实例」遇到新平台名时的遭遇——
    /// 旧进程一存盘就把图床关掉，所以加平台后要提醒用户用新版界面重选一次。
    #[test]
    fn unknown_image_host_kind_falls_back_to_disabled_and_keeps_the_rest() {
        let lenient = Config::load_lenient(
            r#"{"theme":"Dark","image_host":"tencent-two","image_host_cos_bucket":"markd-1253518225","image_host_obs_bucket":"markd-makrd","font_size":"很大"}"#,
        );
        assert_eq!(
            lenient.image_host,
            crate::image_host::ImageHostKind::Disabled,
            "解析不了的平台名要落回默认值"
        );
        assert_eq!(lenient.image_host_cos_bucket, "markd-1253518225");
        assert_eq!(lenient.image_host_obs_bucket, "markd-makrd");
        // 出错那项之前的 theme、之后的两家桶名都要活下来。
        assert_eq!(lenient.theme, crate::theme::ThemeKind::Dark);
        assert_eq!(lenient.image_host_cos_secret_id, "", "没写的项保持为空");
    }
}
