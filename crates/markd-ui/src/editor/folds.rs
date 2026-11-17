//! 代码块折叠状态的持久化（按文档路径与块 source 存取）。

use super::*;

pub(super) fn folds_path() -> PathBuf {
    let dir = crate::workspace::Config::dir();
    let _ = std::fs::create_dir_all(&dir);
    dir.join("folds.json")
}

pub(super) fn read_folds_map() -> HashMap<String, Vec<String>> {
    std::fs::read_to_string(folds_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(super) fn write_folds_map(map: &HashMap<String, Vec<String>>) {
    if let Ok(s) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(folds_path(), s);
    }
}

/// 可折叠的块：代码块或 Mermaid 图表块。
pub(super) fn is_foldable(b: &Block) -> bool {
    matches!(
        b,
        Block::Code { .. } | Block::Diagram { .. } | Block::CodeGroup { .. }
    )
}

/// 纯函数：折叠下标集合 → 各折叠块的 source 键（排序去重）。
pub(super) fn fold_keys(doc: &Document, folded: &HashSet<usize>) -> Vec<String> {
    let mut keys: Vec<String> = folded
        .iter()
        .filter_map(|&i| doc.blocks.get(i))
        .filter(|b| is_foldable(b))
        .map(|b| b.source().to_string())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// 纯函数：source 键 → 当前文档里匹配的块下标集合。
pub(super) fn fold_indices(doc: &Document, keys: &[String]) -> HashSet<usize> {
    let mut out = HashSet::new();
    for (i, b) in doc.blocks.iter().enumerate() {
        if is_foldable(b) && keys.iter().any(|k| k == b.source()) {
            out.insert(i);
        }
    }
    out
}

/// 打开文档时把已保存的折叠状态还原成块下标集合：按 source 匹配当前块。
pub fn load_folds(path: &Path, doc: &Document) -> HashSet<usize> {
    let map = read_folds_map();
    let Some(keys) = map.get(&path.to_string_lossy().to_string()) else {
        return HashSet::new();
    };
    fold_indices(doc, keys)
}

/// 保存当前文档的折叠状态（无折叠项时移除该路径的条目）。
pub fn save_folds(path: &Path, doc: &Document, folded: &HashSet<usize>) {
    let mut map = read_folds_map();
    let key = path.to_string_lossy().to_string();
    let keys = fold_keys(doc, folded);
    if keys.is_empty() {
        map.remove(&key);
    } else {
        map.insert(key, keys);
    }
    write_folds_map(&map);
}
