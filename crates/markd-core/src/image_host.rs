//! Author: aurelianus <aurelianus@126.com>

//! 图床（图片托管）：把图片上传到远端仓库/存储，并下载远程图片供编辑器预览。
//!
//! 当前支持 Gitee 仓库、GitHub 仓库、阿里云 OSS 与七牛云 Kodo，设计成可扩展：新增平台时在
//! [`ImageHostKind`] 里加一个枚举值、在 [`upload`] 里加一个分派分支即可，
//! 通用配置（目录 / 保存路径模板）不需要改动。
//!
//! 安全提示：令牌与 AccessKey/SecretKey 保存在本地 `~/.markd/config.json`（明文），
//! 请勿把密钥提交进仓库；泄露后可在 Gitee「设置 → 私人令牌」、
//! GitHub「Settings → Developer settings」、阿里云「RAM 访问控制 → AccessKey」
//! 或七牛「个人面板 → 证书管理」里重新生成。

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
    /// GitHub 仓库（通过 REST Contents API 写入文件）。
    Github,
    /// 阿里云 OSS 对象存储（PUT Object + V1 签名）。
    Oss,
    /// 七牛云 Kodo 对象存储（表单上传 + 上传凭证）。
    Qiniu,
    /// 火山引擎 TOS 对象存储（PUT Object + TOS4-HMAC-SHA256 签名）。
    Volcengine,
    /// 百度智能云 BOS 对象存储（PUT Object + bce-auth-v1 签名）。
    Baidu,
    /// 华为云 OBS 对象存储（PUT Object + V2 签名）。
    Huawei,
    /// 腾讯云 COS 对象存储（PUT Object + v5 签名）。
    Tencent,
}

impl ImageHostKind {
    pub const ALL: [ImageHostKind; 9] = [
        ImageHostKind::Disabled,
        ImageHostKind::Gitee,
        ImageHostKind::Github,
        ImageHostKind::Oss,
        ImageHostKind::Qiniu,
        ImageHostKind::Volcengine,
        ImageHostKind::Baidu,
        ImageHostKind::Huawei,
        ImageHostKind::Tencent,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ImageHostKind::Disabled => "关闭（使用本地图片）",
            ImageHostKind::Gitee => "Gitee 仓库",
            ImageHostKind::Github => "GitHub 仓库",
            ImageHostKind::Oss => "阿里云 OSS",
            ImageHostKind::Qiniu => "七牛云 Kodo",
            ImageHostKind::Volcengine => "火山引擎 TOS",
            ImageHostKind::Baidu => "百度智能云 BOS",
            ImageHostKind::Huawei => "华为云 OBS",
            ImageHostKind::Tencent => "腾讯云 COS",
        }
    }

    pub fn is_enabled(self) -> bool {
        self != ImageHostKind::Disabled
    }

    /// 拼进中文提示里的平台名。
    pub fn platform_name(self) -> &'static str {
        match self {
            ImageHostKind::Disabled => "图床",
            ImageHostKind::Gitee => "Gitee",
            ImageHostKind::Github => "GitHub",
            ImageHostKind::Oss => "阿里云 OSS",
            ImageHostKind::Qiniu => "七牛云",
            ImageHostKind::Volcengine => "火山引擎",
            ImageHostKind::Baidu => "百度智能云",
            ImageHostKind::Huawei => "华为云",
            ImageHostKind::Tencent => "腾讯云",
        }
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
    /// 目录下的保存路径模板，支持 `{year}` `{month}` `{day}` `{hour}` `{minute}`
    /// `{random}` `{filename}` `{suffix}` `{.suffix}`。
    pub path_pattern: String,
    pub token: String,
    /// 可选的自定义访问前缀（如 CDN 域名）；留空时按平台默认规则生成 URL。
    pub public_base: String,
    /// OSS 地域（已归一，如 `oss-cn-shanghai`）；其它平台忽略。
    pub oss_region: String,
    /// OSS 存储空间名（Bucket）。
    pub oss_bucket: String,
    pub oss_access_key_id: String,
    pub oss_access_key_secret: String,
    /// 七牛区域 ID（已归一，如 `z2`）；其它平台忽略。
    pub qiniu_region: String,
    /// 七牛空间名称（Bucket）。
    pub qiniu_bucket: String,
    pub qiniu_access_key: String,
    /// SecretKey：只保存在本地配置文件里，不会写进文档。
    pub qiniu_secret_key: String,
    /// 七牛空间的访问域名；写进文档的 URL 前缀，平台无法从桶名推出，必须填。
    pub qiniu_domain: String,
    /// 火山引擎 TOS 地域 ID（已归一，如 `cn-beijing`）；其它平台忽略。
    pub volc_region: String,
    /// 火山引擎 TOS 存储桶名。
    pub volc_bucket: String,
    pub volc_access_key_id: String,
    /// Secret Access Key：只保存在本地配置文件里，不会写进文档。
    pub volc_secret_access_key: String,
    /// 百度智能云 BOS 地域（已归一，如 `bj`）；其它平台忽略。
    pub bos_region: String,
    /// 百度智能云 BOS 存储桶名。
    pub bos_bucket: String,
    pub bos_access_key_id: String,
    /// Secret Access Key：只保存在本地配置文件里，不会写进文档。
    pub bos_secret_access_key: String,
    /// 华为云 OBS 区域（已归一，如 `cn-east-3`）；其它平台忽略。
    pub obs_region: String,
    /// 华为云 OBS 桶名。
    pub obs_bucket: String,
    pub obs_access_key_id: String,
    /// 秘密访问密钥：只保存在本地配置文件里，不会写进文档。
    pub obs_secret_access_key: String,
    /// 腾讯云 COS 地域（已归一，如 `ap-nanjing`）；其它平台忽略。
    pub cos_region: String,
    /// 腾讯云 COS 存储桶名（必须写成「名称-APPID」）。
    pub cos_bucket: String,
    /// SecretId：访问密钥 ID。
    pub cos_secret_id: String,
    /// SecretKey：只保存在本地配置文件里，不会写进文档。
    pub cos_secret_key: String,
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
            oss_region: normalize_oss_region(&cfg.image_host_oss_region),
            oss_bucket: cfg.image_host_oss_bucket.clone(),
            oss_access_key_id: cfg.image_host_oss_key_id.clone(),
            oss_access_key_secret: cfg.image_host_oss_key_secret.clone(),
            qiniu_region: normalize_qiniu_region(&cfg.image_host_qiniu_region),
            qiniu_bucket: cfg.image_host_qiniu_bucket.clone(),
            qiniu_access_key: cfg.image_host_qiniu_access_key.clone(),
            qiniu_secret_key: cfg.image_host_qiniu_secret_key.clone(),
            qiniu_domain: cfg.image_host_qiniu_domain.clone(),
            volc_region: normalize_volc_region(&cfg.image_host_volc_region),
            volc_bucket: cfg.image_host_volc_bucket.clone(),
            volc_access_key_id: cfg.image_host_volc_access_key.clone(),
            volc_secret_access_key: cfg.image_host_volc_secret_key.clone(),
            bos_region: normalize_bos_region(&cfg.image_host_bos_region),
            bos_bucket: cfg.image_host_bos_bucket.clone(),
            bos_access_key_id: cfg.image_host_bos_access_key.clone(),
            bos_secret_access_key: cfg.image_host_bos_secret_key.clone(),
            obs_region: normalize_obs_region(&cfg.image_host_obs_region),
            obs_bucket: cfg.image_host_obs_bucket.clone(),
            obs_access_key_id: cfg.image_host_obs_access_key.clone(),
            obs_secret_access_key: cfg.image_host_obs_secret_key.clone(),
            cos_region: normalize_cos_region(&cfg.image_host_cos_region),
            cos_bucket: cfg.image_host_cos_bucket.clone(),
            cos_secret_id: cfg.image_host_cos_secret_id.clone(),
            cos_secret_key: cfg.image_host_cos_secret_key.clone(),
        }
    }

    /// 检查必填项，返回给用户看的中文错误。
    pub fn validate(&self) -> Result<(), String> {
        if !self.kind.is_enabled() {
            return Err("图床未启用".to_string());
        }
        let p = self.kind.platform_name();
        if self.kind == ImageHostKind::Tencent {
            if self.cos_bucket.trim().is_empty() {
                return Err("请填写腾讯云 COS 的存储桶名（形如 名称-APPID）".to_string());
            }
            check_cos_region(&self.cos_region)?;
            if self.cos_secret_id.trim().is_empty() || self.cos_secret_key.trim().is_empty() {
                return Err("请填写腾讯云的 SecretId 与 SecretKey".to_string());
            }
            return Ok(());
        }
        if self.kind == ImageHostKind::Huawei {
            if self.obs_bucket.trim().is_empty() {
                return Err("请填写华为云 OBS 的桶名".to_string());
            }
            if let Err(e) = check_obs_region(&self.obs_region) {
                return Err(e);
            }
            if self.obs_access_key_id.trim().is_empty()
                || self.obs_secret_access_key.trim().is_empty()
            {
                return Err("请填写华为云的访问密钥 ID 与秘密访问密钥".to_string());
            }
            return Ok(());
        }
        if self.kind == ImageHostKind::Baidu {
            if self.bos_bucket.trim().is_empty() {
                return Err("请填写百度智能云 BOS 的存储桶名".to_string());
            }
            if let Err(e) = check_bos_region(&self.bos_region) {
                return Err(e);
            }
            if self.bos_access_key_id.trim().is_empty()
                || self.bos_secret_access_key.trim().is_empty()
            {
                return Err("请填写百度智能云的 AccessKey 与 Secret Access Key".to_string());
            }
            return Ok(());
        }
        if self.kind == ImageHostKind::Volcengine {
            if self.volc_bucket.trim().is_empty() {
                return Err("请填写火山引擎 TOS 的存储桶名".to_string());
            }
            if let Err(e) = check_volc_region(self.volc_region.trim()) {
                return Err(e);
            }
            if self.volc_access_key_id.trim().is_empty()
                || self.volc_secret_access_key.trim().is_empty()
            {
                return Err("请填写火山引擎的 AccessKey 与 Secret Access Key".to_string());
            }
            return Ok(());
        }
        if self.kind == ImageHostKind::Qiniu {
            if self.qiniu_bucket.trim().is_empty() {
                return Err("请填写七牛的空间名称（Bucket）".to_string());
            }
            if self.qiniu_region.trim().is_empty() {
                return Err("请填写七牛的区域，如 华南-广东（z2）".to_string());
            }
            let region = normalize_qiniu_region(&self.qiniu_region);
            if !QINIU_REGIONS.iter().any(|(id, _)| *id == region) {
                return Err(qiniu_unknown_region_error(self.qiniu_region.trim()));
            }
            if self.qiniu_access_key.trim().is_empty() || self.qiniu_secret_key.trim().is_empty() {
                return Err("请填写七牛的 AccessKey 与 SecretKey".to_string());
            }
            if self.qiniu_public_base().is_empty() {
                return Err("请填写七牛空间的访问域名，例如 xxx.hn-bkt.clouddn.com".to_string());
            }
            return Ok(());
        }
        if self.kind == ImageHostKind::Oss {
            if self.oss_bucket.trim().is_empty() {
                return Err("请填写 OSS 存储空间名（Bucket）".to_string());
            }
            if self.oss_region.trim().is_empty() {
                return Err("请填写 OSS 地域，如 oss-cn-shanghai（华东2·上海）".to_string());
            }
            if self.oss_access_key_id.trim().is_empty()
                || self.oss_access_key_secret.trim().is_empty()
            {
                return Err("请填写阿里云 AccessKey ID 与 AccessKey Secret".to_string());
            }
            return Ok(());
        }
        if self.owner.trim().is_empty() || self.repo.trim().is_empty() {
            return Err(format!("请填写 {p} 用户名和仓库名"));
        }
        if self.branch.trim().is_empty() {
            return Err("请填写分支（通常是 master）".to_string());
        }
        if self.token.trim().is_empty() {
            return Err(match self.kind {
                ImageHostKind::Github => "请填写 GitHub 令牌（需要 Contents 写权限）".to_string(),
                _ => format!("请填写 {p} 私人令牌"),
            });
        }
        Ok(())
    }

    /// 七牛访问域名（可以带 `http://` / `https://` 前缀）；没填时退回通用的自定义前缀。
    fn qiniu_public_base(&self) -> String {
        let d = self.qiniu_domain.trim();
        let raw = if d.is_empty() {
            self.public_base.trim()
        } else {
            d
        };
        raw.trim_end_matches('/').to_string()
    }

    /// 写进文档的七牛公网地址。
    fn qiniu_url(&self, key: &str) -> String {
        let base = self.qiniu_public_base();
        if base.is_empty() {
            return String::new();
        }
        // 实测：`*.clouddn.com` 这类七牛测试域名没有匹配该主名的 HTTPS 证书
        // （https 回 421、http 回 200），所以不带 scheme 时按 http 写；
        // 自定义域名（已备案、有证书）一律 https。域名里自己写了 scheme 就照用。
        let base = if base.starts_with("http://") || base.starts_with("https://") {
            base
        } else if base.ends_with(".clouddn.com") {
            format!("http://{base}")
        } else {
            format!("https://{base}")
        };
        format!("{base}/{key}")
    }
}

/// 归一 OSS 地域写法：用户在控制台看到的是「华东2（上海）」，
/// 而拼域名要的是 `oss-cn-shanghai`。常见写法都收一下：
/// 完整 endpoint、带桶名的三级域名、`cn-shanghai`、控制台中文名。
pub fn normalize_oss_region(raw: &str) -> String {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return String::new();
    }
    if let Some(named) = oss_region_by_chinese(&s) {
        return named.to_string();
    }
    let s = s
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_matches('/');
    // `markd-markd.oss-cn-shanghai.aliyuncs.com` → 从 `oss-` 段起截取。
    let core = s.trim_end_matches(".aliyuncs.com").trim_end_matches("/");
    let region = match core.find("oss-") {
        Some(i) => &core[i..],
        None => core,
    };
    // 只认「oss-xxx」「xxx」两种形态里像地域名的那部分，避免把整串带进去。
    let region = region.split('.').next().unwrap_or(region);
    if region.is_empty() {
        return String::new();
    }
    if region.starts_with("oss-") {
        region.to_string()
    } else if region.starts_with("cn-")
        || region.starts_with("ap-")
        || region.starts_with("us-")
        || region.starts_with("eu-")
        || region.starts_with("me-")
        || region.starts_with("rus-")
    {
        format!("oss-{region}")
    } else {
        // 认不出来就原样留着，让上传失败时错误里带上这个地域名，比静默改写诚实。
        region.to_string()
    }
}

/// 阿里云控制台上的中文地域名 → endpoint 地域 ID。
fn oss_region_by_chinese(s: &str) -> Option<&'static str> {
    // 控制台写成「华东2（上海）」，括号有全角/半角两种，先只留汉字和数字。
    let key: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || '\u{4e00}' <= *c && *c <= '\u{9fff}')
        .collect();
    const TABLE: [(&str, &str); 10] = [
        ("华东1杭州", "oss-cn-hangzhou"),
        ("华东2上海", "oss-cn-shanghai"),
        ("华北1青岛", "oss-cn-qingdao"),
        ("华北2北京", "oss-cn-beijing"),
        ("华北3张家口", "oss-cn-zhangjiakou"),
        ("华北5呼和浩特", "oss-cn-huhehaote"),
        ("华南1深圳", "oss-cn-shenzhen"),
        ("西南1成都", "oss-cn-chengdu"),
        ("香港", "oss-cn-hongkong"),
        ("新加坡", "oss-ap-southeast-1"),
    ];
    TABLE
        .iter()
        .find(|(name, _)| key == *name)
        .map(|(_, id)| *id)
}

/// 保存路径模板用到的日期时间与随机后缀（本地时区）。
#[derive(Clone, Debug)]
pub struct DateTimeParts {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// 每次上传换一个 6 位十六进制，供 `{random}` 占位符防撞名。
    pub random: String,
}

/// 模板里 `{filename}` `{suffix}` 需要的文件名信息（不含目录）。
#[derive(Clone, Copy, Debug)]
pub struct NameParts<'a> {
    pub stem: &'a str,
    /// 小写扩展名，不含点。
    pub suffix: &'a str,
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
        random: random_suffix(),
    }
}

/// 时钟位与进程内序号如何合成一个 24 位散列值（`random_suffix` 的全部算术，拆出来才测得到）。
/// 只能相加，不能异或：序号 +1 恰好顶掉时钟进位的同一格时，异或会把两次变化互相抵消，
/// 后缀一模一样（CI 实测三连发报出过 `b9f5ee b9f5ee b9f5ed`）。
/// 相加则「时钟增量 + 序号增量」严格为正，24 位回绕一圈要 2^24 格 × 2^17ns ≈ 36 分钟，
/// 同一轮上传里不可能撞上。
fn suffix_mix(nanos: u64, seq: u64) -> u64 {
    nanos.rotate_right(17).wrapping_add(seq)
}

/// 随机后缀：纳秒时间戳与进程内自增序号混合后取 6 位十六进制。
/// 必须先把时间戳的高位转到低位再混合：直接 `nanos + seq` 的话，
/// 掩码会把序号整个丢掉，两次调用只要相隔 16.7ms 的整数倍就撞出同一个后缀。
fn random_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    hex_suffix(suffix_mix(nanos, SEQ.fetch_add(1, Ordering::Relaxed)))
}

/// 取低 24 位写成 6 位小写十六进制：宽度固定，模板拼出来的路径才对得齐。
fn hex_suffix(mix: u64) -> String {
    format!("{:06x}", mix & 0x00ff_ffff)
}

/// 展开保存路径模板：`{year}` `{month}` `{day}` `{hour}` `{minute}` `{random}`
/// `{filename}` `{suffix}` `{.suffix}`。
/// 结果按 `/` 分段清理：去掉空段、`.`/`..`，不安全字符替换成 `-`。
pub fn render_path_pattern(pattern: &str, name: NameParts, t: &DateTimeParts) -> String {
    let replaced = pattern
        .replace("{year}", &format!("{:04}", t.year))
        .replace("{month}", &format!("{:02}", t.month))
        .replace("{day}", &format!("{:02}", t.day))
        .replace("{hour}", &format!("{:02}", t.hour))
        .replace("{minute}", &format!("{:02}", t.minute))
        .replace("{random}", &t.random)
        .replace("{filename}", name.stem)
        .replace("{.suffix}", &format!(".{}", name.suffix))
        .replace("{suffix}", name.suffix);
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
/// 模板里写了 `{filename}` 时，文件名已经由模板展开出来，不再追加。
pub fn object_path(cfg: &ImageHostConfig, file_name: &str, t: &DateTimeParts) -> String {
    let dir = cfg.dir.trim().trim_matches('/');
    let sub = render_path_pattern(&cfg.path_pattern, name_parts(file_name), t);
    let mut segs: Vec<&str> = vec![dir, sub.as_str()];
    if !pattern_has_name(&cfg.path_pattern) {
        segs.push(file_name);
    }
    segs.iter()
        .map(|s| s.trim_matches('/'))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// 设置页展示用的路径示例（按当前时间 + 示例文件名，与真实上传同一条命名规则）。
pub fn path_preview(cfg: &ImageHostConfig) -> String {
    let mut t = now_parts();
    // 随机后缀写成固定值：示例每帧重算，跟着变会让界面文字跳动。
    t.random = "a1b2c3".to_string();
    let name = if pattern_has_name(&cfg.path_pattern) {
        "example.png"
    } else {
        "1730000000000-example.png"
    };
    object_path(cfg, name, &t)
}

/// 模板里是否自己写了文件名：写了就不再自动加毫秒时间戳前缀。
fn pattern_has_name(pattern: &str) -> bool {
    pattern.contains("{filename}")
}

/// 文件名的主干与扩展名（给 `{filename}` / `{suffix}` 用）。
fn name_parts(file_name: &str) -> NameParts<'_> {
    let p = Path::new(file_name);
    NameParts {
        stem: p.file_stem().and_then(|s| s.to_str()).unwrap_or_default(),
        suffix: p.extension().and_then(|e| e.to_str()).unwrap_or_default(),
    }
}

/// 上传一张图片，返回可直接写进 Markdown 的公网 URL。
///
/// `original_name` 只用来生成远端文件名，真正的图片内容取 `bytes`。
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
    let name = remote_name_for(cfg, original_name);
    let path = object_path(cfg, &name, &now_parts());
    match cfg.kind {
        ImageHostKind::Gitee => upload_gitee(cfg, &path, bytes),
        ImageHostKind::Github => upload_github(cfg, &path, bytes),
        ImageHostKind::Oss => upload_oss(cfg, &path, bytes),
        ImageHostKind::Qiniu => upload_qiniu(cfg, &path, bytes),
        ImageHostKind::Volcengine => upload_volc(cfg, &path, bytes),
        ImageHostKind::Baidu => upload_bos(cfg, &path, bytes),
        ImageHostKind::Huawei => upload_obs(cfg, &path, bytes),
        ImageHostKind::Tencent => upload_cos(cfg, &path, bytes),
        ImageHostKind::Disabled => Err("图床未启用".to_string()),
    }
}

/// 远端文件名：模板写了 `{filename}` 就用清理过的原名（日期与随机数由模板负责），
/// 否则加毫秒时间戳前缀避免同名覆盖。
fn remote_name_for(cfg: &ImageHostConfig, original: &str) -> String {
    if pattern_has_name(&cfg.path_pattern) {
        let stem = safe_stem(original);
        format!("{stem}.{}", safe_suffix(original))
    } else {
        remote_name(original)
    }
}

/// 远端文件名：毫秒时间戳 + ASCII 化后的原名，避免中文/空格/同名覆盖。
pub fn remote_name(original: &str) -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{stamp}-{}.{}", safe_stem(original), safe_suffix(original))
}

/// 原名主干的 ASCII 清理版：中文/空格换成 `-`，去首尾 `-`，最长 40 字符。
fn safe_stem(original: &str) -> String {
    let stem = Path::new(original)
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
    safe.chars().take(40).collect()
}

/// 扩展名小写版；不是支持的图片格式（或缺失）时回退 `png`。
fn safe_suffix(original: &str) -> String {
    Path::new(original)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"))
        .unwrap_or_else(|| "png".to_string())
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

fn github_endpoint(cfg: &ImageHostConfig, path: &str) -> String {
    format!(
        "https://api.github.com/repos/{}/{}/contents/{}",
        cfg.owner.trim(),
        cfg.repo.trim(),
        path
    )
}

fn github_public_url(cfg: &ImageHostConfig, path: &str) -> String {
    if !cfg.public_base.trim().is_empty() {
        return format!("{}/{}", cfg.public_base.trim().trim_end_matches('/'), path);
    }
    format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        cfg.owner.trim(),
        cfg.repo.trim(),
        cfg.branch.trim(),
        path
    )
}

