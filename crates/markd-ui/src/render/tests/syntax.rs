use super::*;

/// 15 种语言的关键字/函数/类型/标签都要被着色，且编辑态文本与缓冲区一致。
#[test]
fn code_highlights_all_supported_languages() {
    let theme = Theme::get(ThemeKind::Dark);
    let cases: &[(&str, &str, &[&str])] = &[
        ("rust", "fn main() { let x = 1; }", &["fn", "let", "main"]),
        (
            "python",
            "def foo():\n    return 42",
            &["def", "return", "foo"],
        ),
        ("bash", "echo hi", &["echo"]),
        ("zsh", "if true; then\nfi", &["if", "then", "fi"]),
        ("java", "public class Main {}", &["public", "class", "Main"]),
        ("xml", "<tag attr=\"v\">t</tag>", &["tag"]),
        (
            "go",
            "package main\nfunc run() {}",
            &["package", "func", "run"],
        ),
        ("c", "int main() { return 0; }", &["int", "main", "return"]),
        (
            "c++",
            "int main() { return 0; }",
            &["int", "main", "return"],
        ),
        ("css", "body { color: red; }", &["color"]),
        ("yml", "key: value", &["key"]),
        ("yaml", "key: value", &["key"]),
        ("html", "<div id=\"x\">hi</div>", &["div"]),
        (
            "sql",
            "SELECT * FROM users WHERE id = 1",
            &["SELECT", "FROM", "WHERE"],
        ),
        (
            "vue",
            "<template><div>hi</div></template>",
            &["template", "div"],
        ),
    ];
    for &(lang, code, toks) in cases {
        let edit = code_job_edit(code, lang, &theme);
        assert_eq!(edit.text, code, "编辑态文本必须与缓冲区一致: {lang}");
        let job = code_job(code, lang, &theme);
        assert!(job.sections.len() > 2, "{lang} 应产生多段高亮: {job:?}");
        for tok in toks {
            let pos = code.find(tok).expect("token 应在代码里");
            let colored = job.sections.iter().any(|s| {
                let a = s.byte_range.start.0;
                let b = s.byte_range.end.0;
                a <= pos && pos < b && s.format.color != theme.text && s.format.color != theme.dim
            });
            assert!(colored, "{lang} 的 `{tok}` 应被着色: {job:?}");
        }
    }
}

/// 各语言的注释语法（# / // / -- / % / ; / rem / /* */ / <!-- -->）都要按注释色渲染。
#[test]
fn code_highlights_comments_across_languages() {
    let theme = Theme::get(ThemeKind::Dark);
    let cases: &[(&str, &str, &str)] = &[
        ("rust", "// 注释", "// 注释"),
        ("python", "# 注释", "# 注释"),
        ("bash", "# 注释", "# 注释"),
        ("sql", "-- 注释", "-- 注释"),
        ("html", "<!-- 注释 -->", "<!-- 注释 -->"),
        ("c", "/* 注释 */", "/* 注释 */"),
        ("dockerfile", "# 注释", "# 注释"),
        ("yaml", "# 注释", "# 注释"),
        ("lua", "-- 注释", "-- 注释"),
        ("haskell", "-- 注释", "-- 注释"),
        ("latex", "% 注释", "% 注释"),
        ("matlab", "% 注释", "% 注释"),
        ("ini", "; 注释", "; 注释"),
        ("asm", "; 注释", "; 注释"),
        ("clojure", "; 注释", "; 注释"),
        ("fortran", "! 注释", "! 注释"),
        ("bat", "rem 注释", "rem 注释"),
        ("mermaid", "%% 注释", "%% 注释"),
        ("julia", "# 注释", "# 注释"),
        ("pascal", "{ 注释 }", "{ 注释 }"),
        ("ocaml", "(* 注释 *)", "(* 注释 *)"),
        ("markdown", "https://example.com", ""),
    ];
    for &(lang, code, comment) in cases {
        let [_, _, _, comment_color, _, _] = theme.syntax(lang);
        let job = code_job(code, lang, &theme);
        if comment.is_empty() {
            // 正文型语言没有注释语法，`//` 不该被当注释染掉
            assert!(
                job.sections.iter().all(|s| s.format.color != comment_color),
                "{lang} 不应出现注释色：{job:?}"
            );
            continue;
        }
        let pos = job.text.find(comment).expect("注释应在代码里");
        let is_comment_color = job.sections.iter().any(|s| {
            let a = s.byte_range.start.0;
            let b = s.byte_range.end.0;
            a <= pos && pos < b && s.format.color == comment_color
        });
        assert!(is_comment_color, "{lang} 的注释应按注释色渲染: {job:?}");
    }
}

