//! 自研词法着色：把代码渲染成带颜色的 `LayoutJob`。

use super::*;

/// 轻量词法着色，覆盖常见语言，不依赖 syntect。
/// 这里给每行末尾都补 `\n`，让区块底部多一行留白、看得更像代码框。
/// 各语言的关键字表。大小写敏感：SQL 用大写，其余多为小写。
/// 按语言族区分，避免把 A 语言的关键字误标到 B 语言。
pub(super) fn keywords_for(lang: &str) -> &'static [&'static str] {
    match lang_family(lang) {
        LangFamily::Rust => &[
            "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
            "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
            "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
            "trait", "true", "type", "unsafe", "use", "where", "while",
        ],
        LangFamily::Python => &[
            "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
            "elif", "else", "except", "False", "finally", "for", "from", "global", "if", "import",
            "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return",
            "self", "True", "try", "while", "with", "yield",
        ],
        LangFamily::JavaScript => &[
            "as",
            "async",
            "await",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "default",
            "delete",
            "do",
            "else",
            "enum",
            "export",
            "extends",
            "false",
            "finally",
            "for",
            "from",
            "function",
            "if",
            "implements",
            "import",
            "in",
            "instanceof",
            "interface",
            "let",
            "new",
            "null",
            "of",
            "private",
            "protected",
            "public",
            "readonly",
            "return",
            "static",
            "super",
            "switch",
            "this",
            "throw",
            "true",
            "try",
            "typeof",
            "undefined",
            "var",
            "void",
            "while",
            "yield",
        ],
        LangFamily::CLike => &[
            "auto",
            "bool",
            "break",
            "byte",
            "case",
            "catch",
            "char",
            "class",
            "const",
            "continue",
            "default",
            "delete",
            "do",
            "double",
            "else",
            "enum",
            "extends",
            "false",
            "final",
            "finally",
            "float",
            "for",
            "foreach",
            "func",
            "goto",
            "if",
            "implements",
            "import",
            "int",
            "interface",
            "long",
            "namespace",
            "new",
            "nullptr",
            "NULL",
            "package",
            "private",
            "protected",
            "public",
            "return",
            "short",
            "sizeof",
            "static",
            "struct",
            "super",
            "switch",
            "template",
            "this",
            "throw",
            "true",
            "try",
            "typedef",
            "typename",
            "unsigned",
            "using",
            "var",
            "virtual",
            "void",
            "while",
        ],
        LangFamily::Css => &[
            "@import",
            "@media",
            "@keyframes",
            "@font-face",
            "from",
            "to",
            "important",
            "inherit",
            "initial",
            "none",
            "auto",
            "solid",
            "dashed",
            "block",
            "flex",
            "grid",
            "absolute",
            "relative",
            "fixed",
            "center",
            "bold",
            "italic",
        ],
        LangFamily::Data => &[
            "true", "false", "null", "True", "False", "None", "yes", "no", "on", "off",
        ],
        LangFamily::Shell => &[
            "alias", "break", "case", "cd", "continue", "declare", "do", "done", "echo", "elif",
            "else", "esac", "exit", "export", "false", "fi", "for", "function", "if", "in",
            "local", "readonly", "return", "set", "source", "then", "true", "unset", "while",
        ],
        LangFamily::Sql => &[
            "ADD",
            "ALL",
            "ALTER",
            "AND",
            "AS",
            "ASC",
            "AVG",
            "BETWEEN",
            "BY",
            "CASE",
            "COUNT",
            "CREATE",
            "CROSS",
            "DATABASE",
            "DEFAULT",
            "DELETE",
            "DESC",
            "DISTINCT",
            "DROP",
            "ELSE",
            "END",
            "EXISTS",
            "FALSE",
            "FROM",
            "FULL",
            "GROUP",
            "HAVING",
            "IN",
            "INDEX",
            "INNER",
            "INSERT",
            "INTO",
            "IS",
            "JOIN",
            "KEY",
            "LEFT",
            "LIKE",
            "LIMIT",
            "MAX",
            "MIN",
            "NOT",
            "NULL",
            "OFFSET",
            "ON",
            "OR",
            "ORDER",
            "OUTER",
            "PRIMARY",
            "REFERENCES",
            "RIGHT",
            "SELECT",
            "SET",
            "SUM",
            "TABLE",
            "THEN",
            "TRUE",
            "UNION",
            "UPDATE",
            "VALUES",
            "VIEW",
            "WHERE",
        ],
        LangFamily::Markup | LangFamily::Generic => &[],
    }
}

