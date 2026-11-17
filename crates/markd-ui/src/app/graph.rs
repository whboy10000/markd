//! 工作区双向链接图。

use super::*;

pub(super) fn build_link_graph(root: Option<&Path>) -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    if let Some(r) = root {
        for p in workspace::collect_markdown(r) {
            if let Ok(s) = workspace::read(&p) {
                let blocks = crate::ast::parse(&s);
                let mut links = Vec::new();
                for b in &blocks {
                    for inl in crate::document::block_inlines(b) {
                        crate::ast::collect_wiki(std::slice::from_ref(inl), &mut links);
                    }
                }
                links.sort();
                links.dedup();
                let name = p
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                m.insert(name, links);
            }
        }
    }
    m
}
