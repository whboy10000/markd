//! 文件附件：把指向 PDF / Word / Excel 等文档的链接认成可预览、可下载的文件。
//!
//! 附件在文档里就是一个普通 Markdown 链接 `[季度报告.xlsx](files/季度报告.xlsx)`，
//! 不引入自定义语法：其它渲染器看到的是链接，本编辑器按后缀把它画成附件卡片。
//! 判定与命名全是纯字符串工作（渲染每帧都要跑），只有真正下载时才碰网络。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// 附件大类：决定卡片徽标上的文字与配色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pdf,
    Word,
    /// 电子表格（含 CSV/TSV）。
    Sheet,
    /// 演示文稿。
    Slide,
    /// 电子书。
    Ebook,
    /// 压缩包与镜像。
    Archive,
    /// 纯文本类文档。
    Doc,
    /// 结构化数据文件。
    Data,
    Audio,
    Video,
    Font,
}

impl Kind {
    /// 徽标文字：卡片与导出 HTML 的类型标签共用。
    pub fn label(self) -> &'static str {
        match self {
            Kind::Pdf => "PDF",
            Kind::Word => "WORD",
            Kind::Sheet => "表格",
            Kind::Slide => "演示",
            Kind::Ebook => "电子书",
            Kind::Archive => "压缩包",
            Kind::Doc => "文本",
            Kind::Data => "数据",
            Kind::Audio => "音频",
            Kind::Video => "视频",
            Kind::Font => "字体",
        }
    }

    /// 类型说明，卡片副标题用。
    pub fn describe(self) -> &'static str {
        match self {
            Kind::Pdf => "PDF 文档",
            Kind::Word => "Word 文档",
            Kind::Sheet => "电子表格",
            Kind::Slide => "演示文稿",
            Kind::Ebook => "电子书",
            Kind::Archive => "压缩包",
            Kind::Doc => "文本文档",
            Kind::Data => "数据文件",
            Kind::Audio => "音频文件",
            Kind::Video => "视频文件",
            Kind::Font => "字体文件",
        }
    }
}

/// 扩展名 → 类型。图片（png/jpg/…）不在表内：它们由图片预览负责；
/// `md` 也不在：应用内点开应该是切标签页读文档，而不是下载附件。
fn kind_of_ext(ext: &str) -> Option<Kind> {
    let k = match ext {
        "pdf" => Kind::Pdf,
        "doc" | "docx" | "docm" | "dotx" | "rtf" | "wps" => Kind::Word,
        "xls" | "xlsx" | "xlsm" | "xlsb" | "csv" | "tsv" | "et" => Kind::Sheet,
        "ppt" | "pptx" | "pptm" | "pps" | "ppsx" | "key" => Kind::Slide,
        "epub" | "mobi" | "azw" | "azw3" | "djvu" | "cbz" | "cbr" => Kind::Ebook,
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "iso" => {
            Kind::Archive
        }
        "txt" | "log" | "nfo" | "srt" | "lrc" => Kind::Doc,
        "json" | "jsonl" | "xml" | "yaml" | "yml" | "toml" | "ini" | "sql" | "parquet"
        | "db" | "sqlite" => Kind::Data,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus" | "mid" | "midi" => Kind::Audio,
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" | "wmv" | "m4v" => Kind::Video,
        "ttf" | "otf" | "woff" | "woff2" | "eot" => Kind::Font,
        _ => return None,
    };
    Some(k)
}

