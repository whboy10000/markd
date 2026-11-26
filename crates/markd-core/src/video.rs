//! 视频外链：把哔哩哔哩的分享链接解析成可嵌入的播放器地址。
//!
//! 解析是纯字符串工作，不发网络请求——渲染层每帧都会重画，这里必须廉价。
//! 封面与标题走 [`fetch_video_info`]，由渲染层的后台线程取一次并缓存。

/// 一段可嵌入的哔哩哔哩视频。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoSource {
    /// 普通投稿视频：BV 号 + 分 P 序号（从 1 开始）。
    Bv { bvid: String, page: u32 },
    /// 旧式 `av` 号链接。
    Av { aid: String, page: u32 },
    /// 番剧 / 电影 / 国创的单集（`ep` 号）。
    Ep { ep_id: String },
    /// 番剧季度（`ss` 号）：播放器自己定位到第一集。
    Ss { season_id: String },
}

/// 校验数字 ID：只允许 ASCII 数字，避免把用户原文拼进 URL。
fn digits(s: &str) -> Option<String> {
    if s.is_empty() || s.len() > 20 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(s.to_string())
}

/// BV 号：`BV` 开头，后跟 8–12 位大小写字母与数字（BV 号大小写有意义，原样保留）。
fn bvid(s: &str) -> Option<String> {
    let rest = s.strip_prefix("BV")?;
    if rest.len() < 8 || rest.len() > 12 || !rest.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(s.to_string())
}

/// 取 `?` 之前的路径部分，查询串单独留下（`?p=3` 是分 P）。
fn split_url(s: &str) -> (&str, &str) {
    match s.find('?') {
        Some(i) => {
            let (path, tail) = s.split_at(i);
            (path, &tail[1..])
        }
        None => (s, ""),
    }
}

/// 查询串里按 key 取值（`p` / `page` / `bvid` …）。
fn query_of<'a>(query: &'a str, keys: &[&str]) -> Option<&'a str> {
    for part in query.split(['&', '#']) {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if keys.iter().any(|want| k.eq_ignore_ascii_case(want)) {
            return Some(v);
        }
    }
    None
}

/// 分 P 序号（`p` 或 `page`），缺失或越界按 1。
fn page_of(query: &str) -> u32 {
    query_of(query, &["p", "page"])
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|n| (1..=10_000).contains(n))
        .unwrap_or(1)
}

/// 只在分 P 类链接上覆盖页码；番剧的 `page` 由播放器自己决定。
fn with_page(src: VideoSource, page: u32) -> VideoSource {
    match src {
        VideoSource::Bv { bvid, .. } => VideoSource::Bv { bvid, page },
        VideoSource::Av { aid, .. } => VideoSource::Av { aid, page },
        other => other,
    }
}

/// 路径里的标识段：`/video/BV…`、`/video/av…`、`/bangumi/play/ep…`。
fn from_path(path: &str) -> Option<VideoSource> {
    let rest = path.split("bilibili.com").nth(1)?;
    let mut it = rest.split('/').filter(|s| !s.is_empty());
    while let Some(seg) = it.next() {
        let ids = |id: &str| match id.strip_prefix("av").and_then(digits) {
            Some(aid) => Some(VideoSource::Av { aid, page: 1 }),
            None => bvid(id).map(|bvid| VideoSource::Bv { bvid, page: 1 }),
        };
        match seg {
            "video" | "sv" => {
                let id = it.next()?;
                return ids(id);
            }
            "bangumi" | "media" | "pgc" => {
                // 路径还有 /play/ 这一段，逐段找到第一个 ep/ss 标识为止。
                for id in it.by_ref() {
                    if let Some(ep_id) = id.strip_prefix("ep").and_then(digits) {
                        return Some(VideoSource::Ep { ep_id });
                    }
                    if let Some(season_id) = id.strip_prefix("ss").and_then(digits) {
                        return Some(VideoSource::Ss { season_id });
                    }
                }
                return None;
            }
            _ => {}
        }
    }
    None
}

