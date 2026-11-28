//! Pandoc 导出后端：把 Markdown 交给外部 pandoc 进程转成各种格式。
//!
//! 这里只做进程构造、字节搬运和结果校验，不碰 UI。调用方必须把它放到后台线程：
//! pandoc 是外部进程，同步调用会把界面冻住直到它退出。
//! 取消由调用方传入的闭包控制，每个关键节点检查一次，避免旧任务覆盖新结果。
//!
//! 输出一律走 `-o <target>` 让 pandoc 自己写盘：docx/odt/epub/pdf 都是二进制，
//! 不能塞进 String；chunkedhtml 还会产出 zip。stdout 只做诊断，不承载内容。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 主动取消时不该被当成失败提示给用户。
pub const CANCELLED: &str = "转换已取消";

/// 输出格式分组，UI 按它把下拉项分开。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatGroup {
    Documents,
    Presentation,
    Web,
    Ebook,
    Markup,
    Academic,
    Data,
    Wiki,
}

impl FormatGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Documents => "文字处理",
            Self::Presentation => "演示文稿",
            Self::Web => "网页",
            Self::Ebook => "电子书",
            Self::Markup => "标记语言",
            Self::Academic => "学术与排版",
            Self::Data => "数据与 AST",
            Self::Wiki => "Wiki",
        }
    }
}

/// 一种 pandoc 输出格式。表是静态的，UI 直接遍历即可。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportFormat {
    /// pandoc `--to` 的值。
    pub id: &'static str,
    /// 中文显示名。
    pub label: &'static str,
    /// 目标文件扩展名（不含点）。
    pub ext: &'static str,
    /// 是否需要 `--standalone`：纯片段格式（参考文献列表、AST）不要。
    pub standalone: bool,
    pub group: FormatGroup,
    /// 需要额外说明时在 UI 上展示（PDF 引擎、幻灯片资源等）。
    pub hint: &'static str,
}

