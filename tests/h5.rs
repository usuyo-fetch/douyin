//! 方法 `h5-token`，对着本机的假服务器跑：分享页里的令牌、`iteminfo` 的参数、回答的几种形状、
//! 错误码。不碰真网络。
//!
//! 夹具是手写的，内容是编的：`share_h5.html` 里 `douyin_reflow_*` 三个标签的写法照 2026-11-12 量到的
//! 分享页（令牌和 webId 是编的）；`iteminfo_*.json` 的外层 `{status_code, item_list, filter_list}`
//! 照 parse-video PR #106 的代码和测试写，条目和分享页的 `item_list[0]` 一样。真实的 `iteminfo`
//! 回答没有量到过（见 README）。

mod support;

use std::net::IpAddr;
use std::path::Path;

use support::{Request, Response, Server, fixture};
use usuyo_fetch_douyin::api::{Douyin, Endpoints};
use usuyo_fetch_douyin::h5;
use usuyo_fetch_douyin::share;
use usuyo_plugin::protocol::{
    Access, CallFailure, Completeness, ErrorCode, ItemKind, NotFoundReason, OutputKind,
};
use usuyo_plugin_kit::http::Policy;

const VIDEO: &str = "7431234567890123456";
const IMAGES: &str = "7431234567890123457";
const REMOVED: &str = "7431234567890123458";
const REJECTED: &str = "7431234567890000101";
const OTHER_POST: &str = "7431234567890000102";
const NO_CONTEXT: &str = "7431234567890000103";
const HTTP_403: &str = "7431234567890000104";
const NOT_JSON: &str = "7431234567890000105";
const HTTP_429: &str = "7431234567890000107";
const NO_ITEM: &str = "7431234567890000106";

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const WEB_ID: &str = "7700000000000000001";

fn local_policy() -> Policy {
    Policy {
        lookup: |_, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no DNS in tests",
            ))
        },
        public: |ip| ip == IpAddr::from([127, 0, 0, 1]),
        cookies_over_http: true,
        ..Policy::strict()
    }
}

fn body(status: u16, body: String) -> Response {
    Response {
        status,
        headers: vec![("Content-Type", "application/json".into())],
        body: body.into_bytes(),
    }
}