/// 播放器地址反解：`player.bilibili.com/player.html?bvid=BV…&p=2` 也算有效输入。
fn from_query(query: &str) -> Option<VideoSource> {
    if let Some(b) = query_of(query, &["bvid"]).and_then(bvid) {
        return Some(VideoSource::Bv { bvid: b, page: 1 });
    }
    if let Some(a) = query_of(query, &["aid"]).and_then(digits) {
        return Some(VideoSource::Av { aid: a, page: 1 });
    }
    if let Some(e) = query_of(query, &["ep_id", "epid"]).and_then(digits) {
        return Some(VideoSource::Ep { ep_id: e });
    }
    if let Some(s) = query_of(query, &["season_id", "seasonid"]).and_then(digits) {
        return Some(VideoSource::Ss { season_id: s });
    }
    None
}

/// 从任意文本里挑出 URL：分享文案常见「【标题】https://… 复制此链接打开」。
fn extract_url(s: &str) -> Option<&str> {
    let idx = s.find("bilibili.com")?;
    let start = s[..idx]
        .rfind([' ', '\t', '\n', '(', '[', '<', '"', '\''])
        .map_or(0, |i| i + 1);
    let rest = &s[start..];
    let end = rest
        .find([' ', '\t', '\n', ')', ']', '>', '"', '\''])
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// 裸 ID：用户只复制了 `BV1GJ411x7h7` 或 `ep406221`。
fn from_bare(s: &str) -> Option<VideoSource> {
    let head = s.split(['?', '#']).next().unwrap_or(s).trim();
    if let Some(b) = bvid(head) {
        return Some(VideoSource::Bv { bvid: b, page: 1 });
    }
    if let Some(a) = head.strip_prefix("av").and_then(digits) {
        return Some(VideoSource::Av { aid: a, page: 1 });
    }
    if let Some(e) = head.strip_prefix("ep").and_then(digits) {
        return Some(VideoSource::Ep { ep_id: e });
    }
    if let Some(ss) = head.strip_prefix("ss").and_then(digits) {
        return Some(VideoSource::Ss { season_id: ss });
    }
    None
}

/// 解析用户粘贴的内容：完整链接、带统计参数的分享链接、播放器地址、裸 ID。
///
/// 认不出的输入返回 `None`，由调用方提示用户，不会插入半截视频块。
pub fn parse_video(input: &str) -> Option<VideoSource> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    // 裸 ID 优先：`av` / `ep` 前面没有域名时也要认。
    let (_, top_query) = split_url(s);
    if let Some(src) = from_bare(s) {
        return Some(with_page(src, page_of(top_query)));
    }
    let url = extract_url(s)?;
    let (path, query) = split_url(url);
    let src = from_path(path).or_else(|| from_query(query))?;
    Some(with_page(src, page_of(query)))
}

impl VideoSource {
    /// 嵌入式播放器地址：导出 HTML 时放进 `<iframe src>`。
    /// `isOutside=true` 是 B 站官方「嵌入」分享给出的参数。
    pub fn player_url(&self) -> String {
        match self {
            VideoSource::Bv { bvid, page } => format!(
                "https://player.bilibili.com/player.html?isOutside=true&bvid={bvid}&p={page}&high_quality=1&danmaku=0"
            ),
            VideoSource::Av { aid, page } => format!(
                "https://player.bilibili.com/player.html?isOutside=true&aid={aid}&p={page}&high_quality=1&danmaku=0"
            ),
            VideoSource::Ep { ep_id } => format!(
                "https://player.bilibili.com/player.html?isOutside=true&ep_id={ep_id}&page=1&high_quality=1&danmaku=0"
            ),
            VideoSource::Ss { season_id } => format!(
                "https://player.bilibili.com/player.html?isOutside=true&season_id={season_id}&page=1&high_quality=1&danmaku=0"
            ),
        }
    }

