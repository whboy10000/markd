//! Author: aurelianus <aurelianus@126.com>

//! 插入图片：把图片放进文档旁边的资源目录，并生成 Markdown 图片链接。
//!
//! 默认把图片复制到 `<文档目录>/<资源目录>/`（资源目录默认 `imgs`），
//! 文档里写相对路径（如 `imgs/a.png`）；未保存的文档回退到当前工作目录。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_ASSETS: &str = "imgs";

/// 剪贴板图片文件名的进程内自增序号：避免同一毫秒内多次粘贴产生同名文件。
static PASTE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 插入结果：Markdown 链接 + 本地文件位置（上传图床时读这里的字节）。
pub struct InsertedImage {
    /// 可直接插入文档的 `![alt](url)`。
    pub markdown: String,
    /// 图片替代文字（文件名去扩展名）。
    pub alt: String,
    /// 文档里实际写入的 URL（已做 Markdown 转义）；上传完成后用它做替换。
    pub url: String,
    /// 本地落盘位置；引用原文件时就是原文件。用于后台上传图床。
    pub path: Option<PathBuf>,
}

/// 未保存文档 / 不可写目录的回退位置：`~/.markd/assets`。
///
/// 不能用 `current_dir()`：从 Finder 启动 `.app` 时工作目录是 `/`（只读），
/// 往那里复制图片或写剪贴板图片都会报「Read-only file system」。
fn fallback_dir() -> PathBuf {
    crate::workspace::Config::dir().join("assets")
}

/// 文档是否有可用的所在目录（未命名文档没有）。
fn has_doc_dir(doc_path: Option<&Path>) -> bool {
    doc_path
        .and_then(|p| p.parent())
        .is_some_and(|p| !p.as_os_str().is_empty())
}

/// 文档所在目录；未保存的文档回退到 `~/.markd/assets`。
pub fn doc_dir(doc_path: Option<&Path>) -> PathBuf {
    doc_path
        .and_then(|p| p.parent())
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(fallback_dir)
}

/// 资源目录名：设置为空时回退到默认的 `imgs`。
fn assets_name(dir: &str) -> &str {
    let d = dir.trim();
    if d.is_empty() { DEFAULT_ASSETS } else { d }
}

/// 插入磁盘上的图片文件，返回可直接插进文档的 Markdown（`![alt](url)`）。
///
/// - `copy = true`：复制到文档旁的资源目录，链接指向复制后的文件；
/// - `copy = false`：不复制，直接引用原文件。
///
/// 文档目录不可写（只读磁盘、权限不足）时自动回退到 `~/.markd/assets`，
/// 并用绝对路径引用，保证插图/上传不会失败。
pub fn from_file(
    src: &Path,
    doc_path: Option<&Path>,
    copy: bool,
    assets: &str,
    relative: bool,
) -> std::io::Result<InsertedImage> {
    let has_doc = has_doc_dir(doc_path);
    let relative = relative && has_doc;
    match file_into(src, &doc_dir(doc_path), copy, assets, relative) {
        Ok(v) => Ok(v),
        Err(e) if has_doc => file_into(src, &fallback_dir(), copy, assets, false).map_err(|_| e),
        Err(e) => Err(e),
    }
}

fn file_into(
    src: &Path,
    base: &Path,
    copy: bool,
    assets: &str,
    relative: bool,
) -> std::io::Result<InsertedImage> {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "image.png".to_string());
    let (link, alt, path) = if copy {
        let assets = assets_name(assets);
        let dir = base.join(assets);
        std::fs::create_dir_all(&dir)?;
        let (dest, dest_name) = unique_path(&dir, &name);
        std::fs::copy(src, &dest)?;
        let link = if relative {
            format!("{}/{}", assets.replace('\\', "/"), dest_name)
        } else {
            dest.to_string_lossy().to_string()
        };
        (link, stem(&dest_name), Some(dest))
    } else {
        let link = if relative {
            relative_to(base, src)
        } else {
            src.to_string_lossy().to_string()
        };
        (link, stem(&name), Some(src.to_path_buf()))
    };
    Ok(inserted(alt, link, path))
}

