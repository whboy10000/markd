//! Author: aurelianus <aurelianus@126.com>

//! 字体加载：
//! - 正文：阿里巴巴普惠体 → 苹方 → 思源黑体 → 微软雅黑 → 系统中文字体；
//! - 代码（代码块 / 代码组 / 行内代码）：JetBrains Mono → Fira Code →
//!   Consolas → Menlo → 内置等宽字体。
//!
//! 查找顺序：`~/.markd/fonts/` → 用户字体目录 → 系统字体目录；
//! 没在系统里安装的字体也可以把 ttf/otf 丢进 `~/.markd/fonts/` 生效。

use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    out.push(PathBuf::from("/System/Library/Fonts"));
    out.push(PathBuf::from("/System/Library/Fonts/Supplemental"));
    out.push(PathBuf::from("/usr/share/fonts"));
    out.push(PathBuf::from("/usr/local/share/fonts"));
    if let Ok(windir) = std::env::var("WINDIR") {
        out.push(PathBuf::from(windir).join("Fonts"));
    }
    out
}

fn is_font_file(p: &Path) -> bool {
    let name = p.file_name().unwrap_or_default().to_string_lossy();
    let n = name.to_ascii_lowercase();
    n.ends_with(".ttf") || n.ends_with(".otf") || n.ends_with(".ttc")
}

/// 递归收集字体文件（最多三层，覆盖 Linux 的 `truetype/xxx/` 子目录）。
fn font_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            font_files(&p, depth - 1, out);
        } else if is_font_file(&p) {
            out.push(p);
        }
    }
}

/// 所有可搜索的字体文件（目录下标用于保持用户字体优先）。
fn all_font_files() -> Vec<(usize, PathBuf)> {
    let mut out = Vec::new();
    for (i, dir) in dirs().iter().enumerate() {
        let mut files = Vec::new();
        font_files(dir, 3, &mut files);
        out.extend(files.into_iter().map(|p| (i, p)));
    }
    out
}

/// 文件名归一化：小写并去掉空格、连字符、下划线，便于关键字匹配。
fn norm_name(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .collect()
}

/// 文件名的字重优先级：普通 > 可变 > 其它 > 斜体 > 细 > 粗。
fn weight_score(norm: &str) -> u8 {
    if norm.contains("regular") {
        0
    } else if norm.contains("wght") || norm.contains("variable") {
        1
    } else if norm.contains("italic") || norm.contains("oblique") {
        4
    } else if norm.contains("thin") || norm.contains("light") {
        5
    } else if norm.contains("bold") || norm.contains("black") || norm.contains("heavy") {
        6
    } else {
        2
    }
}

/// 按文件名关键字查找字体：用户目录优先，其次文件名里的字重。
fn find_font(files: &[(usize, PathBuf)], keys: &[&str]) -> Option<Vec<u8>> {
    let mut hits: Vec<&(usize, PathBuf)> = files
        .iter()
        .filter(|(_, p)| {
            let n = norm_name(p);
            keys.iter().any(|k| n.contains(k))
        })
        .collect();
    hits.sort_by_key(|(d, p)| (*d, weight_score(&norm_name(p)), norm_name(p)));
    hits.into_iter().find_map(|(_, p)| std::fs::read(p).ok())
}

/// 找指定字重的普惠体：`want` 里的关键字命中即算（如 "85-bold"）。
fn find_puhuiti_weight(files: &[(usize, PathBuf)], want: &[&str]) -> Option<Vec<u8>> {
    let mut hits: Vec<&(usize, PathBuf)> = files
        .iter()
        .filter(|(_, p)| {
            let n = norm_name(p);
            n.contains("alibabapuhuiti") || n.contains("puhuiti")
        })
        .collect();
    hits.sort_by_key(|(d, p)| (*d, weight_score(&norm_name(p)), norm_name(p)));
    for (_, p) in hits {
        let n = norm_name(p);
        let matched = want.iter().any(|w| {
            let w: String = w.chars().filter(|c| !matches!(c, '-' | '_')).collect();
            n.contains(&w)
        });
        if matched && let Ok(bytes) = std::fs::read(p) {
            return Some(bytes);
        }
    }
    None
}