    /// 网页播放地址：卡片点击后用系统浏览器打开。
    pub fn page_url(&self) -> String {
        match self {
            VideoSource::Bv { bvid, page } => {
                format!("https://www.bilibili.com/video/{bvid}/?p={page}")
            }
            VideoSource::Av { aid, page } => {
                format!("https://www.bilibili.com/video/av{aid}/?p={page}")
            }
            VideoSource::Ep { ep_id } => format!("https://www.bilibili.com/bangumi/play/ep{ep_id}"),
            VideoSource::Ss { season_id } => {
                format!("https://www.bilibili.com/bangumi/play/ss{season_id}")
            }
        }
    }

    /// 元数据接口：投稿视频走 web-interface，番剧走 pgc。
    pub fn api_url(&self) -> String {
        match self {
            VideoSource::Bv { bvid, .. } => {
                format!("https://api.bilibili.com/x/web-interface/view?bvid={bvid}")
            }
            VideoSource::Av { aid, .. } => {
                format!("https://api.bilibili.com/x/web-interface/view?aid={aid}")
            }
            VideoSource::Ep { ep_id } => {
                format!("https://api.bilibili.com/pgc/view/web/season?ep_id={ep_id}")
            }
            VideoSource::Ss { season_id } => {
                format!("https://api.bilibili.com/pgc/view/web/season?season_id={season_id}")
            }
        }
    }

    /// 视频标识，用于卡片副标题与缓存键。
    pub fn id(&self) -> String {
        match self {
            VideoSource::Bv { bvid, .. } => bvid.clone(),
            VideoSource::Av { aid, .. } => format!("av{aid}"),
            VideoSource::Ep { ep_id } => format!("ep{ep_id}"),
            VideoSource::Ss { season_id } => format!("ss{season_id}"),
        }
    }

    /// 番剧接口返回整季的分集列表：命中 `ep_id` 才能拿到本集标题与时长。
    fn episode_id(&self) -> Option<&str> {
        match self {
            VideoSource::Ep { ep_id } => Some(ep_id.as_str()),
            _ => None,
        }
    }
}

/// 卡片显示的元数据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoInfo {
    pub title: String,
    pub cover: String,
    /// 时长（秒）；0 表示接口没给，卡片不显示角标。
    pub duration: u64,
}

impl VideoInfo {
    /// 接口对投稿视频给秒、对番剧给毫秒，统一成 `4:31` / `1:02:33`。
    pub fn duration_text(&self) -> String {
        if self.duration == 0 {
            return String::new();
        }
        let (h, m, s) = (
            self.duration / 3600,
            (self.duration % 3600) / 60,
            self.duration % 60,
        );
        if h > 0 {
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{m}:{s:02}")
        }
    }
}

/// 秒/毫秒混合的时长字段统一成秒。
fn to_secs(v: u64) -> u64 {
    if v > 10_000 { v / 1000 } else { v }
}

