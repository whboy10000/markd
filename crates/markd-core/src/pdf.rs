//! PDF 导出后端：把 Markdown 渲染成自包含 HTML，再用本机浏览器无头打印成 PDF。
//!
//! pandoc 的 PDF 输出需要额外安装 LaTeX 引擎；浏览器几乎总是现成的，
//! 而且能复用 Markd 自己的 HTML 导出样式。这里只做进程构造、临时文件与结果校验，
//! 不碰 UI；调用方必须放到后台线程。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// 主动取消时不该被当成失败提示给用户。
pub const CANCELLED: &str = "转换已取消";

/// 一次 PDF 打印所需的全部输入；所有字符串在进后台线程前就已克隆好。
#[derive(Clone, Debug)]
pub struct PdfRequest {
    pub source: String,
    pub title: String,
    pub target: PathBuf,
    /// 文档所在目录。临时 HTML 里注入 `<base>`，让相对图片/附件仍找得到。
    pub base_dir: Option<PathBuf>,
    /// 浏览器可执行文件路径；由 `resolve_engine` 得到。
    pub engine: String,
}

/// 在 PATH 里找一个可执行文件。
fn from_path_env(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// 决定用哪个浏览器：用户设置 > 常见安装位置 > PATH。
/// 找不到时返回 None，由调用方给出可操作的提示。
pub fn resolve_engine(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        return Some(PathBuf::from(configured));
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        for p in [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ] {
            candidates.push(PathBuf::from(p));
        }
    }
    #[cfg(target_os = "windows")]
    {
        for key in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            let Some(root) = std::env::var_os(key) else {
                continue;
            };
            let root = PathBuf::from(root);
            candidates.push(root.join("Google/Chrome/Application/chrome.exe"));
            candidates.push(root.join("Microsoft/Edge/Application/msedge.exe"));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .or_else(|| from_path_env("google-chrome"))
        .or_else(|| from_path_env("chrome"))
        .or_else(|| from_path_env("chromium"))
        .or_else(|| from_path_env("chromium-browser"))
        .or_else(|| from_path_env("microsoft-edge"))
        .or_else(|| from_path_env("msedge"))
}

/// 给临时 HTML 注入 `<base>`：临时文件在系统临时目录里，文档里的
/// `imgs/a.png` 之类相对路径必须仍按原文档目录解析。
pub fn with_base(html: &str, base_dir: Option<&Path>) -> String {
    let Some(dir) = base_dir else {
        return html.to_string();
    };
    if dir.as_os_str().is_empty() {
        return html.to_string();
    }
    let absolute = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut href = crate::attachment::file_href(&absolute.to_string_lossy());
    if !href.ends_with('/') {
        href.push('/');
    }
    html.replacen("<head>", &format!("<head><base href=\"{href}\"/>"), 1)
}

/// 给没有目录的文档补一个纸面目录：`[TOC]` 会由 `export::to_html`
/// 渲染成 `<nav class="toc">`，列出全文各级标题。只改临时导出用的副本，
/// 不动用户文档；文档里已经写过 `[TOC]`（大小写均可）时不重复注入。
pub fn inject_toc(source: &str) -> String {
    let has_toc = crate::ast::parse(source)
        .iter()
        .any(|b| matches!(b, crate::ast::Block::Toc { .. }));
    if has_toc {
        return source.to_string();
    }
    format!("[TOC]\n\n{source}")
}

/// 命令构造单独拆出来：本机没装浏览器也能对参数做断言。
fn build_command(req: &PdfRequest, html: &Path) -> Command {
    let profile = html
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("profile");
    let mut cmd = Command::new(&req.engine);
    cmd.arg("--headless")
        .arg("--disable-gpu")
        // Linux CI/容器里没有 sandbox 权限；桌面端加上也无害。
        .arg("--no-sandbox")
        .arg("--no-pdf-header-footer")
        // Chromium 用 HTML 标题层级生成 PDF 侧边栏书签（/Outlines）。
        // 老版本不认识这个开关会直接忽略，不会导致打印失败。
        .arg("--generate-pdf-document-outline")
        .arg(format!("--print-to-pdf={}", req.target.display()))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(crate::attachment::file_href(&html.to_string_lossy()));
    cmd
}

/// 截断到字符边界，避免把浏览器的一整屏报错糊到状态栏上。
fn truncate(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let mut end = s.len().min(max_chars * 4);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// 真正打印。临时 HTML 与浏览器 profile 放在系统临时目录，结束后清理。
/// 目标文件由浏览器自己写（二进制），stdout 只做诊断。
pub fn convert(req: PdfRequest, is_cancelled: impl Fn() -> bool) -> Result<PathBuf, String> {
    if is_cancelled() {
        return Err(CANCELLED.to_string());
    }
    if req.engine.trim().is_empty() {
        return Err(
            "找不到 Chrome / Edge / Chromium：请安装其一，或在「偏好设置 → 导出」里指定浏览器路径。"
                .to_string(),
        );
    }
    let tmp = std::env::temp_dir().join(format!(
        "markd-pdf-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("创建临时导出目录失败：{e}"))?;
    let html_path = tmp.join("document.html");
    let source = inject_toc(&req.source);
    let html = crate::export::to_html(&req.title, &source, false);
    let html = with_base(&html, req.base_dir.as_deref());
    let write_result = std::fs::write(&html_path, html);
    if let Err(e) = write_result {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("写入临时 HTML 失败：{e}"));
    }

    let output = build_command(&req, &html_path).output();
    // 无论成功失败都先清掉临时目录，别把用户机器的 /tmp 塞满。
    let _ = std::fs::remove_dir_all(&tmp);

    if is_cancelled() {
        return Err(CANCELLED.to_string());
    }
    let out = output.map_err(|e| {
        format!(
            "启动浏览器失败（{}）：{e}。请确认路径正确，或在「偏好设置 → 导出」里重新指定。",
            req.engine
        )
    })?;
    if !out.status.success() {
        let detail = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "浏览器打印失败（退出码 {:?}）：{}",
            out.status.code(),
            truncate(detail.trim(), 600)
        ));
    }
    if !req.target.is_file() {
        return Err(format!(
            "浏览器报告成功，但没生成 PDF：{}。请换一种格式或检查目标目录权限。",
            req.target.display()
        ));
    }
    let head = std::fs::read(&req.target).map_err(|e| format!("读取生成的 PDF 失败：{e}"))?;
    if head.len() < 5 || &head[..5] != b"%PDF-" {
        return Err("生成的文件不是合法 PDF，请重试或换一种导出方式。".to_string());
    }
    Ok(req.target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_base_keeps_html_when_no_document_dir() {
        let html = "<html><head><meta charset=\"utf-8\"/></head><body>x</body></html>";
        assert_eq!(with_base(html, None), html);
    }

    #[test]
    fn with_base_injects_absolute_file_url() {
        let html = "<html><head><meta></head><body>x</body></html>";
        let out = with_base(html, Some(Path::new("/tmp/我的 文档")));
        assert!(
            out.contains("<head><base href=\"file:///tmp/"),
            "应注入 file 基地址：{out}"
        );
        assert!(out.contains("%E6%88%91%E7%9A%84%20%E6%96%87%E6%A1%A3/"));
        assert!(out.matches("<head>").count() == 1);
    }

    #[test]
    fn build_command_uses_headless_print_flags() {
        let req = PdfRequest {
            source: "x".to_string(),
            title: "T".to_string(),
            target: PathBuf::from("/tmp/out.pdf"),
            base_dir: None,
            engine: "/usr/bin/fake-chrome".to_string(),
        };
        let cmd = build_command(&req, Path::new("/tmp/markd-pdf-1/document.html"));
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.iter().any(|a| a == "--headless"));
        assert!(args.iter().any(|a| a == "--no-pdf-header-footer"));
        assert!(args.iter().any(|a| a == "--generate-pdf-document-outline"));
        assert!(args.iter().any(|a| a == "--print-to-pdf=/tmp/out.pdf"));
        assert!(
            args.iter()
                .any(|a| a == "file:///tmp/markd-pdf-1/document.html")
        );
    }

    #[test]
    fn configured_engine_wins() {
        let p = resolve_engine("/opt/my-browser");
        assert_eq!(p, Some(PathBuf::from("/opt/my-browser")));
    }

    #[test]
    fn inject_toc_prepends_marker_and_renders_nav() {
        let src = "# 一\n\n## 二\n\n正文";
        let injected = inject_toc(src);
        assert!(injected.starts_with("[TOC]\n\n"));
        let html = crate::export::to_html("T", &injected, false);
        assert!(
            html.contains("<nav class=\"toc\">"),
            "应生成纸面目录：{html}"
        );
        assert!(html.contains("<h1"), "原标题应保留");
    }

    #[test]
    fn inject_toc_does_not_duplicate_existing_marker() {
        for marker in ["[TOC]", "[toc]"] {
            let src = format!("# 一\n\n{marker}\n\n正文");
            assert_eq!(inject_toc(&src), src, "{marker} 已存在时不应重复注入");
        }
    }

    /// 手工验证用：本机有 Chrome/Edge 时跑一次真实打印。
    /// 默认 ignore，避免 CI 上没有浏览器或沙箱限制导致假失败。
    #[test]
    #[ignore = "需要本机安装 Chrome / Edge / Chromium"]
    fn convert_with_real_engine_writes_pdf() {
        let Some(engine) = resolve_engine("") else {
            return;
        };
        let target = std::env::temp_dir().join(format!(
            "markd-pdf-real-{}-{}.pdf",
            std::process::id(),
            unique_suffix()
        ));
        let req = PdfRequest {
            source: "# 中文标题\n\n正文 One two three.\n\n```rust\nfn main() {}\n```".to_string(),
            title: "PDF 测试".to_string(),
            target: target.clone(),
            base_dir: None,
            engine: engine.to_string_lossy().to_string(),
        };
        let out = convert(req, || false).expect("真实浏览器打印应成功");
        assert_eq!(out, target);
        let bytes = std::fs::read(&target).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(
            bytes.windows(9).any(|w| w == b"/Outlines"),
            "应该生成 PDF 侧边栏书签大纲"
        );
        let _ = std::fs::remove_file(&target);
    }
}
