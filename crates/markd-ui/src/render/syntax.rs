//! 自研词法着色：把代码渲染成带颜色的 `LayoutJob`，不依赖 syntect。
//! 展示态给每行末尾补 `\n`，让区块底部多一行留白、看得更像代码框。

use super::*;

/// 语言专属关键字表。配色族的通用表在这些语言上会漏色（Lua 的 `end`）
/// 或误色（Dockerfile 的大写指令、LaTeX 正文里的 if/in/is），所以先按语言查，查不到再回落到族表。
const DOCKERFILE_KEYWORDS: &[&str] = &[
    "FROM",
    "RUN",
    "CMD",
    "LABEL",
    "MAINTAINER",
    "EXPOSE",
    "ENV",
    "ADD",
    "COPY",
    "ENTRYPOINT",
    "VOLUME",
    "USER",
    "WORKDIR",
    "ARG",
    "ONBUILD",
    "STOPSIGNAL",
    "HEALTHCHECK",
    "SHELL",
    "AS",
    "as",
];
const MAKEFILE_KEYWORDS: &[&str] = &[
    "ifeq", "ifneq", "ifdef", "ifndef", "else", "endif", "include", "define", "endef", "export",
    "unexport", "override", "vpath", "default",
];
const CMAKE_KEYWORDS: &[&str] = &[
    "cmake_minimum_required",
    "project",
    "add_executable",
    "add_library",
    "add_subdirectory",
    "target_link_libraries",
    "target_include_directories",
    "target_compile_definitions",
    "find_package",
    "include_directories",
    "link_directories",
    "set",
    "list",
    "string",
    "option",
    "message",
    "if",
    "elseif",
    "else",
    "endif",
    "foreach",
    "endforeach",
    "while",
    "endwhile",
    "function",
    "endfunction",
    "macro",
    "endmacro",
    "install",
];
const TERRAFORM_KEYWORDS: &[&str] = &[
    "resource",
    "variable",
    "output",
    "locals",
    "module",
    "provider",
    "data",
    "terraform",
    "backend",
    "provisioner",
    "dynamic",
    "lifecycle",
    "count",
    "for_each",
    "depends_on",
    "true",
    "false",
    "null",
];
const NGINX_KEYWORDS: &[&str] = &[
    "server",
    "location",
    "upstream",
    "http",
    "events",
    "stream",
    "worker_processes",
    "if",
    "set",
    "return",
    "rewrite",
    "proxy_pass",
    "fastcgi_pass",
    "root",
    "listen",
    "include",
    "types",
    "map",
    "geo",
    "auth_basic",
];
const LUA_KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while", "self",
];
const HASKELL_KEYWORDS: &[&str] = &[
    "case",
    "class",
    "data",
    "default",
    "deriving",
    "do",
    "else",
    "forall",
    "import",
    "in",
    "instance",
    "let",
    "module",
    "newtype",
    "of",
    "qualified",
    "then",
    "type",
    "where",
];
const LISP_KEYWORDS: &[&str] = &[
    "and",
    "apply",
    "cond",
    "defmacro",
    "defparameter",
    "defun",
    "defvar",
    "do",
    "dolist",
    "dotimes",
    "funcall",
    "if",
    "lambda",
    "let",
    "let*",
    "loop",
    "not",
    "or",
    "progn",
    "provide",
    "require",
    "return",
    "setq",
    "t",
    "unless",
    "use-package",
    "when",
];
const CLOJURE_KEYWORDS: &[&str] = &[
    "def",
    "defn",
    "defmacro",
    "defmethod",
    "defmulti",
    "defprotocol",
    "defrecord",
    "deftype",
    "do",
    "doseq",
    "for",
    "if",
    "if-let",
    "let",
    "loop",
    "ns",
    "recur",
    "require",
    "try",
    "when",
    "when-let",
    "fn",
    "false",
    "nil",
    "true",
];
const RUBY_KEYWORDS: &[&str] = &[
    "alias", "and", "begin", "break", "case", "class", "def", "defined", "do", "else", "elsif",
    "end", "ensure", "for", "if", "in", "module", "next", "nil", "not", "or", "redo", "require",
    "rescue", "retry", "return", "self", "super", "then", "true", "false", "undef", "unless",
    "until", "when", "while", "yield",
];
const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    "nil",
    "true",
    "false",
    "iota",
];
const LATEX_KEYWORDS: &[&str] = &[
    "begin",
    "end",
    "documentclass",
    "usepackage",
    "section",
    "subsection",
    "subsubsection",
    "chapter",
    "part",
    "title",
    "author",
    "date",
    "maketitle",
    "tableofcontents",
    "item",
    "itemize",
    "enumerate",
    "description",
    "caption",
    "label",
    "ref",
    "eqref",
    "cite",
    "textbf",
    "textit",
    "emph",
    "frac",
    "sum",
    "int",
    "includegraphics",
    "newcommand",
    "renewcommand",
    "centering",
];
const MATLAB_KEYWORDS: &[&str] = &[
    "and",
    "break",
    "case",
    "catch",
    "continue",
    "else",
    "elseif",
    "end",
    "false",
    "for",
    "function",
    "global",
    "if",
    "otherwise",
    "parfor",
    "persistent",
    "return",
    "switch",
    "true",
    "try",
    "while",
];
const JULIA_KEYWORDS: &[&str] = &[
    "abstract",
    "begin",
    "break",
    "catch",
    "const",
    "continue",
    "do",
    "else",
    "elseif",
    "end",
    "export",
    "false",
    "finally",
    "for",
    "function",
    "global",
    "if",
    "import",
    "in",
    "isa",
    "let",
    "local",
    "macro",
    "missing",
    "module",
    "mutable",
    "nothing",
    "primitive",
    "quote",
    "return",
    "struct",
    "true",
    "try",
    "using",
    "where",
    "while",
];
const R_KEYWORDS: &[&str] = &[
    "TRUE", "FALSE", "NULL", "NA", "NaN", "Inf", "break", "else", "for", "function", "if", "in",
    "library", "next", "repeat", "require", "return", "source", "while",
];
const ELIXIR_KEYWORDS: &[&str] = &[
    "alias",
    "and",
    "case",
    "cond",
    "def",
    "defdelegate",
    "defimpl",
    "defmacro",
    "defmodule",
    "defp",
    "defprotocol",
    "defstruct",
    "do",
    "else",
    "end",
    "false",
    "for",
    "fn",
    "if",
    "import",
    "in",
    "nil",
    "not",
    "or",
    "raise",
    "receive",
    "require",
    "rescue",
    "self",
    "true",
    "try",
    "unless",
    "unquote",
    "use",
    "when",
    "with",
];
const SOLIDITY_KEYWORDS: &[&str] = &[
    "abstract",
    "address",
    "assert",
    "bool",
    "bytes",
    "calldata",
    "constructor",
    "contract",
    "delete",
    "emit",
    "enum",
    "event",
    "external",
    "fallback",
    "for",
    "function",
    "if",
    "immutable",
    "import",
    "interface",
    "internal",
    "library",
    "mapping",
    "memory",
    "modifier",
    "new",
    "payable",
    "pragma",
    "private",
    "public",
    "pure",
    "require",
    "revert",
    "return",
    "returns",
    "storage",
    "struct",
    "throw",
    "uint",
    "view",
    "while",
];
const GRAPHQL_KEYWORDS: &[&str] = &[
    "directive",
    "enum",
    "extend",
    "fragment",
    "input",
    "interface",
    "mutation",
    "on",
    "query",
    "repeatable",
    "scalar",
    "schema",
    "subscription",
    "type",
    "union",
];