/// 从元数据接口的响应里取标题与封面。
///
/// 两套接口结构不同：投稿视频是 `data.{title,pic,duration}`；
/// 番剧是 `result.{title,cover}` + `result.episodes[]`，按 `ep_id` 定位单集。
pub fn video_info(json: &serde_json::Value, ep_id: Option<&str>) -> Option<VideoInfo> {
    if json.get("code").and_then(|c| c.as_i64()) != Some(0) {
        return None;
    }
    if let Some(d) = json.get("data") {
        let title = d
            .get("title")
            .and_then(|t| t.as_str())
            .filter(|s| !s.is_empty())?;
        let cover = d
            .get("pic")
            .and_then(|c| c.as_str())
            .filter(|s| !s.is_empty())?;
        return Some(VideoInfo {
            title: title.to_string(),
            cover: cover.to_string(),
            duration: to_secs(d.get("duration").and_then(|v| v.as_u64()).unwrap_or(0)),
        });
    }
    let r = json.get("result")?;
    let season = r.get("title").and_then(|t| t.as_str()).unwrap_or("");
    let season_cover = r.get("cover").and_then(|c| c.as_str()).unwrap_or("");
    let episodes = r.get("episodes").and_then(|e| e.as_array());
    if let Some(list) = episodes {
        let ep = ep_id
            .and_then(|want| {
                list.iter().find(|e| {
                    e.get("ep_id")
                        .and_then(|v| v.as_u64())
                        .map(|v| v.to_string() == want)
                        .unwrap_or(false)
                })
            })
            .or_else(|| list.first());
        if let Some(ep) = ep {
            let ep_title = ["long_title", "show_title", "title"]
                .iter()
                .filter_map(|k| ep.get(*k).and_then(|t| t.as_str()))
                .find(|s| !s.is_empty())
                .unwrap_or("");
            let ep_cover = ep.get("cover").and_then(|c| c.as_str()).unwrap_or("");
            let duration = to_secs(ep.get("duration").and_then(|v| v.as_u64()).unwrap_or(0));
            let title = if ep_title.is_empty() || ep_title == season {
                season.to_string()
            } else if season.is_empty() {
                ep_title.to_string()
            } else {
                format!("{season}·{ep_title}")
            };
            let cover = if ep_cover.is_empty() {
                season_cover.to_string()
            } else {
                ep_cover.to_string()
            };
            if !title.is_empty() && !cover.is_empty() {
                return Some(VideoInfo {
                    title,
                    cover,
                    duration,
                });
            }
        }
    }
    if season.is_empty() || season_cover.is_empty() {
        return None;
    }
    Some(VideoInfo {
        title: season.to_string(),
        cover: season_cover.to_string(),
        duration: 0,
    })
}

/// 取封面与标题；网络失败或结构变化返回 `None`，卡片退化成纯链接样式。
pub fn fetch_video_info(src: &VideoSource) -> Option<VideoInfo> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .into();
    let mut resp = agent
        .get(&src.api_url())
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
        )
        .call()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp
        .body_mut()
        .with_config()
        .limit(4 << 20)
        .read_to_string()
        .ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    video_info(&json, src.episode_id())
}

/// 把视频写成围栏代码块：源码态看得到链接，渲染态是播放器卡片。
pub fn video_block(src: &VideoSource) -> String {
    format!("```video\n{}\n```", src.page_url())
}

/// `lang` 是不是视频围栏。
pub fn is_video_lang(lang: &str) -> bool {
    matches!(
        lang.trim().to_ascii_lowercase().as_str(),
        "video" | "bilibili" | "b站"
    )
}

