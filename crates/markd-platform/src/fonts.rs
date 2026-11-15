//! 字体加载：优先使用「阿里巴巴普惠体 3.0」，找不到再回退到系统中文字体。
//!
//! 查找顺序：`~/.markd/fonts/` → 用户字体目录 → 系统字体目录。
//! 这样即使用户没在系统里安装，也可以把 ttf 丢进 `~/.markd/fonts/` 生效。

use std::path::PathBuf;
use std::sync::Arc;

/// 常见的中文字体回退路径（避免 CJK 显示成方框）。
const CJK_FALLBACKS: [&str; 6] = [
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "C:\\Windows\\Fonts\\msyh.ttc",
];

fn dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        out.push(home.join(".markd").join("fonts"));
        out.push(home.join("Library").join("Fonts"));
    }
    out.push(PathBuf::from("/Library/Fonts"));
    out.push(PathBuf::from("/usr/share/fonts"));
    out.push(PathBuf::from("/usr/local/share/fonts"));
    if let Ok(windir) = std::env::var("WINDIR") {
        out.push(PathBuf::from(windir).join("Fonts"));
    }
    out
}

/// 在字体目录里找阿里巴巴普惠体 3.0，Regular 字重优先。
fn find_puhuiti() -> Option<Vec<u8>> {
    let mut hits: Vec<PathBuf> = Vec::new();
    for dir in dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let is_ttf = name.ends_with(".ttf") || name.ends_with(".otf") || name.ends_with(".ttc");
            if is_ttf && (name.contains("alibabapuhuiti") || name.contains("puhuiti")) {
                hits.push(entry.path());
            }
        }
    }
    // Regular 优先，其次按文件名稳定排序
    hits.sort_by_key(|p| {
        let n = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        let rank = if n.contains("55-regular") || n.contains("regular") {
            0
        } else if n.contains("65-medium") || n.contains("85-bold") {
            1
        } else {
            2
        };
        (rank, n)
    });
    hits.into_iter().find_map(|p| std::fs::read(p).ok())
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 首选：阿里巴巴普惠体 3.0（只影响 Proportional，代码块仍用等宽字体）
    if let Some(bytes) = find_puhuiti() {
        let key = "markd_puhuiti".to_string();
        fonts
            .font_data
            .insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, key);
    }

    // 回退：系统中文字体，保证缺字时不会出现方框
    for p in CJK_FALLBACKS {
        if let Ok(bytes) = std::fs::read(p) {
            let key = "cjk_fallback".to_string();
            fonts
                .font_data
                .insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                let list = fonts.families.entry(family).or_default();
                if !list.iter().any(|f| f == &key) {
                    list.push(key.clone());
                }
            }
            break;
        }
    }

    ctx.set_fonts(fonts);
}