/// 语言专属关键字表：这些词不属于所属配色族的通用表，只靠专属表才会上色。
#[test]
fn code_highlights_language_specific_keywords() {
    let theme = Theme::get(ThemeKind::Dark);
    let cases: &[(&str, &str, &[&str])] = &[
        (
            "dockerfile",
            "FROM rust:1.85 AS build\nRUN cargo build",
            &["FROM", "RUN"],
        ),
        (
            "makefile",
            "ifeq ($(OS),Linux)\n\tinclude mk/common.mk\nendif",
            &["ifeq", "include", "endif"],
        ),
        (
            "cmake",
            "add_executable(app main.c)\ntarget_link_libraries(app m)",
            &["add_executable"],
        ),
        (
            "terraform",
            "resource \"aws_s3_bucket\" \"b\" {\n  count = 1\n}",
            &["resource", "count"],
        ),
        (
            "nginx",
            "server {\n  location / { proxy_pass http://up; }\n}",
            &["server", "location", "proxy_pass"],
        ),
        (
            "lua",
            "local function f(x)\n  if x then return 1 end\nend",
            &["local", "function", "then"],
        ),
        (
            "haskell",
            "module Main where\nmain = putStrLn \"hi\"",
            &["module", "where"],
        ),
        ("ruby", "def call\n  rescue => e\n  end", &["def", "rescue"]),
        (
            "go",
            "func main() {\n  ch := make(chan int)\n  go run(ch)\n}",
            &["func", "chan", "go"],
        ),
        (
            "latex",
            "\\begin{itemize}\\item \\textbf{x}\\end{itemize}",
            &["begin", "item", "textbf"],
        ),
        (
            "matlab",
            "function y = f(x)\n  if x, y = 1; end\nend",
            &["function", "end"],
        ),
        (
            "r",
            "f <- function(x) { if (is.na(x)) return(NA) }",
            &["function", "return"],
        ),
        (
            "julia",
            "function f(x)::Int\n  for i in 1:3 end\nend",
            &["function", "for"],
        ),
        (
            "elixir",
            "defmodule A do\n  def f(x), do: x\nend",
            &["defmodule", "def", "do", "end"],
        ),
        (
            "clojure",
            "(defn f [x] (when x (println x)))",
            &["defn", "when"],
        ),
        (
            "solidity",
            "contract A {\n  mapping(address => uint) m;\n}",
            &["contract", "mapping"],
        ),
        (
            "graphql",
            "type Query {\n  user(id: ID!): User\n}",
            &["type"],
        ),
    ];
    for &(lang, code, toks) in cases {
        let job = code_job(code, lang, &theme);
        for tok in toks {
            let Some(pos) = job.text.find(tok) else {
                panic!("{lang} 的代码里找不到 `{tok}`：{:?}", job.text);
            };
            let colored = job.sections.iter().any(|s| {
                let a = s.byte_range.start.0;
                let b = s.byte_range.end.0;
                a <= pos && pos < b && s.format.color == theme.syntax(lang)[0]
            });
            assert!(colored, "{lang} 的 `{tok}` 应按关键字色渲染: {job:?}");
        }
    }
}

/// 代码块在编辑态复用同一套高亮排版：排版文本必须与缓冲区完全一致，
/// 否则 TextEdit 的光标/选中会错位；同时应当有多段不同颜色。
#[test]
fn code_job_keeps_raw_text_and_colors_keywords() {
    let theme = Theme::get(ThemeKind::Dark);
    let src = "fn main() {\n    let x = \"s\"; // 注释\n}";
    // 编辑态：排版文本必须和缓冲区逐字符一致，否则光标/选中会错位
    let edit = code_job_edit(src, "rust", &theme);
    assert_eq!(edit.text, src);
    assert!(edit.sections.len() > 1, "应当有多个着色段");
    // 展示态：末尾多补一个换行，当作代码框的底部留白
    assert_eq!(code_job(src, "rust", &theme).text, format!("{src}\n"));
}
