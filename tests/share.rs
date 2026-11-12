//! 对着本机的假服务器跑：短链展开、分享页（视频、图文、已删除、验证页、没有数据）、媒体主机、
//! 错误码映射、输出目录里有什么。不碰真网络。
//!
//! `fixtures/` 里的页面都是手写的，内容是编的。只有「没有数据的页面」（`share_empty.html`）照
//! 2026-11-12 量到的真实分享页的形状写（`loaderData` 里只有页面上下文，没有 `videoInfoRes`）；
//! 带作品的页面（`item_list[0]` 的字段）和 `filter_list` 是照 parse-video、ParseHub 的字段名写的，
//! 没有在真实的回答上量过。

mod support;

use std::net::IpAddr;
use std::path::Path;

use support::{Request, Response, Server, fixture};
use usuyo_fetch_douyin::api::{Douyin, Endpoints};
use usuyo_fetch_douyin::canonical::canonicalize;
use usuyo_fetch_douyin::share;
use usuyo_plugin::protocol::{
    Access, CallFailure, Canonicalized, Completeness, ErrorCode, Identity, ItemKind, MediaKind,
    NotFoundReason, OutputKind,
};
use usuyo_plugin_kit::http::Policy;

const VIDEO: &str = "7431234567890123456";
const IMAGES: &str = "7431234567890123457";
const REMOVED: &str = "7431234567890123458";
const EMPTY: &str = "7431234567890123460";

/// 只在测试里用的 id：各自触发一种服务器行为。
const NO_PLAY: &str = "7431234567890000001";
const BAD_COVER: &str = "7431234567890000002";
const BAD_IMAGE: &str = "7431234567890000003";
const EVIL_VIDEO: &str = "7431234567890000004";
const PRIVATE: &str = "7431234567890000005";
const OTHER_POST: &str = "7431234567890000006";
const VERIFICATION: &str = "7431234567890000007";
const NO_DATA: &str = "7431234567890000008";
const CHALLENGE: &str = "7431234567890000009";
const HTTP_403: &str = "7431234567890000403";
const HTTP_429: &str = "7431234567890000429";
const HTTP_500: &str = "7431234567890000500";
const HTTP_302: &str = "7431234567890000302";
const HTTP_302_VERIFY: &str = "7431234567890000303";

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

fn html(name: &str, base: &str) -> Response {
    Response {
        status: 200,
        headers: vec![("Content-Type", "text/html; charset=UTF-8".into())],
        body: fixture(name, base).into_bytes(),
    }
}