fn register(fonts: &mut egui::FontDefinitions, key: &str, bytes: Vec<u8>) {
    fonts
        .font_data
        .insert(key.to_string(), Arc::new(egui::FontData::from_owned(bytes)));
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let proportional = egui::FontFamily::Proportional;
    let monospace = egui::FontFamily::Monospace;
    let bold_family = egui::FontFamily::Name("markd-bold".into());

    // ---------------- 正文字体：普惠体 → 苹方 → 思源黑体 → 微软雅黑 ----------------
    let files = all_font_files();
    let mut body_keys: Vec<String> = Vec::new();
    if let Some(bytes) = find_puhuiti_weight(&files, &["55-regular", "regular"]) {
        register(&mut fonts, "markd_puhuiti", bytes);
        body_keys.push("markd_puhuiti".to_string());
    }
    for (key, keys) in [
        ("markd_pingfang", &["pingfang"][..]),
        (
            "markd_noto_sans_sc",
            &["notosanssc", "notosanscjk", "notosanshans"][..],
        ),
        ("markd_yahei", &["msyh"][..]),
    ] {
        if let Some(bytes) = find_font(&files, keys) {
            register(&mut fonts, key, bytes);
            body_keys.push(key.to_string());
        }
    }

    // ---------------- 代码字体：JetBrains Mono → Fira Code → Consolas → Menlo ----------------
    let mut mono_keys: Vec<String> = Vec::new();
    for (key, keys) in [
        ("markd_jetbrains_mono", &["jetbrainsmono", "jetbrains"][..]),
        ("markd_fira_code", &["firacode"][..]),
        ("markd_consolas", &["consola"][..]),
        ("markd_menlo", &["menlo"][..]),
    ] {
        if let Some(bytes) = find_font(&files, keys) {
            register(&mut fonts, key, bytes);
            mono_keys.push(key.to_string());
        }
    }

    // 按优先级插到字族最前面（倒序插入，保证 body_keys[0] 最优）。
    for key in body_keys.iter().rev() {
        fonts
            .families
            .entry(proportional.clone())
            .or_default()
            .insert(0, key.clone());
    }
    for key in mono_keys.iter().rev() {
        fonts
            .families
            .entry(monospace.clone())
            .or_default()
            .insert(0, key.clone());
    }

    // 粗体：注册具名字族 markd-bold，供内联加粗使用（普惠体是静态字重，
    // 变量字体的 wght 轴对它无效，所以必须单独加载 Bold 文件）。
    let mut bold_list: Vec<String> = Vec::new();
    if let Some(bytes) = find_puhuiti_weight(&files, &["85-bold", "75-semibold", "bold"]) {
        register(&mut fonts, "markd_puhuiti_bold", bytes);
        bold_list.push("markd_puhuiti_bold".to_string());
    }
    bold_list.extend(
        fonts
            .families
            .get(&proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts.families.insert(bold_family.clone(), bold_list);

    // ---------------- 缺字兜底：系统中文字体 ----------------
    // 只有前面列出的中文字体都没找到时才加载，省一份几十 MB 的字体内存。
    if !body_keys.iter().any(|k| {
        matches!(
            k.as_str(),
            "markd_pingfang" | "markd_noto_sans_sc" | "markd_yahei"
        )
    }) && let Some(bytes) = find_font(
        &files,
        &[
            "hiraginosansgb",
            "stheiti",
            "arialunicode",
            "wqymicrohei",
            "notosanscjk",
        ],
    ) {
        register(&mut fonts, "cjk_fallback", bytes);
        for family in [proportional.clone(), monospace.clone(), bold_family.clone()] {
            let list = fonts.families.entry(family).or_default();
            if !list.iter().any(|f| f == "cjk_fallback") {
                list.push("cjk_fallback".to_string());
            }
        }
    }

    // 中文正文也必须能落到等宽字体族里（代码块里的中文注释、代码组标签）。
    for key in body_keys.iter() {
        let list = fonts.families.entry(monospace.clone()).or_default();
        if !list.iter().any(|f| f == key) {
            list.push(key.clone());
        }
    }

    // Windows 系统粗体（微软雅黑 Bold）：没装普惠体时让加粗文字真的有字重
    if cfg!(target_os = "windows")
        && let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\msyhbd.ttc")
    {
        register(&mut fonts, "cjk_fallback_bold", bytes);
        if let Some(list) = fonts.families.get_mut(&bold_family) {
            list.insert(0, "cjk_fallback_bold".to_string());
        }
    }

    ctx.set_fonts(fonts);
}