/// 代码块语言专属的关键字表；未列出的语言用所属族的通用表。
pub(super) fn keywords_for(lang: &str) -> &'static [&'static str] {
    match lang_key(lang).as_str() {
        "dockerfile" | "docker" | "containerfile" => DOCKERFILE_KEYWORDS,
        "makefile" | "make" | "gmake" | "bsdmake" | "mk" => MAKEFILE_KEYWORDS,
        "cmake" | "cmakelists" => CMAKE_KEYWORDS,
        "terraform" | "tf" | "tfvars" | "hcl" | "nomad" | "vault" => TERRAFORM_KEYWORDS,
        "nginx" | "apache" | "httpd" | "htaccess" | "caddy" | "caddyfile" => NGINX_KEYWORDS,
        "lua" | "luau" | "openresty" => LUA_KEYWORDS,
        "haskell" | "hs" | "cabal" => HASKELL_KEYWORDS,
        "lisp" | "common-lisp" | "scheme" | "scm" | "racket" | "rkt" | "elisp" | "emacs"
        | "emacs-lisp" => LISP_KEYWORDS,
        "clojure" | "clj" | "cljs" => CLOJURE_KEYWORDS,
        "ruby" | "rb" | "jruby" | "rails" | "rake" | "gemfile" => RUBY_KEYWORDS,
        "go" | "golang" => GO_KEYWORDS,
        "latex" | "tex" | "ltx" | "cls" | "sty" | "dtx" | "beamer" | "bibtex" | "bib" => {
            LATEX_KEYWORDS
        }
        "matlab" | "octave" | "scilab" => MATLAB_KEYWORDS,
        "julia" | "jl" => JULIA_KEYWORDS,
        "r" | "rlang" | "rscript" => R_KEYWORDS,
        "elixir" | "ex" | "exs" | "phoenix" => ELIXIR_KEYWORDS,
        "solidity" | "sol" | "evm" => SOLIDITY_KEYWORDS,
        "graphql" | "gql" | "apollo" => GRAPHQL_KEYWORDS,
        _ => family_keywords(lang_family(lang)),
    }
}