/// 分享页：按 id 给不同的页面。
fn share_page(id: &str, base: &str) -> Response {
    let as_id = |body: String| -> String { body.replace(VIDEO, id) };
    match id {
        VIDEO => html("share_video.html", base),
        IMAGES => html("share_images.html", base),
        REMOVED => html("share_removed.html", base),
        EMPTY => html("share_empty.html", base),
        PRIVATE => Response {
            status: 200,
            headers: vec![],
            body: fixture("share_removed.html", base)
                .replace(REMOVED, id)
                .replace("status_deleted", "status_private")
                .replace("作品已被删除", "作品是私密的")
                .into_bytes(),
        },
        NO_PLAY => Response {
            status: 200,
            headers: vec![],
            body: as_id(fixture("share_video.html", base))
                .replace(r#""url_list":["http"#, r#""x":["http"#)
                .into_bytes(),
        },
        BAD_COVER => Response {
            status: 200,
            headers: vec![],
            body: as_id(fixture("share_video.html", base))
                .replace("/img/cover.webp", "/img/missing.jpeg")
                .replace("/img/cover.jpeg", "/img/missing.jpeg")
                .into_bytes(),
        },
        BAD_IMAGE => Response {
            status: 200,
            headers: vec![],
            body: fixture("share_images.html", base)
                .replace(IMAGES, id)
                .replace("/img/2.jpeg", "/img/missing.jpeg")
                .into_bytes(),
        },
        // 视频地址先跳到名单外的主机
        EVIL_VIDEO => Response {
            status: 200,
            headers: vec![],
            body: as_id(fixture("share_video.html", base))
                .replace(
                    "/play/v1?video_id=v0200fsample&ratio=720p&line=0",
                    "/play/evil",
                )
                .into_bytes(),
        },
        OTHER_POST => html("share_video.html", base),
        VERIFICATION => html("share_verification.html", base),
        CHALLENGE => html("share_challenge.html", base),
        NO_DATA => html("share_no_data.html", base),
        HTTP_403 => Response::status(403).header("Content-Type", "text/html"),
        HTTP_429 => Response::status(429).header("Retry-After", "30"),
        HTTP_500 => Response::status(500),
        HTTP_302 => Response::redirect(
            "https://www.douyin.com/video/7431234567890000302?previous_page=app_code_link",
        ),
        HTTP_302_VERIFY => Response::redirect("https://example.com/verify?next=1"),
        _ => Response::status(404),
    }
}

/// 一个什么都有的抖音。
fn douyin(req: &Request, base: &str) -> Response {
    if let Some(id) = req.path.strip_prefix("/share/video/") {
        return share_page(id, base);
    }
    match req.path.as_str() {
        "/play/v1" => Response::redirect("/cdn/video.mp4"),
        "/play/evil" => Response::redirect("https://evil.example/v.mp4"),
        "/cdn/video.mp4" => Response::bytes(b"VIDEOBYTES").header("Content-Type", "video/mp4"),
        "/img/cover.jpeg" => Response::bytes(b"COVER"),
        "/img/cover.webp" => Response::bytes(b"COVERWEBP"),
        "/img/1.jpeg" => Response::bytes(b"IMG1"),
        "/img/2.jpeg" => Response::bytes(b"IMG2"),
        "/s1" => Response::redirect("/s2"),
        "/s2" => Response::redirect(
            "https://www.iesdouyin.com/share/video/7431234567890123456/?region=CN&mid=1&u_code=0",
        ),
        "/to-note" => Response::redirect(
            "https://www.iesdouyin.com/share/note/7431234567890123457/?region=CN",
        ),
        "/to-user" => Response::redirect("https://www.douyin.com/user/MS4wLjABAAAAsample"),
        "/to-off" => Response::redirect("https://example.com/video/7431234567890123456"),
        "/to-xigua" => Response::redirect("https://www.ixigua.com/7144194760184594977"),
        "/loop" => Response::redirect("/loop"),
        "/gone" => Response::status(404),
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

/// 输出目录里的全部文件，相对路径，排好序。
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

fn content_id(identity: &Option<Identity>) -> String {
    match identity {
        Some(Identity::Platform {
            platform,
            content_id,
            url,
        }) => {
            assert_eq!(platform.as_str(), "douyin");
            assert_eq!(url.as_deref(), Some(post_url(content_id).as_str()));
            content_id.clone()
        }
        other => panic!("{other:?}"),
    }
}

fn paths(server: &Server) -> Vec<String> {
    server.paths()
}

#[test]
fn a_video_post_gives_text_author_counts_cover_and_the_video_file() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let out = share::fetch(&api, &post_url(VIDEO), tmp.path(), None).unwrap();

    assert_eq!(content_id(&out.identity), VIDEO);
    assert_eq!(out.kind, ItemKind::Video);
    assert_eq!(out.title.as_deref(), Some("手冲咖啡三段式注水"));
    assert_eq!(
        out.text.as_deref(),
        Some("手冲咖啡三段式注水\n第一段 40 克闷蒸 30 秒 #手冲 #咖啡日常")
    );
    let author = out.author.as_ref().unwrap();
    assert_eq!(
        (
            author.name.as_str(),
            author.platform_id.as_deref(),
            author.url.as_deref()
        ),
        (
            "咖啡日常",
            Some("MS4wLjABAAAAsample"),
            Some("https://www.douyin.com/user/MS4wLjABAAAAsample")
        )
    );
    assert_eq!(
        out.published_at.as_ref().unwrap().as_str(),
        "2024-10-01T12:00:00Z"
    );
    assert_eq!(out.platform_tags, ["手冲", "咖啡日常"]);
    // 封面取不是 .webp 的那个
    assert_eq!(
        out.images.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        ["images/cover.jpeg"]
    );
    assert_eq!(
        std::fs::read(tmp.path().join("images/cover.jpeg")).unwrap(),
        b"COVER"
    );
    // 视频：页面给的第一个地址，经一次跳转到 CDN
    assert_eq!(out.media.len(), 1);
    assert_eq!(
        (out.media[0].kind, out.media[0].path.as_str()),
        (MediaKind::Video, "media/video.mp4")
    );
    assert_eq!(out.media[0].duration_sec, Some(183.0));
    assert_eq!(
        std::fs::read(tmp.path().join("media/video.mp4")).unwrap(),
        b"VIDEOBYTES"
    );
    assert_eq!(out.completeness, Completeness::Full);
    assert!(out.missing.is_empty() && out.missing_reason.is_none());
    assert_eq!(out.evidence.access, Access::PublicWeb);
    assert_eq!(
        (out.evidence.site.as_deref(), out.evidence.status),
        (None, Some(200))
    );

    assert_eq!(
        files(tmp.path()),
        [
            "images/cover.jpeg",
            "media/video.mp4",
            "meta.json",
            "share-data.json"
        ]
    );
    // 原始数据原样存：就是页面里 `_ROUTER_DATA` 的那一段
    let page = fixture("share_video.html", &server.base);
    let raw = std::fs::read_to_string(tmp.path().join("share-data.json")).unwrap();
    assert!(page.contains(&raw) && raw.starts_with(r#"{"loaderData""#) && raw.ends_with("}"));
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(tmp.path().join("meta.json")).unwrap()).unwrap();
    assert_eq!(meta["awemeId"], VIDEO);
    assert_eq!(meta["kind"], "video");
    assert_eq!(meta["counts"]["digg_count"], 1234);
    assert_eq!(meta["counts"]["play_count"], 4567);
    assert_eq!(meta["music"]["title"], "原声 - 咖啡日常");
    assert_eq!(meta["author"]["secUid"], "MS4wLjABAAAAsample");
    assert_eq!(meta["createTime"], 1727784000);
    assert_eq!(meta["video"]["width"], 720);

    // 分享页用手机 UA；没有一个请求带 cookie
    let requests = server.requests();
    let share = requests
        .iter()
        .find(|r| r.path.starts_with("/share/video/"))
        .unwrap();
    assert!(share.headers["user-agent"].contains("iPhone"));
    assert!(share.query.is_empty());
    assert!(requests.iter().all(|r| r.cookie().is_none()));
    // 页面给了两个视频地址，只用第一个，地址原样（参数不改）
    assert!(!paths(&server).contains(&"/play/v2".to_owned()));
}

#[test]
fn an_image_post_gives_the_cover_and_every_image_and_no_video() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let out = share::fetch(&api, &post_url(IMAGES), tmp.path(), None).unwrap();
    assert_eq!(content_id(&out.identity), IMAGES);
    assert_eq!(out.kind, ItemKind::Post);
    assert_eq!(
        out.images.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        ["images/cover.jpeg", "images/001.jpeg", "images/002.jpeg"]
    );
    assert_eq!(
        std::fs::read(tmp.path().join("images/001.jpeg")).unwrap(),
        b"IMG1"
    );
    assert!(out.media.is_empty());
    // 图文本来没有视频：不算缺
    assert_eq!(out.completeness, Completeness::Full);
    assert!(out.missing.is_empty());
    assert!(
        !paths(&server).iter().any(|p| p.starts_with("/play")),
        "{:?}",
        paths(&server)
    );
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(tmp.path().join("meta.json")).unwrap()).unwrap();
    assert_eq!(meta["kind"], "images");
}

#[test]
fn links_in_every_accepted_shape_reach_the_same_post() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    for input in [
        format!("https://www.douyin.com/video/{VIDEO}"),
        format!("https://www.douyin.com/note/{VIDEO}"),
        format!("https://www.iesdouyin.com/share/video/{VIDEO}/?region=CN"),
        format!("7.87 Pjm:/ 复制打开抖音 https://www.douyin.com/video/{VIDEO} 复制此链接"),
        // 短链（本机的 /s1 算短链主机）
        format!("{}/s1", server.base),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let out = share::fetch(
            &api,
            &input,
            tmp.path(),
            Some(&[OutputKind::Text, OutputKind::Meta]),
        )
        .unwrap_or_else(|e| panic!("{input}: {e:?}"));
        assert_eq!(content_id(&out.identity), VIDEO, "{input}");
    }
}