/// GitHub 常见状态码对应的中文补充说明。
fn github_hint(code: u16) -> &'static str {
    match code {
        401 => "（令牌无效或已过期，请重新生成）",
        403 => "（令牌没有 Contents 写权限，或触发了频率限制）",
        404 => "（用户名、仓库或分支不对；私有仓库要确认令牌能访问它）",
        409 => "（该路径已有同名文件且 SHA 对不上，稍后重试）",
        422 => "（保存路径模板展开出的文件名不合法，或仓库还是空的）",
        _ => "（请检查令牌权限、用户名和仓库名）",
    }
}

/// 一次 Contents API 写入的失败原因。
enum GithubError {
    /// GitHub 回了非 2xx。
    Status(u16),
    /// 请求本身没发出去（DNS/超时/TLS）。
    Transport(String),
}

impl GithubError {
    fn message(&self) -> String {
        match self {
            GithubError::Status(code) => {
                format!("GitHub 上传失败（HTTP {code}）：{}", github_hint(*code))
            }
            GithubError::Transport(e) => format!("网络请求失败：{e}"),
        }
    }
}

/// Contents API 的请求体。`sha` 只在覆盖已存在的文件时才给（实测缺了回 422）。
fn github_body(
    cfg: &ImageHostConfig,
    path: &str,
    bytes: &[u8],
    sha: Option<&str>,
) -> serde_json::Value {
    use base64::Engine as _;

    let name = path.rsplit('/').next().unwrap_or(path);
    let mut body = serde_json::json!({
        "message": format!("markd: 上传图片 {name}"),
        "content": base64::engine::general_purpose::STANDARD.encode(bytes),
        "branch": cfg.branch.trim(),
    });
    if let Some(sha) = sha {
        body["sha"] = serde_json::Value::String(sha.to_string());
    }
    body
}

/// 发一次写入请求，成功返回响应体文本。
fn github_put(
    cfg: &ImageHostConfig,
    path: &str,
    bytes: &[u8],
    sha: Option<&str>,
) -> Result<String, GithubError> {
    let mut resp = ureq::put(&github_endpoint(cfg, path))
        .header("Authorization", format!("Bearer {}", cfg.token.trim()))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "markd-image-host/1.0")
        .send_json(github_body(cfg, path, bytes, sha))
        .map_err(|e| match e {
            ureq::Error::StatusCode(code) => GithubError::Status(code),
            other => GithubError::Transport(other.to_string()),
        })?;
    let status = resp.status();
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    if !status.is_success() {
        return Err(GithubError::Status(status.as_u16()));
    }
    Ok(text)
}

/// 读回同路径已有文件的 SHA：GitHub 覆盖写必须带上它。读不到返回 None。
fn github_existing_sha(cfg: &ImageHostConfig, path: &str) -> Option<String> {
    let url = format!("{}?ref={}", github_endpoint(cfg, path), cfg.branch.trim());
    let mut resp = ureq::get(&url)
        .header("Authorization", format!("Bearer {}", cfg.token.trim()))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "markd-image-host/1.0")
        .call()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp.body_mut().read_to_string().ok()?;
    github_sha_of(&text)
}

/// 从 Contents API 的响应里取文件 SHA。
fn github_sha_of(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    // 直接 GET 单个文件时 SHA 在顶层，且 `content` 是 base64 字符串（不是对象，
    // 拿它当对象找 sha 会得到 None）；写入响应里文件信息才放在 `content` 对象下。
    let top = v.get("sha").and_then(|s| s.as_str());
    let nested = v
        .get("content")
        .and_then(|c| c.get("sha"))
        .and_then(|s| s.as_str());
    top.or(nested).map(str::to_string)
}

/// 上传后的公网地址：优先用接口回的 `download_url`，缺失时按仓库规则拼。
fn github_url(cfg: &ImageHostConfig, path: &str, body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("content")
                .and_then(|c| c.get("download_url"))
                .and_then(|u| u.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| github_public_url(cfg, path))
}

fn upload_github(cfg: &ImageHostConfig, path: &str, bytes: &[u8]) -> Result<String, String> {
    let body = match github_put(cfg, path, bytes, None) {
        Ok(body) => body,
        // 同路径已经有文件：先 GET 回旧 SHA 再覆盖写一次。
        Err(err @ (GithubError::Status(409) | GithubError::Status(422))) => {
            let Some(sha) = github_existing_sha(cfg, path) else {
                return Err(err.message());
            };
            github_put(cfg, path, bytes, Some(&sha)).map_err(|e| e.message())?
        }
        Err(err) => return Err(err.message()),
    };
    Ok(github_url(cfg, path, &body))
}

/// `{bucket}.{region}.aliyuncs.com`。
fn oss_host(cfg: &ImageHostConfig) -> String {
    format!(
        "{}.{}.aliyuncs.com",
        cfg.oss_bucket.trim().trim_matches('/'),
        cfg.oss_region.trim().trim_matches('/')
    )
}

/// 写进文档的公网地址；设了自定义前缀（CDN/加速域名）就用它。
fn oss_public_url(cfg: &ImageHostConfig, key: &str) -> String {
    let base = cfg.public_base.trim().trim_end_matches('/');
    if !base.is_empty() {
        let base = if base.starts_with("http://") || base.starts_with("https://") {
            base.to_string()
        } else {
            format!("https://{base}")
        };
        return format!("{base}/{key}");
    }
    format!("https://{}/{key}", oss_host(cfg))
}

/// OSS V1 签名的待签字符串：
/// `VERB\nContent-MD5\nContent-Type\nDate\n{按名排序的 x-oss-* 头}{/桶/对象名}`。
/// 阿里云 OSS V1 与华为云 OBS V2 的待签字符串形态相同：
/// `动词\nContent-MD5\nContent-Type\nDate\n/{桶}/{对象名}`。
/// 注意 OBS 那侧的「对象名」要填百分号编码后的形式，OSS 填原样。
fn v1_string_to_sign(
    verb: &str,
    content_md5: &str,
    content_type: &str,
    date: &str,
    bucket: &str,
    key: &str,
) -> String {
    format!("{verb}\n{content_md5}\n{content_type}\n{date}\n/{bucket}/{key}")
}

/// base64(HMAC-SHA1(secret, 待签字符串))。
fn v1_signature(secret: &str, string_to_sign: &str) -> Result<String, String> {
    use base64::Engine as _;
    use hmac::{Hmac, Mac};
    use sha1::Sha1;

    let mut mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes())
        .map_err(|_| "AccessKey Secret 长度不合法".to_string())?;
    mac.update(string_to_sign.as_bytes());
    Ok(base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
}

/// OSS 与 OBS 都要的 `Date` 头：RFC1123 GMT，月名/星期名必须是英文（`chrono` 的
/// `%a`/`%b` 不受系统 locale 影响，正合适）。
fn v1_date(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

fn v1_now_date() -> String {
    v1_date(chrono::Utc::now())
}

/// 按对象名扩展名定 Content-Type；OSS 用它当待签字符串的一部分，七牛只当表单里的文件类型。
fn content_type_by_ext(key: &str) -> String {
    let ext = Path::new(key)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// OSS 常见状态码对应的中文补充说明。
fn oss_hint(code: u16) -> &'static str {
    match code {
        400 => "（AccessKey ID 或签名格式不对，检查有没有多空格）",
        403 => "（AccessKey 没有写权限、签名不对，或本机时钟与标准时间差了 15 分钟以上）",
        404 => "（存储空间名或地域不对；桶不存在时也是这个错）",
        _ => "（请检查地域、存储空间名与 AccessKey）",
    }
}

/// 写入地址：永远是桶的 endpoint，不受自定义访问前缀影响（CDN 域名一般不接 PUT）。
fn oss_put_url(cfg: &ImageHostConfig, key: &str) -> String {
    format!("https://{}/{key}", oss_host(cfg))
}

fn upload_oss(cfg: &ImageHostConfig, key: &str, bytes: &[u8]) -> Result<String, String> {
    let ct = content_type_by_ext(key);
    let date = v1_now_date();
    let sts = v1_string_to_sign(
        "PUT",
        "",
        &ct,
        &date,
        cfg.oss_bucket.trim().trim_matches('/'),
        key,
    );
    let sig = v1_signature(cfg.oss_access_key_secret.trim(), &sts)?;
    ureq::put(&oss_put_url(cfg, key))
        .header("Date", &date)
        .header("Content-Type", &ct)
        .header(
            "Authorization",
            format!("OSS {}:{}", cfg.oss_access_key_id.trim(), sig),
        )
        .header("User-Agent", "markd-image-host/1.0")
        .send(bytes)
        .map_err(|e| match e {
            ureq::Error::StatusCode(code) => {
                format!("OSS 上传失败（HTTP {code}）：{}", oss_hint(code))
            }
            other => format!("网络请求失败：{other}"),
        })?;
    Ok(oss_public_url(cfg, key))
}

// ---------------------------------------------------------------------------
// 七牛云 Kodo：表单上传（multipart/form-data）+ 上传凭证（uptoken）
// ---------------------------------------------------------------------------

/// 上传凭证的有效期（秒）。每次上传现签现用，过期后七牛返回 401。
const QINIU_TOKEN_TTL: i64 = 3600;

/// 区域 ID → 控制台上的区域名（上传域名由区域 ID 拼出来）。
const QINIU_REGIONS: [(&str, &str); 8] = [
    ("z0", "华东-浙江"),
    ("cn-east-2", "华东-浙江2"),
    ("z1", "华北-河北"),
    ("z2", "华南-广东"),
    ("na0", "北美-洛杉矶"),
    ("as0", "亚太-新加坡"),
    ("ap-southeast-2", "亚太-河内"),
    ("ap-southeast-3", "亚太-胡志明"),
];

/// 归一七牛区域写法：控制台与文档里写「华南-广东」，拼上传域名要的是 `z2`。
/// 常见写法都收一下：区域 ID、`up-z2.qiniup.com` 这类上传/管理域名、中文区域名。
pub fn normalize_qiniu_region(raw: &str) -> String {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return String::new();
    }
    if let Some(id) = qiniu_region_by_chinese(&s) {
        return id.to_string();
    }
    let s = s
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    if QINIU_REGIONS.iter().any(|(id, _)| *id == s) {
        return s.to_string();
    }
    // `up-z2.qiniup.com` / `rs-z2.qiniuapi.com` / `s3-z2.qiniucs.com` → 取首段里的区域 ID。
    for prefix in ["up-", "upload-", "s3-", "rs-", "rsf-", "iovip-"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let id = rest.split('.').next().unwrap_or(rest);
            if QINIU_REGIONS.iter().any(|(known, _)| *known == id) {
                return id.to_string();
            }
        }
    }
    // 认不出来就原样留着，让上传失败的错误里带上这个区域名，比静默改写诚实。
    s.to_string()
}

/// 中文区域名（含只写大区/省份的简写）→ 区域 ID。
fn qiniu_region_by_chinese(s: &str) -> Option<&'static str> {
    // 括号有全角/半角两种，先只留字母数字和汉字。
    let key: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || '\u{4e00}' <= *c && *c <= '\u{9fff}')
        .collect();
    const TABLE: [(&str, &str); 25] = [
        ("华东浙江", "z0"),
        ("浙江", "z0"),
        ("杭州", "z0"),
        ("华东", "z0"),
        ("华东浙江2", "cn-east-2"),
        ("华东2", "cn-east-2"),
        ("华北河北", "z1"),
        ("河北", "z1"),
        ("北京", "z1"),
        ("华北", "z1"),
        ("华南广东", "z2"),
        ("广东", "z2"),
        ("深圳", "z2"),
        ("华南", "z2"),
        ("北美洛杉矶", "na0"),
        ("北美", "na0"),
        ("洛杉矶", "na0"),
        ("亚太新加坡", "as0"),
        ("亚太东南", "as0"),
        ("东南亚", "as0"),
        ("新加坡", "as0"),
        ("亚太河内", "ap-southeast-2"),
        ("河内", "ap-southeast-2"),
        ("亚太胡志明", "ap-southeast-3"),
        ("胡志明", "ap-southeast-3"),
    ];
    TABLE
        .iter()
        .find(|(name, _)| key == *name)
        .map(|(_, id)| *id)
}

/// 源站上传域名，实测 `https://up-z2.qiniup.com/` 直接可传。
/// 区域在这里再归一一次：`ImageHostConfig` 可以绕过 [`ImageHostConfig::from_config`]
/// 直接构造，写成「华南-广东」也该拼得出域名。
fn qiniu_upload_host(region: &str) -> String {
    format!("up-{}.qiniup.com", normalize_qiniu_region(region))
}

/// 区域不在已知列表里时的中文提示（列表由 [`QINIU_REGIONS`] 生成，不会写串）。
fn qiniu_unknown_region_error(raw: &str) -> String {
    let list: Vec<String> = QINIU_REGIONS
        .iter()
        .map(|(id, name)| format!("{name}（{id}）"))
        .collect();
    format!("七牛区域「{raw}」认不出来，可填：{}", list.join("、"))
}

/// 七牛「URL 安全的 Base64」：`+`→`-`、`/`→`_`，**保留**末尾 `=` 填充
/// （官方文档示例与各家 SDK 都带填充，实测服务端接受）。
fn urlsafe_base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE.encode(bytes)
}

/// urlsafe_base64(HMAC-SHA1(SecretKey, 待签数据))。
fn qiniu_sign(secret: &str, data: &str) -> Result<String, String> {
    use hmac::{Hmac, Mac};
    use sha1::Sha1;

    let mut mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes())
        .map_err(|_| "七牛 SecretKey 长度不合法".to_string())?;
    mac.update(data.as_bytes());
    Ok(urlsafe_base64(&mac.finalize().into_bytes()))
}

/// 上传策略 JSON，七牛要求紧凑无空格：`{"scope":"桶:对象名","deadline":秒}`。
pub fn qiniu_put_policy(bucket: &str, key: &str, deadline: i64) -> String {
    format!(
        "{{\"scope\":\"{}:{}\",\"deadline\":{deadline}}}",
        json_escape(bucket.trim()),
        json_escape(key)
    )
}

/// 字符串值里的 JSON 转义（对象名允许用户自己写路径模板，引号得能活着进去）。
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// 上传凭证：`AccessKey:urlsafe_base64(签名):urlsafe_base64(上传策略)`。
pub fn qiniu_upload_token(
    access_key: &str,
    secret_key: &str,
    put_policy: &str,
) -> Result<String, String> {
    let encoded = urlsafe_base64(put_policy.as_bytes());
    let sign = qiniu_sign(secret_key, &encoded)?;
    Ok(format!("{}:{sign}:{encoded}", access_key.trim()))
}

/// 表单里的 `filename` 只给人看，桶内名字由 `key` 决定；去掉会破坏 multipart 头的字符。
fn qiniu_file_name(key: &str) -> String {
    key.rsplit('/')
        .next()
        .unwrap_or(key)
        .chars()
        .filter(|c| !matches!(c, '"' | '\\' | '\r' | '\n'))
        .collect()
}

/// multipart 分隔符：出现在文件内容里会把请求体截断，所以撞上就换一个。
fn qiniu_boundary(bytes: &[u8]) -> String {
    boundary_for(&random_suffix(), bytes)
}

/// 用给定种子生成分隔符；内容与它冲突时按 `-1`、`-2` 递增重试。
fn boundary_for(seed: &str, bytes: &[u8]) -> String {
    let mut b = format!("----markd{seed}qiniu");
    let mut n = 0u32;
    while !b.is_empty() && bytes.windows(b.len()).any(|w| w == b.as_bytes()) {
        n += 1;
        b = format!("----markd{seed}-{n}qiniu");
    }
    b
}

/// 表单上传的请求体：`key`、`token` 两个普通域加一个 `file` 文件域。
pub fn qiniu_form_body(
    boundary: &str,
    key: &str,
    token: &str,
    file_name: &str,
    content_type: &str,
    bytes: &[u8],
) -> Vec<u8> {
    let head = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"key\"\r\n\r\n{key}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"token\"\r\n\r\n{token}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: {content_type}\r\n\r\n"
    );
    let mut out = Vec::with_capacity(head.len() + bytes.len() + boundary.len() + 8);
    out.extend_from_slice(head.as_bytes());
    out.extend_from_slice(bytes);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

/// 七牛失败时的中文说明。错误正文 `{"error":"..."}` 里常常直接写着该怎么办——
/// 实测选错区域时它回「incorrect region, please use up-z2.qiniup.com」，
/// 把这句话原样带出来比只报状态码有用得多。
fn qiniu_error(status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.as_str())
                .map(|s| s.to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| brief(body));
    let advice = match status {
        401 => "请核对 AccessKey 与 SecretKey；上传凭证只有 1 小时有效，本机时钟偏差太大也会这样",
        400 => "多半是区域与空间不匹配，或空间名写错",
        573 => "空间不存在，请核对空间名称",
        579 => "空间访问受限（可能被冻结或欠费）",
        599 => "七牛服务端错误，稍后重试",
        _ => "请检查区域、空间名称与密钥",
    };
    let mut msg = format!("七牛上传失败（HTTP {status}）：{advice}");
    if !detail.is_empty() {
        msg.push_str("；七牛说：");
        msg.push_str(&detail);
        if detail.contains("incorrect region") {
            msg.push_str("——把偏好设置里的「区域」改成七牛点名的那个即可");
        }
    }
    msg
}

fn upload_qiniu(cfg: &ImageHostConfig, key: &str, bytes: &[u8]) -> Result<String, String> {
    let url = format!("https://{}/", qiniu_upload_host(&cfg.qiniu_region));
    let deadline = chrono::Utc::now().timestamp() + QINIU_TOKEN_TTL;
    let policy = qiniu_put_policy(&cfg.qiniu_bucket, key, deadline);
    let token = qiniu_upload_token(&cfg.qiniu_access_key, &cfg.qiniu_secret_key, &policy)?;
    let boundary = qiniu_boundary(bytes);
    let body = qiniu_form_body(
        &boundary,
        key,
        &token,
        &qiniu_file_name(key),
        &content_type_by_ext(key),
        bytes,
    );
    // 关掉「非 2xx 直接抛错」，这样才能读到七牛的错误正文。
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    let mut resp = agent
        .post(&url)
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header("User-Agent", "markd-image-host/1.0")
        .send(&body)
        .map_err(|e| format!("网络请求失败：{e}（上传地址 {url}）"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let text = resp
            .body_mut()
            .with_config()
            .limit(8 * 1024)
            .read_to_vec()
            .ok()
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default();
        return Err(qiniu_error(status, &text));
    }
    Ok(cfg.qiniu_url(key))
}

// ---------------------------------------------------------------------------
// 火山引擎 TOS：PUT Object + V4 签名（`TOS4-HMAC-SHA256`）
// ---------------------------------------------------------------------------

/// 实测在线的 TOS 地域 ID：`https://{桶}.tos-{地域}.volces.com/` 能解析并完成 TLS 握手
/// （不存在的regionTLS 直接断在握手，比如 `tos-cn-shenzhen`）。
const VOLC_REGIONS: [&str; 7] = [
    "cn-beijing",
    "cn-shanghai",
    "cn-guangzhou",
    "cn-hongkong",
    "ap-southeast-1",
    "ap-southeast-3",
    "me-central-1",
];

/// 控制台写法里的城市名 → 地域 ID。按城市名包含匹配，「华北2（北京）」「北京」都能认，
/// 不必跟控制台的地域前缀写法较劲。
fn volc_region_by_city(s: &str) -> Option<&'static str> {
    const TABLE: [(&str, &str); 5] = [
        ("北京", "cn-beijing"),
        ("上海", "cn-shanghai"),
        ("广州", "cn-guangzhou"),
        ("香港", "cn-hongkong"),
        ("新加坡", "ap-southeast-1"),
    ];
    TABLE
        .iter()
        .find(|(city, _)| s.contains(city))
        .map(|(_, id)| *id)
}

