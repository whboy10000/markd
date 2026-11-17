//! 字符位置与行首标记工具：按字符切分、列表/引用标记的继承与转换。

pub(super) fn split_at_char(s: &str, pos: usize) -> (String, String) {
    let mut a = String::new();
    let mut b = String::new();
    for (i, c) in s.chars().enumerate() {
        if i < pos {
            a.push(c);
        } else {
            b.push(c);
        }
    }
    (a, b)
}

pub(super) fn insert_at_char(s: &str, pos: usize, ins: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i == pos {
            out.push_str(ins);
        }
        out.push(c);
    }
    if pos >= s.chars().count() {
        out.push_str(ins);
    }
    out
}

pub(super) fn inherited_marker(s: &str) -> String {
    let indent: String = s.chars().take_while(|c| *c == ' ').collect();
    let rest = &s[indent.len()..];
    let b = rest.as_bytes();
    if b.is_empty() {
        return String::new();
    }
    if (b[0] == b'-' || b[0] == b'*' || b[0] == b'+') && b.get(1) == Some(&b' ') {
        let after = &rest[2..];
        if after.starts_with("[ ] ") || after.starts_with("[x] ") || after.starts_with("[X] ") {
            return format!("{}- [ ] ", indent);
        }
        return format!("{}- ", indent);
    }
    let mut k = 0usize;
    while k < b.len() && b[k].is_ascii_digit() {
        k += 1;
    }
    if k > 0 && k + 1 < b.len() && (b[k] == b'.' || b[k] == b')') && b[k + 1] == b' ' {
        return format!("{}1. ", indent);
    }
    String::new()
}

/// 识别有序列表前缀里的起始序号与符号："1. " → (1, '.')，"3) " → (3, ')')。
pub(super) fn ordered_prefix(prefix: &str) -> Option<(u64, char)> {
    let t = prefix.trim();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    match t[digits.len()..].trim_end() {
        "." => Some((digits.parse().ok()?, '.')),
        ")" => Some((digits.parse().ok()?, ')')),
        _ => None,
    }
}

/// 任务项标记 `- [x]` → `- [ ]`（回车新建的任务项从不勾选状态开始）。
pub(super) fn replace_task_checkbox(marker: &str) -> String {
    marker.replace("[x]", "[ ]").replace("[X]", "[ ]")
}

/// 有序列表回车新建下一项时的序号：`3.` → `4.`，`9)` → `10)`。
/// 非有序标记原样返回。
pub(super) fn next_ordered_marker(marker: &str) -> String {
    let digits: String = marker.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return marker.to_string();
    }
    let tail = &marker[digits.len()..];
    if tail != "." && tail != ")" {
        return marker.to_string();
    }
    let n: u64 = digits.parse().unwrap_or(0);
    format!("{}{}", n + 1, tail)
}

pub(super) fn pair_close(t: &str) -> Option<&'static str> {
    match t {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        "\"" => Some("\""),
        "'" => Some("'"),
        _ => None,
    }
}
