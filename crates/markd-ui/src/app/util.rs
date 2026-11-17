//! 应用层通用工具：自动保存判定、标签批量写盘、图片源码重建等。

use super::*;

/// 这份文档是否应该被自动保存：开关打开、有未保存修改、且已经落过盘。
/// 未命名文档（没有路径）不自动写盘，要等用户按保存时再选路径。
pub(super) fn should_autosave(enabled: bool, dirty: bool, has_path: bool) -> bool {
    enabled && dirty && has_path
}

/// 距上次自动保存是否已经够久（间隔下限 5 秒，和设置里的范围一致）。
pub(super) fn autosave_due(now: u64, last: u64, interval: u64) -> bool {
    now.saturating_sub(last) >= interval.max(5)
}

/// 把 paths[i] 指定的位置写回第 i 个标签里所有有修改的文档，返回真正写过的路径。
/// 没有路径或没有修改的标签跳过；退出前的批量保存都走这里，便于单测覆盖。
pub(super) fn write_dirty_tabs(
    tabs: &mut [Tab],
    paths: &[Option<PathBuf>],
) -> std::io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for i in 0..tabs.len() {
        let Some(st) = tabs[i].state.as_mut() else {
            continue;
        };
        if !st.doc.dirty {
            continue;
        }
        let Some(path) = paths.get(i).cloned().flatten() else {
            continue;
        };
        st.doc.path = Some(path.clone());
        st.doc.save()?;
        tabs[i].path = Some(path.clone());
        written.push(path);
    }
    Ok(written)
}

/// 拖动缩放后重建「纯图片段落」的源码：第 `no` 张图片写入 `{width=N}`，
/// 其余图片保留原有属性。不是纯图片段落时返回 None。
pub(super) fn resize_image_source(
    doc: &Document,
    block: usize,
    no: usize,
    width: u32,
) -> Option<String> {
    let crate::ast::Block::Paragraph { content, .. } = doc.blocks.get(block)? else {
        return None;
    };
    let imgs: Vec<(&String, &String, Option<u32>)> = content
        .iter()
        .filter_map(|i| match i {
            crate::ast::Inline::Image { alt, url, width } => Some((alt, url, *width)),
            _ => None,
        })
        .collect();
    // 只重建「纯图片段落」：混着正文的段落重建会把文字弄丢。
    let only_images = !imgs.is_empty()
        && content.iter().all(|i| {
            matches!(
                i,
                crate::ast::Inline::Image { .. } | crate::ast::Inline::Break
            )
        });
    if !only_images || no >= imgs.len() {
        return None;
    }
    Some(
        imgs.iter()
            .enumerate()
            .map(|(k, (alt, url, w))| {
                let w = if k == no { Some(width) } else { *w };
                match w {
                    Some(w) => format!("![{alt}]({url}){{width={w}}}"),
                    None => format!("![{alt}]({url})"),
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// 关闭请求是否需要先弹「有未保存的修改」确认框：
/// 用户已经选过「不保存」（force_quit）时直接放行。
pub(super) fn needs_exit_confirm(force_quit: bool, close_requested: bool, unsaved: usize) -> bool {
    !force_quit && close_requested && unsaved > 0
}

/// 保存路径统一补上 .md 扩展名。
pub(super) fn with_md_ext(path: PathBuf) -> PathBuf {
    let has_ext = path
        .extension()
        .map(|e| {
            let e = e.to_string_lossy().to_lowercase();
            e == "md" || e == "markdown"
        })
        .unwrap_or(false);
    if has_ext {
        return path;
    }
    let mut p = path;
    p.set_extension("md");
    p
}

pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
