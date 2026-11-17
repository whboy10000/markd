//! Author: aurelianus <aurelianus@126.com>

//! 图床（图片托管）：把图片上传到远端仓库，并下载远程图片供编辑器预览。
//!
//! 当前支持 Gitee 仓库，设计成可扩展：新增平台时在 [`ImageHostKind`] 里加一个
//! 枚举值、在 [`upload`] 里加一个分派分支即可，配置与 UI 不需要改动。
//!
//! 安全提示：令牌保存在本地 `~/.markd/config.json`（明文），请勿把令牌提交进仓库；
//! 泄露后可在 Gitee「设置 → 私人令牌」里重新生成。

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// 单张图片上传/下载的大小上限（20 MB），避免误传超大文件把界面拖垮。
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

/// 支持的图床平台。新增平台时在这里加枚举值。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageHostKind {
    /// 关闭图床：插入图片仍然写本地路径。
    #[default]
    Disabled,
    /// Gitee 仓库（通过 OpenAPI v5 写入文件）。
    Gitee,
}

impl ImageHostKind {
    pub const ALL: [ImageHostKind; 2] = [ImageHostKind::Disabled, ImageHostKind::Gitee];

    pub fn label(self) -> &'static str {
        match self {
            ImageHostKind::Disabled => "关闭（使用本地图片）",
            ImageHostKind::Gitee => "Gitee 仓库",
        }
    }

    pub fn is_enabled(self) -> bool {
        self != ImageHostKind::Disabled
    }
}

/// 图床配置：从 `Config` 取出后复制给后台线程，避免 UI 线程与网络线程共享状态。
#[derive(Clone, Debug)]
pub struct ImageHostConfig {
    pub kind: ImageHostKind,
    pub owner: String,
    pub repo: String,
    pub branch: String,
    /// 仓库里的图片目录，留空表示根目录。
    pub dir: String,
    /// 目录下的保存路径模板，支持 `{year}` `{month}` `{day}` `{hour}` `{minute}`。
    pub path_pattern: String,
    pub token: String,
    /// 可选的自定义访问前缀（如 CDN 域名）；留空时按平台默认规则生成 URL。
    pub public_base: String,
}

impl ImageHostConfig {
    /// 从全局配置里取出图床所需的字段（复制一份，便于交给后台线程）。
    pub fn from_config(cfg: &crate::workspace::Config) -> Self {
        Self {
            kind: cfg.image_host,
            owner: cfg.image_host_owner.clone(),
            repo: cfg.image_host_repo.clone(),
            branch: cfg.image_host_branch.clone(),
            dir: cfg.image_host_dir.clone(),
            path_pattern: cfg.image_host_path.clone(),
            token: cfg.image_host_token.clone(),
            public_base: cfg.image_host_public_base.clone(),
        }
    }

    /// 检查必填项，返回给用户看的中文错误。
    pub fn validate(&self) -> Result<(), String> {
        if !self.kind.is_enabled() {
            return Err("图床未启用".to_string());
        }
        if self.owner.trim().is_empty() || self.repo.trim().is_empty() {
            return Err("请填写 Gitee 用户名和仓库名".to_string());
        }
        if self.branch.trim().is_empty() {
            return Err("请填写分支（通常是 master）".to_string());
        }
        if self.token.trim().is_empty() {
            return Err("请填写 Gitee 私人令牌".to_string());
        }
        Ok(())
    }
}