#[test]
fn want_limits_the_downloads() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let out = share::fetch(
        &api,
        &post_url(VIDEO),
        tmp.path(),
        Some(&[OutputKind::Text, OutputKind::Meta]),
    )
    .unwrap();
    assert_eq!(out.completeness, Completeness::Full);
    assert!(out.images.is_empty() && out.media.is_empty());
    assert!(
        paths(&server)
            .iter()
            .all(|p| p.starts_with("/share/video/")),
        "{:?}",
        paths(&server)
    );
    // 只给清单里没有的输出，回 invalid-args，不发请求
    let f = failure(share::fetch(
        &api,
        &post_url(VIDEO),
        tmp.path(),
        Some(&[OutputKind::Audio]),
    ));
    assert_eq!(f.code, ErrorCode::InvalidArgs);
}

#[test]
fn a_cover_or_an_image_that_does_not_come_is_missing_with_a_reason() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let out = share::fetch(&api, &post_url(BAD_COVER), tmp.path(), None).unwrap();
    // 封面没下来、视频下来了：图片缺，其余照交
    assert_eq!(out.completeness, Completeness::Partial);
    assert_eq!(out.missing, [OutputKind::Images]);
    assert!(out.missing_reason.as_deref().unwrap().contains("封面"));
    assert_eq!(out.media.len(), 1);
    assert!(out.text.is_some());

    // 两张图下到一张：交来一部分，照样记缺
    let tmp = tempfile::tempdir().unwrap();
    let out = share::fetch(&api, &post_url(BAD_IMAGE), tmp.path(), None).unwrap();
    assert_eq!(out.completeness, Completeness::Partial);
    assert_eq!(out.missing, [OutputKind::Images]);
    assert!(out.missing_reason.as_deref().unwrap().contains("图片 2"));
    assert_eq!(out.images.len(), 2);
}