/// 该语言的行注释前缀（返回 true 表示从此处到行尾是注释）。
pub(super) fn line_comment_at(family: LangFamily, c: &[char], i: usize) -> bool {
    match family {
        // # : Python / Shell / YAML / TOML
        LangFamily::Python | LangFamily::Shell | LangFamily::Data => c[i] == '#',
        // -- : SQL
        LangFamily::Sql => c[i] == '-' && i + 1 < c.len() && c[i + 1] == '-',
        // // : Rust / JS / C-like / CSS；HTML/XML 用 <!-- -->（块注释处理）
        _ => c[i] == '/' && i + 1 < c.len() && c[i + 1] == '/',
    }
}

/// 该语言是否支持 /* */ 块注释。
pub(super) fn has_block_comment(family: LangFamily) -> bool {
    matches!(
        family,
        LangFamily::Rust | LangFamily::JavaScript | LangFamily::CLike | LangFamily::Css
    )
}

/// HTML/XML 注释 <!-- -->。
pub(super) fn is_markup_comment(c: &[char], i: usize) -> bool {
    c[i] == '<' && i + 3 < c.len() && c[i + 1] == '!' && c[i + 2] == '-' && c[i + 3] == '-'
}

pub fn code_job(code: &str, lang: &str, theme: &Theme) -> LayoutJob {
    code_job_impl(code, lang, theme, true)
}

/// 编辑态用的高亮排版：**文本与缓冲区逐字符一致**（不补行尾换行），
/// 否则 TextEdit 的光标/选中会按错位的字符下标走。
pub fn code_job_edit(code: &str, lang: &str, theme: &Theme) -> LayoutJob {
    code_job_impl(code, lang, theme, false)
}