fn h5_site(req: &Request, base: &str) -> Response {
    if let Some(id) = req.path.strip_prefix("/share/video/") {
        return match id {
            NO_CONTEXT => Response {
                status: 200,
                headers: vec![],
                body: fixture("share_no_data.html", base).into_bytes(),
            },
            _ => Response {
                status: 200,
                headers: vec![("Content-Type", "text/html; charset=UTF-8".into())],
                body: fixture("share_h5.html", base).into_bytes(),
            },
        };
    }
    match req.path.as_str() {
        "/web/api/v2/aweme/iteminfo/" => {
            let id = req.query.get("item_ids").map(String::as_str).unwrap_or("");
            match id {
                VIDEO => body(200, fixture("iteminfo_video.json", base)),
                IMAGES => body(200, fixture("iteminfo_images.json", base)),
                REMOVED => body(200, fixture("iteminfo_removed.json", base)),
                REJECTED => body(200, fixture("iteminfo_rejected.json", base)),
                OTHER_POST => body(200, fixture("iteminfo_video.json", base)),
                HTTP_403 => Response::status(403),
                HTTP_429 => Response::status(429).header("Retry-After", "30"),
                NOT_JSON => body(200, "<html>nope</html>".into()),
                NO_ITEM => body(200, r#"{"status_code":0,"item_list":[]}"#.into()),
                _ => Response::status(404),
            }
        }
        "/play/v1" => Response::redirect("/cdn/video.mp4"),
        "/cdn/video.mp4" => Response::bytes(b"VIDEOBYTES"),
        "/img/cover.jpeg" => Response::bytes(b"COVER"),
        "/img/1.jpeg" => Response::bytes(b"IMG1"),
        "/img/2.jpeg" => Response::bytes(b"IMG2"),
        _ => Response::status(404),
    }
}

fn api_for(server: &Server) -> Douyin {
    Douyin::new(Endpoints::local(&server.base, local_policy()))
}

fn failure<T: std::fmt::Debug>(result: Result<T, CallFailure>) -> CallFailure {
    result.expect_err("expected a failure")
}

fn post_url(id: &str) -> String {
    format!("https://www.douyin.com/video/{id}")
}

fn files(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                out.push(
                    path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn a_video_post_comes_through_the_item_info_call() {
    let server = Server::start(h5_site);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let out = share::fetch_h5(&api, &post_url(VIDEO), tmp.path(), None).unwrap();

    assert_eq!(out.kind, ItemKind::Video);
    assert_eq!(out.title.as_deref(), Some("手冲咖啡三段式注水"));
    assert_eq!(out.author.as_ref().unwrap().name, "咖啡日常");
    assert_eq!(out.platform_tags, ["手冲", "咖啡日常"]);
    assert_eq!(out.images.len(), 1);
    assert_eq!(out.media.len(), 1);
    assert_eq!(out.completeness, Completeness::Full);
    // 这一层是 private-api，不是 public-web
    assert_eq!(out.evidence.access, Access::PrivateApi);
    assert_eq!(
        std::fs::read(tmp.path().join("media/video.mp4")).unwrap(),
        b"VIDEOBYTES"
    );
    // 平台的回答原样存成 item-info.json；没有 share-data.json
    assert_eq!(
        files(tmp.path()),
        [
            "images/cover.jpeg",
            "item-info.json",
            "media/video.mp4",
            "meta.json"
        ]
    );
    let raw = std::fs::read_to_string(tmp.path().join("item-info.json")).unwrap();
    assert_eq!(raw, fixture("iteminfo_video.json", &server.base));

    // 请求：先分享页（手机 UA），再 iteminfo（带页面算出的参数），没有 cookie
    let requests = server.requests();
    let page = &requests[0];
    assert!(
        page.path.starts_with("/share/video/") && page.headers["user-agent"].contains("iPhone")
    );
    let item = &requests[1];
    assert_eq!(item.path, "/web/api/v2/aweme/iteminfo/");
    assert_eq!(item.query["item_ids"], VIDEO);
    assert_eq!(item.query["web_id"], WEB_ID);
    assert_eq!(item.query["device_id"], WEB_ID);
    assert_eq!(item.query["user_cip"], "203.0.113.7");
    assert_eq!(item.query["reflow_source"], "reflow_page");
    assert_eq!(item.query["use_new_select_scope"], "0");
    assert_eq!(item.query["reflow_id"], h5::reflow_id(TOKEN, WEB_ID));
    assert!(!item.query.values().any(|v| v == TOKEN));
    assert!(item.headers["referer"].ends_with(&format!("/share/video/{VIDEO}")));
    assert!(requests.iter().all(|r| r.cookie().is_none()));
    // 令牌不进输出目录
    for name in files(tmp.path()) {
        let bytes = std::fs::read(tmp.path().join(&name)).unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(TOKEN),
            "{name} has the token"
        );
    }
}

#[test]
fn an_image_post_comes_through_the_same_way() {
    let server = Server::start(h5_site);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let out = share::fetch_h5(&api, &post_url(IMAGES), tmp.path(), None).unwrap();
    assert_eq!(out.kind, ItemKind::Post);
    assert_eq!(
        out.images.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        ["images/cover.jpeg", "images/001.jpeg", "images/002.jpeg"]
    );
    assert!(out.media.is_empty());
    assert_eq!(out.completeness, Completeness::Full);
}

#[test]
fn want_limits_the_downloads_but_the_calls_are_the_same_two() {
    let server = Server::start(h5_site);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let out = share::fetch_h5(
        &api,
        &post_url(VIDEO),
        tmp.path(),
        Some(&[OutputKind::Text, OutputKind::Meta]),
    )
    .unwrap();
    assert!(out.images.is_empty() && out.media.is_empty());
    assert_eq!(
        server.paths(),
        [
            format!("/share/video/{VIDEO}"),
            "/web/api/v2/aweme/iteminfo/".to_owned()
        ]
    );
}

#[test]
fn a_removed_post_is_not_found_with_the_reason_and_the_answer() {
    let server = Server::start(h5_site);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let f = failure(share::fetch_h5(&api, &post_url(REMOVED), tmp.path(), None));
    assert_eq!(f.code, ErrorCode::NotFound);
    assert_eq!(f.reason, Some(NotFoundReason::Deleted));
    assert!(f.detail.as_deref().unwrap().contains("status_deleted"));
}

#[test]
fn a_changed_algorithm_or_a_rejected_token_is_upstream_changed_with_a_hint() {
    let server = Server::start(h5_site);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let run = |id: &str| failure(share::fetch_h5(&api, &post_url(id), tmp.path(), None));
    let rejected = run(REJECTED);
    assert_eq!(rejected.code, ErrorCode::UpstreamChanged);
    assert!(rejected.message.contains("status_code"));
    let detail = rejected.detail.as_deref().unwrap();
    assert!(detail.contains("2154") && detail.contains("invalid reflow token"));
    // 提示里说清这种方法靠平台能改的参数
    assert!(rejected.hint.as_deref().unwrap().contains("参数"));
    // 令牌从不进 detail
    assert!(!detail.contains(TOKEN));

    let no_context = run(NO_CONTEXT);
    assert_eq!(no_context.code, ErrorCode::UpstreamChanged);
    assert!(no_context.message.contains("xsstoken"));
    assert!(no_context.hint.is_some());

    for (id, needle) in [
        (OTHER_POST, "different post"),
        (NOT_JSON, "not JSON"),
        (NO_ITEM, "neither an item"),
    ] {
        let f = run(id);
        assert_eq!(f.code, ErrorCode::UpstreamChanged, "{id}");
        assert!(f.message.contains(needle), "{id}: {}", f.message);
    }
    assert!(
        run(NOT_JSON)
            .detail
            .as_deref()
            .unwrap()
            .contains("<html>nope")
    );
    assert!(files(tmp.path()).is_empty());
}

#[test]
fn http_errors_map_the_same_way_as_for_the_share_page() {
    let server = Server::start(h5_site);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let f = failure(share::fetch_h5(&api, &post_url(HTTP_403), tmp.path(), None));
    assert_eq!(f.code, ErrorCode::Blocked);
    let f = failure(share::fetch_h5(&api, &post_url(HTTP_429), tmp.path(), None));
    assert_eq!((f.code, f.retry_after), (ErrorCode::RateLimited, Some(30)));
    let f = failure(share::fetch_h5(
        &api,
        "https://example.com/video/1",
        tmp.path(),
        None,
    ));
    assert_eq!(f.code, ErrorCode::UnsupportedInput);
}