#[test]
fn a_video_on_a_host_off_the_list_is_missing_and_the_rest_is_kept() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let out = share::fetch(&api, &post_url(EVIL_VIDEO), tmp.path(), None).unwrap();
    assert_eq!(out.completeness, Completeness::Partial);
    assert_eq!(out.missing, [OutputKind::Video]);
    assert!(out.missing_reason.as_deref().unwrap().contains("视频"));
    assert!(out.media.is_empty());
    assert_eq!(out.images.len(), 1);
    assert!(!files(tmp.path()).contains(&"media/video.mp4".to_owned()));
}

#[test]
fn a_video_post_without_a_play_address_is_missing_the_video() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let out = share::fetch(&api, &post_url(NO_PLAY), tmp.path(), None).unwrap();
    assert_eq!(out.completeness, Completeness::Partial);
    assert_eq!(out.missing, [OutputKind::Video]);
    assert_eq!(
        out.missing_reason.as_deref(),
        Some("分享页没有给出视频地址")
    );
}

#[test]
fn a_removed_or_private_post_is_not_found_with_the_reason_and_the_start_of_the_answer() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let f = failure(share::fetch(&api, &post_url(REMOVED), tmp.path(), None));
    assert_eq!(f.code, ErrorCode::NotFound);
    assert_eq!(f.reason, Some(NotFoundReason::Deleted));
    let detail = f.detail.as_deref().unwrap();
    assert!(
        detail.contains("status_deleted") && detail.contains("loaderData"),
        "{detail}"
    );

    let f = failure(share::fetch(&api, &post_url(PRIVATE), tmp.path(), None));
    assert_eq!(f.code, ErrorCode::NotFound);
    assert_eq!(f.reason, Some(NotFoundReason::Private));
}

#[test]
fn a_page_without_the_post_data_is_upstream_changed() {
    // 2026-11-12 量到的真实分享页的形状：有 `_ROUTER_DATA`，没有 `videoInfoRes`
    let server = Server::start(douyin);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let f = failure(share::fetch(&api, &post_url(EMPTY), tmp.path(), None));
    assert_eq!(f.code, ErrorCode::UpstreamChanged);
    assert!(f.message.contains("videoInfoRes"));
    let detail = f.detail.as_deref().unwrap();
    assert!(detail.contains("抱歉出错了"), "{detail}");
    assert!(detail.contains("loaderData"), "{detail}");
    assert!(f.hint.is_some());
    // 没有 `_ROUTER_DATA` 的页面，同一个码，detail 是页面的文字
    let f = failure(share::fetch(&api, &post_url(NO_DATA), tmp.path(), None));
    assert_eq!(f.code, ErrorCode::UpstreamChanged);
    assert!(f.message.contains("_ROUTER_DATA"));
    assert!(f.detail.as_deref().unwrap().contains("抖音"));
    // 页面回的是另一个作品
    let f = failure(share::fetch(&api, &post_url(OTHER_POST), tmp.path(), None));
    assert_eq!(f.code, ErrorCode::UpstreamChanged);
    assert!(f.message.contains("different post"));
    // 失败时输出目录里不留半截文件
    assert!(files(tmp.path()).is_empty());
}

#[test]
fn a_verification_page_is_blocked() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let f = failure(share::fetch(
        &api,
        &post_url(VERIFICATION),
        tmp.path(),
        None,
    ));
    assert_eq!(f.code, ErrorCode::Blocked);
    assert!(f.detail.as_deref().unwrap().contains("请完成下列验证"));
    // 脚本验证页：空的 body 加一大段脚本，没有可见的文字（2026-11-12 量到的形状，内容是编的）
    let f = failure(share::fetch(&api, &post_url(CHALLENGE), tmp.path(), None));
    assert_eq!(f.code, ErrorCode::Blocked);
    assert!(f.detail.as_deref().unwrap().contains("_$jsvmprt"));
}