/// 来自 pandoc 官方 MANUAL 的 writer 列表（只收真实存在的输出格式）。
/// 注意：BibTeX/BibLaTeX/CSL JSON 在这里是「参考文献列表」的输出；
/// RIS、EndNote XML、CSV/TSV、TikiWiki/TWiki/Creole 只有 reader，不能导出。
pub const FORMATS: &[ExportFormat] = &[
    // 文字处理
    ExportFormat {
        id: "docx",
        label: "Microsoft Word (.docx)",
        ext: "docx",
        standalone: false,
        group: FormatGroup::Documents,
        hint: "",
    },
    ExportFormat {
        id: "odt",
        label: "OpenDocument Text (.odt)",
        ext: "odt",
        standalone: false,
        group: FormatGroup::Documents,
        hint: "",
    },
    ExportFormat {
        id: "fodt",
        label: "Flat ODT (.fodt)",
        ext: "fodt",
        standalone: false,
        group: FormatGroup::Documents,
        hint: "",
    },
    ExportFormat {
        id: "rtf",
        label: "富文本 (.rtf)",
        ext: "rtf",
        standalone: false,
        group: FormatGroup::Documents,
        hint: "",
    },
    // 演示文稿
    ExportFormat {
        id: "pptx",
        label: "PowerPoint (.pptx)",
        ext: "pptx",
        standalone: false,
        group: FormatGroup::Presentation,
        hint: "",
    },
    ExportFormat {
        id: "beamer",
        label: "LaTeX Beamer (.tex)",
        ext: "tex",
        standalone: true,
        group: FormatGroup::Presentation,
        hint: "",
    },
    ExportFormat {
        id: "revealjs",
        label: "reveal.js (.html)",
        ext: "html",
        standalone: true,
        group: FormatGroup::Presentation,
        hint: "主题资源需自行部署，离线播放可能缺样式。",
    },
    ExportFormat {
        id: "slidy",
        label: "Slidy (.html)",
        ext: "html",
        standalone: true,
        group: FormatGroup::Presentation,
        hint: "依赖在线资源。",
    },
    ExportFormat {
        id: "slideous",
        label: "Slideous (.html)",
        ext: "html",
        standalone: true,
        group: FormatGroup::Presentation,
        hint: "需把 slideous 资源放在输出目录旁。",
    },
    ExportFormat {
        id: "s5",
        label: "S5 (.html)",
        ext: "html",
        standalone: true,
        group: FormatGroup::Presentation,
        hint: "",
    },
    ExportFormat {
        id: "dzslides",
        label: "DZSlides (.html)",
        ext: "html",
        standalone: true,
        group: FormatGroup::Presentation,
        hint: "",
    },
    // PDF
    ExportFormat {
        id: "pdf",
        label: "PDF（需外部引擎）",
        ext: "pdf",
        standalone: false,
        group: FormatGroup::Documents,
        hint: "PDF 由 LaTeX/Typst 等引擎渲染，内置 Pandoc 不含这些引擎；失败时请在「偏好设置 → 导出」里指定 --pdf-engine。",
    },
    // 网页
    ExportFormat {
        id: "html5",
        label: "HTML5",
        ext: "html",
        standalone: true,
        group: FormatGroup::Web,
        hint: "",
    },
    ExportFormat {
        id: "html4",
        label: "HTML4 / XHTML 1.0",
        ext: "html",
        standalone: true,
        group: FormatGroup::Web,
        hint: "",
    },
    ExportFormat {
        id: "chunkedhtml",
        label: "分块 HTML (.zip)",
        ext: "zip",
        standalone: true,
        group: FormatGroup::Web,
        hint: "产出多页站点压缩包，解压后打开 index.html。",
    },
    // 电子书
    ExportFormat {
        id: "epub3",
        label: "EPUB v3",
        ext: "epub",
        standalone: false,
        group: FormatGroup::Ebook,
        hint: "",
    },
    ExportFormat {
        id: "epub2",
        label: "EPUB v2",
        ext: "epub",
        standalone: false,
        group: FormatGroup::Ebook,
        hint: "",
    },
    ExportFormat {
        id: "fb2",
        label: "FictionBook2 (.fb2)",
        ext: "fb2",
        standalone: false,
        group: FormatGroup::Ebook,
        hint: "",
    },
    // Markdown 家族
    ExportFormat {
        id: "markdown",
        label: "Pandoc Markdown (.md)",
        ext: "md",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "gfm",
        label: "GitHub-Flavored Markdown (.md)",
        ext: "md",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "commonmark",
        label: "CommonMark (.md)",
        ext: "md",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "commonmark_x",
        label: "CommonMark 扩展 (.md)",
        ext: "md",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "markdown_mmd",
        label: "MultiMarkdown (.md)",
        ext: "md",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "markdown_phpextra",
        label: "PHP Markdown Extra (.md)",
        ext: "md",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "markdown_strict",
        label: "原始 Markdown (.md)",
        ext: "md",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "djot",
        label: "Djot (.djot)",
        ext: "djot",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "markua",
        label: "Markua (.md)",
        ext: "md",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    // 其他标记语言
    ExportFormat {
        id: "rst",
        label: "reStructuredText (.rst)",
        ext: "rst",
        standalone: true,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "asciidoc",
        label: "AsciiDoc (.adoc)",
        ext: "adoc",
        standalone: true,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "org",
        label: "Emacs Org-Mode (.org)",
        ext: "org",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "muse",
        label: "Emacs Muse (.muse)",
        ext: "muse",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "textile",
        label: "Textile (.textile)",
        ext: "textile",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "t2t",
        label: "txt2tags (.t2t)",
        ext: "t2t",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    ExportFormat {
        id: "plain",
        label: "纯文本 (.txt)",
        ext: "txt",
        standalone: false,
        group: FormatGroup::Markup,
        hint: "",
    },
    // 学术与排版
    ExportFormat {
        id: "latex",
        label: "LaTeX (.tex)",
        ext: "tex",
        standalone: true,
        group: FormatGroup::Academic,
        hint: "",
    },
    ExportFormat {
        id: "context",
        label: "ConTeXt (.tex)",
        ext: "tex",
        standalone: true,
        group: FormatGroup::Academic,
        hint: "",
    },
    ExportFormat {
        id: "typst",
        label: "Typst (.typ)",
        ext: "typ",
        standalone: true,
        group: FormatGroup::Academic,
        hint: "",
    },
    ExportFormat {
        id: "docbook4",
        label: "DocBook 4 (.xml)",
        ext: "xml",
        standalone: true,
        group: FormatGroup::Academic,
        hint: "",
    },
    ExportFormat {
        id: "docbook5",
        label: "DocBook 5 (.xml)",
        ext: "xml",
        standalone: true,
        group: FormatGroup::Academic,
        hint: "",
    },
    ExportFormat {
        id: "jats",
        label: "JATS XML (.xml)",
        ext: "xml",
        standalone: true,
        group: FormatGroup::Academic,
        hint: "",
    },
    ExportFormat {
        id: "tei",
        label: "TEI Simple (.xml)",
        ext: "xml",
        standalone: true,
        group: FormatGroup::Academic,
        hint: "",
    },
    ExportFormat {
        id: "opendocument",
        label: "OpenDocument XML (.fodt)",
        ext: "fodt",
        standalone: true,
        group: FormatGroup::Academic,
        hint: "",
    },
    ExportFormat {
        id: "opml",
        label: "OPML 大纲 (.opml)",
        ext: "opml",
        standalone: true,
        group: FormatGroup::Academic,
        hint: "",
    },
    // 参考文献列表（作为输出时是「文献条目」而非正文）
    ExportFormat {
        id: "bibtex",
        label: "BibTeX 文献列表 (.bib)",
        ext: "bib",
        standalone: false,
        group: FormatGroup::Academic,
        hint: "输出文档里的参考文献条目，需要文中有引用及 --bibliography。",
    },
    ExportFormat {
        id: "biblatex",
        label: "BibLaTeX 文献列表 (.bib)",
        ext: "bib",
        standalone: false,
        group: FormatGroup::Academic,
        hint: "同 BibTeX，输出的是文献条目。",
    },
    ExportFormat {
        id: "csljson",
        label: "CSL JSON 文献列表 (.json)",
        ext: "json",
        standalone: false,
        group: FormatGroup::Academic,
        hint: "同上，输出 JSON 格式的文献条目。",
    },
    // 数据与 AST
    ExportFormat {
        id: "ipynb",
        label: "Jupyter Notebook (.ipynb)",
        ext: "ipynb",
        standalone: false,
        group: FormatGroup::Data,
        hint: "",
    },
    ExportFormat {
        id: "json",
        label: "Pandoc AST (.json)",
        ext: "json",
        standalone: false,
        group: FormatGroup::Data,
        hint: "抽象语法树，便于程序化处理。",
    },
    ExportFormat {
        id: "native",
        label: "Pandoc 原生 AST (.hs)",
        ext: "hs",
        standalone: false,
        group: FormatGroup::Data,
        hint: "Haskell 表现形式，一般只用于调试。",
    },
    ExportFormat {
        id: "xml",
        label: "AST 的 XML 表示 (.xml)",
        ext: "xml",
        standalone: false,
        group: FormatGroup::Data,
        hint: "",
    },
    // Wiki
    ExportFormat {
        id: "mediawiki",
        label: "MediaWiki (.wiki)",
        ext: "wiki",
        standalone: false,
        group: FormatGroup::Wiki,
        hint: "",
    },
    ExportFormat {
        id: "dokuwiki",
        label: "DokuWiki (.txt)",
        ext: "txt",
        standalone: false,
        group: FormatGroup::Wiki,
        hint: "",
    },
    ExportFormat {
        id: "jira",
        label: "Jira / Confluence (.txt)",
        ext: "txt",
        standalone: false,
        group: FormatGroup::Wiki,
        hint: "",
    },
    ExportFormat {
        id: "vimwiki",
        label: "Vimwiki (.wiki)",
        ext: "wiki",
        standalone: false,
        group: FormatGroup::Wiki,
        hint: "",
    },
    ExportFormat {
        id: "xwiki",
        label: "XWiki (.txt)",
        ext: "txt",
        standalone: false,
        group: FormatGroup::Wiki,
        hint: "",
    },
    ExportFormat {
        id: "zimwiki",
        label: "ZimWiki (.txt)",
        ext: "txt",
        standalone: false,
        group: FormatGroup::Wiki,
        hint: "",
    },
];

pub fn format_by_id(id: &str) -> Option<&'static ExportFormat> {
    FORMATS.iter().find(|f| f.id == id)
}

/// 一次转换所需的全部输入。所有字符串在进后台线程前就已克隆好。
#[derive(Clone, Debug)]
pub struct PandocRequest {
    pub source: String,
    pub target: PathBuf,
    /// 可执行文件：内置绝对路径、用户指定路径，或 PATH 里的名字。
    pub executable: String,
    /// 选定的输出格式（静态表项，Copy）。
    pub format: ExportFormat,
    /// 从设置里透传的额外参数（如 `--pdf-engine=xelatex`）；空行已过滤。
    pub extra_args: Vec<String>,
}

/// 按目标格式生成落盘路径；与原 `export_html` 的规则一致，只换扩展名。
pub fn output_path(path: Option<&Path>, format: &ExportFormat) -> std::io::Result<PathBuf> {
    let base = match path {
        Some(p) => p.with_extension(format.ext),
        None => std::env::current_dir()?.join(format!("untitled.{}", format.ext)),
    };
    Ok(base)
}

/// 查找随应用分发的 pandoc 副本。
///
/// 优先级由调用方决定：`resolve_executable` 先看用户设置，再看这里。
/// 候选路径覆盖三种发行布局：
/// - macOS .app: `Contents/MacOS/markd` → `Contents/Resources/vendor/pandoc`
/// - Windows: exe 同目录 `vendor/pandoc.exe`
/// - Linux 包: `/usr/share/markd/vendor/pandoc`
/// 以及仓库里直接 `cargo run` 时的 `vendor/pandoc`。
pub fn bundled_executable() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let exe_name = if cfg!(windows) {
        "pandoc.exe"
    } else {
        "pandoc"
    };
    let mut candidates = vec![dir.join("vendor").join(exe_name)];
    #[cfg(target_os = "macos")]
    candidates.push(dir.join("../Resources/vendor/pandoc"));
    #[cfg(target_os = "linux")]
    {
        candidates.push(PathBuf::from("/usr/share/markd/vendor/pandoc"));
        candidates.push(dir.join("../share/markd/vendor/pandoc"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// 决定实际用哪个 pandoc：用户设置 > 内置副本 > PATH。
pub fn resolve_executable(configured: &str) -> String {
    let configured = configured.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    if let Some(p) = bundled_executable() {
        return p.to_string_lossy().to_string();
    }
    "pandoc".to_string()
}

/// 命令构造单独拆出来：本机没装 pandoc 也能对参数做断言。
fn build_command(req: &PandocRequest) -> Command {
    let mut cmd = Command::new(&req.executable);
    cmd.arg("--from=markdown")
        .arg(format!("--to={}", req.format.id));
    if req.format.standalone {
        cmd.arg("--standalone");
    }
    // 二进制格式（docx/odt/epub/pdf/pptx）必须 -o，不能靠 stdout 捕获。
    cmd.arg("-o").arg(&req.target);
    for arg in &req.extra_args {
        cmd.arg(arg);
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// 截断到字符边界，避免把 pandoc 的一整屏报错糊到状态栏上。
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

/// 执行一次转换。此函数阻塞，调用方必须放在后台线程。
///
/// `is_cancelled` 在启动前、写完后各检查一次：中途被新任务取代时直接杀掉
/// 子进程，不回传结果。目标文件由 pandoc 写（可能是二进制或 zip），
/// 若 pandoc 退出码为 0 但没生成文件，也按失败处理。
pub fn convert(req: PandocRequest, is_cancelled: impl Fn() -> bool) -> Result<PathBuf, String> {
    if is_cancelled() {
        return Err(CANCELLED.to_string());
    }
    if let Some(parent) = req.target.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建导出目录失败：{e}"))?;
        }
    }

    let mut child = build_command(&req).spawn().map_err(|e| {
        format!(
            "启动 pandoc 失败（{}）：{e}。请确认安装包自带 Pandoc、系统已安装，或在「偏好设置 → 导出」里指定可执行文件路径。",
            req.executable
        )
    })?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "无法写入 pandoc 标准输入".to_string())?;
    // 写 stdin 单独起线程：wait_with_output 会并发读 stdout/stderr。
    // 若在主线程先写完 stdin 再读 stdout，遇到「边读边写」的进程就会双向
    // 管道死锁。用户可以在设置里换成任意脚本，不能赌它先读完再输出。
    let source = req.source;
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(source.as_bytes());
        // 闭包结束 drop stdin，送出 EOF。
    });

    if is_cancelled() {
        let _ = child.kill();
        let _ = child.wait();
        let _ = writer.join();
        return Err(CANCELLED.to_string());
    }

    let out = child
        .wait_with_output()
        .map_err(|e| format!("等待 pandoc 结束失败：{e}"))?;
    let _ = writer.join();
    if is_cancelled() {
        return Err(CANCELLED.to_string());
    }

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let detail = stderr.trim();
        let mut msg = if detail.is_empty() {
            format!("pandoc 退出码：{:?}", out.status.code())
        } else {
            format!("pandoc 失败：{}", truncate(detail, 600))
        };
        if !req.format.hint.is_empty() {
            msg.push('\n');
            msg.push_str(req.format.hint);
        }
        return Err(msg);
    }
    if !req.target.exists() {
        return Err(format!(
            "pandoc 报告成功，但没生成目标文件：{}。可尝试换一种格式。",
            req.target.display()
        ));
    }
    Ok(req.target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn request(executable: &str) -> PandocRequest {
        PandocRequest {
            source: "# 标题\n".to_string(),
            target: PathBuf::from("/tmp/markd-pandoc-test.html"),
            executable: executable.to_string(),
            format: *format_by_id("html5").unwrap(),
            extra_args: Vec::new(),
        }
    }

    #[test]
    fn output_path_uses_format_extension() {
        let html = format_by_id("html5").unwrap();
        assert_eq!(
            output_path(Some(Path::new("/tmp/note.md")), html).unwrap(),
            PathBuf::from("/tmp/note.html")
        );
        let docx = format_by_id("docx").unwrap();
        assert_eq!(
            output_path(Some(Path::new("/tmp/note.md")), docx).unwrap(),
            PathBuf::from("/tmp/note.docx")
        );
        assert_eq!(
            output_path(None, html).unwrap(),
            std::env::current_dir().unwrap().join("untitled.html")
        );
    }

    #[test]
    fn command_uses_markdown_to_html() {
        let cmd = build_command(&request("pandoc-custom"));
        assert_eq!(cmd.get_program(), OsStr::new("pandoc-custom"));
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert!(args.contains(&OsStr::new("--from=markdown")));
        assert!(args.contains(&OsStr::new("--to=html5")));
        assert!(args.contains(&OsStr::new("--standalone")));
        assert!(args.contains(&OsStr::new("-o")));
    }

    #[test]
    fn binary_formats_do_not_use_standalone() {
        for id in ["docx", "odt", "pptx", "epub3", "pdf"] {
            let mut req = request("pandoc-custom");
            req.format = *format_by_id(id).unwrap();
            let cmd = build_command(&req);
            let args: Vec<&OsStr> = cmd.get_args().collect();
            assert!(
                !args.contains(&OsStr::new("--standalone")),
                "{id} 不该带 standalone"
            );
            assert!(args.contains(&OsStr::new("-o")), "{id} 必须用 -o 写二进制");
        }
    }

    #[test]
    fn extra_args_are_passed_through() {
        let mut req = request("pandoc-custom");
        req.extra_args.push("--pdf-engine=xelatex".to_string());
        let cmd = build_command(&req);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert!(args.contains(&OsStr::new("--pdf-engine=xelatex")));
    }

    #[test]
    fn format_table_has_no_duplicate_ids() {
        let mut ids: Vec<&str> = FORMATS.iter().map(|f| f.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "格式表和 --to id 不能重复");
    }

    #[test]
    fn cancellation_is_checked_before_spawn() {
        let err = convert(request("definitely-not-a-real-pandoc-xyz"), || true).unwrap_err();
        assert_eq!(err, CANCELLED);
    }

    #[test]
    fn missing_pandoc_reports_actionable_error() {
        let err = convert(request("definitely-not-a-real-pandoc-xyz"), || false).unwrap_err();
        assert!(err.contains("启动 pandoc 失败"));
        assert!(err.contains("偏好设置"));
    }

    /// 大文档 + 边读边写的假 pandoc：验证两条管道不会互相等死。
    /// 主线程若先写完 stdin 再读 stdout，这个测试会挂住。
    #[cfg(unix)]
    #[test]
    fn large_document_does_not_deadlock_on_pipes() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("markd-pandoc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("fake-pandoc");
        // 假 pandoc：读完 stdin，写目标文件，顺手往 stdout 喷一大堆。
        std::fs::write(
            &script,
            "#!/bin/sh\nout=\"\"\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"-o\" ]; then out=\"$2\"; shift 2; else shift; fi\ndone\ncat > \"$out\"\nyes x | head -c 2000000\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let source = "# 标题\n".repeat(200_000); // 约 2.4 MB，远超管道缓冲
        let target = dir.join("out.html");
        let req = PandocRequest {
            source: source.clone(),
            target: target.clone(),
            executable: script.to_string_lossy().to_string(),
            ..request("unused")
        };
        let written = convert(req, || false).unwrap();
        assert_eq!(written, target);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), source);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
