//! 列表层级、编号与项目符号绘制，以及行内公式的美化。

use super::*;

/// 列表的嵌套层级：按前导空格估算（2 空格一层，1 空格也算一层）。
/// 用于让子列表的标记和父列表不同。
pub fn list_level(indent: usize) -> usize {
    indent.div_ceil(2)
}

/// 有序列表的显示标记：顶层保留源文件里的数字（`1.`），子层级依次显示
/// 小写字母（`a.`）、小写罗马数字（`i.`），再往下循环。数字取自源标记，
/// 所以子列表自身的 1、2、3 会变成 a、b、c。
pub fn ordered_list_marker(marker: &str, level: usize) -> String {
    if level % 3 == 0 {
        return marker.to_string();
    }
    let tail = marker
        .chars()
        .last()
        .filter(|c| *c == '.' || *c == ')')
        .unwrap_or('.');
    let digits: String = marker.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: usize = digits.parse().unwrap_or(1).max(1);
    let body = if level % 3 == 1 {
        alpha_label(n)
    } else {
        roman_label(n)
    };
    format!("{body}{tail}")
}

/// 1 → a、26 → z、27 → aa。
pub(super) fn alpha_label(n: usize) -> String {
    let mut n = n;
    let mut out = Vec::new();
    while n > 0 {
        let r = (n - 1) % 26;
        out.push((b'a' + r as u8) as char);
        n = (n - 1) / 26;
    }
    out.iter().rev().collect()
}

/// 1 → i、4 → iv、9 → ix、14 → xiv。
pub(super) fn roman_label(n: usize) -> String {
    const TABLE: &[(usize, &str)] = &[
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut n = n;
    let mut s = String::new();
    for (v, r) in TABLE {
        while n >= *v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// 无序列表标记：按层级在实心圆 / 空心圆 / 小方块之间循环，
/// 这样子列表一眼就能和父列表区分开。
pub fn draw_list_bullet(ui: &mut egui::Ui, level: usize, color: Color32, d: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(d + 2.0), Sense::hover());
    let p = ui.painter();
    let c = r.center();
    match level % 3 {
        0 => {
            p.circle_filled(c, d * 0.5, color);
        }
        1 => {
            p.circle_stroke(c, d * 0.42, Stroke::new(1.6, color));
        }
        _ => {
            p.rect_filled(Rect::from_center_size(c, Vec2::splat(d * 0.6)), 1.5, color);
        }
    }
}