#[test]
fn http_statuses_map_to_codes() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    let run = |id: &str| failure(share::fetch(&api, &post_url(id), tmp.path(), None));
    assert_eq!(run(HTTP_403).code, ErrorCode::Blocked);
    let limited = run(HTTP_429);
    assert_eq!(
        (limited.code, limited.retry_after),
        (ErrorCode::RateLimited, Some(30))
    );
    assert_eq!(run(HTTP_500).code, ErrorCode::Network);
    assert_eq!(run("7431234567890009999").code, ErrorCode::NotFound);
    // 桌面 UA 被送去 www.douyin.com 一类的跳转：不跟，当页面改了，原因里写去了哪里
    let moved = run(HTTP_302);
    assert_eq!(moved.code, ErrorCode::UpstreamChanged);
    assert!(
        moved.message.contains("www.douyin.com/video"),
        "{}",
        moved.message
    );
    assert_eq!(run(HTTP_302_VERIFY).code, ErrorCode::Blocked);
    // 不跟跳转：没有请求落到 www.douyin.com 上（本机服务器只会看到自己的路径）
    assert!(
        paths(&server)
            .iter()
            .all(|p| p.starts_with("/share/video/"))
    );
}

#[test]
fn unsupported_input_never_reaches_the_network() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    let tmp = tempfile::tempdir().unwrap();
    for input in [
        "https://example.com/video/7431234567890123456",
        "https://www.douyin.com/user/MS4wLjABAAAAsample",
        "没有链接的一段话",
    ] {
        let f = failure(share::fetch(&api, input, tmp.path(), None));
        assert_eq!(f.code, ErrorCode::UnsupportedInput, "{input}");
    }
    assert!(paths(&server).is_empty());
}

fn matched(result: Canonicalized) -> (String, Option<String>) {
    match result {
        Canonicalized::Match {
            identity,
            resolved_url,
        } => (content_id(&Some(identity)), resolved_url),
        Canonicalized::NoMatch => panic!("no match"),
    }
}

#[test]
fn canonicalize_needs_no_request_for_full_links() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    for input in [
        format!("https://www.douyin.com/video/{VIDEO}"),
        format!("https://www.douyin.com/note/{VIDEO}?previous_page=app_code_link"),
        format!("https://www.iesdouyin.com/share/video/{VIDEO}/?region=CN&mid=1"),
        format!("看看 https://www.douyin.com/video/{VIDEO} 这条"),
    ] {
        let (id, resolved) = matched(canonicalize(&api, &input).unwrap());
        assert_eq!((id.as_str(), resolved), (VIDEO, None), "{input}");
    }
    assert_eq!(
        canonicalize(&api, "https://example.com/video/1").unwrap(),
        Canonicalized::NoMatch
    );
    assert!(paths(&server).is_empty());
}

#[test]
fn canonicalize_expands_a_short_link_through_douyins_own_hosts_only() {
    let server = Server::start(douyin);
    let api = api_for(&server);
    // 两跳：/s1 → /s2 → 抖音的分享页地址（到了作品页就停，不请求它）
    let (id, resolved) = matched(canonicalize(&api, &format!("{}/s1", server.base)).unwrap());
    assert_eq!(id, VIDEO);
    assert_eq!(
        resolved.as_deref(),
        Some("https://www.iesdouyin.com/share/video/7431234567890123456/?region=CN&mid=1&u_code=0")
    );
    // 图文的短链落在 share/note
    let (id, _) = matched(canonicalize(&api, &format!("{}/to-note", server.base)).unwrap());
    assert_eq!(id, IMAGES);
    // 落在用户页、别的主机、西瓜视频上的，不匹配，也不去请求它们
    for path in ["/to-user", "/to-off", "/to-xigua"] {
        let result = canonicalize(&api, &format!("{}{path}", server.base)).unwrap();
        assert_eq!(result, Canonicalized::NoMatch, "{path}");
    }
    assert!(
        paths(&server).iter().all(|p| matches!(
            p.as_str(),
            "/s1" | "/s2" | "/to-note" | "/to-user" | "/to-off" | "/to-xigua"
        )),
        "{:?}",
        paths(&server)
    );
    // 跳不完的、短链本身 404 的
    let f = failure(canonicalize(&api, &format!("{}/loop", server.base)));
    assert_eq!(f.code, ErrorCode::UpstreamChanged);
    let f = failure(canonicalize(&api, &format!("{}/gone", server.base)));
    assert_eq!(f.code, ErrorCode::NotFound);
    // 短链请求不带 cookie
    assert!(server.requests().iter().all(|r| r.cookie().is_none()));
}
