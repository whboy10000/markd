use super::*;

/// 下拉表与核心高亮表不能脱节：别名必须与规范名解析出同一个配色族、
/// 同一套注释语法、同一张关键字表。漏在核心表里的别名会在这里一次性全列出来。
#[test]
fn language_aliases_resolve_like_their_id() {
    let mut bad: Vec<String> = Vec::new();
    for o in LANGUAGE_OPTIONS {
        for a in o.aliases {
            let mut check = |what: &str, same: bool| {
                if !same {
                    bad.push(format!("{a} → {} 的 {what}不一致", o.id));
                }
            };
            check("配色族", lang_family(a) == lang_family(o.id));
            check("行注释", line_comments_for(a) == line_comments_for(o.id));
            check("块注释", block_comment_for(a) == block_comment_for(o.id));
            check("关键字表", keywords_for(a) == keywords_for(o.id));
        }
    }
    assert!(
        bad.is_empty(),
        "这些别名没进核心表（theme::lang_family / line_comments_for / block_comment_for / render::syntax::keywords_for）：\n{}",
        bad.join("\n")
    );
}

/// 表本身的约束：id 唯一且小写，别名非空/小写且不占用别的 id；
/// 展平后每个写法各成一行，行文本不重复。
#[test]
fn language_table_has_no_conflicting_entries() {
    let ids: Vec<&str> = LANGUAGE_OPTIONS.iter().map(|o| o.id).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    let dup = sorted
        .windows(2)
        .filter(|w| w[0] == w[1])
        .map(|w| w[0])
        .collect::<Vec<_>>();
    assert!(dup.is_empty(), "重复的语言 id: {dup:?}");
    for o in LANGUAGE_OPTIONS {
        assert_eq!(o.id, lang_key(o.id), "语言 id 应是小写规范名: {:?}", o.id);
        for a in o.aliases {
            assert!(!a.is_empty(), "{:?} 的别名里有空串", o.id);
            assert_eq!(a, &a.to_lowercase(), "别名应是小写: {a}");
            assert!(
                !ids.contains(a),
                "别名 `{a}`（{} 的）与另一个语言 id 冲突",
                o.id
            );
        }
    }

    let rows = lang_rows();
    let aliases = LANGUAGE_OPTIONS
        .iter()
        .map(|o| o.aliases.len())
        .sum::<usize>();
    assert_eq!(rows.len(), ids.len() + aliases, "规范名与别名应各自成一行");
    let mut texts: Vec<&str> = rows.iter().map(|r| r.text).collect();
    let before = texts.len();
    texts.sort_unstable();
    texts.dedup();
    assert_eq!(texts.len(), before, "展平后有重复写法");
    // 覆盖度：用户报的「太少」以可选项数量为硬指标
    assert!(
        rows.len() > 400,
        "语言项应覆盖常见语言的全部写法，实际只有 {} 行",
        rows.len()
    );
}

/// 用户点名的几种写法：都单独成一行，点它就把这一行的写法写进围栏。
#[test]
fn language_rows_include_every_spelling() {
    let rows = lang_rows();
    let texts: Vec<&str> = rows.iter().map(|r| r.text).collect();
    for want in [
        "cpp",
        "c++",
        "cxx",
        "cc",
        "dart",
        "flutter",
        "dockerfile",
        "docker",
        "containerfile",
        "yaml",
        "yml",
        "k8s",
        "github-actions",
        "python",
        "py",
        "go",
        "golang",
        "csharp",
        "c#",
        "bash",
        "shell",
        "shell-session",
        "mermaid",
        "plantuml",
        "",
    ] {
        assert!(texts.contains(&want), "下拉里应有 `{want}` 这一行");
    }
    // 别名行带得出规范名，供「与 dart 等价」提示用
    let row = rows
        .iter()
        .find(|r| r.text == "flutter")
        .expect("flutter 行");
    assert_eq!(row.canonical, "dart");
}

/// 搜索框按子串过滤：空关键词不过滤，无关关键词清空。
#[test]
fn language_search_filters_rows() {
    let hit = |q: &str| -> Vec<&'static str> {
        let q = q.to_lowercase();
        lang_rows()
            .iter()
            .filter(|r| lang_hit(r, &q))
            .map(|r| r.text)
            .collect()
    };
    // 子串匹配：搜 C++ 也会带出 objective-c++，这是想要的（两种都能点）
    assert!(hit("C++").contains(&"c++"));
    assert_eq!(hit("flutter"), vec!["flutter"]);
    assert_eq!(
        hit("docker"),
        vec!["docker", "docker-compose", "dockerfile"]
    );
    assert_eq!(hit("纯文本"), vec![""]);
    assert!(hit("sh").contains(&"bash"));
    assert!(hit("sh").contains(&"shell"));
    assert_eq!(hit("").len(), lang_rows().len());
    assert!(hit("不存在的语言").is_empty());
}