/// 把剪贴板图片（RGBA）存进文档旁的资源目录，返回 Markdown 图片链接。
/// 剪贴板图片没有原始文件，所以总是落盘；目录不可写时回退到用户目录。
pub fn from_clipboard(
    size: [usize; 2],
    pixels: &[egui::Color32],
    doc_path: Option<&Path>,
    assets: &str,
    relative: bool,
) -> std::io::Result<InsertedImage> {
    let (w, h) = (size[0], size[1]);
    let mut buf = Vec::with_capacity(w * h * 4);
    for p in pixels {
        buf.extend_from_slice(&[p.r(), p.g(), p.b(), p.a()]);
    }
    let rgba = image::RgbaImage::from_raw(w as u32, h as u32, buf).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "剪贴板图片尺寸与像素数不一致",
        )
    })?;
    let has_doc = has_doc_dir(doc_path);
    let relative = relative && has_doc;
    match clipboard_into(&rgba, &doc_dir(doc_path), assets, relative) {
        Ok(v) => Ok(v),
        Err(e) if has_doc => clipboard_into(&rgba, &fallback_dir(), assets, false).map_err(|_| e),
        Err(e) => Err(e),
    }
}

fn clipboard_into(
    rgba: &image::RgbaImage,
    base: &Path,
    assets: &str,
    relative: bool,
) -> std::io::Result<InsertedImage> {
    let assets = assets_name(assets);
    let dir = base.join(assets);
    std::fs::create_dir_all(&dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    // 同一毫秒内多次粘贴时文件名会撞车，unique_path 的「先查后写」在并发下
    // 也可能同时选中同一个名字；补一个进程内自增序号，保证每次都唯一。
    let seq = PASTE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (dest, name) = unique_path(&dir, &format!("image-{stamp}-{seq}.png"));
    rgba.save(&dest)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let link = if relative {
        format!("{}/{}", assets.replace('\\', "/"), name)
    } else {
        dest.to_string_lossy().to_string()
    };
    Ok(inserted(stem(&name), link, Some(dest)))
}

/// 资源目录里已有同名文件时追加 `-1`、`-2`…，不覆盖用户已有图片。
fn unique_path(dir: &Path, name: &str) -> (PathBuf, String) {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return (candidate, name.to_string());
    }
    let p = Path::new(name);
    let base = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());
    let ext = p.extension().map(|e| e.to_string_lossy().to_string());
    for n in 1..10_000 {
        let name = match &ext {
            Some(e) => format!("{base}-{n}.{e}"),
            None => format!("{base}-{n}"),
        };
        let q = dir.join(&name);
        if !q.exists() {
            return (q, name);
        }
    }
    (candidate, name.to_string())
}

fn stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string())
}

fn markdown(alt: &str, link: &str) -> String {
    let alt = alt.replace('[', "").replace(']', "");
    format!("![{}]({})", alt, escape_link(link))
}

/// 组装插入结果：markdown 里的 URL 与 `url` 字段保持同一份转义结果，
/// 图床上传完成后可以直接按字符串替换。
fn inserted(alt: String, link: String, path: Option<PathBuf>) -> InsertedImage {
    let url = escape_link(&link);
    InsertedImage {
        markdown: markdown(&alt, &link),
        alt,
        url,
        path,
    }
}

/// Markdown 的链接里空格/括号要转义，否则解析器会把地址截断。
fn escape_link(link: &str) -> String {
    link.replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
}