/// 归一 TOS 地域写法：城市名、`cn-beijing`、`tos-cn-beijing.volces.com`、
/// 桶的三级域名（`markd-markd.tos-cn-beijing.volces.com`）都收。
pub fn normalize_volc_region(raw: &str) -> String {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return String::new();
    }
    if VOLC_REGIONS.contains(&s.as_str()) {
        return s;
    }
    if let Some(id) = volc_region_by_city(&s) {
        return id.to_string();
    }
    let s = s
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    if VOLC_REGIONS.contains(&s) {
        return s.to_string();
    }
    // 从域名里抠 `tos-` 后面那一段：`tos-cn-beijing.volces.com` → `cn-beijing`。
    for token in s.split('.') {
        if let Some(rest) = token
            .strip_prefix("tos-s3-")
            .or_else(|| token.strip_prefix("tos-"))
        {
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    // 认不出来原样返回，交给 `check_volc_region` 决定是放行还是点名报错。
    s.to_string()
}

/// 地域校验。认得出的 ID 直接过；形如 `xx-yyyy` 的陌生 ID 也放行（火山新开的地域
/// Markd 不一定及时跟上）；只有明显不是地域 ID 的才报错，并把认得的列出来。
fn check_volc_region(region: &str) -> Result<(), String> {
    if region.is_empty() {
        return Err("请填写火山引擎 TOS 的地域，如 cn-beijing（华北2·北京）".to_string());
    }
    if VOLC_REGIONS.contains(&region) {
        return Ok(());
    }
    let plausible = region.len() >= 4
        && region.contains('-')
        && !region.starts_with('-')
        && !region.ends_with('-')
        && region
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if plausible {
        return Ok(());
    }
    Err(format!(
        "火山引擎 TOS 地域「{region}」认不出来，可填：{}",
        VOLC_REGIONS.join("、")
    ))
}

/// 桶的访问域名。**写入永远走这个端点**，CDN 前缀只改文档里的链接。
/// 实测原生端点只认 `TOS4-HMAC-SHA256`：给它 AWS SigV4 会回
/// `400 InvalidArgument: Unsupported Authorization Type`。
/// 地域在这里再归一一次：`ImageHostConfig` 可以绕过 `from_config` 直接构造。
fn volc_host(cfg: &ImageHostConfig) -> String {
    format!(
        "{}.tos-{}.volces.com",
        cfg.volc_bucket.trim().trim_matches('/'),
        normalize_volc_region(&cfg.volc_region)
    )
}

/// 写进文档的公网地址；设了自定义前缀（CDN/加速域名）就用它。
fn volc_public_url(cfg: &ImageHostConfig, key: &str) -> String {
    let base = cfg.public_base.trim().trim_end_matches('/');
    if !base.is_empty() {
        let base = if base.starts_with("http://") || base.starts_with("https://") {
            base.to_string()
        } else {
            format!("https://{base}")
        };
        return format!("{base}/{key}");
    }
    format!("https://{}/{key}", volc_host(cfg))
}

/// 小写十六进制的 SHA-256：既当请求体摘要，也当规范请求的哈希。
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for b in out {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 不参与百分号编码的字符：字母数字与 `-` `_` `.` `~`（`/` 由调用方决定）。
fn uri_unreserved(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~')
}

/// 与官方签名规则一致的百分号编码：保留未保留字符，`/` 仅在 `encode_slash` 时编码，
/// 其余按 UTF-8 字节写成大写 `%XX`。
fn uri_encode(s: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '/' {
            out.push_str(if encode_slash { "%2F" } else { "/" });
        } else if uri_unreserved(c) {
            out.push(c);
        } else {
            for b in c.to_string().as_bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// 头部值参与签名前要把连续空白压成一个空格（与官方规则一致，也和各家 SDK 一样）。
fn volc_header_value(v: &str) -> String {
    v.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 规范请求：`方法\n路径\n查询\n每个头 k:v 一行\n空行\n参与签名的头名\n体摘要`。
/// 头必须按名字排序，本平台的对象写入没有查询串，所以那一行恒为空。
fn volc_canonical_request(
    method: &str,
    path: &str,
    headers: &[(String, String)],
    payload_sha: &str,
) -> String {
    let names: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
    let mut out = String::new();
    out.push_str(method);
    out.push('\n');
    out.push_str(&uri_encode(path, false));
    out.push_str("\n\n");
    for (k, v) in headers {
        out.push_str(k);
        out.push(':');
        out.push_str(&volc_header_value(v));
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&names.join(";"));
    out.push('\n');
    out.push_str(payload_sha);
    out
}

/// 待签字符串：`算法\n时间\n日期/地域/tos/request\n规范请求的哈希`。
fn volc_string_to_sign(
    canonical: &str,
    date_iso: &str,
    date_compact: &str,
    region: &str,
) -> String {
    format!(
        "TOS4-HMAC-SHA256\n{date_iso}\n{date_compact}/{region}/tos/request\n{}",
        sha256_hex(canonical.as_bytes())
    )
}

fn hmac_sha256(key: &[u8], data: &str) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn hmac_sha256_hex(key: &[u8], data: &str) -> String {
    let mut out = String::with_capacity(64);
    for b in hmac_sha256(key, data) {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// hex(SHA1(数据))——腾讯云 COS 的格式串摘要用。
fn sha1_hex(data: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// hex(HMAC-SHA1(key, 数据))——腾讯云 COS 的两段式密钥与签名都是这个。
fn hmac_sha1_hex(key: &[u8], data: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha1::Sha1;
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// 派生签名密钥：`SK → 日期 → 地域 → "tos" → "request"`（四轮 HMAC-SHA256，无固定前缀）。
fn volc_signing_key(secret: &str, date_compact: &str, region: &str) -> Vec<u8> {
    let k_date = hmac_sha256(secret.as_bytes(), date_compact);
    let k_region = hmac_sha256(&k_date, region);
    let k_service = hmac_sha256(&k_region, "tos");
    hmac_sha256(&k_service, "request")
}

/// `Authorization` 头的值：
/// `TOS4-HMAC-SHA256 Credential={AK}/{日期}/{地域}/tos/request,SignedHeaders={头名},Signature={hex}`。
/// 三段之间是**逗号 + 空格**（官方 SDK 同款）。少那个空格服务端解析不出信任状，
/// 回的是 `InvalidAccessKeyId` 而不是签名错——实测踩过，别改回去。
fn volc_authorization(
    access_key: &str,
    secret: &str,
    region: &str,
    date_iso: &str,
    date_compact: &str,
    signed_headers: &str,
    canonical: &str,
) -> String {
    let sts = volc_string_to_sign(canonical, date_iso, date_compact, region);
    let key = volc_signing_key(secret, date_compact, region);
    // 官方 SDK 用「逗号 + 空格」分隔三段；省掉空格会让 TOS 解析不出信任状，
    // 反过来报 InvalidAccessKeyId（实测：火山说密钥不存在，其实是我这里少了一个空格）。
    format!(
        "TOS4-HMAC-SHA256 Credential={access_key}/{date_compact}/{region}/tos/request, SignedHeaders={signed_headers}, Signature={}",
        hmac_sha256_hex(&key, &sts)
    )
}

fn upload_volc(cfg: &ImageHostConfig, key: &str, bytes: &[u8]) -> Result<String, String> {
    let region = normalize_volc_region(&cfg.volc_region);
    check_volc_region(&region)?;
    let host = volc_host(cfg);
    let path = format!("/{}", uri_encode(key, false));
    let now = chrono::Utc::now();
    let date_iso = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_compact = now.format("%Y%m%d").to_string();
    let payload_sha = sha256_hex(bytes);
    let content_type = content_type_by_ext(key);
    let mut headers: Vec<(String, String)> = vec![
        ("content-type".to_string(), content_type.clone()),
        ("host".to_string(), host.clone()),
        ("x-tos-content-sha256".to_string(), payload_sha.clone()),
        ("x-tos-date".to_string(), date_iso.clone()),
    ];
    headers.sort();
    let canonical = volc_canonical_request("PUT", &path, &headers, &payload_sha);
    let signed_headers: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
    let auth = volc_authorization(
        cfg.volc_access_key_id.trim(),
        cfg.volc_secret_access_key.trim(),
        &region,
        &date_iso,
        &date_compact,
        &signed_headers.join(";"),
        &canonical,
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    let mut resp = agent
        .put(format!("https://{host}{path}"))
        .header("Content-Type", &content_type)
        .header("x-tos-date", &date_iso)
        .header("x-tos-content-sha256", &payload_sha)
        .header("Authorization", &auth)
        .header("User-Agent", "markd-image-host/1.0")
        .send(bytes)
        .map_err(|e| format!("网络请求失败：{e}（上传地址 https://{host}/）"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let text = resp
            .body_mut()
            .with_config()
            .limit(8 * 1024)
            .read_to_vec()
            .ok()
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default();
        return Err(volc_error(status, &text));
    }
    Ok(volc_public_url(cfg, key))
}

/// TOS 的错误正文是 JSON（实测：`{"Code":"InvalidAccessKeyId","Message":...,"EC":...}`），
/// 里面的 `Code` 比状态码精确得多，直接翻成中文给用户。
fn volc_error(status: u16, body: &str) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let code = parsed
        .as_ref()
        .and_then(|v| v.get("Code").and_then(|c| c.as_str()))
        .unwrap_or("")
        .to_string();
    let message = parsed
        .as_ref()
        .and_then(|v| v.get("Message").and_then(|m| m.as_str()))
        .unwrap_or("")
        .to_string();
    let advice = match code.as_str() {
        "InvalidAccessKeyId" => {
            "火山引擎查不到这个 AccessKey：确认填的是「访问控制 → API 访问密钥」里那把（不是 TOS 控制台显示的账号名），状态是「启用」，并且与桶属于同一个账号"
        }
        "SignatureDoesNotMatch" => {
            "Secret Access Key 不对，或本机时间与标准时间差太多（签名里带 UTC 时间戳）"
        }
        "AccessDenied" => "这把密钥没有该桶的写权限（TOS 的 PutObject），或桶策略拒绝了公网写入",
        "NoSuchBucket" => "存储桶不存在：核对桶名与地域是否配对",
        "InvalidArgument" => {
            "请求参数不对：常见是把 AWS S3 的签名发到原生端点（原生只认 TOS4-HMAC-SHA256）"
        }
        "RequestTimeTooSkewed" => "本机时间与标准时间差太多，签名失效",
        "" => "请核对地域、桶名与密钥",
        other => {
            return format!("火山引擎上传失败（HTTP {status}，{other}）：请核对地域、桶名与密钥");
        }
    };
    let mut msg = format!("火山引擎上传失败（HTTP {status}）：{advice}");
    // 有 Message 带 Message；正文不是 JSON（网关吐 HTML）时截一段原文带上，
    // 免得用户只看到一个没头没尾的状态码。
    let detail = if message.is_empty() {
        brief(body)
    } else {
        message.clone()
    };
    if !detail.is_empty() && detail != advice {
        msg.push_str("；火山说：");
        msg.push_str(&detail);
    }
    msg
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

// ---------------------------------------------------------------------------
// 百度智能云 BOS：PUT Object + bce-auth-v1 签名
// ---------------------------------------------------------------------------

/// 信任状里写的有效期（秒），与官方 SDK 默认值一致。
const BOS_EXPIRATION_SECONDS: u32 = 1800;

/// 官方文档列出的地域 ID 与控制台中文名。实测：`bj` 上这个桶公共读回 200，
/// `bd`/`su`/`gz`/`cd`/`hkg`/`fwh` 都有 HTTP 响应（端点在），`fsh` 是金融云专用、公网握手不通。
const BOS_REGIONS: [(&str, &str); 8] = [
    ("bj", "华北-北京"),
    ("bd", "华北-保定"),
    ("su", "华东-苏州"),
    ("gz", "华南-广州"),
    ("cd", "西南-成都"),
    ("hkg", "中国香港"),
    ("fwh", "华中-武汉"),
    ("fsh", "华东金融-上海"),
];

/// 只写城市名也认（控制台上的区域全称容易少打个前缀）。
fn bos_region_by_city(s: &str) -> Option<&'static str> {
    if s.contains("北京") {
        return Some("bj");
    }
    if s.contains("保定") {
        return Some("bd");
    }
    if s.contains("苏州") {
        return Some("su");
    }
    if s.contains("广州") {
        return Some("gz");
    }
    if s.contains("成都") {
        return Some("cd");
    }
    if s.contains("香港") {
        return Some("hkg");
    }
    if s.contains("武汉") {
        return Some("fwh");
    }
    if s.contains("上海") {
        return Some("fsh");
    }
    None
}

/// 把用户可能写的形式归一成 BOS 地域 ID：`bj`、「华北-北京」、「北京」、
/// `bj.bcebos.com`、`https://bj.bcebos.com`、桶的三级域名 `markd-markd.bj.bcebos.com`。
/// 认不出的原样返回，交给 `check_bos_region` 报错，不静默改写。
pub fn normalize_bos_region(raw: &str) -> String {
    let mut t = raw.trim().to_ascii_lowercase();
    for pre in ["https://", "http://"] {
        if let Some(rest) = t.strip_prefix(pre) {
            t = rest.to_string();
        }
    }
    t = t.trim_matches('/').to_string();
    if t.is_empty() {
        return String::new();
    }
    if let Some(head) = t.strip_suffix(".bcebos.com") {
        // 三级域名取最后一段（bj），endpoint 本身取到的也是 bj
        if let Some(seg) = head.rsplit('.').next() {
            if !seg.is_empty() {
                return seg.to_string();
            }
        }
    }
    for (id, zh) in BOS_REGIONS {
        if t == *id || t == *zh {
            return id.to_string();
        }
    }
    bos_region_by_city(&t).unwrap_or(t.as_str()).to_string()
}

/// 校验归一后的地域：已知 ID 直接过；没在文档里但形如 2-4 个小写字母的放行
/// （百度会陆续开新区域，写对了不该拦）；其余报错并列出可用地域。
fn check_bos_region(region: &str) -> Result<(), String> {
    if region.is_empty() {
        return Err("请填写百度智能云 BOS 的地域，如 华北-北京（bj）".to_string());
    }
    if BOS_REGIONS.iter().any(|(id, _)| *id == region) {
        return Ok(());
    }
    let short_id =
        (2..=4).contains(&region.chars().count()) && region.chars().all(|c| c.is_ascii_lowercase());
    if short_id {
        return Ok(());
    }
    let list = BOS_REGIONS
        .iter()
        .map(|(id, zh)| format!("{zh}（{id}）"))
        .collect::<Vec<_>>()
        .join("、");
    Err(format!(
        "认不出百度智能云的地域「{region}」。可用地域：{list}；也可以直接粘 endpoint，如 bj.bcebos.com"
    ))
}

fn bos_host(cfg: &ImageHostConfig) -> String {
    format!(
        "{}.{}.bcebos.com",
        cfg.bos_bucket.trim().trim_matches('/'),
        normalize_bos_region(&cfg.bos_region)
    )
}

/// 规范请求里的 URI 与真正发出去的 URL 必须同一个编码：`/` 保留，其余按 `%XX`。
fn bos_canonical_uri(key: &str) -> String {
    format!("/{}", uri_encode(key, false))
}

/// 信任状与 `x-bce-date` 用的 UTC 时间：`%Y-%m-%dT%H:%M:%SZ`（官方 SDK 同格式）。
fn bos_canonical_time(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 写进文档的公网地址；设了自定义前缀（CDN/加速域名）就用它。
fn bos_public_url(cfg: &ImageHostConfig, key: &str) -> String {
    let base = cfg.public_base.trim().trim_end_matches('/');
    if !base.is_empty() {
        let base = if base.starts_with("http://") || base.starts_with("https://") {
            base.to_string()
        } else {
            format!("https://{base}")
        };
        return format!("{base}/{key}");
    }
    format!("https://{}/{key}", bos_host(cfg))
}

/// BOS 的规范化头：头名转小写后与头值**都要**做百分号编码，空值的头不参与，
/// 整行 `名字:值` 按字节排序后用 `\n` 连接。
/// 实测：`image/png` 进签名是 `image%2Fpng`，时间里的 `:` 是 `%3A`——值不编码就签不上。
fn bos_canonical_headers(headers: &[(String, String)]) -> String {
    let mut lines: Vec<String> = headers
        .iter()
        .filter(|(_, v)| !v.trim().is_empty())
        .map(|(k, v)| {
            format!(
                "{}:{}",
                uri_encode(&k.trim().to_ascii_lowercase(), true),
                uri_encode(v.trim(), true)
            )
        })
        .collect();
    lines.sort();
    lines.join("\n")
}

/// 信任状信息：`bce-auth-v1/{AK}/{UTC 时间}/{有效期}`，时间是 `%Y-%m-%dT%H:%M:%SZ`。
fn bos_sign_key_info(access_key: &str, canonical_time: &str, expiration: u32) -> String {
    format!("bce-auth-v1/{access_key}/{canonical_time}/{expiration}")
}

/// 两段式密钥的第一步：`hex(HMAC-SHA256(SK, 信任状信息))`。
/// 反直觉但官方规则如此：这个十六进制**字符串**接着当第二步的密钥用。
fn bos_sign_key(secret: &str, sign_key_info: &str) -> String {
    hmac_sha256_hex(secret.as_bytes(), sign_key_info)
}

/// 规范请求：`方法\n规范化 URI\n规范化查询串\n规范化头`。
/// 对象写入没有查询串，所以第三行是空串；头那一段本身不再补空行（与 AWS 系不同）。
fn bos_string_to_sign(method: &str, canonical_uri: &str, canonical_headers: &str) -> String {
    format!("{method}\n{canonical_uri}\n\n{canonical_headers}")
}

/// `Authorization`：`{信任状}//{签名}`。中间「参与签名的头名」一段留空，
/// 表示服务端按默认集合算（host、content-md5、content-length、content-type 加所有 x-bce-*），
/// 与官方 SDK 在没显式指定 headers_to_sign 时发出去的形态一字不差（实测 200）。
fn bos_authorization(sign_key_info: &str, sign_key: &str, string_to_sign: &str) -> String {
    format!(
        "{sign_key_info}//{}",
        hmac_sha256_hex(sign_key.as_bytes(), string_to_sign)
    )
}

/// 参与签名的头集合，同时也是实际发出去的头：`host`、`content-type`、`content-length`
/// 加 `x-bce-date`。这是百度默认签名头集合（host/content-md5/content-length/content-type
/// 与所有 x-bce-*）在本请求下的实际形态；少签一个或多签一个都签不上。
/// `x-bce-date` 不能省：实测不带它服务端直接回 `400 MissingDateHeader`。
fn bos_signed_headers(
    host: &str,
    content_type: &str,
    length: usize,
    canonical_time: &str,
) -> Vec<(String, String)> {
    vec![
        ("content-length".to_string(), length.to_string()),
        ("content-type".to_string(), content_type.to_string()),
        ("host".to_string(), host.to_string()),
        ("x-bce-date".to_string(), canonical_time.to_string()),
    ]
}

fn upload_bos(cfg: &ImageHostConfig, key: &str, bytes: &[u8]) -> Result<String, String> {
    let region = normalize_bos_region(&cfg.bos_region);
    check_bos_region(&region)?;
    let host = bos_host(cfg);
    let canonical_uri = bos_canonical_uri(key);
    let content_type = content_type_by_ext(key);
    let canonical_time = bos_canonical_time(chrono::Utc::now());
    let headers = bos_signed_headers(&host, &content_type, bytes.len(), &canonical_time);
    let info = bos_sign_key_info(
        cfg.bos_access_key_id.trim(),
        &canonical_time,
        BOS_EXPIRATION_SECONDS,
    );
    let sign_key = bos_sign_key(cfg.bos_secret_access_key.trim(), &info);
    let sts = bos_string_to_sign("PUT", &canonical_uri, &bos_canonical_headers(&headers));
    let auth = bos_authorization(&info, &sign_key, &sts);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    let mut resp = agent
        .put(format!("https://{host}{canonical_uri}"))
        .header("Content-Type", &content_type)
        .header("x-bce-date", &canonical_time)
        .header("Authorization", &auth)
        .header("User-Agent", "markd-image-host/1.0")
        .send(bytes)
        .map_err(|e| format!("网络请求失败：{e}（上传地址 https://{host}/）"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let text = resp
            .body_mut()
            .with_config()
            .limit(8 * 1024)
            .read_to_vec()
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        return Err(bos_error(status, &text));
    }
    Ok(bos_public_url(cfg, key))
}

/// 百度的错误正文是 JSON，键名小写：`{"code":"...","message":"...","requestId":"..."}`。
/// 按 code 翻成中文，并原样带出百度说的话。
fn bos_error(status: u16, body: &str) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let code = parsed
        .as_ref()
        .and_then(|v| v.get("code").and_then(|c| c.as_str()))
        .unwrap_or("")
        .to_string();
    let message = parsed
        .as_ref()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()))
        .unwrap_or("")
        .to_string();
    let advice = match code.as_str() {
        "AccessDenied" => {
            "桶是私有读或这把密钥没有写权限：把桶设成公共读，或在访问控制里授予对象存储写入权限"
        }
        "InvalidAccessKeyId" => {
            "百度查不到这个 AccessKey：确认取自「安全认证 → 访问控制 → AccessKey 管理」，且与桶属于同一个账号"
        }
        "SignatureDoesNotMatch" => {
            "Secret Access Key 不对，或本机时间与标准时间差太多（信任状里带 UTC 时间，默认 1800 秒有效）"
        }
        "NoSuchBucket" => "存储桶不存在：核对桶名与地域是否配对",
        "BucketAlreadyExists" => "桶名已被占用",
        "MissingDateHeader" => "请求没带 x-bce-date：这是 Markd 自己的缺陷，请反馈或升级版本",
        "RequestTimeTooSkewed" => "本机时间与标准时间差太多，信任状失效",
        "InvalidRequestRegion" | "PermanentRedirect" => {
            "地域与桶不配对：把「地域」改成桶实际所在的地域"
        }
        "" => "请核对地域、桶名与密钥",
        other => {
            return format!("百度智能云上传失败（HTTP {status}，{other}）：请核对地域、桶名与密钥");
        }
    };
    let mut msg = format!("百度智能云上传失败（HTTP {status}）：{advice}");
    let detail = if message.is_empty() {
        brief(body)
    } else {
        message.clone()
    };
    if !detail.is_empty() && detail != advice {
        msg.push_str("；百度说：");
        msg.push_str(&detail);
    }
    msg
}

// ---------------------------------------------------------------------------
// 华为云 OBS：PUT Object + V2 签名
// ---------------------------------------------------------------------------

/// 实测能解析并应答的区域编码（`obs.<编码>.myhuaweicloud.com` 都有 DNS，匿名请求回 403）。
/// 右列只填官方文档里查得到中文全称的七个：华为在不同产品文档里对同一编码写的中文名
/// 并不一致（例如新加坡一处 `ap-southeast-1`、另一处 `ap-southeast-3`），
/// 猜着映射会把用户引到错误的桶，所以其余区域只认编码本身。
const OBS_REGIONS: [(&str, &str); 16] = [
    ("cn-north-1", "华北-北京一"),
    ("cn-north-2", "华北-北京二"),
    ("cn-north-4", "华北-北京四"),
    ("cn-north-9", "华北-乌兰察布一"),
    ("cn-east-2", "华东-上海二"),
    ("cn-east-3", "华东-上海一"),
    ("cn-east-4", ""),
    ("cn-south-1", "华南-广州"),
    ("cn-south-2", ""),
    ("cn-southwest-2", "西南-贵阳一"),
    ("ap-southeast-1", ""),
    ("ap-southeast-2", ""),
    ("ap-southeast-3", ""),
    ("ap-southeast-4", ""),
    ("af-south-1", ""),
    ("na-mexico-1", ""),
];

/// 只映射「城市 + 序号」这种唯一写法。裸写「上海」「北京」不映射——
/// 两个城市各有多个区域编码，猜哪个都可能把桶拼到别的区去。
fn obs_region_by_city(s: &str) -> Option<&'static str> {
    match s {
        _ if s.contains("北京一") => Some("cn-north-1"),
        _ if s.contains("北京二") => Some("cn-north-2"),
        _ if s.contains("北京四") => Some("cn-north-4"),
        _ if s.contains("乌兰察布") => Some("cn-north-9"),
        _ if s.contains("上海一") => Some("cn-east-3"),
        _ if s.contains("上海二") => Some("cn-east-2"),
        _ if s.contains("广州") => Some("cn-south-1"),
        _ if s.contains("贵阳") => Some("cn-southwest-2"),
        _ => None,
    }
}

/// 归一区域写法：`cn-east-3`、「华东-上海一」、`obs.cn-east-3.myhuaweicloud.com`、
/// `https://obs.cn-east-3.myhuaweicloud.com`、桶的三级域名
/// `markd-markd.obs.cn-east-3.myhuaweicloud.com`。认不出的原样返回，交给校验报错。
pub fn normalize_obs_region(raw: &str) -> String {
    let mut t = raw.trim().to_ascii_lowercase();
    for pre in ["https://", "http://"] {
        if let Some(rest) = t.strip_prefix(pre) {
            t = rest.to_string();
        }
    }
    t = t.trim_matches('/').to_string();
    if t.is_empty() {
        return String::new();
    }
    if let Some(head) = t.strip_suffix(".myhuaweicloud.com") {
        // 最后一段就是区域编码：obs.cn-east-3 与 桶名.obs.cn-east-3 都成立
        if let Some(seg) = head.rsplit('.').next() {
            if !seg.is_empty() {
                return seg.to_string();
            }
        }
    }
    for (id, zh) in OBS_REGIONS {
        if t == *id {
            return id.to_string();
        }
        if !zh.is_empty() && (t == *zh || t.ends_with(zh)) {
            return id.to_string();
        }
    }
    obs_region_by_city(&t).unwrap_or(t.as_str()).to_string()
}

/// 已知编码放行；没在表里但形如「字母-字母-数字」的也放行（华为还在开新区）；
/// 其余报错并列出这份表。
fn check_obs_region(region: &str) -> Result<(), String> {
    if region.is_empty() {
        return Err("请填写华为云 OBS 的区域，如 华东-上海一（cn-east-3）".to_string());
    }
    if OBS_REGIONS.iter().any(|(id, _)| *id == region) {
        return Ok(());
    }
    let parts: Vec<&str> = region.split('-').collect();
    let shaped = parts.len() == 3
        && parts[0].len() == 2
        && parts[0].chars().all(|c| c.is_ascii_lowercase())
        && parts[1].chars().all(|c| c.is_ascii_lowercase())
        && !parts[1].is_empty()
        && parts[2].chars().all(|c| c.is_ascii_digit())
        && !parts[2].is_empty();
    if shaped {
        return Ok(());
    }
    let list = OBS_REGIONS
        .iter()
        .filter(|(_, zh)| !zh.is_empty())
        .map(|(id, zh)| format!("{zh}（{id}）"))
        .collect::<Vec<_>>()
        .join("、");
    Err(format!(
        "认不出华为云的区域「{region}」。有中文全称的区域是：{list}；\
         其它区域请写区域编码（如 ap-southeast-1）或直接粘 obs.<区域>.myhuaweicloud.com"
    ))
}

fn obs_host(cfg: &ImageHostConfig) -> String {
    format!(
        "{}.obs.{}.myhuaweicloud.com",
        cfg.obs_bucket.trim().trim_matches('/'),
        normalize_obs_region(&cfg.obs_region)
    )
}

/// 写进文档的公网地址；设了自定义前缀（CDN/漫游域名）就用它。
fn obs_public_url(cfg: &ImageHostConfig, key: &str) -> String {
    let base = cfg.public_base.trim().trim_end_matches('/');
    if !base.is_empty() {
        let base = if base.starts_with("http://") || base.starts_with("https://") {
            base.to_string()
        } else {
            format!("https://{base}")
        };
        return format!("{base}/{key}");
    }
    format!("https://{}/{key}", obs_host(cfg))
}

/// 对象名的百分号编码，请求行与待签字符串共用这一处。
/// 实测对象名里有空格、`#`、中文时，必须按编码后的形式签才签得上
/// （官方 SDK 也是这么做的：保留 `/~` 与字母数字和 `.-_`）。
fn obs_encoded_key(key: &str) -> String {
    uri_encode(key, false)
}

/// V2 的信任状头：`OBS {访问密钥 ID}:{base64(HMAC-SHA1(秘密访问密钥, 待签字符串))}`。
fn obs_authorization(access_key: &str, signature: &str) -> String {
    format!("OBS {access_key}:{signature}")
}

/// OBS V2 的待签字符串：`PUT\n<空 Content-MD5>\n<Content-Type>\n<Date>\n/<桶>/<编码后的对象名>`。
/// 与阿里云 OSS V1 同一条五段式（`v1_string_to_sign`），差别只在对象名要先编码。
fn obs_string_to_sign(content_type: &str, date: &str, bucket: &str, key: &str) -> String {
    v1_string_to_sign(
        "PUT",
        "",
        content_type,
        date,
        bucket.trim().trim_matches('/'),
        &obs_encoded_key(key),
    )
}

fn upload_obs(cfg: &ImageHostConfig, key: &str, bytes: &[u8]) -> Result<String, String> {
    let region = normalize_obs_region(&cfg.obs_region);
    check_obs_region(&region)?;
    let host = obs_host(cfg);
    let encoded_key = obs_encoded_key(key);
    let content_type = content_type_by_ext(key);
    let date = v1_now_date();
    let sts = obs_string_to_sign(&content_type, &date, cfg.obs_bucket.trim(), key);
    let sig = v1_signature(cfg.obs_secret_access_key.trim(), &sts)?;
    let auth = obs_authorization(cfg.obs_access_key_id.trim(), &sig);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    let mut resp = agent
        .put(format!("https://{host}/{encoded_key}"))
        .header("Content-Type", &content_type)
        .header("Date", &date)
        .header("Authorization", &auth)
        .header("User-Agent", "markd-image-host/1.0")
        .send(bytes)
        .map_err(|e| format!("网络请求失败：{e}（上传地址 https://{host}/）"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let text = resp
            .body_mut()
            .with_config()
            .limit(8 * 1024)
            .read_to_vec()
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        return Err(obs_error(status, &text));
    }
    Ok(obs_public_url(cfg, key))
}

/// 从 XML 错误正文里取一个标签的内容（华为 OBS 与腾讯云 COS 的正文都形如
/// `<Error><Code>SignatureDoesNotMatch</Code><Message>…</Message><RequestId>…</RequestId></Error>`）。
/// XML 实体还原：腾讯云的错误正文里实测出现过
/// `Bucket format should be &lt;bucketname&gt;-&lt;appid&gt;`，照原样显示给用户就是一串转义符。
/// `&amp;` 放最后换，否则 `&amp;lt;` 会被二次解成 `<`。
fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn xml_tag(body: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let rest = body.split(&open).nth(1)?;
    rest.split(&close)
        .next()
        .map(|v| xml_unescape(v.trim()))
}

fn obs_error(status: u16, body: &str) -> String {
    let code = xml_tag(body, "Code").unwrap_or_default();
    let message = xml_tag(body, "Message").unwrap_or_default();
    let advice = match code.as_str() {
        "SignatureDoesNotMatch" => {
            "秘密访问密钥不对，或本机时间与标准时间差太多（V2 签名里的 Date 头允许偏差 15 分钟）"
        }
        "InvalidAccessKeyId" => {
            "华为云查不到这个访问密钥 ID：在「我的凭证 → 访问密钥」里核对，且要与桶属于同一个账号"
        }
        "AccessDenied" => {
            "没有写入权限：给这把密钥所属用户挂 OBS 的写权限（PutObject），或桶策略拒绝了公网写入"
        }
        "NoSuchBucket" => "桶不存在：核对桶名与区域是否配对（实测桶名对了、区域错了也回这一条）",
        "SlowDown" | "TooManyRequests" => "请求太密，稍后重试",
        "" => "请核对区域、桶名与密钥",
        other => {
            return format!("华为云上传失败（HTTP {status}，{other}）：请核对区域、桶名与密钥");
        }
    };
    let mut msg = format!("华为云上传失败（HTTP {status}）：{advice}");
    let detail = if message.is_empty() {
        brief(body)
    } else {
        message
    };
    if !detail.is_empty() && detail != advice {
        msg.push_str("；华为说：");
        msg.push_str(&detail);
    }
    msg
}

// ── 腾讯云 COS ────────────────────────────────────────────────────────────────

/// 腾讯云 COS 地域表：`{地域 ID, 控制台中文名}`。中文名照官方「地域和访问域名」页，
/// 用户在控制台看到的是「公有云园区-中国-南京」这种带前缀的写法，归一时按后缀匹配。
pub const COS_REGIONS: [(&str, &str); 22] = [
    ("ap-beijing-1", "北京一区（已售罄）"),
    ("ap-beijing", "北京"),
    ("ap-nanjing", "南京"),
    ("ap-shanghai", "上海"),
    ("ap-guangzhou", "广州"),
    ("ap-chengdu", "成都"),
    ("ap-chongqing", "重庆"),
    ("ap-shenzhen-fsi", "深圳金融"),
    ("ap-shanghai-fsi", "上海金融"),
    ("ap-beijing-fsi", "北京金融"),
    ("ap-hongkong", "中国香港"),
    ("ap-singapore", "新加坡"),
    ("ap-jakarta", "雅加达"),
    ("ap-seoul", "首尔"),
    ("ap-bangkok", "曼谷"),
    ("ap-tokyo", "东京"),
    ("ap-osaka", "大阪"),
    ("me-saudi-arabia", "利雅得"),
    ("na-siliconvalley", "硅谷（美西）"),
    ("na-ashburn", "弗吉尼亚（美东）"),
    ("sa-saopaulo", "圣保罗"),
    ("eu-frankfurt", "法兰克福"),
];

/// 只映射城市名本身，且先长后短：「北京一区」要在「北京」之前命中，「上海金融」在「上海」之前。
fn cos_region_by_city(s: &str) -> Option<&'static str> {
    match s {
        _ if s.contains("北京一区") || s.contains("北京一") => Some("ap-beijing-1"),
        _ if s.contains("上海金融") => Some("ap-shanghai-fsi"),
        _ if s.contains("深圳金融") => Some("ap-shenzhen-fsi"),
        _ if s.contains("北京金融") => Some("ap-beijing-fsi"),
        _ if s.contains("硅谷") => Some("na-siliconvalley"),
        _ if s.contains("弗吉尼亚") => Some("na-ashburn"),
        _ if s.contains("圣保罗") => Some("sa-saopaulo"),
        _ if s.contains("法兰克福") => Some("eu-frankfurt"),
        _ if s.contains("利雅得") => Some("me-saudi-arabia"),
        _ if s.contains("大阪") => Some("ap-osaka"),
        _ if s.contains("东京") => Some("ap-tokyo"),
        _ if s.contains("首尔") => Some("ap-seoul"),
        _ if s.contains("曼谷") => Some("ap-bangkok"),
        _ if s.contains("雅加达") => Some("ap-jakarta"),
        _ if s.contains("新加坡") => Some("ap-singapore"),
        _ if s.contains("香港") => Some("ap-hongkong"),
        _ if s.contains("南京") => Some("ap-nanjing"),
        _ if s.contains("广州") => Some("ap-guangzhou"),
        _ if s.contains("成都") => Some("ap-chengdu"),
        _ if s.contains("重庆") => Some("ap-chongqing"),
        _ if s.contains("上海") => Some("ap-shanghai"),
        _ if s.contains("北京") => Some("ap-beijing"),
        _ => None,
    }
}

/// 归一地域写法：`ap-nanjing`、「公有云园区-中国-南京」、「南京」、
/// `cos.ap-nanjing.myqcloud.com`、桶域名 `markd-1253518225.cos.ap-nanjing.myqcloud.com`。
/// 认不出的原样返回，交给校验报错。
pub fn normalize_cos_region(raw: &str) -> String {
    let mut t = raw.trim().to_ascii_lowercase();
    for pre in ["https://", "http://"] {
        if let Some(rest) = t.strip_prefix(pre) {
            t = rest.to_string();
        }
    }
    t = t.trim_matches('/').to_string();
    if t.is_empty() {
        return String::new();
    }
    if let Some(head) = t.strip_suffix(".myqcloud.com") {
        // 域名倒数第二段就是地域：cos.ap-nanjing 与 桶名.cos.ap-nanjing 都成立
        if let Some(seg) = head.rsplit('.').next()
            && !seg.is_empty()
        {
            return seg.to_string();
        }
    }
    for (id, zh) in COS_REGIONS {
        if t == *id {
            return id.to_string();
        }
        if !zh.is_empty() && (t == *zh || t.ends_with(zh)) {
            return id.to_string();
        }
    }
    cos_region_by_city(&t).unwrap_or(t.as_str()).to_string()
}

/// 表里的地域放行；没在表里但形如「ap-xxx」这种腾讯云样子的也放行（腾讯云还在开新区）；
/// 其余报错并列出中文对照表。
fn check_cos_region(region: &str) -> Result<(), String> {
    if region.is_empty() {
        return Err("请填写腾讯云 COS 的地域，如 南京（ap-nanjing）".to_string());
    }
    if COS_REGIONS.iter().any(|(id, _)| *id == region) {
        return Ok(());
    }
    if region == "accelerate" {
        return Err(
            "腾讯云 COS 的全球加速域名（cos.accelerate.myqcloud.com）Markd 不走，请填桶所在的地域，如 南京（ap-nanjing）"
                .to_string(),
        );
    }
    let shaped = ["ap-", "na-", "sa-", "eu-", "me-"]
        .iter()
        .any(|p| region.starts_with(p))
        && region.len() > 3
        && region
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if shaped {
        return Ok(());
    }
    let list = COS_REGIONS
        .iter()
        .map(|(id, zh)| format!("{zh}（{id}）"))
        .collect::<Vec<_>>()
        .join("、");
    Err(format!(
        "认不出腾讯云 COS 的地域「{region}」。可选地域是：{list}；也可以直接粘 cos.<地域>.myqcloud.com 或桶域名"
    ))
}

fn cos_host(cfg: &ImageHostConfig) -> String {
    format!(
        "{}.cos.{}.myqcloud.com",
        cfg.cos_bucket.trim().trim_matches('/'),
        normalize_cos_region(&cfg.cos_region)
    )
}

/// 写进文档的公网地址；设了自定义前缀（CDN 域名）就用它。
fn cos_public_url(cfg: &ImageHostConfig, key: &str) -> String {
    let base = cfg.public_base.trim().trim_end_matches('/');
    if !base.is_empty() {
        let base = if base.starts_with("http://") || base.starts_with("https://") {
            base.to_string()
        } else {
            format!("https://{base}")
        };
        return format!("{base}/{key}");
    }
    format!("https://{}/{key}", cos_host(cfg))
}

/// `q-sign-time`：`{现在-60};{现在+10000}`，与官方 SDK 一致（前头留 60 秒抵消时钟差）。
fn cos_sign_time(now: i64) -> String {
    format!("{};{}", now - 60, now + 10000)
}

/// 参与签名的头：`content-type` 与 `host`。头值要百分号编码（实测 `image/png` 签进去是
/// `image%2Fpng`），按名字排序；同时把 `q-header-list` 一起返回，两者必须同源——
/// 实测只声明不签、或签了不声明都回 `SignatureDoesNotMatch`。
fn cos_signed_headers(host: &str, content_type: &str) -> (String, String) {
    let mut pairs = [
        ("content-type", uri_encode(content_type, true)),
        ("host", uri_encode(host, true)),
    ];
    pairs.sort();
    let canonical = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let list = pairs.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(";");
    (canonical, list)
}

/// 格式串：`put\n/{对象名}\n\n{签名头}\n`。第三行是 URL 参数，对象写入恒为空。
/// **对象名按原样进签名**（实测空格、`#`、中文、`%`、`+` 都要原样），与华为 OBS 相反：
/// OBS 要先把对象名百分号编码，这里编码了反而 `SignatureDoesNotMatch`。
fn cos_format_str(key: &str, canonical_headers: &str) -> String {
    format!("put\n/{key}\n\n{canonical_headers}\n")
}

/// 待签字符串：`sha1\n{q-sign-time}\n{格式串的 SHA1}\n`。
fn cos_string_to_sign(sign_time: &str, format_str: &str) -> String {
    format!("sha1\n{sign_time}\n{}\n", sha1_hex(format_str.as_bytes()))
}

/// 两段式密钥（和百度 BOS 一个套路）：`SignKey = hex(HMAC-SHA1(SecretKey, q-sign-time))`，
/// 再把这段十六进制字符串本身当密钥去签待签字符串。
fn cos_signature(secret_key: &str, sign_time: &str, string_to_sign: &str) -> String {
    let sign_key = hmac_sha1_hex(secret_key.as_bytes(), sign_time);
    hmac_sha1_hex(sign_key.as_bytes(), string_to_sign)
}

/// `Authorization` 头：六个 `q-*` 字段用 `&` 连，`q-key-time` 与 `q-sign-time` 同值。
fn cos_authorization(secret_id: &str, sign_time: &str, header_list: &str, signature: &str) -> String {
    format!(
        "q-sign-algorithm=sha1&q-ak={secret_id}&q-sign-time={sign_time}&q-key-time={sign_time}&q-header-list={header_list}&q-url-param-list=&q-signature={signature}"
    )
}

fn upload_cos(cfg: &ImageHostConfig, key: &str, bytes: &[u8]) -> Result<String, String> {
    let region = normalize_cos_region(&cfg.cos_region);
    check_cos_region(&region)?;
    let host = cos_host(cfg);
    let content_type = content_type_by_ext(key);
    let (canonical, header_list) = cos_signed_headers(&host, &content_type);
    let sign_time = cos_sign_time(chrono::Utc::now().timestamp());
    let sts = cos_string_to_sign(&sign_time, &cos_format_str(key, &canonical));
    let sig = cos_signature(cfg.cos_secret_key.trim(), &sign_time, &sts);
    let auth = cos_authorization(cfg.cos_secret_id.trim(), &sign_time, &header_list, &sig);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    // 请求行里的对象名要百分号编码（`/` 保留），签名的格式串用原样——两处不一样，别写反。
    let url = format!("https://{host}/{}", uri_encode(key, false));
    let mut resp = agent
        .put(&url)
        .header("Content-Type", &content_type)
        .header("Authorization", &auth)
        .header("User-Agent", "markd-image-host/1.0")
        .send(bytes)
        .map_err(|e| format!("网络请求失败：{e}（上传地址 https://{host}/）"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let text = resp
            .body_mut()
            .with_config()
            .limit(8 * 1024)
            .read_to_vec()
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        return Err(cos_error(status, &text));
    }
    Ok(cos_public_url(cfg, key))
}

/// 把腾讯云的错误正文（XML，与华为 OBS 同构）翻成中文，并带上腾讯云原话。
/// 下面每条对应关系都是实测拿回来的。
fn cos_error(status: u16, body: &str) -> String {
    let code = xml_tag(body, "Code").unwrap_or_default();
    let message = xml_tag(body, "Message").unwrap_or_default();
    let advice = match code.as_str() {
        "SignatureDoesNotMatch" => {
            "SecretKey 不对，或声明的签名头与实际签进格式串的不一致（对象名要原样、头值要百分号编码）"
        }
        "InvalidAccessKeyId" => {
            "腾讯云查不到这个 SecretId：在「访问管理 → API 密钥管理」里核对，且要与桶属于同一个账号"
        }
        "AccessDenied" if message.contains("expired") => {
            "签名已过期：q-sign-time 只有 10000 秒，本机时钟不准就会一直这样"
        }
        "AccessDenied" => "没有写入权限：给这把密钥挂 COS 的写权限（PutObject），或桶策略拒绝了公网写入",
        "NoSuchBucket" => "桶不存在：桶名要写成「名称-APPID」，且地域要与桶配对（实测桶名对、地域错了也回这一条）",
        "InvalidRequest" if message.contains("Appid") => {
            "桶名格式不对：腾讯云的桶名必须是「名称-APPID」，例如 markd-1253518225"
        }
        "SlowDown" | "TooManyRequests" => "请求太密，稍后重试",
        "" => "请核对地域、桶名与密钥",
        other => {
            return format!("腾讯云上传失败（HTTP {status}，{other}）：请核对地域、桶名与密钥");
        }
    };
    let mut msg = format!("腾讯云上传失败（HTTP {status}）：{advice}");
    let detail = if message.is_empty() {
        brief(body)
    } else {
        message
    };
    if !detail.is_empty() && detail != advice {
        msg.push_str("；腾讯云说：");
        msg.push_str(&detail);
    }
    msg
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
            oss_region: "oss-cn-shanghai".into(),
            oss_bucket: "my-bucket".into(),
            oss_access_key_id: "LTAItestid".into(),
            oss_access_key_secret: "testsecret".into(),
            qiniu_region: String::new(),
            qiniu_bucket: String::new(),
            qiniu_access_key: String::new(),
            qiniu_secret_key: String::new(),
            qiniu_domain: String::new(),
            volc_region: String::new(),
            volc_bucket: String::new(),
            volc_access_key_id: String::new(),
            volc_secret_access_key: String::new(),
            bos_region: String::new(),
            bos_bucket: String::new(),
            bos_access_key_id: String::new(),
            bos_secret_access_key: String::new(),
            obs_region: String::new(),
            obs_bucket: String::new(),
            obs_access_key_id: String::new(),
            obs_secret_access_key: String::new(),
            cos_region: String::new(),
            cos_bucket: String::new(),
            cos_secret_id: String::new(),
            cos_secret_key: String::new(),
        }
    }

    fn oss_cfg() -> ImageHostConfig {
        let mut c = cfg();
        c.kind = ImageHostKind::Oss;
        c
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
            random: "a1b2c3".into(),
        }
    }

    /// 模板里没用到文件名时的空 NameParts。
    fn no_name() -> NameParts<'static> {
        NameParts {
            stem: "",
            suffix: "",
        }
    }

    fn github_cfg() -> ImageHostConfig {
        let mut c = cfg();
        c.kind = ImageHostKind::Github;
        c.owner = "whboy10000".into();
        c.repo = "oss".into();
        c
    }

    /// 默认模板按年月日分文件夹。
    #[test]
    fn path_pattern_expands_date_parts() {
        let t = fixed_time();
        assert_eq!(
            render_path_pattern("{year}/{month}/{day}", no_name(), &t),
            "2026/11/17"
        );
        assert_eq!(
            render_path_pattern("posts/{year}-{month}/{day}", no_name(), &t),
            "posts/2026-11/17"
        );
        assert_eq!(
            render_path_pattern("{year}{month}{day}/{hour}{minute}", no_name(), &t),
            "20261117/0905"
        );
        // 留空 → 直接放在目录下
        assert_eq!(render_path_pattern("", no_name(), &t), "");
    }

    /// 路径清理：丢掉空段/`.`/`..`，非法字符替换成 `-`。
    #[test]
    fn path_pattern_is_sanitized() {
        let t = fixed_time();
        assert_eq!(
            render_path_pattern("../{year}//a b", no_name(), &t),
            "2026/a-b"
        );
        assert_eq!(render_path_pattern("/./", no_name(), &t), "");
    }

    /// `{random}` `{filename}` `{.suffix}` 展开成用户写的一整套模板。
    #[test]
    fn path_pattern_expands_random_and_file_name() {
        let t = fixed_time();
        let name = NameParts {
            stem: "screenshot",
            suffix: "png",
        };
        assert_eq!(
            render_path_pattern(
                "pics/markd/{year}_{month}_{day}_{hour}_{random}_{filename}{.suffix}",
                name,
                &t
            ),
            "pics/markd/2026_11_17_09_a1b2c3_screenshot.png"
        );
        // 不带点的 {suffix} 只给扩展名本身。
        assert_eq!(
            render_path_pattern("{filename}-{suffix}", name, &t),
            "screenshot-png"
        );
    }

    /// 完整对象路径 = 目录 / 模板 / 文件名。
    #[test]
    fn object_path_joins_dir_pattern_and_name() {
        let t = fixed_time();
        let c = cfg();
        assert_eq!(object_path(&c, "a.png", &t), "images/2026/11/17/a.png");
        let mut c = cfg();
        c.dir = "/pics/".into();
        c.path_pattern = String::new();
        assert_eq!(object_path(&c, "a.png", &t), "pics/a.png");
    }

    /// 模板自己写了 `{filename}` 时不再追加文件名，也不再加毫秒时间戳前缀。
    #[test]
    fn object_path_uses_the_name_from_the_pattern() {
        let t = fixed_time();
        let mut c = cfg();
        c.dir = String::new();
        c.path_pattern = "markd/{year}_{random}_{filename}{.suffix}".into();
        assert_eq!(
            object_path(&c, "Screenshot-1.png", &t),
            "markd/2026_a1b2c3_Screenshot-1.png"
        );
        // 同一套模板下，上传用的远端文件名取清理过的原名，没有 13 位时间戳。
        assert_eq!(remote_name_for(&c, "Screenshot 1.png"), "Screenshot-1.png");
        // 默认模板（没有 {filename}）仍走「毫秒时间戳-原名」。
        let plain = cfg();
        let n = remote_name_for(&plain, "Screenshot 1.png");
        assert!(n.starts_with(|c: char| c.is_ascii_digit()), "{n}");
        assert!(n.ends_with("-Screenshot-1.png"), "{n}");
        // 模板里只有 {random}、没写 {filename}：文件名仍要追加在模板之后。
        let mut c = cfg();
        c.dir = String::new();
        c.path_pattern = "markd/{year}_{random}".into();
        assert_eq!(object_path(&c, "a.png", &t), "markd/2026_a1b2c3/a.png");
    }

    /// 用户给的 GitHub 保存路径整条展开一遍。
    #[test]
    fn object_path_follows_the_github_template() {
        let t = fixed_time();
        let mut c = github_cfg();
        c.dir = String::new();
        c.path_pattern =
            "pics/markd/{year}_{month}_{day}_{hour}_{random}_{filename}{.suffix}".into();
        let name = remote_name_for(&c, "截图 2026.png");
        assert_eq!(name, "2026.png");
        assert_eq!(
            object_path(&c, &name, &t),
            "pics/markd/2026_11_17_09_a1b2c3_2026.png"
        );
    }

    /// 随机后缀：宽度钉死 6 位十六进制，高位掩掉，连发不重样。
    #[test]
    fn random_suffix_is_six_hex_and_distinct() {
        assert_eq!(hex_suffix(0xcd1234), "cd1234");
        assert_eq!(hex_suffix(0), "000000");
        assert_eq!(hex_suffix(1), "000001");
        assert_eq!(hex_suffix(u64::MAX), "ffffff", "超过 24 位的高位要掩掉");
        // 连发 200 次去重：只看三次会漏掉「序号与时钟同时进一格」这一类碰撞。
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let s = random_suffix();
            assert_eq!(s.len(), 6, "{s}");
            assert!(s.bytes().all(|b| b.is_ascii_hexdigit()), "{s}");
            assert!(seen.insert(s.clone()), "连发出现重样后缀：{s}");
        }
        let t1 = now_parts().random;
        let t2 = now_parts().random;
        assert_eq!(t1.len(), 6, "{t1}");
        assert_ne!(t1, t2, "{t1} == {t2}");
    }

    /// 时钟进一格（2^17ns）的同时序号 +1：这两次变化不得互相抵消。
    /// CI 上 `random_suffix_is_six_hex_and_distinct` 的偶发失败就是这一组。
    #[test]
    fn suffix_mix_does_not_cancel_the_counter_with_the_clock() {
        // 低 24 位取 0x000002 与 0x000003，正是相邻两格时钟。
        let base = 2u64 << 17;
        let a = hex_suffix(suffix_mix(base, 0));
        let b = hex_suffix(suffix_mix(base + (1 << 17), 1));
        assert_ne!(a, b, "时钟 +1 格、序号 +1 不得撞名：{a} == {b}");
        // 时钟纹丝不动时，序号本身也要能撑开 4096 个不同后缀。
        let mut seen = std::collections::HashSet::new();
        for seq in 0..4096u64 {
            assert!(
                seen.insert(hex_suffix(suffix_mix(base, seq))),
                "序号 {seq} 与前一次重样"
            );
        }
    }

    /// 覆盖写必须带旧 SHA，新建不能带（实测缺 SHA 时 GitHub 回 422）。
    #[test]
    fn github_body_only_carries_sha_when_overwriting() {
        let c = github_cfg();
        let new = github_body(&c, "pics/a.png", b"ab", None);
        assert_eq!(new["branch"].as_str(), Some("master"));
        assert!(new["message"].as_str().unwrap().contains("a.png"));
        assert!(new.get("sha").is_none(), "新建不该带 sha");
        assert_eq!(new["content"].as_str(), Some("YWI="), "内容要 base64");
        let over = github_body(&c, "pics/a.png", b"ab", Some("deadbeef"));
        assert_eq!(over["sha"].as_str(), Some("deadbeef"));
        assert_eq!(over["content"].as_str(), new["content"].as_str());
    }

    /// 响应里的 SHA：直接 GET 放在顶层（此时 `content` 是 base64 字符串），
    /// 写入响应放在 `content` 对象下。
    #[test]
    fn github_sha_reads_both_shapes() {
        assert_eq!(
            github_sha_of(r#"{"content":{"sha":"aaa"},"commit":{"sha":"bbb"}}"#).as_deref(),
            Some("aaa")
        );
        assert_eq!(
            github_sha_of(r#"{"sha":"ccc"}"#).as_deref(),
            Some("ccc"),
            "顶层 SHA 也要认"
        );
        // 回归：GET 单文件的响应里 content 是 base64 字符串，把它当对象找 sha 会得到 None，
        // 于是覆盖写永远重试不上（实测 422 "sha" wasn't supplied 直接报给用户）。
        assert_eq!(
            github_sha_of(
                r#"{"path":"a.png","type":"file","encoding":"base64","content":"YWI=","sha":"ddd"}"#
            )
            .as_deref(),
            Some("ddd")
        );
        assert_eq!(github_sha_of("不是 json"), None);
        assert_eq!(
            github_sha_of(r#"{"message":"Bad credentials"}"#),
            None,
            "没有 sha 字段不能瞎猜"
        );
        assert_eq!(
            github_sha_of(r#"{"content":"YWI=","sha":null}"#),
            None,
            "sha 不是字符串时不能当成有值"
        );
    }

    /// 有 `download_url` 就用它，没有再按仓库规则拼。
    #[test]
    fn github_url_prefers_download_url() {
        let c = github_cfg();
        assert_eq!(
            github_url(
                &c,
                "pics/a.png",
                r#"{"content":{"download_url":"https://raw.example/p/a.png"}}"#
            ),
            "https://raw.example/p/a.png"
        );
        assert_eq!(
            github_url(&c, "pics/a.png", "响应不是 json"),
            "https://raw.githubusercontent.com/whboy10000/oss/master/pics/a.png"
        );
    }

    #[test]
    fn github_endpoints_use_the_contents_api() {
        let c = github_cfg();
        assert_eq!(
            github_endpoint(&c, "pics/markd/a.png"),
            "https://api.github.com/repos/whboy10000/oss/contents/pics/markd/a.png"
        );
        assert_eq!(
            github_public_url(&c, "pics/markd/a.png"),
            "https://raw.githubusercontent.com/whboy10000/oss/master/pics/markd/a.png"
        );
        let mut c = github_cfg();
        c.public_base = "https://cdn.example.com/pics/".into();
        assert_eq!(
            github_public_url(&c, "pics/markd/a.png"),
            "https://cdn.example.com/pics/pics/markd/a.png"
        );
    }

    /// 配置文件里的 `"github"` 要能读回来并写回原样。
    #[test]
    fn github_kind_round_trips_through_serde() {
        assert_eq!(
            serde_json::from_str::<ImageHostKind>("\"github\"").unwrap(),
            ImageHostKind::Github
        );
        assert_eq!(
            serde_json::to_string(&ImageHostKind::Github).unwrap(),
            "\"github\""
        );
        assert!(ImageHostKind::ALL.contains(&ImageHostKind::Github));
        assert_eq!(ImageHostKind::Github.label(), "GitHub 仓库");
    }

    /// GitHub 缺令牌时要说清楚需要什么权限。
    #[test]
    fn github_validation_mentions_contents_permission() {
        let mut c = github_cfg();
        assert!(c.validate().is_ok());
        c.token.clear();
        let e = c.validate().unwrap_err();
        assert!(e.contains("GitHub") && e.contains("Contents"), "{e}");
        c.token = "token".into();
        c.owner.clear();
        assert!(c.validate().unwrap_err().contains("GitHub"));
    }

    /// OSS V1 签名：待签字符串的排布 + 已知答案（另一套实现独立算出）。
    #[test]
    fn v1_signature_matches_known_answers() {
        let sts = v1_string_to_sign(
            "PUT",
            "",
            "image/png",
            "Sat, 21 Nov 2026 06:13:09 GMT",
            "my-bucket",
            "markd/pics/shot.png",
        );
        let want = [
            "PUT",
            "",
            "image/png",
            "Sat, 21 Nov 2026 06:13:09 GMT",
            "/my-bucket/markd/pics/shot.png",
        ]
        .join("\n");
        assert_eq!(sts, want);
        assert_eq!(
            v1_signature("testsecret", &sts).unwrap(),
            "PrzEDqP89Mkj5cujCzh0aFFK8m4="
        );
        assert_eq!(
            v1_signature("nvSeEdI1rOantHktXzBcxdWHG13rLB", &sts).unwrap(),
            "wdRTdAy/ACtQLxoMdtRxYcy7Vds="
        );
        // 换 secret / 换内容排布都必须换签名（防「签了个常量的假绿」）。
        assert_ne!(
            v1_signature("other-secret", &sts).unwrap(),
            v1_signature("testsecret", &sts).unwrap()
        );
        let sts2 = v1_string_to_sign(
            "PUT",
            "hCga7mHbGv0F3A4B5C6D7E==",
            "image/jpeg",
            "Tue, 09 Dec 2025 10:20:30 GMT",
            "b",
            "k.jpg",
        );
        assert_eq!(
            v1_signature("testsecret", &sts2).unwrap(),
            "G1lI8zBO3nP34yPu8yTMl8VzFiw="
        );
    }

    /// `Date` 头必须是英文 RFC1123 GMT，且按 UTC 计。
    #[test]
    fn oss_date_is_rfc1123_gmt() {
        let t = chrono::DateTime::parse_from_rfc3339("2026-11-21T06:13:09Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(v1_date(t), "Sat, 21 Nov 2026 06:13:09 GMT");
        let t2 = chrono::DateTime::parse_from_rfc3339("2025-12-09T10:20:30+08:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(v1_date(t2), "Tue, 09 Dec 2025 02:20:30 GMT", "要转成 UTC");
        assert_eq!(v1_now_date().len(), 29, "定长 29：{}", v1_now_date());
    }

    /// 写入打桶的 endpoint，链接才走自定义域名。
    #[test]
    fn oss_urls_separate_write_endpoint_and_public_base() {
        let c = oss_cfg();
        assert_eq!(oss_host(&c), "my-bucket.oss-cn-shanghai.aliyuncs.com");
        assert_eq!(
            oss_put_url(&c, "markd/pics/shot.png"),
            "https://my-bucket.oss-cn-shanghai.aliyuncs.com/markd/pics/shot.png"
        );
        assert_eq!(
            oss_public_url(&c, "markd/pics/shot.png"),
            "https://my-bucket.oss-cn-shanghai.aliyuncs.com/markd/pics/shot.png"
        );
        // 设了 CDN 前缀：链接换域名，写入地址不动；没写协议时补 https。
        let mut cdn = oss_cfg();
        cdn.public_base = "img.example.com/".into();
        assert_eq!(
            oss_public_url(&cdn, "markd/pics/shot.png"),
            "https://img.example.com/markd/pics/shot.png"
        );
        assert_eq!(
            oss_put_url(&cdn, "markd/pics/shot.png"),
            "https://my-bucket.oss-cn-shanghai.aliyuncs.com/markd/pics/shot.png"
        );
        cdn.public_base = "https://img.example.com".into();
        assert_eq!(
            oss_public_url(&cdn, "a.png"),
            "https://img.example.com/a.png"
        );
    }

    /// Content-Type 参与签名，按扩展名给；未知扩展名退回 octet-stream。
    #[test]
    fn content_type_by_ext_follows_extension() {
        assert_eq!(content_type_by_ext("a/b.PNG"), "image/png");
        assert_eq!(content_type_by_ext("a/b.jpg"), "image/jpeg");
        assert_eq!(content_type_by_ext("a/b.jpeg"), "image/jpeg");
        assert_eq!(content_type_by_ext("a/b.gif"), "image/gif");
        assert_eq!(content_type_by_ext("a/b.webp"), "image/webp");
        assert_eq!(content_type_by_ext("a/b.bmp"), "application/octet-stream");
        assert_eq!(content_type_by_ext("noext"), "application/octet-stream");
    }

    /// 控制台写「华东2（上海）」，配置文件里要的是 `oss-cn-shanghai`。
    #[test]
    fn oss_region_accepts_the_usual_spellings() {
        for input in [
            "oss-cn-shanghai",
            "  OSS-CN-Shanghai ",
            "cn-shanghai",
            "oss-cn-shanghai.aliyuncs.com",
            "https://oss-cn-shanghai.aliyuncs.com",
            "markd-markd.oss-cn-shanghai.aliyuncs.com",
            "华东2（上海）",
            "华东2(上海)",
        ] {
            assert_eq!(
                normalize_oss_region(input),
                "oss-cn-shanghai",
                "输入 {input}"
            );
        }
        assert_eq!(normalize_oss_region("华东1（杭州）"), "oss-cn-hangzhou");
        assert_eq!(normalize_oss_region("新加坡"), "oss-ap-southeast-1");
        assert_eq!(normalize_oss_region("华北2（北京）"), "oss-cn-beijing");
        // 空串交给 validate 报错；认不出的原样返回，不静默改写。
        assert_eq!(normalize_oss_region("   "), "");
        assert_eq!(normalize_oss_region("随便写的"), "随便写的");
    }

    /// OSS 必填项：桶、地域、两把 Key，缺哪个说哪个。
    #[test]
    fn oss_validation_lists_what_is_missing() {
        let mut c = oss_cfg();
        assert!(c.validate().is_ok());
        c.oss_bucket.clear();
        assert!(c.validate().unwrap_err().contains("存储空间"), "缺桶名");
        c.oss_bucket = "b".into();
        c.oss_region.clear();
        assert!(c.validate().unwrap_err().contains("地域"), "缺地域");
        c.oss_region = "oss-cn-shanghai".into();
        c.oss_access_key_secret.clear();
        let e = c.validate().unwrap_err();
        assert!(e.contains("AccessKey") && e.contains("Secret"), "{e}");
        // 仓库平台的必填项不该被 OSS 字段影响。
        let mut g = cfg();
        g.oss_bucket.clear();
        g.oss_access_key_id.clear();
        assert!(g.validate().is_ok(), "Gitee 不看 OSS 字段");
        g.kind = ImageHostKind::Oss;
        assert!(g.validate().is_err(), "切到 OSS 就要看 OSS 字段");
    }

    /// `"oss"` 这个配置值要能读写，且平台列表里有它。
    #[test]
    fn oss_kind_round_trips_through_serde() {
        assert_eq!(
            serde_json::from_str::<ImageHostKind>("\"oss\"").unwrap(),
            ImageHostKind::Oss
        );
        assert_eq!(
            serde_json::to_string(&ImageHostKind::Oss).unwrap(),
            "\"oss\""
        );
        assert!(ImageHostKind::ALL.contains(&ImageHostKind::Oss));
        assert_eq!(ImageHostKind::Oss.label(), "阿里云 OSS");
    }

    /// 用户给的 OSS 保存路径整条展开（含中文原名与大小写扩展名）。
    #[test]
    fn object_path_follows_the_oss_template() {
        let t = fixed_time();
        let mut c = oss_cfg();
        c.dir = String::new();
        c.path_pattern = "markd/{year}/{month}/{day}/{hour}_{random}_{filename}{.suffix}".into();
        let name = remote_name_for(&c, "截图 Shot.PNG");
        assert_eq!(name, "Shot.png");
        let path = object_path(&c, &name, &t);
        // 期望值用 join 拼出来：写成字面量时这一串日期形态的文本会被显示层改写。
        let want = ["markd", "2026", "11", "17", "09_a1b2c3_Shot.png"].join("/");
        assert_eq!(path, want);
        assert_eq!(
            oss_public_url(&c, &path),
            format!("https://my-bucket.oss-cn-shanghai.aliyuncs.com/{want}")
        );
    }

    /// 七牛配置：区域 z2、空间 markd-markd、访问域名与一对密钥。
    fn qiniu_cfg() -> ImageHostConfig {
        let mut c = cfg();
        c.kind = ImageHostKind::Qiniu;
        c.token = String::new();
        c.qiniu_region = "z2".into();
        c.qiniu_bucket = "markd-markd".into();
        c.qiniu_access_key = "MY_ACCESS_KEY".into();
        c.qiniu_secret_key = "MY_SECRET_KEY".into();
        c.qiniu_domain = "img.example.com".into();
        c
    }

    /// 官方文档「上传凭证」页的完整示例：待签字符串是 encodedPutPolicy 本身，
    /// 签名与凭证格式都由它决定。这里拿它当外部期望值，不看实现脸色。
    #[test]
    fn qiniu_token_matches_the_official_example() {
        let doc_policy = "{\"scope\":\"my-bucket:sunflower.jpg\",\"deadline\":1451491200,\"returnBody\":\"{\\\"name\\\":$(fname),\\\"size\\\":$(fsize),\\\"w\\\":$(imageInfo.width),\\\"h\\\":$(imageInfo.height),\\\"hash\\\":$(etag)}\"}";
        let doc_encoded_policy = "eyJzY29wZSI6Im15LWJ1Y2tldDpzdW5mbG93ZXIuanBnIiwiZGVhZGxpbmUiOjE0NTE0OTEyMDAsInJldHVybkJvZHkiOiJ7XCJuYW1lXCI6JChmbmFtZSksXCJzaXplXCI6JChmc2l6ZSksXCJ3XCI6JChpbWFnZUluZm8ud2lkdGgpLFwiaFwiOiQoaW1hZ2VJbmZvLmhlaWdodCksXCJoYXNoXCI6JChldGFnKX0ifQ==";
        let doc_sign = "wQ4ofysef1R7IKnrziqtomqyDvI=";
        // 待签字符串就是 URL 安全 Base64（带 = 填充）后的上传策略。
        assert_eq!(urlsafe_base64(doc_policy.as_bytes()), doc_encoded_policy);
        // 字母表必须是 `-`/`_` 那一套：`+`/`/` 会被服务端当成别的东西。
        assert_eq!(
            urlsafe_base64(&[0x00, 0xff, 0xf0, 0x0f, 0xff, 0xff]),
            "AP_wD___"
        );
        assert_eq!(
            qiniu_sign("MY_SECRET_KEY", doc_encoded_policy).unwrap(),
            doc_sign
        );
        assert_eq!(
            qiniu_upload_token("MY_ACCESS_KEY", "MY_SECRET_KEY", doc_policy).unwrap(),
            format!("MY_ACCESS_KEY:{doc_sign}:{doc_encoded_policy}")
        );
        // 换任意一个字节都要改签名（scope 里的对象名不是摆设）。
        let other = qiniu_put_policy("my-bucket", "other.png", 1451491200);
        assert_ne!(
            qiniu_upload_token("MY_ACCESS_KEY", "MY_SECRET_KEY", &other).unwrap(),
            qiniu_upload_token("MY_ACCESS_KEY", "MY_SECRET_KEY", doc_policy).unwrap()
        );
    }

    /// 上传策略是紧凑 JSON（七牛不接受多余空格换行），scope 为 `桶:对象名`。
    #[test]
    fn qiniu_put_policy_is_compact_json() {
        assert_eq!(
            qiniu_put_policy("markd-markd", "markd/2026/11/a.png", 1795249153),
            "{\"scope\":\"markd-markd:markd/2026/11/a.png\",\"deadline\":1795249153}"
        );
        // 桶名/对象名里出现引号或反斜杠要转义，否则 JSON 直接被掰坏。
        assert_eq!(
            qiniu_put_policy("a\"b", "c\\d.png", 1),
            "{\"scope\":\"a\\\"b:c\\\\d.png\",\"deadline\":1}"
        );
        assert!(serde_json::from_str::<serde_json::Value>(&qiniu_put_policy("b", "k", 9)).is_ok());
    }

    /// 区域写法归一：控制台中文、大区简写、上传/管理域名都能认。
    #[test]
    fn normalize_qiniu_region_accepts_console_names_and_hosts() {
        for raw in [
            "z2",
            " Z2 ",
            "华南-广东",
            "华南（广东）",
            "华南",
            "广东",
            "up-z2.qiniup.com",
            "https://up-z2.qiniup.com/",
            "rs-z2.qiniuapi.com",
            "s3-z2.qiniucs.com",
        ] {
            assert_eq!(normalize_qiniu_region(raw), "z2", "{raw} 应归一成 z2");
        }
        assert_eq!(normalize_qiniu_region("华东-浙江"), "z0");
        assert_eq!(normalize_qiniu_region("华东-浙江2"), "cn-east-2");
        assert_eq!(normalize_qiniu_region("华北-河北"), "z1");
        assert_eq!(normalize_qiniu_region("北美-洛杉矶"), "na0");
        assert_eq!(normalize_qiniu_region("亚太-新加坡"), "as0");
        assert_eq!(normalize_qiniu_region("河内"), "ap-southeast-2");
        assert_eq!(normalize_qiniu_region(""), "");
        // 认不出来的原样留着：错误里能看见用户到底填了什么。
        assert_eq!(normalize_qiniu_region("火星-奥林帕斯"), "火星-奥林帕斯");
    }

    #[test]
    fn qiniu_upload_host_is_built_from_region() {
        assert_eq!(qiniu_upload_host("z2"), "up-z2.qiniup.com");
        assert_eq!(qiniu_upload_host(" as0 "), "up-as0.qiniup.com");
        // 直接构造 ImageHostConfig（不经 from_config）时填中文区域也要拼得出域名，
        // 实跑抓到过拼成 `up-华南-广东.qiniup.com` 的坑。
        assert_eq!(qiniu_upload_host("华南-广东"), "up-z2.qiniup.com");
        assert_eq!(qiniu_upload_host("华南-广东"), qiniu_upload_host("z2"));
    }

    /// 写进文档的地址：七牛不能从桶名推域名，所以按填的域名拼，
    /// 并按实测结论决定默认 scheme（测试域名没有 HTTPS 证书）。
    #[test]
    fn qiniu_url_follows_the_domain_kind() {
        let c = qiniu_cfg();
        assert_eq!(
            c.qiniu_url("markd/a.png"),
            "https://img.example.com/markd/a.png"
        );
        // 实测 `*.hn-bkt.clouddn.com` 测试域名 https 回 421、http 回 200。
        let mut test_domain = qiniu_cfg();
        test_domain.qiniu_domain = "touadcd4p.hn-bkt.clouddn.com".into();
        assert_eq!(
            test_domain.qiniu_url("markd/a.png"),
            "http://touadcd4p.hn-bkt.clouddn.com/markd/a.png"
        );
        // 自己写了 scheme 就照用；末尾多余的 / 吃掉。
        let mut explicit = qiniu_cfg();
        explicit.qiniu_domain = "http://img.example.com/".into();
        assert_eq!(explicit.qiniu_url("a.png"), "http://img.example.com/a.png");
        // 没填七牛域名时退回通用的自定义访问前缀。
        let mut fallback = qiniu_cfg();
        fallback.qiniu_domain = String::new();
        fallback.public_base = "https://cdn.example.com/x/".into();
        assert_eq!(
            fallback.qiniu_url("a.png"),
            "https://cdn.example.com/x/a.png"
        );
        // 两个都没有：validate() 会拦住，这里只确认不 panic。
        let mut none = qiniu_cfg();
        none.qiniu_domain = String::new();
        none.public_base = String::new();
        assert_eq!(none.qiniu_url("a.png"), "");
        assert!(none.validate().is_err());
    }

    /// 表单体：key、token、file 三段，二进制内容原样夹在中间。
    #[test]
    fn qiniu_form_body_is_a_valid_multipart() {
        let bytes: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x00, 0xFF, b'\r', b'\n', 0x1A];
        let body = qiniu_form_body(
            "----markdBndqiniu",
            "markd/a.png",
            "AK:SIG:POLICY",
            "a.png",
            "image/png",
            &bytes,
        );
        let text = String::from_utf8_lossy(&body).to_string();
        assert!(text.starts_with("------markdBndqiniu\r\n"), "{text}");
        assert!(
            text.contains("Content-Disposition: form-data; name=\"key\"\r\n\r\nmarkd/a.png\r\n")
        );
        assert!(text
            .contains("Content-Disposition: form-data; name=\"token\"\r\n\r\nAK:SIG:POLICY\r\n"));
        assert!(text.contains("name=\"file\"; filename=\"a.png\""));
        assert!(text.contains("Content-Type: image/png\r\n\r\n"));
        // 收尾是 `--` + 分隔符 + `--`，即六条横线开头、两条结尾。
        assert!(text.ends_with("\r\n------markdBndqiniu--\r\n"), "{text}");
        // 文件字节必须一个不少、一个不改地在文件头之后。
        let at = body
            .windows(bytes.len())
            .position(|w| w == bytes)
            .expect("文件内容没进表单体");
        assert_eq!(&body[at..at + bytes.len()], bytes.as_slice());
    }

    /// `filename` 只是给人看的，仍要把能破坏头部的字符去掉。
    #[test]
    fn qiniu_file_name_drops_header_breaking_chars() {
        assert_eq!(qiniu_file_name("markd/2026/a b.png"), "a b.png");
        // 引号与反斜杠会破坏 multipart 头部，必须去掉。
        assert_eq!(qiniu_file_name("dir/we\"ird\\a.png"), "weirda.png");
        // 控制字符同理（名字里不该有换行）。
        assert_eq!(qiniu_file_name("dir/x\ny\rz.png"), "xyz.png");
    }

    /// 分隔符撞进文件内容会把请求体截断——撞上就换一个。
    #[test]
    fn qiniu_boundary_avoids_colliding_content() {
        let primary = "----markda1b2c3qiniu";
        let evil = format!("prefix{primary}suffix").into_bytes();
        let got = boundary_for("a1b2c3", &evil);
        assert_eq!(got, "----markda1b2c3-1qiniu");
        assert!(!evil.windows(got.len()).any(|w| w == got.as_bytes()));
        // 普通图片内容不会触发重试，前缀固定便于人读。
        let clean = boundary_for("a1b2c3", b"\x89PNG\r\n\x1a\n");
        assert_eq!(clean, primary);
        // 短于分隔符的内容不能 panic。
        assert_eq!(boundary_for("x", b"ab"), "----markdxqiniu");
    }

    /// 七牛的错误正文里常常直接写着解法（实测选错区域时它点名正确域名）。
    #[test]
    fn qiniu_error_quotes_what_the_server_said() {
        let wrong_region =
            r#"{"error":"incorrect region, please use up-z2.qiniup.com, bucket is: markd-markd"}"#;
        let msg = qiniu_error(400, wrong_region);
        assert!(msg.contains("up-z2.qiniup.com"), "{msg}");
        assert!(msg.contains("改成"), "{msg}");
        let bad_token = qiniu_error(401, r#"{"error":"bad token"}"#);
        assert!(bad_token.contains("HTTP 401"), "{bad_token}");
        assert!(bad_token.contains("SecretKey"), "{bad_token}");
        assert!(bad_token.contains("bad token"), "{bad_token}");
        // 没有正文时不能冒出空的「七牛说：」。
        let empty = qiniu_error(599, "");
        assert!(!empty.contains("七牛说"), "{empty}");
        assert!(empty.contains("HTTP 599"), "{empty}");
        // 非 JSON 正文（反向代理的 HTML 页）只带一小段，不刷屏。
        let html = qiniu_error(502, &format!("<html>{}</html>", "x".repeat(500)));
        assert!(html.contains("<html>"), "{html}");
        assert!(
            html.chars().count() < 400,
            "错误正文没截断：{}",
            html.chars().count()
        );
    }

    /// 七牛的必填项逐个点名，别让用户猜是哪一格空了。
    #[test]
    fn qiniu_validate_names_the_missing_field() {
        let mut c = qiniu_cfg();
        assert!(c.validate().is_ok(), "{:?}", c.validate());
        c.qiniu_bucket = " ".into();
        assert!(
            c.validate().unwrap_err().contains("空间名称"),
            "{:?}",
            c.validate()
        );
        c = qiniu_cfg();
        c.qiniu_region = String::new();
        assert!(
            c.validate().unwrap_err().contains("区域"),
            "{:?}",
            c.validate()
        );
        c = qiniu_cfg();
        c.qiniu_secret_key = String::new();
        assert!(
            c.validate().unwrap_err().contains("SecretKey"),
            "{:?}",
            c.validate()
        );
        c = qiniu_cfg();
        c.qiniu_domain = String::new();
        assert!(
            c.validate().unwrap_err().contains("访问域名"),
            "{:?}",
            c.validate()
        );
        // 区域写错要当场点出来，并且告诉用户能填什么（别等网络请求拼出个非法域名）。
        c = qiniu_cfg();
        c.qiniu_region = "火星-奥林帕斯".into();
        let err = c.validate().unwrap_err();
        assert!(err.contains("认不出来"), "{err}");
        assert!(
            err.contains("华南-广东（z2）"),
            "错误里要列出可填的区域：{err}"
        );
        // 控制台写法直接填进配置也算合法（归一后是 z2）。
        c = qiniu_cfg();
        c.qiniu_region = "华南-广东".into();
        assert!(c.validate().is_ok(), "{:?}", c.validate());
        // 仓库类平台仍要令牌，七牛不要（签名用的是 AK/SK，不是 token）。
        c = qiniu_cfg();
        assert!(c.token.is_empty() && c.validate().is_ok());
    }

    #[test]
    fn qiniu_kind_is_a_platform_of_its_own() {
        assert_eq!(
            serde_json::from_str::<ImageHostKind>("\"qiniu\"").unwrap(),
            ImageHostKind::Qiniu
        );
        assert_eq!(
            serde_json::to_string(&ImageHostKind::Qiniu).unwrap(),
            "\"qiniu\""
        );
        assert!(ImageHostKind::ALL.contains(&ImageHostKind::Qiniu));
        assert_eq!(ImageHostKind::Qiniu.label(), "七牛云 Kodo");
        assert_eq!(ImageHostKind::Qiniu.platform_name(), "七牛云");
        assert!(ImageHostKind::Qiniu.is_enabled());
    }

    /// 用户给的七牛保存路径整条展开，并拼成 http 的测试域名地址。
    #[test]
    fn qiniu_path_and_url_follow_the_template() {
        let t = fixed_time();
        let mut c = qiniu_cfg();
        c.dir = String::new();
        c.path_pattern = "markd/{year}/{month}/{day}/{hour}_{random}_{filename}{.suffix}".into();
        c.qiniu_domain = "touadcd4p.hn-bkt.clouddn.com".into();
        let name = remote_name_for(&c, "截图 Shot.PNG");
        assert_eq!(name, "Shot.png");
        let path = object_path(&c, &name, &t);
        // 期望值用 join 拼出来：写成字面量时这一串日期形态的文本会被显示层改写。
        let want = ["markd", "2026", "11", "17", "09_a1b2c3_Shot.png"].join("/");
        assert_eq!(path, want);
        assert_eq!(
            c.qiniu_url(&path),
            format!("http://touadcd4p.hn-bkt.clouddn.com/{want}")
        );
    }

    /// 火山 TOS 测试用的合成凭据——不是真密钥，只是让签名结果可复算。
    const VOLC_AK: &str = "AKLTtest0000000000000";
    const VOLC_SK: &str = "test-secret-key-0123456789";

    fn volc_cfg() -> ImageHostConfig {
        let mut c = cfg();
        c.kind = ImageHostKind::Volcengine;
        c.token = String::new();
        c.volc_region = "cn-beijing".into();
        c.volc_bucket = "markd-test".into();
        c.volc_access_key_id = VOLC_AK.into();
        c.volc_secret_access_key = VOLC_SK.into();
        c
    }

    /// 整条 V4 签名链与火山引擎官方 Python SDK（tos v2.9.3 的 `Auth.sign_request`）
    /// 逐字节对齐：体摘要 → 规范请求 → 待签字符串 → 派生密钥 → 签名 → Authorization 头。
    /// 定值用合成密钥离线生成，不是从本产品代码里抄回来的。
    /// 对象名刻意不带形如日期的串，免得任何一层把 `/` 显示成别的样子后抄错期望值。
    #[test]
    fn volc_signature_matches_the_official_sdk() {
        let mut body: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        body.extend_from_slice(b"hello markd");
        let psha = sha256_hex(&body);
        assert_eq!(
            psha,
            "ae577d84eea9548402cc1283546032bf86477b7358735de015f098bcc1b1df42"
        );
        let host = "markd-test.tos-cn-beijing.volces.com";
        let mut headers = vec![
            ("content-type".to_string(), "image/png".to_string()),
            ("host".to_string(), host.to_string()),
            ("x-tos-content-sha256".to_string(), psha.clone()),
            ("x-tos-date".to_string(), "20261121T090000Z".to_string()),
        ];
        headers.sort();
        let canonical = volc_canonical_request("PUT", "/markd/tostest/shot_a.png", &headers, &psha);
        assert_eq!(
            canonical,
            "PUT\n/markd/tostest/shot_a.png\n\ncontent-type:image/png\nhost:markd-test.tos-cn-beijing.volces.com\nx-tos-content-sha256:ae577d84eea9548402cc1283546032bf86477b7358735de015f098bcc1b1df42\nx-tos-date:20261121T090000Z\n\ncontent-type;host;x-tos-content-sha256;x-tos-date\nae577d84eea9548402cc1283546032bf86477b7358735de015f098bcc1b1df42"
        );
        assert_eq!(
            volc_string_to_sign(&canonical, "20261121T090000Z", "20261121", "cn-beijing"),
            "TOS4-HMAC-SHA256\n20261121T090000Z\n20261121/cn-beijing/tos/request\na8e7d6705ef076d85579fa282ee32f0569e19350a50bae4a859091de0c26962f"
        );
        assert_eq!(
            volc_signing_key(VOLC_SK, "20261121", "cn-beijing")
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            "dbe5d75031d30d28bc774143d750f9b07571e98ba1dcfa47457678254ea3fc4c"
        );
        let auth = volc_authorization(
            VOLC_AK,
            VOLC_SK,
            "cn-beijing",
            "20261121T090000Z",
            "20261121",
            "content-type;host;x-tos-content-sha256;x-tos-date",
            &canonical,
        );
        assert_eq!(
            auth,
            "TOS4-HMAC-SHA256 Credential=AKLTtest0000000000000/20261121/cn-beijing/tos/request, SignedHeaders=content-type;host;x-tos-content-sha256;x-tos-date, Signature=e0daf167555764227e2e238afb55e73a559ac7ba197392b4ff2693bd2b7af17f"
        );
        // 三段之间是「逗号 + 空格」，和官方 Python SDK 一字不差。
        // 实测：漏掉空格，火山直接报 InvalidAccessKeyId（说密钥不存在），而不是报签名错。
        assert!(auth.contains("request, SignedHeaders="), "{auth}");
        assert!(auth.contains(", Signature="), "{auth}");
        // 换一个字节签名整个变掉，说明体摘要真的进了签名链。
        let mut other = body.clone();
        other[0] ^= 0x01;
        let other_sha = sha256_hex(&other);
        assert_ne!(other_sha, psha);
        assert_ne!(
            volc_canonical_request("PUT", "/markd/tostest/shot_a.png", &headers, &other_sha),
            canonical
        );
    }

    /// 体摘要与百分号编码用公开定值钉住（空串的 SHA-256 是行业定值）。
    #[test]
    fn volc_hash_and_uri_encoding_are_the_documented_ones() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(uri_encode("/a b/c.png", false), "/a%20b/c.png");
        assert_eq!(uri_encode("/a b/c.png", true), "%2Fa%20b%2Fc.png");
        assert_eq!(
            uri_encode("目录/图 1.png", false),
            "%E7%9B%AE%E5%BD%95/%E5%9B%BE%201.png"
        );
        // 未保留集是字母数字与 - _ . ~
        assert_eq!(uri_encode("aZ0-_.~!'", false), "aZ0-_.~%21%27");
        // 头部值参与签名前压掉连续空白。
        assert_eq!(volc_header_value("  image/png   x "), "image/png x");
    }

    /// 地域归一：ID、控制台中文、原生域名、S3 兼容域名、桶的三级域名都要认。
    #[test]
    fn normalize_volc_region_accepts_console_names_and_domains() {
        for raw in [
            "cn-beijing",
            " CN-Beijing ",
            "华北2（北京）",
            "北京",
            "tos-cn-beijing.volces.com",
            "https://tos-cn-beijing.volces.com/",
            "markd-markd.tos-cn-beijing.volces.com",
            "tos-s3-cn-beijing.volces.com",
        ] {
            assert_eq!(
                normalize_volc_region(raw),
                "cn-beijing",
                "{raw} 应归一成 cn-beijing"
            );
        }
        assert_eq!(normalize_volc_region("华东2（上海）"), "cn-shanghai");
        assert_eq!(normalize_volc_region("华南1（广州）"), "cn-guangzhou");
        assert_eq!(normalize_volc_region("新加坡"), "ap-southeast-1");
        assert_eq!(normalize_volc_region(""), "");
        assert_eq!(normalize_volc_region("火星-奥林帕斯"), "火星-奥林帕斯");
    }

    /// 陌生但形如 `xx-yyyy` 的地域放行（火山新开区不该被 Markd 卡住）；
    /// 中文没映射上、明显不是 ID 的才报错，并把认得的地域列出来。
    #[test]
    fn check_volc_region_is_permissive_on_ids_strict_on_garbage() {
        assert!(check_volc_region("cn-beijing").is_ok());
        assert!(
            check_volc_region("sa-east-1").is_ok(),
            "没实测过但像 ID 的要放行"
        );
        let err = check_volc_region("北京北京市").unwrap_err();
        assert!(err.contains("认不出来"), "{err}");
        assert!(err.contains("cn-beijing"), "错误里要列出可填的地域：{err}");
        assert!(check_volc_region("").unwrap_err().contains("cn-beijing"));
        assert!(check_volc_region("-cn-beijing").is_err());
        assert!(check_volc_region("cnbeijing").is_err());
    }

    #[test]
    fn volc_host_and_public_url_follow_bucket_and_region() {
        let c = volc_cfg();
        assert_eq!(volc_host(&c), "markd-test.tos-cn-beijing.volces.com");
        assert_eq!(
            volc_public_url(&c, "markd/a.png"),
            "https://markd-test.tos-cn-beijing.volces.com/markd/a.png"
        );
        // 直接构造配置（绕过 from_config）时填中文地域也要拼得出域名。
        let mut zh = volc_cfg();
        zh.volc_region = "华北2（北京）".into();
        assert_eq!(volc_host(&zh), "markd-test.tos-cn-beijing.volces.com");
        // 设了 CDN 前缀只改文档里的链接，写入端点不变。
        let mut cdn = volc_cfg();
        cdn.public_base = "img.example.com/".into();
        assert_eq!(
            volc_public_url(&cdn, "markd/a.png"),
            "https://img.example.com/markd/a.png"
        );
        assert_eq!(volc_host(&cdn), "markd-test.tos-cn-beijing.volces.com");
    }

    /// TOS 的错误正文是 JSON，`Code` 比状态码有用得多，逐条翻成中文。
    #[test]
    fn volc_error_translates_the_measured_codes() {
        let ak_missing = r#"{"Code":"InvalidAccessKeyId","Message":"The Access Key Id you provided does not exist in our records.","EC":"0002-00000020"}"#;
        let msg = volc_error(403, ak_missing);
        assert!(msg.contains("查不到这个 AccessKey"), "{msg}");
        assert!(msg.contains("HTTP 403"), "{msg}");
        assert!(msg.contains("does not exist"), "火山原话要带出来：{msg}");
        // 注意：这条错误不一定真是密钥的问题——实测 Authorization 三段少个空格，
        // 火山也回 InvalidAccessKeyId 而不是 SignatureDoesNotMatch。所以别只让用户查密钥。
        assert!(volc_error(403, r#"{"Code":"SignatureDoesNotMatch"}"#).contains("Secret"));
        assert!(
            volc_error(403, r#"{"Code":"AccessDenied","Message":"Access Denied"}"#)
                .contains("写权限")
        );
        assert!(volc_error(
            404,
            r#"{"Code":"NoSuchBucket","Message":"The specified bucket does not exist."}"#
        )
        .contains("存储桶"));
        // 实测：给原生端点发 AWS SigV4 就是回这条。
        let wrong_scheme =
            r#"{"Code":"InvalidArgument","Message":"Unsupported Authorization Type"}"#;
        assert!(volc_error(400, wrong_scheme).contains("TOS4-HMAC-SHA256"));
        // 认不出的 Code 也要报得出来，不能沉默。
        let odd = volc_error(500, r#"{"Code":"InternalError","Message":"boom"}"#);
        assert!(odd.contains("InternalError"), "{odd}");
        // 非 JSON 正文（网关 HTML）截断后带上。
        let html = volc_error(502, &format!("<html>{}</html>", "x".repeat(500)));
        assert!(html.contains("<html>"), "{html}");
        assert!(
            html.chars().count() < 400,
            "正文没截断：{}",
            html.chars().count()
        );
    }

    /// 必填项逐个点名；地域写成中文没映射上时要在校验里拦住。
    #[test]
    fn volc_validate_names_the_missing_field() {
        let mut c = volc_cfg();
        assert!(c.validate().is_ok(), "{:?}", c.validate());
        c.volc_bucket = " ".into();
        assert!(
            c.validate().unwrap_err().contains("存储桶"),
            "{:?}",
            c.validate()
        );
        c = volc_cfg();
        c.volc_region = String::new();
        assert!(
            c.validate().unwrap_err().contains("地域"),
            "{:?}",
            c.validate()
        );
        c = volc_cfg();
        c.volc_region = "北京北京市".into();
        assert!(
            c.validate().unwrap_err().contains("认不出来"),
            "{:?}",
            c.validate()
        );
        c = volc_cfg();
        c.volc_secret_access_key = String::new();
        assert!(
            c.validate().unwrap_err().contains("Secret Access Key"),
            "{:?}",
            c.validate()
        );
        // 火山不要仓库令牌：token 空着也要通过（防仓库平台的检查漏进来）。
        c = volc_cfg();
        assert!(c.token.is_empty() && c.validate().is_ok());
    }

    #[test]
    fn volc_kind_is_a_platform_of_its_own() {
        assert_eq!(
            serde_json::from_str::<ImageHostKind>(r#""volcengine""#).unwrap(),
            ImageHostKind::Volcengine
        );
        assert_eq!(
            serde_json::to_string(&ImageHostKind::Volcengine).unwrap(),
            r#""volcengine""#
        );
        assert!(ImageHostKind::ALL.contains(&ImageHostKind::Volcengine));
        assert_eq!(ImageHostKind::Volcengine.label(), "火山引擎 TOS");
        assert_eq!(ImageHostKind::Volcengine.platform_name(), "火山引擎");
        assert!(ImageHostKind::Volcengine.is_enabled());
    }

    /// 保存路径模板 + 桶域名拼成最终链接（用户给的那条模板整条展开）。
    #[test]
    fn volc_path_and_url_follow_the_template() {
        let t = fixed_time();
        let mut c = volc_cfg();
        c.dir = String::new();
        c.path_pattern = "markd/{year}/{month}/{day}/{hour}_{random}_{filename}{.suffix}".into();
        let name = remote_name_for(&c, "截图 Shot.PNG");
        assert_eq!(name, "Shot.png");
        let path = object_path(&c, &name, &t);
        // 期望值用 join 拼出来：写成字面量时这一串日期形态的文本会被显示层改写。
        let want = ["markd", "2026", "11", "17", "09_a1b2c3_Shot.png"].join("/");
        assert_eq!(path, want);
        assert_eq!(
            volc_public_url(&c, &path),
            format!("https://markd-test.tos-cn-beijing.volces.com/{want}")
        );
        // 对象名要进 URL 路径，也要进规范请求，两处编码必须一致。
        assert_eq!(uri_encode(&format!("/{path}"), false), format!("/{path}"));
    }

    // -------------------------------------------------------------- 百度智能云 BOS

    const BOS_AK: &str = "ALTAKtesttesttesttesttest0";
    const BOS_SK: &str = "0123456789abcdef0123456789abcdef";
    /// 官方 SDK 定值对应的那个 UTC 时间（epoch 1795250000）。
    const BOS_TIME: &str = "2026-11-21T08:33:20Z";

    fn bos_cfg() -> ImageHostConfig {
        let mut c = cfg();
        c.kind = ImageHostKind::Baidu;
        c.token = String::new();
        c.bos_region = "bj".into();
        c.bos_bucket = "markd-test".into();
        c.bos_access_key_id = BOS_AK.into();
        c.bos_secret_access_key = BOS_SK.into();
        c
    }

    /// 整条 bce-auth-v1 签名链与百度官方 Python SDK（bce-python-sdk 的 `bce_v1_signer.sign`）
    /// 逐字节对齐：信任状 → 两段式密钥 → 规范化头 → 待签字符串 → 签名 → Authorization。
    /// 定值是离线跑官方 SDK 生成的合成密钥，不是从本产品代码回抄的。
    #[test]
    fn bos_signature_matches_the_official_sdk() {
        let headers = vec![
            ("content-length".to_string(), "70".to_string()),
            ("content-type".to_string(), "image/png".to_string()),
            ("host".to_string(), "markd-test.bj.bcebos.com".to_string()),
            ("x-bce-date".to_string(), BOS_TIME.to_string()),
        ];
        let ch = bos_canonical_headers(&headers);
        assert_eq!(
            ch,
            "content-length:70\ncontent-type:image%2Fpng\nhost:markd-test.bj.bcebos.com\nx-bce-date:2026-11-21T08%3A33%3A20Z"
        );
        // 实测要点：头值也要百分号编码，`image/png` 进签名是 `image%2Fpng`，
        // 时间里的冒号是 `%3A`。值不编码就签不上，光看文档很容易漏。
        assert!(ch.contains("image%2Fpng"), "头值没编码：{ch}");
        assert!(ch.contains("%3A33%3A20Z"), "时间没编码：{ch}");
        let info = bos_sign_key_info(BOS_AK, BOS_TIME, BOS_EXPIRATION_SECONDS);
        assert_eq!(
            info,
            "bce-auth-v1/ALTAKtesttesttesttesttest0/2026-11-21T08:33:20Z/1800"
        );
        let key = bos_sign_key(BOS_SK, &info);
        // 两段式密钥：第一步的 hex 字符串本身当第二步的密钥。
        assert_eq!(
            key,
            "b4f7042a1062d91d1675bf705fe3c84c610264150b030541c77d4774c0cada2f"
        );
        for (obj, want_uri, want_sts, want_auth) in [
            (
                "markd/kat/shot a#1.png",
                "/markd/kat/shot%20a%231.png",
                "PUT\n/markd/kat/shot%20a%231.png\n\ncontent-length:70\ncontent-type:image%2Fpng\nhost:markd-test.bj.bcebos.com\nx-bce-date:2026-11-21T08%3A33%3A20Z",
                "bce-auth-v1/ALTAKtesttesttesttesttest0/2026-11-21T08:33:20Z/1800//129c51a1f3a8ead6860f5fca5cf499d492ce347767b98144c03c9ca3cfb78db6",
            ),
            (
                "markd/kat/测试 图.png",
                "/markd/kat/%E6%B5%8B%E8%AF%95%20%E5%9B%BE.png",
                "PUT\n/markd/kat/%E6%B5%8B%E8%AF%95%20%E5%9B%BE.png\n\ncontent-length:70\ncontent-type:image%2Fpng\nhost:markd-test.bj.bcebos.com\nx-bce-date:2026-11-21T08%3A33%3A20Z",
                "bce-auth-v1/ALTAKtesttesttesttesttest0/2026-11-21T08:33:20Z/1800//188b7ae27b2f885cc35bfac7dd3c74047676c8489353f2031ba70b440e0db7a4",
            ),
        ] {
            assert_eq!(
                format!("/{}", uri_encode(obj, false)),
                want_uri,
                "对象名编码"
            );
            let sts = bos_string_to_sign("PUT", want_uri, &ch);
            assert_eq!(sts, want_sts, "待签字符串");
            assert_eq!(
                bos_authorization(&info, &key, &sts),
                want_auth,
                "Authorization"
            );
            // 信任状与签名之间是「//」：中间那段留空表示按服务端默认头集合算。
            assert!(
                want_auth.contains("/1800//"),
                "信任状分隔形态变了：{want_auth}"
            );
            // 签名头集合本身也钉住：漏掉 content-length 或 x-bce-date 都会签不上。
            let got = bos_signed_headers("markd-test.bj.bcebos.com", "image/png", 70, BOS_TIME);
            assert_eq!(got, headers, "签名头集合形态变了");
            // 空值的头不参与（官方规则如此，带上就签不上）。
            let mut with_empty = headers.clone();
            with_empty.push(("content-md5".to_string(), "  ".to_string()));
            assert_eq!(bos_canonical_headers(&with_empty), ch, "空值头不该进签名");
            // 规范 URI 与时间格式各钉一个定值，改错任何一处都会红。
            assert_eq!(
                bos_canonical_uri("markd/kat/shot a#1.png"),
                "/markd/kat/shot%20a%231.png"
            );
            let t = chrono::DateTime::parse_from_rfc3339("2026-11-21T08:33:20+00:00")
                .unwrap()
                .with_timezone(&chrono::Utc);
            assert_eq!(bos_canonical_time(t), "2026-11-21T08:33:20Z");
            // 请求体长度进了签名链：多一个字节就是另一个签名。
            let mut other = headers.clone();
            other[0].1 = "71".to_string();
            assert_ne!(
                bos_string_to_sign("PUT", want_uri, &bos_canonical_headers(&other)),
                want_sts
            );
        }
    }

    /// 地域写法归一：ID、控制台全称、只写城市名、endpoint、桶的三级域名。
    #[test]
    fn bos_region_normalization_tolerates_console_spellings() {
        assert_eq!(normalize_bos_region("bj"), "bj");
        assert_eq!(normalize_bos_region("BJ"), "bj");
        assert_eq!(normalize_bos_region(" 华北-北京 "), "bj");
        assert_eq!(normalize_bos_region("北京"), "bj");
        assert_eq!(normalize_bos_region("bj.bcebos.com"), "bj");
        assert_eq!(normalize_bos_region("https://bj.bcebos.com"), "bj");
        assert_eq!(normalize_bos_region("markd-markd.bj.bcebos.com"), "bj");
        assert_eq!(normalize_bos_region("中国-香港"), "hkg");
        assert_eq!(normalize_bos_region("华中-武汉"), "fwh");
        assert_eq!(normalize_bos_region(""), "");
        // 认不出的原样返回，别静默改成某个默认地域。
        assert_eq!(normalize_bos_region("火星-奥林匹斯"), "火星-奥林匹斯");
        assert!(check_bos_region("bj").is_ok());
        // 文档没列但形如小写字母 ID 的放行（百度会开新区域）。
        assert!(check_bos_region("zz").is_ok());
        let err = check_bos_region("火星-奥林匹斯").unwrap_err();
        assert!(err.contains("华北-北京（bj）"), "要列出可用地域：{err}");
        assert!(check_bos_region("").unwrap_err().contains("请填写"));
    }

    /// 上传主机名与公网地址：桶 + 地域拼三级域名；设了自定义前缀就走前缀。
    #[test]
    fn bos_host_and_public_url_follow_bucket_and_region() {
        let c = bos_cfg();
        assert_eq!(bos_host(&c), "markd-test.bj.bcebos.com");
        assert_eq!(
            bos_public_url(&c, "markd/kat/a.png"),
            "https://markd-test.bj.bcebos.com/markd/kat/a.png"
        );
        let mut cdn = bos_cfg();
        cdn.public_base = "img.example.com".into();
        assert_eq!(
            bos_public_url(&cdn, "markd/kat/a.png"),
            "https://img.example.com/markd/kat/a.png"
        );
        let mut https = bos_cfg();
        https.public_base = "http://cdn.example.com/".into();
        assert_eq!(
            bos_public_url(&https, "a.png"),
            "http://cdn.example.com/a.png"
        );
    }

    /// 错误正文是 JSON（键名小写），实测到的 code 逐个翻中文并带出原话。
    #[test]
    fn bos_error_translates_the_measured_codes() {
        // 这条是实测拿到的原样正文（没带 x-bce-date 时百度就这么回）。
        let missing_date = r#"{"code":"MissingDateHeader","message":"Request must have a \"date\" or \"x-bce-date\" header.","requestId":"1cf9f9f5-5c41-4fcb-811c-5a7141ffa26f"}"#;
        let msg = bos_error(400, missing_date);
        assert!(msg.contains("HTTP 400"), "{msg}");
        assert!(msg.contains("x-bce-date"), "{msg}");
        assert!(msg.contains("date"), "百度原话要带出来：{msg}");
        assert!(
            bos_error(403, r#"{"code":"AccessDenied","message":"Access Denied."}"#)
                .contains("公共读")
        );
        assert!(bos_error(
            403,
            r#"{"code":"SignatureDoesNotMatch","message":"bad signature"}"#
        )
        .contains("Secret Access Key"));
        assert!(bos_error(403, r#"{"code":"InvalidAccessKeyId"}"#).contains("AccessKey"));
        assert!(bos_error(404, r#"{"code":"NoSuchBucket"}"#).contains("存储桶"));
        assert!(bos_error(400, r#"{"code":"InvalidRequestRegion"}"#).contains("地域"));
        // 认不出的 code 也要报得出来。
        let odd = bos_error(500, r#"{"code":"InternalError","message":"boom"}"#);
        assert!(odd.contains("InternalError"), "{odd}");
        // 非 JSON 正文（网关 HTML）截一段带上，不留个光秃秃的状态码。
        let html = bos_error(502, &format!("<html>{}</html>", "x".repeat(500)));
        assert!(html.contains("<html>"), "{html}");
        assert!(
            html.chars().count() < 400,
            "正文没截断：{}",
            html.chars().count()
        );
    }

    /// 必填项逐个点名，顺序稳定：桶 → 地域 → 密钥对。
    #[test]
    fn bos_validate_names_the_missing_field() {
        let mut c = bos_cfg();
        assert!(c.validate().is_ok());
        c.bos_bucket = "  ".into();
        assert!(c.validate().unwrap_err().contains("存储桶名"));
        c.bos_bucket = "markd-test".into();
        c.bos_region = "火星-奥林匹斯".into();
        assert!(c.validate().unwrap_err().contains("认不出"));
        c.bos_region = "bj".into();
        c.bos_secret_access_key = String::new();
        assert!(c.validate().unwrap_err().contains("Secret Access Key"));
        c.bos_secret_access_key = BOS_SK.into();
        c.bos_access_key_id = String::new();
        assert!(c.validate().unwrap_err().contains("AccessKey"));
    }

    /// 新平台是独立一档：下拉八个选项、名字与 serde 取值都不跟别人串。
    #[test]
    fn bos_kind_is_a_platform_of_its_own() {
        assert_eq!(ImageHostKind::ALL.len(), 9);
        assert!(ImageHostKind::ALL.contains(&ImageHostKind::Baidu));
        assert_eq!(ImageHostKind::Baidu.label(), "百度智能云 BOS");
        assert_eq!(ImageHostKind::Baidu.platform_name(), "百度智能云");
        assert!(ImageHostKind::Baidu.is_enabled());
        assert_eq!(
            serde_json::to_string(&ImageHostKind::Baidu).unwrap(),
            "\"baidu\""
        );
        assert_eq!(
            serde_json::from_str::<ImageHostKind>("\"baidu\"").unwrap(),
            ImageHostKind::Baidu
        );
        // 归一化只在 from_config 里做一次，地域写中文也该落到 bj。
        let mut raw = crate::workspace::Config::default();
        raw.image_host = ImageHostKind::Baidu;
        raw.image_host_bos_region = "华北-北京".into();
        raw.image_host_bos_bucket = "markd-markd".into();
        raw.image_host_bos_access_key = BOS_AK.into();
        raw.image_host_bos_secret_key = BOS_SK.into();
        let h = ImageHostConfig::from_config(&raw);
        assert_eq!(h.bos_region, "bj");
        assert_eq!(bos_host(&h), "markd-markd.bj.bcebos.com");
        h.validate().unwrap();
    }

    /// 保存路径模板与公网地址接得上；编码后的路径两处一致。
    #[test]
    fn bos_path_and_url_follow_the_template() {
        let mut c = bos_cfg();
        c.path_pattern = "markd/{year}/{month}/{day}/{hour}_{random}_{filename}{.suffix}".into();
        c.dir = String::new();
        let t = fixed_time();
        let name = remote_name_for(&c, "截图 Shot.PNG");
        assert_eq!(name, "Shot.png");
        let path = object_path(&c, &name, &t);
        // 期望值用 join 拼出来：写成字面量时这串日期样式的文本会被显示层改写。
        let want = ["markd", "2026", "11", "17", "09_a1b2c3_Shot.png"].join("/");
        assert_eq!(path, want);
        assert_eq!(
            bos_public_url(&c, &path),
            format!("https://markd-test.bj.bcebos.com/{want}")
        );
        assert_eq!(uri_encode(&format!("/{path}"), false), format!("/{path}"));
    }

    // -------------------------------------------------------------- 华为云 OBS

    const OBS_AK: &str = "HWUAKSYNTHETICKEY00000";
    const OBS_SK: &str = "synthetic-secret-key-0123456789abcdef00";
    /// 官方 SDK 定值对应的那个 GMT 时间。
    const OBS_DATE: &str = "Sat, 21 Nov 2026 09:00:00 GMT";

    fn obs_cfg() -> ImageHostConfig {
        let mut c = cfg();
        c.kind = ImageHostKind::Huawei;
        c.token = String::new();
        c.obs_region = "cn-east-3".into();
        c.obs_bucket = "markd-test".into();
        c.obs_access_key_id = OBS_AK.into();
        c.obs_secret_access_key = OBS_SK.into();
        c
    }

    /// V2 签名与华为官方 Python SDK（esdk-obs-python 的 `Authentication.doAuth`）逐字节对齐，
    /// 定值用合成密钥与固定 Date 离线生成。顺带钉住「资源段用编码后的对象名」这条实测规则。
    #[test]
    fn obs_signature_matches_the_official_sdk() {
        let cases = [
            ("markd/probe/shot a#1.png", "PUT\n\nimage/png\nSat, 21 Nov 2026 09:00:00 GMT\n/markd-test/markd/probe/shot%20a%231.png", "OBS HWUAKSYNTHETICKEY00000:WVQ6sX3mYP4P58/M3VEGP58c+fo="),
            ("markd/probe/测试 图.png", "PUT\n\nimage/png\nSat, 21 Nov 2026 09:00:00 GMT\n/markd-test/markd/probe/%E6%B5%8B%E8%AF%95%20%E5%9B%BE.png", "OBS HWUAKSYNTHETICKEY00000:sW2KGeaV0ct2sKZMB/DI2YvApdA="),
        ];
        for (key, want_sts, want_auth) in cases {
            let sts = obs_string_to_sign("image/png", OBS_DATE, "markd-test", key);
            assert_eq!(sts, want_sts, "待签字符串");
            assert_eq!(
                obs_authorization(OBS_AK, &v1_signature(OBS_SK, &sts).unwrap()),
                want_auth,
                "Authorization"
            );
            // Content-MD5 那一行是空的，但换行占位必须在，缺了就整体错位。
            assert!(
                want_sts.starts_with("PUT\n\nimage/png\n"),
                "Content-MD5 占位缺失：{want_sts}"
            );
            // 对象名不编码就与官方 SDK 不同——带空格、# 或中文时实测签不上。
            let raw = v1_string_to_sign("PUT", "", "image/png", OBS_DATE, "markd-test", key);
            // 上面那条 obs_string_to_sign 必须与官方 SDK 一致，且与「不编码」的写法不同。
            assert_ne!(raw, want_sts, "资源段必须用编码后的对象名");
        }
        // 换一把秘密访问密钥就是另一个签名。
        assert_ne!(v1_signature("other-secret-key", "PUT\n\nimage/png\nSat, 21 Nov 2026 09:00:00 GMT\n/markd-test/markd/probe/shot%20a%231.png").unwrap(), "WVQ6sX3mYP4P58/M3VEGP58c+fo=");
    }

    /// GMT 时间格式：星期与月份用英文缩写，不受系统 locale 影响。
    #[test]
    fn obs_date_is_rfc1123_gmt() {
        let t = chrono::DateTime::parse_from_rfc3339("2026-11-21T09:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(v1_date(t), OBS_DATE);
    }

    /// 区域写法归一：编码、中文全称、只写「城市+序号」、endpoint、桶的三级域名。
    /// 裸写「上海」「北京」不猜——两城各有多个区域编码，猜错会把桶拼到别区。
    #[test]
    fn obs_region_normalization_covers_spellings_without_guessing() {
        assert_eq!(normalize_obs_region("cn-east-3"), "cn-east-3");
        assert_eq!(normalize_obs_region("CN-East-3"), "cn-east-3");
        assert_eq!(normalize_obs_region(" 华东-上海一 "), "cn-east-3");
        assert_eq!(normalize_obs_region("上海一"), "cn-east-3");
        assert_eq!(
            normalize_obs_region("obs.cn-east-3.myhuaweicloud.com"),
            "cn-east-3"
        );
        assert_eq!(
            normalize_obs_region("https://obs.cn-east-3.myhuaweicloud.com"),
            "cn-east-3"
        );
        assert_eq!(
            normalize_obs_region("markd-markd.obs.cn-east-3.myhuaweicloud.com"),
            "cn-east-3"
        );
        assert_eq!(normalize_obs_region("华北-北京四"), "cn-north-4");
        assert_eq!(normalize_obs_region("西南-贵阳一"), "cn-southwest-2");
        // 有歧义的写法保持原样，由校验报错，不会静默挑一个区。
        assert_eq!(normalize_obs_region("上海"), "上海");
        assert_eq!(normalize_obs_region("北京"), "北京");
        assert_eq!(normalize_obs_region(""), "");
    }

    /// 表外但形状对的编码放行（华为还在开新区），认不出的要列出有中文全称的那些。
    #[test]
    fn obs_check_region_passes_shaped_ids_and_lists_named_ones() {
        assert!(check_obs_region("cn-east-3").is_ok());
        assert!(check_obs_region("ap-southeast-1").is_ok());
        assert!(check_obs_region("na-mexico-1").is_ok());
        assert!(check_obs_region("").unwrap_err().contains("请填写"));
        let err = check_obs_region("上海").unwrap_err();
        assert!(
            err.contains("华东-上海一（cn-east-3）"),
            "要列出有中文全称的区域：{err}"
        );
        assert!(
            err.contains("obs.<区域>.myhuaweicloud.com"),
            "要提示可以粘 endpoint：{err}"
        );
    }

    /// 上传主机名与公网地址：桶 + 区域拼三级域名；自定义前缀优先。
    #[test]
    fn obs_host_and_public_url_follow_bucket_and_region() {
        let c = obs_cfg();
        assert_eq!(obs_host(&c), "markd-test.obs.cn-east-3.myhuaweicloud.com");
        assert_eq!(
            obs_public_url(&c, "markd/kat/a.png"),
            "https://markd-test.obs.cn-east-3.myhuaweicloud.com/markd/kat/a.png"
        );
        let mut cdn = obs_cfg();
        cdn.public_base = "cdn.example.com/".into();
        assert_eq!(
            obs_public_url(&cdn, "a.png"),
            "https://cdn.example.com/a.png"
        );
        // 资源段与 URL 路径必须同一份编码，两处不一致就签不上；
        // 断言走 obs_string_to_sign（真正签名的那条），不要另写一份拼法。
        let key = "markd/kat/测试 图#1.png";
        let enc = obs_encoded_key(key);
        assert_eq!(
            obs_string_to_sign("image/png", OBS_DATE, "markd-test", key),
            format!("PUT\n\nimage/png\n{OBS_DATE}\n/markd-test/{enc}")
        );
        assert!(
            enc.contains("%E6%B5%8B%E8%AF%95"),
            "中文按 UTF-8 字节编码：{enc}"
        );
        assert!(
            enc.contains("%20") && enc.contains("%23"),
            "空格与 # 要编码：{enc}"
        );
        assert!(!enc.contains("%2F"), "目录分隔符保持原样：{enc}");
        // 桶名多粘了斜杠时，主机名与资源段必须一起剥掉，只剥一边就签不上。
        let mut sloppy = obs_cfg();
        sloppy.obs_bucket = " /markd-test/ ".into();
        assert_eq!(
            obs_host(&sloppy),
            "markd-test.obs.cn-east-3.myhuaweicloud.com"
        );
        assert_eq!(
            obs_string_to_sign("image/png", OBS_DATE, &sloppy.obs_bucket, "a.png"),
            format!("PUT\n\nimage/png\n{OBS_DATE}\n/markd-test/a.png")
        );
    }

    /// 错误正文是 XML（下面几条是实测抓回来的原样），按 Code 翻中文并带出华为原话。
    #[test]
    fn obs_error_translates_the_measured_codes() {
        let sig = "<Error><Code>SignatureDoesNotMatch</Code><Message>The request signature we calculated does not match the signature you provided. Check your key and signing method.</Message><RequestId>000001A1FD62E9628249C</RequestId></Error>";
        let msg = obs_error(403, sig);
        assert!(msg.contains("HTTP 403"), "{msg}");
        assert!(msg.contains("秘密访问密钥不对"), "{msg}");
        assert!(
            msg.contains("signature you provided"),
            "华为原话要带出来：{msg}"
        );
        // 实测：桶名对、区域错，也回 NoSuchBucket。
        let nb = "<Error><Code>NoSuchBucket</Code><Message>The specified bucket does not exist</Message></Error>";
        let m2 = obs_error(404, nb);
        assert!(m2.contains("桶不存在"), "{m2}");
        assert!(m2.contains("does not exist"), "{m2}");
        assert!(obs_error(403, "<Error><Code>AccessDenied</Code></Error>").contains("写权限"));
        assert!(
            obs_error(403, "<Error><Code>InvalidAccessKeyId</Code></Error>").contains("我的凭证")
        );
        assert!(obs_error(503, "<Error><Code>SlowDown</Code></Error>").contains("稍后重试"));
        let odd = obs_error(
            500,
            "<Error><Code>InternalError</Code><Message>boom</Message></Error>",
        );
        assert!(odd.contains("InternalError"), "{odd}");
        let html = obs_error(502, &format!("<html>{}</html>", "x".repeat(500)));
        assert!(html.contains("<html>"), "{html}");
        assert!(
            html.chars().count() < 400,
            "正文没截断：{}",
            html.chars().count()
        );
        assert_eq!(
            xml_tag(sig, "Code").as_deref(),
            Some("SignatureDoesNotMatch")
        );
        assert_eq!(xml_tag("没有标签", "Code"), None);
    }

    /// 必填项逐个点名：桶 → 区域 → 密钥对。
    #[test]
    fn obs_validate_names_the_missing_field() {
        let mut c = obs_cfg();
        assert!(c.validate().is_ok());
        c.obs_bucket = " ".into();
        assert!(c.validate().unwrap_err().contains("桶名"));
        c.obs_bucket = "markd-test".into();
        c.obs_region = "上海".into();
        assert!(c.validate().unwrap_err().contains("认不出"));
        c.obs_region = "cn-east-3".into();
        c.obs_secret_access_key = String::new();
        assert!(c.validate().unwrap_err().contains("秘密访问密钥"));
        c.obs_secret_access_key = OBS_SK.into();
        c.obs_access_key_id = String::new();
        assert!(c.validate().unwrap_err().contains("访问密钥 ID"));
    }

    /// 新平台是独立一档：下拉九个选项、配置里的平台取值是 `huawei`。
    #[test]
    fn obs_kind_is_a_platform_of_its_own() {
        assert_eq!(ImageHostKind::ALL.len(), 9);
        assert!(ImageHostKind::ALL.contains(&ImageHostKind::Huawei));
        assert_eq!(ImageHostKind::Huawei.label(), "华为云 OBS");
        assert_eq!(ImageHostKind::Huawei.platform_name(), "华为云");
        assert!(ImageHostKind::Huawei.is_enabled());
        assert_eq!(
            serde_json::to_string(&ImageHostKind::Huawei).unwrap(),
            "\"huawei\""
        );
        assert_eq!(
            serde_json::from_str::<ImageHostKind>("\"huawei\"").unwrap(),
            ImageHostKind::Huawei
        );
        // from_config 负责归一中文区域，别指望每个调用方都自己记得归一。
        let mut raw = crate::workspace::Config::default();
        raw.image_host = ImageHostKind::Huawei;
        raw.image_host_obs_region = "华东-上海一".into();
        raw.image_host_obs_bucket = "markd-makrd".into();
        raw.image_host_obs_access_key = OBS_AK.into();
        raw.image_host_obs_secret_key = OBS_SK.into();
        let h = ImageHostConfig::from_config(&raw);
        assert_eq!(h.obs_region, "cn-east-3");
        assert_eq!(obs_host(&h), "markd-makrd.obs.cn-east-3.myhuaweicloud.com");
        h.validate().unwrap();
    }

    /// 保存路径模板、公网地址与资源段三处接得上。
    #[test]
    fn obs_path_and_url_follow_the_template() {
        let mut c = obs_cfg();
        c.dir = String::new();
        c.path_pattern = "markd/{year}/{month}/{day}/{hour}_{random}_{filename}{.suffix}".into();
        let t = fixed_time();
        let name = remote_name_for(&c, "截图 Shot.PNG");
        assert_eq!(name, "Shot.png");
        let path = object_path(&c, &name, &t);
        // 期望值用 join 拼出来：写成字面量时这串日期样式的文本会被显示层改写。
        let want = ["markd", "2026", "11", "17", "09_a1b2c3_Shot.png"].join("/");
        assert_eq!(path, want);
        assert_eq!(
            obs_public_url(&c, &path),
            format!("https://markd-test.obs.cn-east-3.myhuaweicloud.com/{want}")
        );
        assert_eq!(
            obs_string_to_sign("image/png", OBS_DATE, "markd-test", &path),
            format!("PUT\n\nimage/png\n{OBS_DATE}\n/markd-test/{want}")
        );
    }

    // ── 腾讯云 COS ────────────────────────────────────────────────────────────

    const COS_SID: &str = "AKIDSYNTHETICCOSKEY00000000000000";
    const COS_SK: &str = "syntheticcossecretkey000000000000";
    const COS_HOST: &str = "markd-1253518225.cos.ap-nanjing.myqcloud.com";
    /// 官方 SDK 用 `{现在-60};{现在+10000}`，钉死 now=1763700000 时就是这一串。
    const COS_SIGN_TIME: &str = "1763699940;1763710000";

    fn cos_cfg() -> ImageHostConfig {
        let mut c = cfg();
        c.kind = ImageHostKind::Tencent;
        c.token = String::new();
        c.dir = String::new();
        c.cos_region = "ap-nanjing".into();
        c.cos_bucket = "markd-1253518225".into();
        c.cos_secret_id = COS_SID.into();
        c.cos_secret_key = COS_SK.into();
        c
    }

    /// v5 签名与腾讯官方 Python SDK（cos-python-sdk-v5 的 `CosS3Auth`）逐字节对齐。
    /// 定值是拿 SDK 本身、用合成密钥与钉死时间离线跑出来的，不是我自己照文档重算的。
    #[test]
    fn cos_signature_matches_the_official_sdk() {
        let canonical = "content-type=image%2Fpng&host=markd-1253518225.cos.ap-nanjing.myqcloud.com";
        let cases = [
            (
                "markd/probe/shot a#1.png",
                "put\n/markd/probe/shot a#1.png\n\ncontent-type=image%2Fpng&host=markd-1253518225.cos.ap-nanjing.myqcloud.com\n",
                "sha1\n1763699940;1763710000\naf71ddaf8e5d91ca3b824bf75c58e15c6b69337d\n",
                "q-sign-algorithm=sha1&q-ak=AKIDSYNTHETICCOSKEY00000000000000&q-sign-time=1763699940;1763710000&q-key-time=1763699940;1763710000&q-header-list=content-type;host&q-url-param-list=&q-signature=99dffa25cc8749ea4d8a3b5f663bb82a2c27ddcf",
            ),
            (
                "markd/probe/测试 图#2.png",
                "put\n/markd/probe/测试 图#2.png\n\ncontent-type=image%2Fpng&host=markd-1253518225.cos.ap-nanjing.myqcloud.com\n",
                "sha1\n1763699940;1763710000\n4f736e89289c2778156f34014dc0ba0b069abdb3\n",
                "q-sign-algorithm=sha1&q-ak=AKIDSYNTHETICCOSKEY00000000000000&q-sign-time=1763699940;1763710000&q-key-time=1763699940;1763710000&q-header-list=content-type;host&q-url-param-list=&q-signature=8123561f1104064424515dbd2f57004131704015",
            ),
            (
                "markd/2026-11-21/09_a1b2c3_Shot.png",
                "put\n/markd/2026-11-21/09_a1b2c3_Shot.png\n\ncontent-type=image%2Fpng&host=markd-1253518225.cos.ap-nanjing.myqcloud.com\n",
                "sha1\n1763699940;1763710000\nc7724c81059908a025a90fd6f0d671a2c3e89e4f\n",
                "q-sign-algorithm=sha1&q-ak=AKIDSYNTHETICCOSKEY00000000000000&q-sign-time=1763699940;1763710000&q-key-time=1763699940;1763710000&q-header-list=content-type;host&q-url-param-list=&q-signature=7e64a0ef557cafd4a0e87ba6ab0c45001f62f182",
            ),
        ];
        let (hdr, list) = cos_signed_headers(COS_HOST, "image/png");
        assert_eq!(hdr, canonical, "头串与官方不一致");
        assert_eq!(list, "content-type;host", "q-header-list 与头串必须同源");
        let mut sigs = Vec::new();
        let mut encoding_matters = 0;
        for (key, want_fmt, want_sts, want_auth) in cases {
            let fmt = cos_format_str(key, &hdr);
            assert_eq!(fmt, want_fmt, "格式串（对象名要原样）");
            let sts = cos_string_to_sign(COS_SIGN_TIME, &fmt);
            assert_eq!(sts, want_sts, "待签字符串");
            let sig = cos_signature(COS_SK, COS_SIGN_TIME, &sts);
            assert_eq!(
                cos_authorization(COS_SID, COS_SIGN_TIME, &list, &sig),
                want_auth,
                "Authorization"
            );
            sigs.push(sig);
            // 反例：对象名先百分号编码再进签名。实测这样腾讯云回 SignatureDoesNotMatch，
            // 所以带空格 / `#` / 中文的键必须与定值不同——否则这个测试是空转的。
            // 纯 ASCII 那条（第三个用例）编码前后一字不差，本来就没有差异可比，
            // 它的作用是覆盖真实保存路径的形状。
            let encoded = uri_encode(key, false);
            if encoded != key {
                encoding_matters += 1;
                assert_ne!(
                    cos_string_to_sign(COS_SIGN_TIME, &cos_format_str(&encoded, &hdr)),
                    want_sts,
                    "对象名编码后应与原样不同：{key}"
                );
            }
        }
        // 三个用例里恰好两个含需要编码的字符：少了就说明这批定值退化成比同一份数据。
        assert_eq!(encoding_matters, 2, "覆盖编码差异的用例数不对");
        // 三个对象名互不相同 ⇒ 三个签名互不相同（防整段断言退化成比同一份数据）。
        assert_eq!(sigs.iter().collect::<std::collections::HashSet<_>>().len(), 3);
        assert_ne!(
            cos_signature("another-secret-key-0000000000000", COS_SIGN_TIME, &cos_string_to_sign(COS_SIGN_TIME, "put\n/a.png\n\nx\n")),
            cos_signature(COS_SK, COS_SIGN_TIME, &cos_string_to_sign(COS_SIGN_TIME, "put\n/a.png\n\nx\n")),
            "换一把 SecretKey 就是另一个签名"
        );
    }

    /// 两段式密钥：SignKey 是 `hex(HMAC-SHA1(SecretKey, q-sign-time))` 这段十六进制本身。
    #[test]
    fn cos_sign_time_and_two_stage_key() {
        assert_eq!(cos_sign_time(1763700000), COS_SIGN_TIME);
        let sign_key = hmac_sha1_hex(COS_SK.as_bytes(), COS_SIGN_TIME);
        assert_eq!(sign_key.len(), 40, "SHA1 摘要转十六进制是 40 个字符");
        assert!(sign_key.chars().all(|c| c.is_ascii_hexdigit()));
        // SignKey 直接当密钥用（不是再哈希一次），所以这一步错则整链错。
        assert_eq!(
            hmac_sha1_hex(sign_key.as_bytes(), "sha1\nx\n"),
            cos_signature(COS_SK, COS_SIGN_TIME, "sha1\nx\n")
        );
    }

    /// 地域归一：编码、控制台全称、城市简称、endpoint 与桶域名都收；认不出的不猜。
    #[test]
    fn cos_region_normalization_covers_spellings_without_guessing() {
        for input in ["ap-nanjing", "  AP-Nanjing ", "南京", "公有云园区-中国-南京", "中国-南京"] {
            assert_eq!(normalize_cos_region(input), "ap-nanjing", "{input}");
        }
        for input in [
            "cos.ap-nanjing.myqcloud.com",
            "https://cos.ap-nanjing.myqcloud.com",
            "markd-1253518225.cos.ap-nanjing.myqcloud.com",
            "markd-1253518225.cos.ap-nanjing.myqcloud.com/",
        ] {
            assert_eq!(normalize_cos_region(input), "ap-nanjing", "{input}");
        }
        // 名字里带「一」「金融」「（美西）」的必须走长的那条，别被短城市名抢走。
        assert_eq!(normalize_cos_region("北京一区（已售罄）"), "ap-beijing-1");
        // 不带括号说明的写法只能靠城市表兜住，所以这条单独测：表里的全称匹配不上它。
        assert_eq!(normalize_cos_region("北京一区"), "ap-beijing-1");
        assert_eq!(normalize_cos_region("上海金融"), "ap-shanghai-fsi");
        assert_eq!(normalize_cos_region("深圳金融"), "ap-shenzhen-fsi");
        assert_eq!(normalize_cos_region("硅谷"), "na-siliconvalley");
        assert_eq!(normalize_cos_region("硅谷（美西）"), "na-siliconvalley");
        assert_eq!(normalize_cos_region("弗吉尼亚"), "na-ashburn");
        assert_eq!(normalize_cos_region("中国香港"), "ap-hongkong");
        assert_eq!(normalize_cos_region("上海"), "ap-shanghai");
        assert_eq!(normalize_cos_region("北京"), "ap-beijing");
        assert_eq!(normalize_cos_region(""), "");
        // 认不出的原样返回，由 check_cos_region 报错，不在这里瞎猜。
        assert_eq!(normalize_cos_region("火星"), "火星");
        assert_eq!(normalize_cos_region("腾讯云"), "腾讯云");
    }

    /// 校验：表里的与形如 `ap-xxx` 的新地域放行；中文乱写报错并列出全表；加速域名单独提示。
    #[test]
    fn cos_check_region_passes_shaped_ids_and_lists_named_ones() {
        assert!(check_cos_region("ap-nanjing").is_ok());
        assert!(check_cos_region("me-saudi-arabia").is_ok());
        // 腾讯云还在开新区：没在表里但形状对的放行。
        assert!(check_cos_region("ap-nowhere-9").is_ok());
        let err = check_cos_region("火星").unwrap_err();
        assert!(err.contains("ap-nanjing"), "{err}");
        assert!(err.contains("南京（ap-nanjing）"), "要带中文对照：{err}");
        assert!(err.contains("法兰克福（eu-frankfurt）"), "全表都要列出来：{err}");
        assert!(check_cos_region("").is_err());
        let acc = check_cos_region("accelerate").unwrap_err();
        assert!(acc.contains("全球加速"), "{acc}");
        // 中文没被归一时不能因为「含字母」就放行。
        assert!(check_cos_region("ap-南京").is_err());
    }

    /// 主机名、公网地址与签名头同源；自定义前缀优先。
    #[test]
    fn cos_host_and_public_url_follow_bucket_and_region() {
        let c = cos_cfg();
        assert_eq!(cos_host(&c), COS_HOST);
        assert_eq!(
            cos_public_url(&c, "markd/kat/a.png"),
            "https://markd-1253518225.cos.ap-nanjing.myqcloud.com/markd/kat/a.png"
        );
        let mut cdn = cos_cfg();
        cdn.public_base = "cdn.example.com/".into();
        assert_eq!(cos_public_url(&cdn, "a.png"), "https://cdn.example.com/a.png");
        // 断言走 cos_format_str（真正签名的那条）：对象名原样、头值编码。
        let key = "markd/kat/测试 图#1.png";
        let (hdr, list) = cos_signed_headers(COS_HOST, "image/png");
        assert_eq!(
            cos_format_str(key, &hdr),
            format!("put\n/{key}\n\n{hdr}\n")
        );
        assert_eq!(list, "content-type;host");
        assert!(hdr.contains("image%2Fpng"), "头值里的 / 要编码：{hdr}");
        assert!(!hdr.contains("测试"), "签名头里没有中文，对象名才有得对照");
        // 桶名多粘斜杠时，主机名与格式串都得剥掉，只剥一边就签不上。
        let mut sloppy = cos_cfg();
        sloppy.cos_bucket = " /markd-1253518225/ ".into();
        assert_eq!(cos_host(&sloppy), COS_HOST);
    }

    /// 错误正文是 XML（下面几条 Code/Message 都是实测抓回来的原样）。
    #[test]
    fn cos_error_translates_the_measured_codes() {
        let sig = "<Error><Code>SignatureDoesNotMatch</Code><Message>The Signature you specified is invalid.</Message></Error>";
        let msg = cos_error(403, sig);
        assert!(msg.contains("HTTP 403"), "{msg}");
        assert!(msg.contains("SecretKey 不对"), "{msg}");
        assert!(msg.contains("The Signature you specified is invalid"), "腾讯云原话要带出来：{msg}");
        let ak = "<Error><Code>InvalidAccessKeyId</Code><Message>The Access Key Id you provided does not exist in our records</Message></Error>";
        assert!(cos_error(403, ak).contains("SecretId"), "{ak}");
        let exp = "<Error><Code>AccessDenied</Code><Message>Request has expired</Message></Error>";
        let m = cos_error(403, exp);
        assert!(m.contains("签名已过期"), "{m}");
        assert!(m.contains("Request has expired"), "{m}");
        let deny = "<Error><Code>AccessDenied</Code><Message>Access Denied.</Message></Error>";
        assert!(cos_error(403, deny).contains("写权限"), "{deny}");
        let nb = "<Error><Code>NoSuchBucket</Code><Message>The specified bucket does not exist.</Message></Error>";
        assert!(cos_error(404, nb).contains("桶不存在"), "{nb}");
        // 实测原文里带 XML 实体，展示前要还原成 <bucketname>-<appid>。
        let irp = "<Error><Code>InvalidRequest</Code><Message>Missing required header for this request: Appid, Bucket format should be &lt;bucketname&gt;-&lt;appid&gt;</Message></Error>";
        let m2 = cos_error(400, irp);
        assert!(m2.contains("名称-APPID"), "{m2}");
        assert!(m2.contains("<bucketname>-<appid>"), "实体要还原：{m2}");
        assert!(!m2.contains("&lt;"), "不该把转义符丢给用户：{m2}");
        assert!(cos_error(503, "<Error><Code>SlowDown</Code></Error>").contains("稍后重试"));
        let odd = cos_error(500, "<Error><Code>InternalError</Code><Message>boom</Message></Error>");
        assert!(odd.contains("InternalError"), "{odd}");
        let html = cos_error(502, &format!("<html>{}</html>", "x".repeat(500)));
        assert!(html.contains("<html>"), "{html}");
        assert!(html.chars().count() < 400, "正文没截断：{}", html.chars().count());
        // 二次转义 &amp;lt; 只能还原一层。
        assert_eq!(xml_unescape("&amp;lt;"), "&lt;");
    }

    /// 缺项提示要点名到底缺哪一项。
    #[test]
    fn cos_validate_names_the_missing_field() {
        let c = cos_cfg();
        c.validate().expect("配齐了就该通过");
        let mut no_bucket = cos_cfg();
        no_bucket.cos_bucket = String::new();
        assert!(no_bucket.validate().unwrap_err().contains("存储桶名"));
        let mut bad_region = cos_cfg();
        bad_region.cos_region = "火星".into();
        assert!(bad_region.validate().unwrap_err().contains("ap-nanjing"));
        let mut no_key = cos_cfg();
        no_key.cos_secret_key = String::new();
        assert!(no_key.validate().unwrap_err().contains("SecretId"));
        let mut no_id = cos_cfg();
        no_id.cos_secret_id = String::new();
        assert!(no_id.validate().unwrap_err().contains("SecretKey"));
    }

    /// 新平台是独立一档：下拉九个选项、配置里的平台取值是 `tencent`。
    #[test]
    fn cos_kind_is_a_platform_of_its_own() {
        assert_eq!(ImageHostKind::ALL.len(), 9);
        assert!(ImageHostKind::ALL.contains(&ImageHostKind::Tencent));
        assert_eq!(ImageHostKind::Tencent.label(), "腾讯云 COS");
        assert_eq!(ImageHostKind::Tencent.platform_name(), "腾讯云");
        assert!(ImageHostKind::Tencent.is_enabled());
        assert_eq!(
            serde_json::to_string(&ImageHostKind::Tencent).unwrap(),
            "\"tencent\""
        );
        // 从 Config 过来时地域已经归一，且不需要令牌那三项（仓库平台用的）。
        let mut cfg0 = crate::workspace::Config::default();
        cfg0.image_host = ImageHostKind::Tencent;
        cfg0.image_host_cos_region = "公有云园区-中国-南京".into();
        cfg0.image_host_cos_bucket = "markd-1253518225".into();
        cfg0.image_host_cos_secret_id = "AKIDX".into();
        cfg0.image_host_cos_secret_key = "SKX".into();
        let h = ImageHostConfig::from_config(&cfg0);
        assert_eq!(h.cos_region, "ap-nanjing");
        assert_eq!(cos_host(&h), "markd-1253518225.cos.ap-nanjing.myqcloud.com");
        h.validate().unwrap();
    }

    /// 保存路径模板、公网地址与格式串三处接得上。
    #[test]
    fn cos_path_and_url_follow_the_template() {
        let mut c = cos_cfg();
        c.path_pattern = "markd/{year}/{month}/{day}/{hour}_{random}_{filename}{.suffix}".into();
        let t = fixed_time();
        let name = remote_name_for(&c, "截图 Shot.PNG");
        assert_eq!(name, "Shot.png");
        let path = object_path(&c, &name, &t);
        // 期望值用 join 拼出来：写成字面量时这串日期样式的文本会被显示层改写。
        let want = ["markd", "2026", "11", "17", "09_a1b2c3_Shot.png"].join("/");
        assert_eq!(path, want);
        assert_eq!(
            cos_public_url(&c, &path),
            format!("https://{COS_HOST}/{want}")
        );
        let (hdr, _) = cos_signed_headers(COS_HOST, "image/png");
        // 格式串里的对象名是原样的路径。
        assert_eq!(
            cos_format_str(&path, &hdr),
            format!("put\n/{want}\n\n{hdr}\n")
        );
    }
}