/// 保存路径模板用到的日期时间（本地时区）。
#[derive(Clone, Copy, Debug)]
pub struct DateTimeParts {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

fn now_parts() -> DateTimeParts {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    DateTimeParts {
        year: now.year(),
        month: now.month(),
        day: now.day(),
        hour: now.hour(),
        minute: now.minute(),
    }
}

/// 展开保存路径模板：`{year}` `{month}` `{day}` `{hour}` `{minute}`。
/// 结果按 `/` 分段清理：去掉空段、`.`/`..`，不安全字符替换成 `-`。
pub fn render_path_pattern(pattern: &str, t: DateTimeParts) -> String {
    let replaced = pattern
        .replace("{year}", &format!("{:04}", t.year))
        .replace("{month}", &format!("{:02}", t.month))
        .replace("{day}", &format!("{:02}", t.day))
        .replace("{hour}", &format!("{:02}", t.hour))
        .replace("{minute}", &format!("{:02}", t.minute));
    replaced
        .split('/')
        .filter_map(|segment| {
            let s: String = segment
                .trim()
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                        c
                    } else {
                        '-'
                    }
                })
                .collect();
            let s = s.trim_matches('-').to_string();
            (!s.is_empty() && s != "." && s != "..").then_some(s)
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// 上传到仓库里的完整对象路径：`<目录>/<模板展开>/<文件名>`。
pub fn object_path(cfg: &ImageHostConfig, file_name: &str, t: DateTimeParts) -> String {
    let dir = cfg.dir.trim().trim_matches('/');
    let sub = render_path_pattern(&cfg.path_pattern, t);
    [dir, sub.as_str(), file_name]
        .iter()
        .map(|s| s.trim_matches('/'))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// 设置页展示用的路径示例（按当前时间 + 示例文件名）。
pub fn path_preview(cfg: &ImageHostConfig) -> String {
    object_path(cfg, "1730000000000-example.png", now_parts())
}

/// 上传一张图片，返回可直接写进 Markdown 的公网 URL。
///
/// `original_name` 只用来生成远端文件名（时间戳 + 清理后的原名），
/// 真正的图片内容取 `bytes`。
pub fn upload(cfg: &ImageHostConfig, original_name: &str, bytes: &[u8]) -> Result<String, String> {
    cfg.validate()?;
    if bytes.is_empty() {
        return Err("图片内容为空".to_string());
    }
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(format!(
            "图片太大（{} MB），图床上限 {} MB",
            bytes.len() / 1024 / 1024,
            MAX_IMAGE_BYTES / 1024 / 1024
        ));
    }
    let name = remote_name(original_name);
    let path = object_path(cfg, &name, now_parts());
    match cfg.kind {
        ImageHostKind::Gitee => upload_gitee(cfg, &path, bytes),
        ImageHostKind::Disabled => Err("图床未启用".to_string()),
    }
}

/// 远端文件名：毫秒时间戳 + ASCII 化后的原名，避免中文/空格/同名覆盖。
pub fn remote_name(original: &str) -> String {
    let p = Path::new(original);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe.trim_matches('-');
    let safe = if safe.is_empty() { "image" } else { safe };
    let safe: String = safe.chars().take(40).collect();
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"))
        .unwrap_or_else(|| "png".to_string());
    format!("{stamp}-{safe}.{ext}")
}

/// 上传一张 1×1 的测试图片，验证配置是否可用（设置页「测试上传」按钮）。
pub fn test_upload(cfg: &ImageHostConfig) -> Result<String, String> {
    // 1×1 透明 PNG，固定内容，不依赖本地文件。
    const TEST_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xFC,
        0xCF, 0xC0, 0x50, 0x0F, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xA9, 0x8C, 0x21, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    upload(cfg, &format!("markd-selftest-{stamp}.png"), TEST_PNG)
}

/// 判断图片地址是否是 http(s) 远程地址（否则按本地文件处理）。
pub fn is_remote(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://")
}

fn gitee_endpoint(cfg: &ImageHostConfig, path: &str) -> String {
    format!(
        "https://gitee.com/api/v5/repos/{}/{}/contents/{}",
        cfg.owner.trim(),
        cfg.repo.trim(),
        path
    )
}

fn gitee_public_url(cfg: &ImageHostConfig, path: &str) -> String {
    if !cfg.public_base.trim().is_empty() {
        return format!("{}/{}", cfg.public_base.trim().trim_end_matches('/'), path);
    }
    format!(
        "https://gitee.com/{}/{}/raw/{}/{}",
        cfg.owner.trim(),
        cfg.repo.trim(),
        cfg.branch.trim(),
        path
    )
}

fn upload_gitee(cfg: &ImageHostConfig, path: &str, bytes: &[u8]) -> Result<String, String> {
    use base64::Engine as _;

    let name = path.rsplit('/').next().unwrap_or(path);
    let body = serde_json::json!({
        "access_token": cfg.token.trim(),
        "content": base64::engine::general_purpose::STANDARD.encode(bytes),
        "message": format!("markd: 上传图片 {name}"),
        "branch": cfg.branch.trim(),
    });
    let mut resp = ureq::post(&gitee_endpoint(cfg, path))
        .header("User-Agent", "markd-image-host/1.0")
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e| match e {
            ureq::Error::StatusCode(code) => {
                format!("Gitee 返回 HTTP {code}（请检查令牌权限、用户名和仓库名）")
            }
            other => format!("网络请求失败：{other}"),
        })?;
    let status = resp.status();
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "Gitee 上传失败（HTTP {}）：{}",
            status.as_u16(),
            brief(&text)
        ));
    }
    // 优先使用接口返回的 download_url；解析失败时按仓库规则自行拼接。
    let download = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("content")
                .and_then(|c| c.get("download_url"))
                .and_then(|u| u.as_str())
                .map(str::to_string)
        });
    Ok(download.unwrap_or_else(|| gitee_public_url(cfg, path)))
}

