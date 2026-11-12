//! 读分享页 `www.iesdouyin.com/share/video/<id>`（手机 UA）里的数据。
//!
//! 数据在 `<script>window._ROUTER_DATA = {…}</script>` 里，作品在
//! `loaderData["video_(id)/page"].videoInfoRes.item_list[0]`，看不到的作品在同一处的 `filter_list`。
//!
//! 来源（MIT 许可证）：
//! - 移植 wujunwei928/parse-video `parser/douyin.go` 的分享页解析。MIT License，Copyright (c) 2022 wujunwei928。
//! - 字段名对照 z-mio/ParseHub `src/parsehub/parsers/parser/douyin.py`。MIT License，Copyright (c) 2024 梓澪。
//!
//! 许可证全文在上面两个项目的仓库里，两份的版权声明也写在 README 末行。
//! 没有移植：设了 Cookie 才用的详情接口、`slidesinfo` 的随机 `a_bogus`、ParseHub 的签名代码、
//! `playwm` 换成 `play` 的改写（视频地址照页面给的原样用）。
//!
//! 2026-11-12 对着真实的分享页量过（两个 2024 年的作品 id，手机 UA 各一次，不带 cookie）：回 200，
//! `_ROUTER_DATA` 在，`loaderData["video_(id)/page"]` 只有页面上下文（`itemId`、`commonContext`、
//! `abParams` 一类），没有 `videoInfoRes`；页面文字是「抱歉出错了 请尝试在抖音内观看」。
//! 同一天从 `usuyo add` 经宿主跑了两次（一条 2026-08 的 `v.douyin.com` 短链，是 parse-video 的 PR #102 里贴的）：
//! 短链展开成 `iesdouyin.com/share/video/<id>/?…share_sign…`，取到的页面和上面一样（`webId` 每次不同，
//! 带上分享链接自带的查询串也一样，`loaderData` 里的 `query` 记下了它们）；宿主换通用网页插件取同一个
//! 带参数的地址时，拿到的是一个空 `<body>` 加一大段 `_$jsvmprt` 的脚本验证页，下面按它认 `blocked`。
//! parse-video 的 issue #98（2026-08-08）同样记下：分享页不再内嵌 `videoInfoRes`，从美国机房 IP 和另一条出口
//! 都一样，不是限流。所以下面读作品的那一段（`item_list`、`filter_list` 和作品里的字段）是照上游源码和
//! ParseHub 的字段名写的，没有在真实的回答上跑过；测试夹具里这一段是手写的、内容是编的。

use serde_json::{Map, Value, json};
use usuyo_plugin_kit::protocol::NotFoundReason;