/// 链接指向文件的扩展名（小写、不含点），认不出返回 `None`。
///
/// 先剥掉 `?query` 与 `#fragment` 再取最后一段路径，所以
/// `https://x.com/a/b.pdf?v=2` 认得出、`https://x.com/only/path` 认不出。
/// 只接受 1–8 位 ASCII 字母数字，且点号前必须有名字（`.pdf` 是隐藏文件名）。
pub fn ext_of(url: &str) -> Option<String> {
    let s = url.trim();
    let path = s.split(['?', '#']).next().unwrap_or(s);
    let last = path.rsplit(['/', '\\']).next().unwrap_or("");
    let (stem, ext) = last.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() || ext.len() > 8 {
        return None;
    }
    if !ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// 这个链接是不是附件：扩展名落在 [`Kind`] 表里才算。
pub fn is_attachment(url: &str) -> bool {
    ext_of(url).and_then(|e| kind_of_ext(&e)).is_some()
}

/// 链接指向的附件类型。
pub fn kind_of(url: &str) -> Option<Kind> {
    ext_of(url).and_then(|e| kind_of_ext(&e))
}

/// 百分号解码：`%20` → 空格，`%E4%B8%AD` → 中文字符。
///
/// 非法转义（`%zz`、截断的 `%E4`）原样保留；解出的字节不是合法 UTF-8
/// （GBK 文件名）时整串退回原文——宁可显示原始地址，也不给出乱码。
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hi = (b[i + 1] as char).to_digit(16);
            let lo = (b[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// 从链接推出显示名：最后一段路径去掉 `?…`/`#…` 后百分号解码。
///
/// 认不出名字（目录链接、根路径）返回空串，由调用方决定兜底文案。
pub fn name_of(url: &str) -> String {
    let s = url.trim();
    let path = s.split(['?', '#']).next().unwrap_or(s);
    let last = path.rsplit(['/', '\\']).next().unwrap_or("");
    percent_decode(last).trim().to_string()
}

/// Markdown 链接里的空格/括号会截断地址，`#` 会被当成锚点，一律百分号编码。
fn escape_url(url: &str) -> String {
    url.replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('#', "%23")
}

/// 生成插入用的 Markdown：`[文件名](地址)`。名字里的方括号必须去掉，否则解析直接错位。
pub fn attachment_link(name: &str, url: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| *c != '[' && *c != ']' && *c != '\n' && *c != '\r')
        .collect();
    let name = name.trim();
    let label = if name.is_empty() { "文件" } else { name };
    format!("[{label}]({})", escape_url(url.trim()))
}

/// 本地附件的真实路径：先把 `%20` 之类还原再拼文档目录。
///
/// `workspace::resolve` 按原样拼路径，而写进文档的地址是转义过的，
/// 不解码就找不到「年度报告 2026.pdf」这类文件。
pub fn local_path(base: Option<&Path>, url: &str) -> Option<PathBuf> {
    crate::workspace::resolve(base, &percent_decode(url.trim()))
}

/// 字节数说成人能读的话：`940 B` / `1.5 MB` / `2.0 GB`。
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut i = 1usize;
    while v >= 1024.0 && i + 1 < UNITS.len() {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", UNITS[i])
}

/// 下载目录：`~/.markd/attachments`。不塞进文档目录，免得污染工作区。
pub fn download_dir() -> PathBuf {
    crate::workspace::Config::dir().join("attachments")
}

/// FNV-1a（64 位）：只用乘法和异或，跨 Rust 版本结果恒定。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 远程附件的缓存子目录名：地址的 FNV-1a 哈希，16 位十六进制。
///
/// 每个地址占一个目录，而不是把文件名直接摊平：`a.com/x/report.pdf` 与
/// `b.com/y/report.pdf` 同名，摊平后第二个会冒认第一个已下载，给出错的预览。
/// 不用 `DefaultHasher` 是怕它跨版本变算法，升级工具链后把用户的下载全当成没下过。
pub fn cache_key(url: &str) -> String {
    format!("{:016x}", fnv1a64(url.trim().as_bytes()))
}

/// 下载落盘的文件名：去掉路径分隔与控制字符，保留中文名与扩展名。
///
/// 目录穿越（`../../etc/passwd`）去掉分隔符后只剩一串点，所以顺手吃掉开头的点；
/// 全被吃空（域名根路径、`.hidden` 去掉点后为空）时补 `file` 前缀兜底。
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && !matches!(*c, '/' | '\\' | ':'))
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        return "file".to_string();
    }
    trimmed.chars().take(180).collect()
}

