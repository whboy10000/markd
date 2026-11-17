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

/// 各语言的注释语法（# / // / -- / /* */ / <!-- -->）都要按注释色渲染。
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
    ];
    for &(lang, code, comment) in cases {
        let [_, _, _, comment_color, _, _] = theme.syntax(lang);
        let job = code_job(code, lang, &theme);
        let pos = job.text.find(comment).expect("注释应在代码里");
        let is_comment_color = job.sections.iter().any(|s| {
            let a = s.byte_range.start.0;
            let b = s.byte_range.end.0;
            a <= pos && pos < b && s.format.color == comment_color
        });
        assert!(is_comment_color, "{lang} 的注释应按注释色渲染: {job:?}");
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