/// 分享页上读出的一个作品。
#[derive(Debug, Clone)]
pub struct Post {
    pub description: String,
    pub author: Option<AuthorInfo>,
    /// 发布时间，Unix 秒。
    pub created_at: Option<i64>,
    pub tags: Vec<String>,
    pub cover_url: Option<String>,
    /// 图文的图片地址，一张一项；视频作品是空的。
    pub images: Vec<String>,
    /// 视频的播放地址，照页面给的 `video.play_addr.url_list[0]`。
    pub video: Option<VideoInfo>,
    /// 写进 `meta.json` 的计数和音乐。
    pub counts: Map<String, Value>,
    pub music: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct AuthorInfo {
    pub name: String,
    pub sec_uid: Option<String>,
    pub uid: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub play_url: String,
    pub duration_sec: Option<f64>,
    pub width: Option<u64>,
    pub height: Option<u64>,
}

impl Post {
    /// 有 `images` 的是图文，没有视频文件。
    pub fn is_image_post(&self) -> bool {
        !self.images.is_empty()
    }
}

/// 读不出作品的几种情况，每种带一段给失败日志的原文开头。
#[derive(Debug)]
pub enum PageError {
    /// 页面像验证页。
    Verification { detail: String },
    /// 分享页说这条看不到（`filter_list`）。
    Unavailable {
        reason: Option<NotFoundReason>,
        message: String,
        detail: String,
    },
    /// 没有 `_ROUTER_DATA`，或里面没有作品：页面改版，或平台这次没给。
    Missing { what: &'static str, detail: String },
}

/// 一页读完的结果：作品，加上原样的数据块。
pub struct Parsed {
    pub post: Post,
    /// `_ROUTER_DATA` 的原文，存成输出目录里的文件。
    pub raw: String,
}

pub fn parse(html: &str, id: &str) -> Result<Parsed, PageError> {
    let Some((raw, root)) = router_data(html) else {
        let text = visible_text(html);
        let lower = text.to_lowercase();
        // 空页面加一大段 `_$jsvmprt`：字节的脚本验证页（2026-11-12 由通用网页插件取同一条分享链接时量到）
        let challenge = html.contains("_$jsvmprt");
        let detail = if text.is_empty() {
            head(html).to_owned()
        } else {
            text
        };
        return Err(
            if challenge || lower.contains("验证") || lower.contains("captcha") {
                PageError::Verification { detail }
            } else {
                PageError::Missing {
                    what: "the page has no `_ROUTER_DATA` block",
                    detail,
                }
            },
        );
    };
    // 数据块的开头：页面文字加 JSON 开头，给失败日志
    let detail = || format!("{}\n{}", visible_text(html), head(raw));
    let info = &root["loaderData"]["video_(id)/page"]["videoInfoRes"];
    if !info.is_object() {
        return Err(PageError::Missing {
            what: "the page data has no `videoInfoRes`",
            detail: detail(),
        });
    }
    let Some(item) = info["item_list"].as_array().and_then(|a| a.first()) else {
        let filtered = info["filter_list"]
            .as_array()
            .and_then(|a| a.iter().find(|e| text_of(&e["aweme_id"]) == id));
        return Err(match filtered {
            Some(entry) => unavailable(entry, detail()),
            None => PageError::Missing {
                what: "`videoInfoRes` has neither an item nor a filter entry for this id",
                detail: detail(),
            },
        });
    };
    if text_of(&item["aweme_id"]) != id {
        return Err(PageError::Missing {
            what: "the page answered for a different post",
            detail: detail(),
        });
    }
    Ok(Parsed {
        post: from_item(item),
        raw: raw.to_owned(),
    })
}

/// `filter_list` 里的一项：分享页（和 H5 接口）说这条看不到。
pub fn unavailable(entry: &Value, detail: String) -> PageError {
    let reason = text_of(&entry["filter_reason"]);
    let message = text_of(&entry["detail_msg"]);
    let both = format!("{reason} {message}").to_lowercase();
    // 原因的词是猜的（没有量到过），认不出就不填 `reason`
    let kind = if both.contains("private") || both.contains("self_see") || both.contains("私密") {
        Some(NotFoundReason::Private)
    } else if both.contains("delete") || both.contains("删除") {
        Some(NotFoundReason::Deleted)
    } else if both.contains("region") || both.contains("地区") {
        Some(NotFoundReason::Region)
    } else {
        None
    };
    PageError::Unavailable {
        reason: kind,
        message: format!("the share page filtered the post: {reason} {message}")
            .trim()
            .to_owned(),
        detail,
    }
}

/// 作品条目（`item_list[0]`）里的字段；分享页和 H5 接口的条目形状相同。
pub fn from_item(item: &Value) -> Post {
    let images: Vec<String> = item["images"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|image| pick_url(&image["url_list"]))
        .collect();
    let video = &item["video"];
    let play_url = video["play_addr"]["url_list"][0]
        .as_str()
        .filter(|u| !u.trim().is_empty());
    let author = &item["author"];
    let mut tags: Vec<String> = Vec::new();
    for tag in item["text_extra"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["hashtag_name"].as_str())
        .filter(|t| !t.is_empty())
    {
        if !tags.iter().any(|t| t == tag) {
            tags.push(tag.to_owned());
        }
    }
    let mut counts = Map::new();
    for key in [
        "digg_count",
        "comment_count",
        "share_count",
        "collect_count",
        "play_count",
    ] {
        if let Some(n) = item["statistics"][key].as_u64() {
            counts.insert(key.into(), n.into());
        }
    }
    let music = &item["music"];
    Post {
        description: item["desc"].as_str().unwrap_or_default().trim().to_owned(),
        author: author["nickname"]
            .as_str()
            .filter(|n| !n.trim().is_empty())
            .map(|name| AuthorInfo {
                name: name.to_owned(),
                sec_uid: non_empty(text_of(&author["sec_uid"])),
                uid: non_empty(text_of(&author["uid"])),
            }),
        created_at: item["create_time"].as_i64().filter(|t| *t > 0),
        tags,
        cover_url: pick_url(&video["cover"]["url_list"]),
        images,
        video: play_url.map(|u| VideoInfo {
            play_url: u.to_owned(),
            // 视频时长在 `video.duration`，单位毫秒
            duration_sec: video["duration"]
                .as_f64()
                .filter(|d| *d > 0.0)
                .map(|ms| ms / 1000.0),
            width: video["width"].as_u64().filter(|w| *w > 0),
            height: video["height"].as_u64().filter(|h| *h > 0),
        }),
        counts,
        music: music.is_object().then(|| {
            json!({
                "title": music["title"],
                "author": music["author"],
                "durationSec": music["duration"],
                "playUrl": music["play_url"]["url_list"][0],
            })
        }),
    }
}

/// 优先取不是 `.webp` 的地址，没有就取第一个（和 parse-video 一样）。
fn pick_url(list: &Value) -> Option<String> {
    let urls: Vec<&str> = list
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|u| !u.trim().is_empty())
        .collect();
    urls.iter()
        .find(|u| !u.contains(".webp"))
        .or(urls.first())
        .map(|u| (*u).to_owned())
}

pub fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

/// 找 `window._ROUTER_DATA =` 后面的第一个 JSON 值，返回它的原文和解析结果。
fn router_data(html: &str) -> Option<(&str, Value)> {
    let marker = "window._ROUTER_DATA";
    let rest = html[html.find(marker)? + marker.len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let mut values = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
    let value = values.next()?.ok()?;
    Some((&rest[..values.byte_offset()], value))
}

/// 失败日志里的原文开头：`CallFailure::with_detail` 还会再截到 4096 字节。
pub fn head(text: &str) -> &str {
    let mut end = text.len().min(3000);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// 页面上看得见的文字：去掉 `<script>`、`<style>` 和标签，空白并成一个空格，最多 300 个字符。
pub fn visible_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        out.push(' ');
        let tag = &rest[open + 1..];
        let name: String = tag
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect::<String>()
            .to_ascii_lowercase();
        let skip_to = if name == "script" || name == "style" {
            let close = format!("</{name}");
            tag.to_ascii_lowercase().find(&close).map(|i| open + 1 + i)
        } else {
            None
        };
        let from = skip_to.unwrap_or(open);
        rest = match rest[from..].find('>') {
            Some(end) => &rest[from + end + 1..],
            None => "",
        };
    }
    out.push_str(rest);
    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_text_skips_scripts_and_tags() {
        let html = "<html><head><title>t</title><script>var a='<b>';</script><style>p{}</style></head><body><p>抱歉出错了</p> <p>请尝试在抖音内观看</p></body></html>";
        assert_eq!(visible_text(html), "t 抱歉出错了 请尝试在抖音内观看");
    }

    #[test]
    fn router_data_stops_at_the_end_of_the_value() {
        let html = r#"<script>window._ROUTER_DATA = {"a":{"b":[1,2]},"c":"}"};</script><p>x</p>"#;
        let (raw, value) = router_data(html).unwrap();
        assert_eq!(raw, r#"{"a":{"b":[1,2]},"c":"}"}"#);
        assert_eq!(value["a"]["b"][1], 2);
        assert!(router_data("<p>no data</p>").is_none());
        assert!(router_data("window._ROUTER_DATA = nonsense").is_none());
    }

    #[test]
    fn webp_addresses_come_last() {
        let list = json!(["https://p/a.webp", "https://p/a.jpeg", "https://p/b.png"]);
        assert_eq!(pick_url(&list).as_deref(), Some("https://p/a.jpeg"));
        assert_eq!(
            pick_url(&json!(["https://p/a.webp"])).as_deref(),
            Some("https://p/a.webp")
        );
        assert_eq!(pick_url(&json!([])), None);
        assert_eq!(pick_url(&Value::Null), None);
    }
}