/// 从围栏内容里取出链接（内容就是一行 URL 或裸 ID）。
pub fn url_from_fence(code: &str) -> Option<VideoSource> {
    code.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .and_then(parse_video)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用户给的例子：番剧单集链接解析成 ep 源，播放器地址带 ep_id。
    #[test]
    fn parses_bangumi_episode() {
        let src = parse_video("https://www.bilibili.com/bangumi/play/ep406221").unwrap();
        assert_eq!(
            src,
            VideoSource::Ep {
                ep_id: "406221".into()
            }
        );
        assert_eq!(src.id(), "ep406221");
        let p = src.player_url();
        assert!(
            p.starts_with("https://player.bilibili.com/player.html?"),
            "{p}"
        );
        assert!(p.contains("isOutside=true"), "{p}");
        assert!(p.contains("ep_id=406221"), "{p}");
        assert_eq!(
            src.page_url(),
            "https://www.bilibili.com/bangumi/play/ep406221"
        );
        assert!(src.api_url().contains("pgc/view/web/season?ep_id=406221"));
    }

    /// 分享链接后面拖着一串统计参数，也要能认出来。
    #[test]
    fn parses_share_link_with_tracking_params() {
        let src = parse_video(
            "https://www.bilibili.com/video/BV1GJ411x7h7?spm_id_from=333.788.videopod.episodes&vd_source=abc123",
        )
        .unwrap();
        assert_eq!(
            src,
            VideoSource::Bv {
                bvid: "BV1GJ411x7h7".into(),
                page: 1
            }
        );
        assert!(src.player_url().contains("bvid=BV1GJ411x7h7"));
    }

    /// `?p=3` 是分 P，不能丢：否则永远从第一 P 播。
    #[test]
    fn keeps_page() {
        let src = parse_video("https://www.bilibili.com/video/BV1GJ411x7h7?p=3").unwrap();
        assert_eq!(
            src,
            VideoSource::Bv {
                bvid: "BV1GJ411x7h7".into(),
                page: 3
            }
        );
        assert!(src.player_url().contains("&p=3"), "{}", src.player_url());
        assert!(src.page_url().contains("?p=3"), "{}", src.page_url());
        // 旧式 av 号同样保留页码
        let src = parse_video("https://www.bilibili.com/video/av113414/page/2").unwrap();
        assert!(matches!(src, VideoSource::Av { .. }), "{src:?}");
    }

    /// 大小写敏感的 BV 号要原样保留（BV 号是编码，改一位就是另一个视频）。
    #[test]
    fn keeps_bvid_case() {
        let src = parse_video("https://www.bilibili.com/video/BV1aB4y1A7Mk").unwrap();
        assert_eq!(
            src,
            VideoSource::Bv {
                bvid: "BV1aB4y1A7Mk".into(),
                page: 1
            }
        );
        assert!(src.player_url().contains("bvid=BV1aB4y1A7Mk"));
    }

    /// 裸 ID、季度号、播放器地址都能插。
    #[test]
    fn parses_other_forms() {
        assert_eq!(
            parse_video("BV1GJ411x7h7").unwrap(),
            VideoSource::Bv {
                bvid: "BV1GJ411x7h7".into(),
                page: 1
            }
        );
        assert_eq!(
            parse_video("av113414").unwrap(),
            VideoSource::Av {
                aid: "113414".into(),
                page: 1
            }
        );
        assert_eq!(
            parse_video("https://www.bilibili.com/bangumi/play/ss33409").unwrap(),
            VideoSource::Ss {
                season_id: "33409".into()
            }
        );
        assert_eq!(
            parse_video(
                "https://player.bilibili.com/player.html?isOutside=true&bvid=BV1GJ411x7h7&p=2"
            )
            .unwrap(),
            VideoSource::Bv {
                bvid: "BV1GJ411x7h7".into(),
                page: 2
            }
        );
        // 带前后空白与换行的粘贴同样可用
        assert!(parse_video("  BV1GJ411x7h7\n").is_some());
    }

    /// 认不出的输入必须返回 None：不能把任意网址当成 B 站视频插进文档。
    #[test]
    fn rejects_non_bilibili() {
        for bad in [
            "",
            "   ",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://example.com/bilibili",
            "https://www.bilibili.com/",
            "https://www.bilibili.com/video/",
            "https://www.bilibili.com/video/BV!bad",
            "https://www.bilibili.com/bangumi/play/epabc",
            "BV1",
            "av",
            "ep",
        ] {
            assert!(parse_video(bad).is_none(), "{bad:?} 不应被认成视频");
        }
    }

    /// 数字 ID 只接受纯数字，防止把 `../../x` 之类拼进 URL。
    #[test]
    fn id_charset_is_strict() {
        assert!(parse_video("https://www.bilibili.com/bangumi/play/ep1?x=%2F..").is_some());
        assert!(digits("12;3").is_none());
        assert!(digits("").is_none());
        assert!(bvid("BV123").is_none());
    }

    /// 投稿视频接口（`data.{title,pic,duration}`）：实测响应形状。
    #[test]
    fn info_from_view_api() {
        let json = serde_json::json!({
            "code": 0,
            "message": "OK",
            "data": {
                "bvid": "BV1GJ411x7h7",
                "title": "【官方 MV】Never Gonna Give You Up - Rick Astley",
                "pic": "http://i1.hdslb.com/bfs/archive/524275.jpg",
                "duration": 213
            }
        });
        let info = video_info(&json, None).unwrap();
        assert_eq!(
            info.title,
            "【官方 MV】Never Gonna Give You Up - Rick Astley"
        );
        assert!(info.cover.ends_with(".jpg"));
        assert_eq!(info.duration, 213);
        assert_eq!(info.duration_text(), "3:33");
    }

    /// 番剧接口（`result` + `episodes[]`）：按 ep_id 命中本集，剧名拼分集名。
    #[test]
    fn info_from_season_api() {
        let json = serde_json::json!({
            "code": 0,
            "result": {
                "title": "康熙来了 2007",
                "cover": "http://i0.hdslb.com/bfs/bangumi/image/season.png",
                "episodes": [
                    {"ep_id": 406118, "long_title": "第一集", "cover": "http://i0.hdslb.com/a.png", "duration": 2671000},
                    {"ep_id": 406221, "long_title": "史上最嗜赌女星，扑克牌不离手",
                     "cover": "http://i0.hdslb.com/bfs/archive/ep.png", "duration": 2605000,
                     "bvid": "BV1kh411e75d"}
                ]
            }
        });
        let info = video_info(&json, Some("406221")).unwrap();
        assert_eq!(info.title, "康熙来了 2007·史上最嗜赌女星，扑克牌不离手");
        assert!(info.cover.contains("ep.png"), "{}", info.cover);
        // 番剧时长是毫秒，卡片按秒显示
        assert_eq!(info.duration, 2605);
        assert_eq!(info.duration_text(), "43:25");
        // 单集封面缺失时退回季度封面
        let no_cover = serde_json::json!({"code":0,"result":{"title":"剧","cover":"http://c.png","episodes":[{"ep_id":1,"long_title":"集","cover":""}]}});
        assert_eq!(
            video_info(&no_cover, Some("1")).unwrap().cover,
            "http://c.png"
        );
    }

    /// 接口报错、缺字段、缺封面时不能崩，也不能返回空卡片。
    #[test]
    fn info_missing_fields() {
        assert!(video_info(&serde_json::json!({"code":-404,"message":"啥都木有"}), None).is_none());
        assert!(video_info(&serde_json::json!({}), None).is_none());
        assert!(
            video_info(
                &serde_json::json!({"code":0,"data":{"title":"无封面"}}),
                None
            )
            .is_none()
        );
        assert!(video_info(&serde_json::json!({"code":0,"result":{}}), None).is_none());
        // 分 P 视频接口不给 duration 时不显示角标
        let d = serde_json::json!({"code":0,"data":{"title":"t","pic":"http://c.jpg"}});
        assert_eq!(video_info(&d, None).unwrap().duration_text(), "");
    }

    /// 插入的围栏源码要能被重新解析回同一个视频（无损往返）。
    #[test]
    fn block_round_trips() {
        let src = parse_video("https://www.bilibili.com/bangumi/play/ep406221").unwrap();
        let md = video_block(&src);
        assert_eq!(
            md,
            "```video\nhttps://www.bilibili.com/bangumi/play/ep406221\n```"
        );
        let crate::ast::Block::Code { lang, code, .. } = &crate::ast::parse(&md)[0] else {
            panic!("围栏应解析为代码块：{:?}", crate::ast::parse(&md));
        };
        assert!(is_video_lang(lang), "{lang}");
        assert_eq!(url_from_fence(code).unwrap(), src);
        assert!(url_from_fence("\n\n  \n").is_none());
    }

    /// 真联网校验接口字段（默认不跑，`cargo test -p markd-core -- --ignored` 手动验证）。
    /// B 站改结构时这条会先于用户发现。
    #[test]
    #[ignore]
    fn fetches_live_metadata() {
        let ep = parse_video("https://www.bilibili.com/bangumi/play/ep406221").unwrap();
        let info = fetch_video_info(&ep).expect("番剧元数据");
        assert!(info.title.contains("康熙来了"), "{}", info.title);
        assert!(info.cover.contains("hdslb.com"), "{}", info.cover);
        let bv = parse_video("https://www.bilibili.com/video/BV1GJ411x7h7").unwrap();
        let info = fetch_video_info(&bv).expect("投稿视频元数据");
        assert!(
            info.title.contains("Never Gonna Give You Up"),
            "{}",
            info.title
        );
        assert_eq!(info.duration, 213);
    }
}