/// 只截取错误响应的一小段，避免把整页 HTML 弹给用户。
fn brief(s: &str) -> String {
    let s = s.trim();
    let mut out: String = s.chars().take(300).collect();
    if s.chars().count() > 300 {
        out.push('…');
    }
    out
}

/// 下载远程图片并解码成 egui 图像；失败返回 None（编辑器显示占位框）。
/// 在后台线程里调用，不要直接放到 UI 线程。
pub fn fetch_remote_image(url: &str) -> Option<egui::ColorImage> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build()
        .into();
    let mut resp = agent
        .get(url)
        .header("User-Agent", "markd-image-host/1.0")
        .call()
        .ok()?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(MAX_IMAGE_BYTES)
        .read_to_vec()
        .ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w == 0 || h == 0 {
        return None;
    }
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [w, h],
        img.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ImageHostConfig {
        ImageHostConfig {
            kind: ImageHostKind::Gitee,
            owner: "domitius".into(),
            repo: "markd_images".into(),
            branch: "master".into(),
            dir: "images".into(),
            path_pattern: "{year}/{month}/{day}".into(),
            token: "token".into(),
            public_base: String::new(),
        }
    }

    /// 远端文件名要去掉中文/空格，并保留合法扩展名。
    #[test]
    fn remote_name_is_ascii_and_keeps_ext() {
        let n = remote_name("截图 2026.png");
        assert!(n.ends_with(".png"), "{n}");
        assert!(
            n.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'),
            "{n}"
        );
        assert!(n.starts_with(|c: char| c.is_ascii_digit()), "{n}");
        // 非法扩展名回退成 png。
        assert!(remote_name("a.tar.gz.bmp").ends_with(".png"));
    }

    #[test]
    fn public_url_uses_repo_rules() {
        let c = cfg();
        assert_eq!(
            gitee_public_url(&c, "images/a.png"),
            "https://gitee.com/domitius/markd_images/raw/master/images/a.png"
        );
        let mut c = cfg();
        c.public_base = "https://cdn.example.com/img/".into();
        assert_eq!(
            gitee_public_url(&c, "images/a.png"),
            "https://cdn.example.com/img/images/a.png"
        );
    }

    #[test]
    fn config_validation_requires_token() {
        let mut c = cfg();
        assert!(c.validate().is_ok());
        c.token.clear();
        assert!(c.validate().unwrap_err().contains("令牌"));
        c.kind = ImageHostKind::Disabled;
        assert!(c.validate().is_err());
    }

    #[test]
    fn remote_detection() {
        assert!(is_remote("https://gitee.com/a.png"));
        assert!(is_remote("HTTP://example.com/a.png"));
        assert!(!is_remote("imgs/a.png"));
        assert!(!is_remote("/tmp/a.png"));
    }

    fn fixed_time() -> DateTimeParts {
        DateTimeParts {
            year: 2026,
            month: 11,
            day: 17,
            hour: 9,
            minute: 5,
        }
    }

    /// 默认模板按年月日分文件夹。
    #[test]
    fn path_pattern_expands_date_parts() {
        assert_eq!(
            render_path_pattern("{year}/{month}/{day}", fixed_time()),
            "2026/11/17"
        );
        assert_eq!(
            render_path_pattern("posts/{year}-{month}/{day}", fixed_time()),
            "posts/2026-11/17"
        );
        assert_eq!(
            render_path_pattern("{year}{month}{day}/{hour}{minute}", fixed_time()),
            "20261117/0905"
        );
        // 留空 → 直接放在目录下
        assert_eq!(render_path_pattern("", fixed_time()), "");
    }

    /// 路径清理：丢掉空段/`.`/`..`，非法字符替换成 `-`。
    #[test]
    fn path_pattern_is_sanitized() {
        assert_eq!(
            render_path_pattern("../{year}//a b", fixed_time()),
            "2026/a-b"
        );
        assert_eq!(render_path_pattern("/./", fixed_time()), "");
    }

    /// 完整对象路径 = 目录 / 模板 / 文件名。
    #[test]
    fn object_path_joins_dir_pattern_and_name() {
        let c = cfg();
        assert_eq!(
            object_path(&c, "a.png", fixed_time()),
            "images/2026/11/17/a.png"
        );
        let mut c = cfg();
        c.dir = "/pics/".into();
        c.path_pattern = String::new();
        assert_eq!(object_path(&c, "a.png", fixed_time()), "pics/a.png");
    }
}