/// 本地绝对路径转 `file://` 地址：导出 HTML 后点附件仍能打开本机文件。
///
/// 只保留 `/ : - . _ ~ @ +` 与字母数字，其余（空格、中文）按 UTF-8 百分号编码。
pub fn file_href(path: &str) -> String {
    let mut p = path.replace('\\', "/");
    if !p.starts_with('/') {
        // Windows 盘符 `C:/x` 要补一个前导斜杠才是合法的 file 地址。
        p.insert(0, '/');
    }
    let mut out = String::from("file://");
    for b in p.as_bytes() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(b, b'/' | b':' | b'-' | b'.' | b'_' | b'~' | b'@' | b'+');
        if keep {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 附件地址 → 导出 HTML 用的 `href`：远程原样，本地绝对路径转 `file://`，
/// 相对路径保持相对（导出的 HTML 就写在文档旁边）。
///
/// 文档里写的是转义过的地址，转 `file://` 前要先解码，否则 `%20` 会被二次编码成
/// `%2520`；相对路径不用动，浏览器解析 `href` 时自己会解 `%20`。
pub fn export_href(url: &str) -> String {
    let u = url.trim();
    if crate::image_host::is_remote(u) || u.contains("://") {
        u.to_string()
    } else if Path::new(u).is_absolute() {
        file_href(&percent_decode(u))
    } else {
        u.to_string()
    }
}

/// 远程附件落到本地后的默认路径：`~/.markd/attachments/<地址哈希>/<净化文件名>`。
///
/// 渲染层靠它判断「这个附件已下载过」，app 层靠它决定往哪写，两边共用同一个函数
/// 才不会各算各的。真正落盘时同名会加 `-1` 后缀，但那是同地址重复下载，
/// 默认路径此时必然已存在，所以用它判存在不会漏报。
pub fn cached_path(url: &str) -> PathBuf {
    download_dir()
        .join(cache_key(url))
        .join(safe_name(&name_of(url)))
}

/// 一次下载的上限：512 MiB。超过算失败，不把磁盘写满。
const MAX_DOWNLOAD_BYTES: u64 = 512 << 20;

/// 把远程附件下到本地文件，返回写出的字节数。
///
/// 先写同目录的 `.part` 再改名：连接中途断掉只留下一个删掉的半成品，
/// 不会留下一个「看着已下载、实际打不开」的截断文件——渲染层就是按这个路径判断
/// 附件是否已下好的，宁可重下一遍，也不能给错的预览。超过上限同样按失败处理。
pub fn download(url: &str, dest: &Path) -> std::io::Result<u64> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .into();
    let mut resp = agent
        .get(url)
        .header("User-Agent", "markd/1.0 (attachment preview)")
        .call()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(std::io::Error::other(format!(
            "服务器返回 {}",
            resp.status()
        )));
    }
    let dir = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = dest
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let tmp = dir.join(format!("{name}.part"));
    let mut reader = resp.body_mut().as_reader();
    let done = write_partial(&mut reader, &tmp).and_then(|n| std::fs::rename(&tmp, dest).map(|_| n));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// 边收边写临时文件：只管写，半成品统一由 [`download`] 在失败时删掉。
/// 上限按已收字节算，超了就中止，不把用户的盘写满。
fn write_partial<R: Read>(reader: &mut R, tmp: &Path) -> std::io::Result<u64> {
    let mut file = std::fs::File::create(tmp)?;
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_DOWNLOAD_BYTES {
            return Err(std::io::Error::other(format!(
                "文件超过 {}",
                size_text(MAX_DOWNLOAD_BYTES)
            )));
        }
        file.write_all(&buf[..n])?;
    }
    file.flush()?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用户点名的几类常见文档必须认得，且类型判对。
    #[test]
    fn recognizes_common_document_types() {
        let cases = [
            ("https://x.com/a/report.pdf", Kind::Pdf),
            ("https://x.com/a/report.docx", Kind::Word),
            ("https://x.com/a/report.doc", Kind::Word),
            ("https://x.com/a/book.xls", Kind::Sheet),
            ("https://x.com/a/book.xlsx", Kind::Sheet),
            ("https://x.com/a/deck.pptx", Kind::Slide),
            ("https://x.com/a/archive.zip", Kind::Archive),
            ("notes/季度报告.csv", Kind::Sheet),
        ];
        for (url, want) in cases {
            assert_eq!(kind_of(url), Some(want), "{url}");
            assert!(is_attachment(url), "{url} 应是附件");
        }
    }

    /// 没有扩展名、图片、网页、Markdown 文档链接都不是附件：
    /// 图片走图片预览，md 走应用内打开，不能抢它们的显示方式。
    #[test]
    fn rejects_non_attachments() {
        for url in [
            "https://www.rust-lang.org/",
            "https://x.com/a.png",
            "https://x.com/a.jpg?v=2",
            "https://x.com/page.html",
            "notes/其它文档.md",
            "https://x.com/a.notatype",
            "https://x.com/a.pdf!",
            "mailto:someone@x.com",
            "",
            "   ",
            "https://x.com/dir/",
        ] {
            assert!(!is_attachment(url), "{url:?} 不应被认成附件");
        }
    }

    /// 查询串与锚点不属于文件名：`a.pdf?v=2#p3` 仍是 PDF。
    #[test]
    fn strips_query_and_fragment() {
        assert_eq!(kind_of("https://x.com/a.pdf?v=2#p=3"), Some(Kind::Pdf));
        assert_eq!(name_of("https://x.com/a.pdf?v=2#p=3"), "a.pdf");
        // 双重扩展名取最后一段：tar.gz 归到压缩包
        assert_eq!(kind_of("https://x.com/pkg.tar.gz"), Some(Kind::Archive));
        // 隐藏文件（点开头）不是附件
        assert_eq!(ext_of("https://x.com/.pdf"), None);
        // 无扩展名的路径段
        assert_eq!(ext_of("https://x.com/only/path"), None);
    }

    /// 中文名与空格要能进文档：地址写回时百分号编码，显示名解码还原。
    #[test]
    fn decodes_percent_in_names() {
        assert_eq!(
            name_of("https://x.com/files/%E5%B9%B4%E6%8A%A5%202026.pdf"),
            "年报 2026.pdf"
        );
        // 非法转义原样保留，不 panic 也不吞字符
        assert_eq!(name_of("https://x.com/100%zz.pdf"), "100%zz.pdf");
        assert_eq!(name_of("https://x.com/bad%E4.pdf"), "bad%E4.pdf");
    }

    /// 生成的 Markdown 往返：写进去的链接能被同一套解析读回同一类型。
    #[test]
    fn link_round_trips() {
        let md = attachment_link("季度 报告 (最终).xlsx", "files/季度 报告 (最终).xlsx");
        assert_eq!(
            md,
            "[季度 报告 (最终).xlsx](files/季度%20报告%20%28最终%29.xlsx)"
        );
        let blocks = crate::ast::parse(&md);
        let crate::ast::Block::Paragraph { content, .. } = &blocks[0] else {
            panic!("应解析为段落：{blocks:?}");
        };
        let crate::ast::Inline::Link { label, url } = &content[0] else {
            panic!("应解析为链接：{content:?}");
        };
        // 解析器不还原转义，扩展名仍在尾部，所以类型判得对
        assert_eq!(kind_of(url), Some(Kind::Sheet));
        assert!(is_attachment(url));
        // 显示名要解码才能把空格找回来
        assert_eq!(name_of(url), "季度 报告 (最终).xlsx");
        assert_eq!(crate::ast::inline_plain(label), "季度 报告 (最终).xlsx");
        // 本地路径解析前先解码，否则带空格的文件打不开
        let p = local_path(Some(Path::new("/doc")), url).expect("本地附件应有路径");
        assert_eq!(p, PathBuf::from("/doc/files/季度 报告 (最终).xlsx"));
    }

    /// 名字里的方括号会破坏链接语法，写回前必须去掉；空名退回「文件」。
    #[test]
    fn sanitizes_label() {
        assert_eq!(attachment_link("a[b]c.pdf", "x/a.pdf"), "[abc.pdf](x/a.pdf)");
        assert_eq!(attachment_link("  ", "x/a.pdf"), "[文件](x/a.pdf)");
        // `#` 在地址里会被当成锚点，编码掉
        assert_eq!(attachment_link("a.pdf", "x/a#1.pdf"), "[a.pdf](x/a%231.pdf)");
    }

    /// 大小文案：边界与进位都得是定值。
    #[test]
    fn size_text_is_exact() {
        assert_eq!(size_text(0), "0 B");
        assert_eq!(size_text(940), "940 B");
        assert_eq!(size_text(1023), "1023 B");
        assert_eq!(size_text(1024), "1.0 KB");
        assert_eq!(size_text(1536), "1.5 KB");
        assert_eq!(size_text(1024 * 1024), "1.0 MB");
        assert_eq!(size_text(5 * 1024 * 1024 + 1024 * 512), "5.5 MB");
        assert_eq!(size_text(3 * 1024 * 1024 * 1024), "3.0 GB");
        // GB 是最高档，不再向上进位
        assert_eq!(size_text(1024 * 1024 * 1024 * 1024), "1024.0 GB");
    }

    /// 下载文件名净化：目录穿越、控制字符、空名都不能写飞。
    #[test]
    fn safe_name_strips_path_separators() {
        assert_eq!(safe_name("a.pdf"), "a.pdf");
        assert_eq!(safe_name("../../etc/passwd"), "etcpasswd");
        assert_eq!(safe_name("a\\b.pdf"), "ab.pdf");
        assert_eq!(safe_name("  年报 2026.docx  "), "年报 2026.docx");
        assert_eq!(safe_name(".hidden"), "hidden");
        assert_eq!(safe_name(".."), "file");
        assert_eq!(safe_name(""), "file");
    }

    /// `file://` 地址：空格与中文要百分号编码，盘符要补前导斜杠。
    #[test]
    fn file_href_quotes_path() {
        assert_eq!(
            file_href("/Users/me/年度报告 2026.pdf"),
            "file:///Users/me/%E5%B9%B4%E5%BA%A6%E6%8A%A5%E5%91%8A%202026.pdf"
        );
        assert_eq!(file_href("C:/tmp/a.pdf"), "file:///C:/tmp/a.pdf");
        assert_eq!(export_href("/tmp/a.pdf"), "file:///tmp/a.pdf");
        assert_eq!(
            export_href("https://x.com/a.pdf"),
            "https://x.com/a.pdf",
            "远程地址原样导出"
        );
        assert_eq!(export_href("files/a.pdf"), "files/a.pdf");
        // 文档里已转义的绝对路径不能二次编码（%20 不能变成 %2520）
        assert_eq!(
            export_href("/tmp/年度%20报告.pdf"),
            "file:///tmp/%E5%B9%B4%E5%BA%A6%20%E6%8A%A5%E5%91%8A.pdf"
        );
    }

    /// 哈希必须对上 FNV-1a 官方测试向量（定值），且不同地址不同键、同名不同源不撞车。
    #[test]
    fn cache_key_matches_fnv_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
        // 官方向量走公开接口也要原样出来：`cache_key` 就是它的 16 位十六进制
        assert_eq!(cache_key("a"), "af63dc4c8601ec8c");
        assert_eq!(cache_key("https://x.com/a.pdf").len(), 16);
        assert_ne!(
            cache_key("https://a.com/x/report.pdf"),
            cache_key("https://b.com/y/report.pdf"),
            "同名不同源的附件必须落在不同子目录"
        );
        // 首尾空白不算地址的一部分
        assert_eq!(cache_key(" https://x.com/a.pdf "), cache_key("https://x.com/a.pdf"));
    }

    /// 远程附件的默认落盘路径是纯函数（渲染层与下载层靠它对齐「是否已下载」）：
    /// 下载目录 + 地址哈希子目录 + 解码净化后的文件名，三段都得是定值。
    #[test]
    fn cached_path_is_url_scoped() {
        assert_eq!(
            cached_path("https://x.com/a/b/%E5%B9%B4.pdf?e=1"),
            download_dir()
                .join("e1e05239f2b54267")
                .join("年.pdf"),
        );
        // 同名不同源 → 不同目录，第二个不会冒认第一个已下载
        assert_ne!(
            cached_path("https://a.com/x/report.pdf"),
            cached_path("https://b.com/y/report.pdf")
        );
        // 文件名里被百分号编码的冒号（Windows 非法字符）进不了本地路径
        let nasty = cached_path("https://x.com/a%3Ab.pdf");
        assert_eq!(
            nasty,
            download_dir().join("6153560000bf7da2").join("ab.pdf"),
        );
        // 无论地址怎么写，落点都在下载目录下的「哈希/文件名」两层里
        let rel = nasty.strip_prefix(download_dir()).unwrap();
        assert_eq!(rel.components().count(), 2, "{nasty:?}");
    }

    /// 下载失败要报错而不是 panic，并且不许留下半成品：
    /// 渲染层按 `dest` 是否存在判断「已下载」，留个截断文件就等于给错的预览。
    #[test]
    fn download_reports_errors_not_panics() {
        assert!(download_dir().ends_with("attachments"));
        let dir = std::env::temp_dir().join(format!(
            "markd-att-dl-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("a.pdf");
        let e = download("http://127.0.0.1:9/a.pdf", &dest).unwrap_err();
        assert!(!e.to_string().is_empty(), "错误信息不能是空串");
        assert!(!dest.exists(), "失败不能留下目标文件");
        assert!(!dir.join("a.pdf.part").exists(), "失败不能留下 .part 半成品");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 落盘环节单独验：写出的字节数与文件内容都得和源数据逐字对上（少写一段
    /// 尾部就会让预览打开一半就报错）。
    #[test]
    fn write_partial_writes_every_byte() {
        let dir = std::env::temp_dir().join(format!(
            "markd-att-part-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let tmp = dir.join("a.pdf.part");
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let n = write_partial(&mut data.as_slice(), &tmp).unwrap();
        assert_eq!(n, data.len() as u64);
        assert_eq!(std::fs::read(&tmp).unwrap(), data);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