/// 各语言的关键字表。大小写敏感：SQL 用大写，其余多为小写。
/// 按语言族区分，避免把 A 语言的关键字误标到 B 语言。
fn family_keywords(family: LangFamily) -> &'static [&'static str] {
    match family {
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

/// 从 `i` 起是否等于 `pat`（ASCII 大小写不敏感，让 batch 的 `rem` 也认 REM）。
fn matches_at(c: &[char], i: usize, pat: &str) -> bool {
    let mut k = i;
    for b in pat.chars() {
        match c.get(k) {
            Some(a) if a.eq_ignore_ascii_case(&b) => k += 1,
            _ => return false,
        }
    }
    true
}

/// 从 `i` 起是否是行注释（注释前缀按语言取自 `line_comments_for`）。
/// 前缀以字母结尾的（`rem`、`dnl`）要求后面是行尾或空白，否则 `remote` 会被当注释。
fn line_comment_at(prefixes: &[&str], c: &[char], i: usize) -> bool {
    prefixes.iter().any(|p| {
        if !matches_at(c, i, p) {
            return false;
        }
        let next = c.get(i + p.chars().count());
        !p.ends_with(|ch: char| ch.is_alphanumeric())
            || matches!(next, None | Some(' ') | Some('\t'))
    })
}

/// 从 `i` 起是否到了块注释的结束串，返回结束串之后的下标。
fn find_seq(c: &[char], from: usize, seq: &str) -> Option<usize> {
    let n = seq.chars().count();
    (from..=c.len().saturating_sub(n)).find(|&k| matches_at(c, k, seq))
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
    // 注释语法按语言查，而不是按配色族：同族之间差异比关键字还大（LaTeX 的 %、Lisp 的 ;、batch 的 rem）。
    let line_comments = line_comments_for(lang);
    let block = block_comment_for(lang);

    let mut job = LayoutJob::default();
    // 代码行常没有空格，允许在任意字符处换行以适配窄窗口。
    job.wrap.break_anywhere = true;
    let lines: Vec<&str> = code.split('\n').collect();
    // 跨行块注释状态：记着当前块注释的结束串。
    let mut in_block: Option<&'static str> = None;

    for (n, line) in lines.iter().enumerate() {
        let c: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < c.len() {
            // 块注释中：找结束串
            if let Some(close) = in_block {
                let n_close = close.chars().count();
                match find_seq(&c, i, close) {
                    Some(pos) => {
                        let end = pos + n_close;
                        let rest: String = c[i..end].iter().collect();
                        let mut f = theme.mono_fmt();
                        f.color = comment_color;
                        job.append(&rest, 0.0, f);
                        i = end;
                        in_block = None;
                    }
                    None => {
                        let rest: String = c[i..].iter().collect();
                        let mut f = theme.mono_fmt();
                        f.color = comment_color;
                        job.append(&rest, 0.0, f);
                        break;
                    }
                }
                continue;
            }
            // 块注释开始（先于行注释：Lua 的 --[[ 前面就是 --）
            if let Some((open, close)) = block
                && matches_at(&c, i, open)
            {
                in_block = Some(close);
                let n_open = open.chars().count();
                let rest: String = c[i..i + n_open].iter().collect();
                let mut f = theme.mono_fmt();
                f.color = comment_color;
                job.append(&rest, 0.0, f);
                i += n_open;
                continue;
            }
            // 行注释
            if line_comment_at(line_comments, &c, i) {
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