/// 计算 `target` 相对 `base` 的路径（写文档里用相对链接）。
/// 附件插入本地文件时同样用它，保证资源相对文档存放。
pub(crate) fn relative_to(base: &Path, target: &Path) -> String {
    let base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let target = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());
    let mut b = base.components().peekable();
    let mut t = target.components().peekable();
    while b.peek().is_some() && b.peek() == t.peek() {
        b.next();
        t.next();
    }
    let mut out = PathBuf::new();
    for _ in b {
        out.push("..");
    }
    for c in t {
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        ".".to_string()
    } else {
        out.to_string_lossy().replace('\\', "/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("markd-img-{tag}-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 插入磁盘图片：复制到 `<文档目录>/imgs/`，Markdown 用相对路径。
    #[test]
    fn copies_file_into_imgs_and_links_relatively() {
        let root = temp_dir("copy");
        let doc = root.join("note.md");
        std::fs::write(&doc, "").unwrap();
        let src = root.join("a.png");
        std::fs::write(&src, b"fake").unwrap();

        let img = from_file(&src, Some(&doc), true, "imgs", true).unwrap();
        assert_eq!(img.markdown, "![a](imgs/a.png)");
        assert_eq!(img.url, "imgs/a.png");
        assert!(
            img.path
                .as_deref()
                .is_some_and(|p| p.ends_with("imgs/a.png"))
        );
        assert_eq!(
            std::fs::read(root.join("imgs/a.png")).unwrap(),
            b"fake",
            "图片应被复制到资源目录"
        );

        // 同名文件不覆盖，自动改名。
        let img = from_file(&src, Some(&doc), true, "imgs", true).unwrap();
        assert_eq!(img.markdown, "![a-1](imgs/a-1.png)");
        assert!(root.join("imgs/a-1.png").is_file());
    }

    /// 不复制时直接引用原文件的相对路径。
    #[test]
    fn links_original_file_when_not_copying() {
        let root = temp_dir("link");
        let doc = root.join("note.md");
        std::fs::write(&doc, "").unwrap();
        let src = root.join("pics/b c.jpg");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"x").unwrap();

        let img = from_file(&src, Some(&doc), false, "imgs", true).unwrap();
        assert_eq!(img.markdown, "![b c](pics/b%20c.jpg)");
        assert_eq!(img.url, "pics/b%20c.jpg");
    }

    /// 剪贴板图片落盘成 png，并返回相对链接。
    #[test]
    fn clipboard_image_is_saved_as_png() {
        let root = temp_dir("paste");
        let doc = root.join("note.md");
        std::fs::write(&doc, "").unwrap();
        let pixels = vec![egui::Color32::from_rgb(255, 0, 0); 4];

        let img = from_clipboard([2, 2], &pixels, Some(&doc), "imgs", true).unwrap();
        assert!(img.markdown.starts_with("![image-"), "{}", img.markdown);
        assert!(
            img.markdown.contains("](imgs/image-") && img.markdown.ends_with(".png)"),
            "{}",
            img.markdown
        );
        let file = img
            .markdown
            .split("](")
            .nth(1)
            .unwrap()
            .trim_end_matches(')')
            .to_string();
        let bytes = std::fs::read(root.join(&file)).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "应写成合法 PNG");
    }

    /// 未保存文档：图片落到 `~/.markd/assets`，链接用绝对路径。
    /// 回归：以前回退到 current_dir，Finder 启动的 .app 工作目录是只读的 `/`。
    #[test]
    fn unsaved_doc_uses_home_fallback() {
        let dir = doc_dir(None);
        assert!(
            dir.starts_with(crate::workspace::Config::dir()),
            "未保存文档应回退到用户目录：{dir:?}"
        );
        let pixels = vec![egui::Color32::from_rgb(1, 2, 3); 4];
        let img = from_clipboard([2, 2], &pixels, None, "imgs", true).unwrap();
        assert!(
            Path::new(&img.url).is_absolute(),
            "未保存文档应写绝对路径：{}",
            img.url
        );
        let path = img.path.clone().unwrap();
        assert!(path.is_file(), "图片应真的落盘：{path:?}");
        let _ = std::fs::remove_file(&path);
    }

    /// 文档目录不可写（只读磁盘/权限不足）时，自动回退到用户目录。
    #[cfg(unix)]
    #[test]
    fn readonly_doc_dir_falls_back_to_home() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_dir("readonly");
        let doc = root.join("note.md");
        std::fs::write(&doc, "").unwrap();
        let mut perms = std::fs::metadata(&root).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&root, perms).unwrap();

        let pixels = vec![egui::Color32::from_rgb(9, 9, 9); 4];
        let img = from_clipboard([2, 2], &pixels, Some(&doc), "imgs", true);
        // 恢复权限，保证后续清理能做。
        let mut perms = std::fs::metadata(&root).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&root, perms).unwrap();

        let img = img.expect("只读目录不应导致插图失败");
        assert!(
            Path::new(&img.url).is_absolute(),
            "应回退为绝对路径：{}",
            img.url
        );
        assert!(
            img.path
                .clone()
                .unwrap()
                .starts_with(crate::workspace::Config::dir()),
            "应写到用户目录：{:?}",
            img.path
        );
        let _ = std::fs::remove_file(img.path.unwrap());
        let _ = std::fs::remove_dir_all(root);
    }
}