pub(super) fn code_job_impl(
    code: &str,
    lang: &str,
    theme: &Theme,
    trailing_newline: bool,
) -> LayoutJob {
    let family = lang_family(lang);
    let kw = keywords_for(lang);
    let [
        kw_color,
        str_color,
        num_color,
        comment_color,
        fun_color,
        type_color,
    ] = theme.syntax(lang);
    let markup = matches!(family, LangFamily::Markup);
    let css = matches!(family, LangFamily::Css);
    let data = matches!(family, LangFamily::Data);
    let block_comments = has_block_comment(family);

    let mut job = LayoutJob::default();
    // 代码行常没有空格，允许在任意字符处换行以适配窄窗口。
    job.wrap.break_anywhere = true;
    let lines: Vec<&str> = code.split('\n').collect();
    // 跨行块注释状态：None=不在块注释中；Some(true)=<!-- -->；Some(false)=/* */
    let mut in_block: Option<bool> = None;

    for (n, line) in lines.iter().enumerate() {
        let c: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < c.len() {
            // 块注释中：找结束符
            if let Some(is_markup) = in_block {
                let end_seq: &[char] = if is_markup {
                    &['-', '-', '>']
                } else {
                    &['*', '/']
                };
                let pos = (i..=c.len().saturating_sub(end_seq.len()))
                    .find(|&k| &c[k..k + end_seq.len()] == end_seq);
                if let Some(pos) = pos {
                    let end = pos + end_seq.len();
                    let rest: String = c[i..end].iter().collect();
                    let mut f = theme.mono_fmt();
                    f.color = comment_color;
                    job.append(&rest, 0.0, f);
                    i = end;
                    in_block = None;
                } else {
                    let rest: String = c[i..].iter().collect();
                    let mut f = theme.mono_fmt();
                    f.color = comment_color;
                    job.append(&rest, 0.0, f);
                    break;
                }
                continue;
            }
            // 块注释开始
            if markup && is_markup_comment(&c, i) {
                in_block = Some(true);
                let rest: String = c[i..i + 4].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = comment_color;
                job.append(&rest, 0.0, f);
                i += 4;
                continue;
            }
            if block_comments && c[i] == '/' && i + 1 < c.len() && c[i + 1] == '*' {
                in_block = Some(false);
                let rest: String = c[i..i + 2].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = comment_color;
                job.append(&rest, 0.0, f);
                i += 2;
                continue;
            }
            // 行注释
            if line_comment_at(family, &c, i) {
                let rest: String = c[i..].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = comment_color;
                job.append(&rest, 0.0, f);
                break;
            }
            // 字符串（含引号）
            if c[i] == '"' || c[i] == '\'' || c[i] == '`' {
                let q = c[i];
                let mut j = i + 1;
                while j < c.len() && c[j] != q {
                    j += 1;
                }
                let end = (j + 1).min(c.len());
                let s: String = c[i..end].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = str_color;
                job.append(&s, 0.0, f);
                i = end;
                continue;
            }
            // HTML/XML 标签：<tag 的标签名用类型色
            if markup
                && c[i] == '<'
                && i + 1 < c.len()
                && (c[i + 1].is_alphabetic() || c[i + 1] == '/')
            {
                let mut j = i + 1;
                if c[j] == '/' {
                    j += 1;
                }
                while j < c.len() && (c[j].is_alphanumeric() || c[j] == '-' || c[j] == ':') {
                    j += 1;
                }
                let s: String = c[i..j].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = type_color;
                job.append(&s, 0.0, f);
                i = j;
                continue;
            }
            // 标识符 / 数字
            if c[i].is_alphanumeric() || c[i] == '_' {
                let mut j = i;
                while j < c.len() && (c[j].is_alphanumeric() || c[j] == '_') {
                    j += 1;
                }
                let tok: String = c[i..j].iter().collect();
                let is_num = tok
                    .chars()
                    .next()
                    .map(|x| x.is_ascii_digit())
                    .unwrap_or(false);
                let is_kw = kw.contains(&tok.as_str());
                // 下一个非空白字符是 '(' → 函数名；大写开头 → 类型/常量
                let next = c[j..].iter().find(|&&x| !x.is_whitespace()).copied();
                let is_call = next == Some('(');
                let is_type = tok.chars().next().is_some_and(|x| x.is_uppercase());
                // YAML 的 key: / CSS 的 属性:
                let is_prop = (data || css) && next == Some(':');
                let (color, bold) = if is_num {
                    (num_color, false)
                } else if is_kw {
                    (kw_color, true)
                } else if is_call {
                    (fun_color, false)
                } else if is_type {
                    (type_color, false)
                } else if is_prop {
                    (fun_color, false)
                } else {
                    (theme.text, false)
                };
                let mut f = theme.mono_fmt();
                f.color = color;
                if bold {
                    f.coords = egui::epaint::text::VariationCoords::new([(b"wght", 700.0)]);
                }
                job.append(&tok, 0.0, f);
                i = j;
                continue;
            }
            let mut f = theme.mono_fmt();
            f.color = theme.dim;
            job.append(&c[i].to_string(), 0.0, f);
            i += 1;
        }
        // 编辑态不补最后一行之后的换行，保证 job.text 与缓冲区完全一致
        if trailing_newline || n + 1 < lines.len() {
            job.append("\n", 0.0, theme.mono_fmt());
        }
    }
    if code.is_empty() && trailing_newline {
        job.append(" ", 0.0, theme.mono_fmt());
    }
    job
}
