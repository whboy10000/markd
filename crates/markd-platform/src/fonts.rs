//! Author: aurelianus <aurelianus@126.com>

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
    // Windows 没有 HOME 环境变量，用户目录是 USERPROFILE
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from);
    if let Some(home) = home {
        out.push(home.join(".markd").join("fonts"));
        if cfg!(target_os = "macos") {
            out.push(home.join("Library").join("Fonts"));
        }
    }
    out.push(PathBuf::from("/Library/Fonts"));
    out.push(PathBuf::from("/usr/share/fonts"));
    out.push(PathBuf::from("/usr/local/share/fonts"));
    if let Ok(windir) = std::env::var("WINDIR") {
        out.push(PathBuf::from(windir).join("Fonts"));
    }
    out
}

/// 收集字体目录里所有普惠体文件，按「字重关键词」排序。
fn puhuiti_files() -> Vec<PathBuf> {
    let mut hits: Vec<PathBuf> = Vec::new();
    for dir in dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let is_font =
                name.ends_with(".ttf") || name.ends_with(".otf") || name.ends_with(".ttc");
            if is_font && (name.contains("alibabapuhuiti") || name.contains("puhuiti")) {
                hits.push(entry.path());
            }
        }
    }
    hits
}

/// 找指定字重的普惠体：`want` 里的关键字命中即算（如 "85-bold"）。
fn find_puhuiti_weight(want: &[&str]) -> Option<Vec<u8>> {
    let mut hits = puhuiti_files();
    hits.sort_by_key(|p| {
        let n = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        (if n.contains(want[0]) { 0 } else { 1 }, n)
    });
    for p in hits {
        let n = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if want.iter().any(|w| n.contains(w)) {
            if let Ok(bytes) = std::fs::read(&p) {
                return Some(bytes);
            }
        }
    }
    None
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 首选：阿里巴巴普惠体 3.0（只影响 Proportional，代码块仍用等宽字体）
    let mut regular_loaded = false;
    if let Some(bytes) = find_puhuiti_weight(&["55-regular", "regular"]) {
        let key = "markd_puhuiti".to_string();
        fonts
            .font_data
            .insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, key);
        regular_loaded = true;
    }

    // 粗体：注册一个具名字体族 markd-bold，供内联加粗使用（普惠体是静态字重，
    // 变量字体的 wght 轴对它无效，所以必须单独加载 Bold 文件）。
    let bold_family = egui::FontFamily::Name("markd-bold".into());
    let mut bold_list: Vec<String> = Vec::new();
    if let Some(bytes) = find_puhuiti_weight(&["85-bold", "75-semibold", "bold"]) {
        let key = "markd_puhuiti_bold".to_string();
        fonts
            .font_data
            .insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
        bold_list.push(key);
    }
    if regular_loaded {
        bold_list.push("markd_puhuiti".to_string());
    }
    bold_list.extend(
        fonts
            .families
            .get(&egui::FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts.families.insert(bold_family, bold_list);

    // 回退：系统中文字体，保证缺字时不会出现方框。
    // 必须同时加进 markd-bold：加粗走的是独立字族，若只加 Proportional/Monospace，
    // 没装普惠体的机器（如 Windows）上中文一加粗就落进默认拉丁字体，显示成乱码。
    for p in CJK_FALLBACKS {
        if let Ok(bytes) = std::fs::read(p) {
            let key = "cjk_fallback".to_string();
            fonts
                .font_data
                .insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
            for family in [
                egui::FontFamily::Proportional,
                egui::FontFamily::Monospace,
                egui::FontFamily::Name("markd-bold".into()),
            ] {
                let list = fonts.families.entry(family).or_default();
                if !list.iter().any(|f| f == &key) {
                    list.push(key.clone());
                }
            }
            break;
        }
    }

    // Windows 系统粗体（微软雅黑 Bold）：没装普惠体时让加粗文字真的有字重
    if cfg!(target_os = "windows")
        && let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\msyhbd.ttc")
    {
        let key = "cjk_fallback_bold".to_string();
        fonts
            .font_data
            .insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
        if let Some(list) = fonts
            .families
            .get_mut(&egui::FontFamily::Name("markd-bold".into()))
        {
            list.insert(0, key);
        }
    }

    ctx.set_fonts(fonts);
}
