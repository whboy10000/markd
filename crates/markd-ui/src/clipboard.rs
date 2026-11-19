//! 富文本剪贴板：复制时同时写入 HTML 与纯文本备选。
//!
//! arboard 的 `set_html` 会把 HTML 作为主格式、纯文本作为 alt 写入系统剪贴板。
//! 这样粘贴到支持富文本的编辑器（Word / 浏览器 / 支持 HTML 的 Markdown 工具）
//! 时保留标题、加粗、行内代码、代码块、表格等格式；粘贴到纯文本环境时退化成
//! 纯文本。若剪贴板不可用，回退到 egui 的 `copy_text`（仅纯文本）。
//!
//! 测试环境下（`cfg(test)`）不碰系统剪贴板：macOS 的 NSPasteboard 不是线程安全的，
//! 并行测试同时访问会抛 Objective-C 异常导致 SIGABRT。改为把纯文本记录到全局
//! 静态、并照常发 `ctx.copy_text`，让既有测试仍可用 OutputCommand 断言。

use egui::Context;

/// 把 HTML 与纯文本备选一起写入系统剪贴板。
/// 返回是否成功写入富文本（false 表示已回退到纯文本）。
pub(crate) fn copy_rich(ctx: &Context, plain: &str, html: &str) -> bool {
    #[cfg(test)]
    {
        // 测试：不访问系统剪贴板，记录文本 + 发 CopyText 命令供断言。
        let _ = html;
        *test_last_copy().lock().unwrap() = plain.to_string();
        ctx.copy_text(plain.to_string());
        true
    }
    #[cfg(not(test))]
    {
        match arboard::Clipboard::new() {
            Ok(mut clip) => match clip.set_html(html.to_string(), Some(plain.to_string())) {
                Ok(()) => true,
                Err(_) => {
                    ctx.copy_text(plain.to_string());
                    false
                }
            },
            Err(_) => {
                ctx.copy_text(plain.to_string());
                false
            }
        }
    }
}

/// 测试用：取出最近一次 `copy_rich` 写入的纯文本。
#[cfg(test)]
pub(crate) fn test_last_copied_text() -> String {
    test_last_copy().lock().unwrap().clone()
}

#[cfg(test)]
fn test_last_copy() -> &'static std::sync::Mutex<String> {
    use std::sync::Mutex;
    static LAST: std::sync::OnceLock<Mutex<String>> = std::sync::OnceLock::new();
    LAST.get_or_init(|| Mutex::new(String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rich_copy_html_builds_expected_markup() {
        let h = crate::export::inline_markdown_html("**粗**与`码`");
        assert!(h.contains("<strong>粗</strong>"), "{h}");
        assert!(h.contains("<code>码</code>"), "{h}");
    }

    #[test]
    fn rich_copy_records_plain_text() {
        let ctx = egui::Context::default();
        copy_rich(&ctx, "纯文本", "<p>带格式</p>");
        assert_eq!(test_last_copied_text(), "纯文本");
    }
}
