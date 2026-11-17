//! Author: aurelianus <aurelianus@126.com>

//! 插入图片：把图片放进文档旁边的资源目录，并生成 Markdown 图片链接。
//!
//! 默认把图片复制到 `<文档目录>/<资源目录>/`（资源目录默认 `imgs`），
//! 文档里写相对路径（如 `imgs/a.png`）；未保存的文档回退到当前工作目录。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_ASSETS: &str = "imgs";

/// 文档所在目录；未保存的文档（没有路径）回退到当前工作目录。
pub fn doc_dir(doc_path: Option<&Path>) -> PathBuf {
    doc_path
        .and_then(|p| p.parent())
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
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
pub fn from_file(
    src: &Path,
    doc_path: Option<&Path>,
    copy: bool,
    assets: &str,
    relative: bool,
) -> std::io::Result<String> {
    let base = doc_dir(doc_path);
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "image.png".to_string());
    let (link, alt) = if copy {
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
        (link, stem(&dest_name))
    } else {
        let link = if relative {
            relative_to(&base, src)
        } else {
            src.to_string_lossy().to_string()
        };
        (link, stem(&name))
    };
    Ok(markdown(&alt, &link))
}

/// 把剪贴板图片（RGBA）存进文档旁的资源目录，返回 Markdown 图片链接。
/// 剪贴板图片没有原始文件，所以总是落盘。
pub fn from_clipboard(
    size: [usize; 2],
    pixels: &[egui::Color32],
    doc_path: Option<&Path>,
    assets: &str,
    relative: bool,
) -> std::io::Result<String> {
    let base = doc_dir(doc_path);
    let assets = assets_name(assets);
    let dir = base.join(assets);
    std::fs::create_dir_all(&dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let (dest, name) = unique_path(&dir, &format!("image-{stamp}.png"));
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
    rgba.save(&dest)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let link = if relative {
        format!("{}/{}", assets.replace('\\', "/"), name)
    } else {
        dest.to_string_lossy().to_string()
    };
    Ok(markdown(&stem(&name), &link))
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

/// Markdown 的链接里空格/括号要转义，否则解析器会把地址截断。
fn escape_link(link: &str) -> String {
    link.replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
}

/// 计算 `target` 相对 `base` 的路径（写文档里用相对链接）。
fn relative_to(base: &Path, target: &Path) -> String {
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

        let md = from_file(&src, Some(&doc), true, "imgs", true).unwrap();
        assert_eq!(md, "![a](imgs/a.png)");
        assert_eq!(
            std::fs::read(root.join("imgs/a.png")).unwrap(),
            b"fake",
            "图片应被复制到资源目录"
        );

        // 同名文件不覆盖，自动改名。
        let md = from_file(&src, Some(&doc), true, "imgs", true).unwrap();
        assert_eq!(md, "![a-1](imgs/a-1.png)");
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

        let md = from_file(&src, Some(&doc), false, "imgs", true).unwrap();
        assert_eq!(md, "![b c](pics/b%20c.jpg)");
    }

    /// 剪贴板图片落盘成 png，并返回相对链接。
    #[test]
    fn clipboard_image_is_saved_as_png() {
        let root = temp_dir("paste");
        let doc = root.join("note.md");
        std::fs::write(&doc, "").unwrap();
        let pixels = vec![egui::Color32::from_rgb(255, 0, 0); 4];

        let md = from_clipboard([2, 2], &pixels, Some(&doc), "imgs", true).unwrap();
        assert!(md.starts_with("![image-"), "{md}");
        assert!(
            md.contains("](imgs/image-") && md.ends_with(".png)"),
            "{md}"
        );
        let file = md
            .split("](")
            .nth(1)
            .unwrap()
            .trim_end_matches(')')
            .to_string();
        let bytes = std::fs::read(root.join(&file)).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "应写成合法 PNG");
    }
}
