//! `ddr-import` 方法：读 DouyinDataReview 导出的 JSON，列成链接。全部离线：起插件进程时清空环境，
//! 代理指向一个关着的本机端口，万一哪条路径真的发了请求，只会失败，不会连到抖音；文件太大用小的上限直接
//! 调库函数。样例文件是手写的，没对着 DouyinDataReview 真正的输出比对过（`tests/fixtures/ddr/NOTES.md`）。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use usuyo_fetch_douyin::ddr::{Limits, list};
use usuyo_plugin::protocol::{
    Access, Account, ErrorCode, ListOrder, ListPage, Login, PluginKind, PluginManifest,
};
use usuyo_plugin::{CallLimits, CallOptions, HostConfig, ListRequest, PluginHost, discover};

const BIN: &str = env!("CARGO_BIN_EXE_usuyo-fetch-douyin");
const DEAD_PROXY: &str = "http://127.0.0.1:9";
const EXPORT: &str = "export.json";

/// 样例文件列出来的样子，按顺序：作品 id、动作、标题、作者、发布时间、`actedAt`。
type Row<'a> = (
    &'a str,
    &'a str,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
);
const EXPECTED: [Row<'static>; 6] = [
    (
        "7431234567890123461",
        "view",
        Some("官方档案里的一条"),
        Some("路人乙"),
        None,
        Some("2026-10-26T08:00:00Z"),
    ),
    (
        "7431234567890123456",
        "view",
        Some("手冲咖啡：三段式注水"),
        Some("咖啡日常"),
        Some("2026-10-01T12:00:00Z"),
        Some("2026-10-25T21:30:00Z"),
    ),
    ("7431234567890123460", "view", None, None, None, None),
    (
        "7431234567890123457",
        "like",
        Some("磨豆子的粗细"),
        Some("咖啡人"),
        Some("2026-10-15T08:30:00Z"),
        None,
    ),
    (
        "7431234567890123456",
        "like",
        Some("手冲咖啡：三段式注水"),
        Some("咖啡日常"),
        None,
        None,
    ),
    (
        "7431234567890123458",
        "favorite",
        Some("一篇图文"),
        Some("路人甲"),
        None,
        None,
    ),
];

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ddr")
            .join(name),
    )
    .unwrap()
}

fn plugin(args: &[&str], input: Option<&Path>) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.env_clear()
        .env("USUYO_PROTOCOL", "1")
        .env("USUYO_LOCALE", "zh-CN")
        .env("HTTP_PROXY", DEAD_PROXY)
        .env("HTTPS_PROXY", DEAD_PROXY)
        .env("ALL_PROXY", DEAD_PROXY)
        .args(args);
    if let Some(i) = input {
        cmd.env("USUYO_INPUT", i);
    }
    cmd.output().unwrap()
}

/// stdout 上只有一个 JSON 值。
fn envelope(out: &Output) -> Value {
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    assert_eq!(
        stdout.lines().count(),
        1,
        "stdout is one JSON value: {stdout}"
    );
    serde_json::from_str(&stdout).unwrap()
}

fn list_call(input: &Path) -> Output {
    plugin(
        &["list", "file:input", "--method", "ddr-import"],
        Some(input),
    )
}

fn listed(input: &Path) -> Value {
    let out = list_call(input);
    assert!(out.status.success(), "{out:?}");
    envelope(&out)
}

fn failure(input: &Path) -> Value {
    let out = list_call(input);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let value = envelope(&out);
    assert_eq!(value["ok"], false, "{value}");
    value["error"].clone()
}

fn file_in(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn rows(value: &Value) -> Vec<Row<'_>> {
    value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            let url = i["url"].as_str().unwrap();
            let id = url.strip_prefix("https://www.douyin.com/video/").unwrap();
            assert_eq!(i["identity"]["contentId"], id);
            (
                id,
                i["action"].as_str().unwrap(),
                i["title"].as_str(),
                i["author"]["name"].as_str(),
                i["publishedAt"].as_str(),
                i["actedAt"].as_str(),
            )
        })
        .collect()
}

#[test]
fn lists_the_three_kinds_with_real_action_times_only() {
    let tmp = tempfile::tempdir().unwrap();
    // 认的是内容，不是文件名
    for name in ["抖音数据_2026-11-12_10-30-00.json", "input"] {
        let path = file_in(tmp.path(), name, &fixture(EXPORT));
        let value = listed(&path);
        assert_eq!(value["ok"], true, "{value}");
        let mut page = value.clone();
        page.as_object_mut().unwrap().remove("ok");
        let parsed: ListPage = serde_json::from_value(page).unwrap();
        assert_eq!(parsed.next_cursor, None);
        assert_eq!(value["evidence"]["access"], "official-export");
        assert_eq!(rows(&value), EXPECTED, "{name}");
        for item in value["items"].as_array().unwrap() {
            // 只有链接：身份和平台插件别的方法给的一样，不带 ref
            assert_eq!(item["identity"]["platform"], "douyin", "{item}");
            assert_eq!(item["identity"]["url"], item["url"], "{item}");
            assert!(item.get("ref").is_none(), "{item}");
        }
    }
}

#[test]
fn a_byte_order_mark_and_untrusted_urls_do_not_matter() {
    let tmp = tempfile::tempdir().unwrap();
    // 链接按作品 id 重建，不用记录里的 url；没有 videoId 时才从记录的链接认，认不出的不要
    let doc = json!({
        "schemaVersion": 2,
        "records": {
            "liked_videos": [
                {"videoId": "7431234567890123456", "url": "https://evil.test/video/1", "title": "a"},
                {"url": "https://www.douyin.com/video/7431234567890123499", "title": "b"},
                {"url": "https://evil.test/video/7431234567890123498", "title": "c"},
                {"videoId": "../../etc/passwd", "title": "d"}
            ]
        }
    });
    let mut bytes = b"\xef\xbb\xbf".to_vec();
    bytes.extend(serde_json::to_vec(&doc).unwrap());
    let path = file_in(tmp.path(), "bom.json", &bytes);
    let value = listed(&path);
    let urls: Vec<&str> = value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["url"].as_str().unwrap())
        .collect();
    assert_eq!(
        urls,
        [
            "https://www.douyin.com/video/7431234567890123456",
            "https://www.douyin.com/video/7431234567890123499"
        ]
    );
}

#[test]
fn an_export_with_only_chat_lists_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = file_in(tmp.path(), "chat.json", &fixture("chat-only.json"));
    let value = listed(&path);
    assert_eq!(value["items"], json!([]));
    assert_eq!(value["nextCursor"], Value::Null);
}

#[test]
fn pages_are_cut_by_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    let path = file_in(tmp.path(), EXPORT, &fixture(EXPORT));
    let out = plugin(
        &["list", "file:x", "--method", "ddr-import", "--cursor", "4"],
        Some(&path),
    );
    let value = envelope(&out);
    assert_eq!(rows(&value), EXPECTED[4..]);
    assert_eq!(value["nextCursor"], Value::Null);
    let bad = plugin(
        &["list", "file:x", "--method", "ddr-import", "--cursor", "x"],
        Some(&path),
    );
    assert_eq!(envelope(&bad)["error"]["code"], "invalid-args");

    // 501 条：第一页 500 条，游标指向第二页
    let many: Vec<Value> = (0..501)
        .map(|n| json!({"videoId": format!("{}", 7_431_234_567_890_000_000_u64 + n), "title": "t"}))
        .collect();
    let big = file_in(
        tmp.path(),
        "big.json",
        &serde_json::to_vec(&json!({"schemaVersion": 2, "records": {"favorite_videos": many}}))
            .unwrap(),
    );
    let first = listed(&big);
    assert_eq!(first["items"].as_array().unwrap().len(), 500);
    assert_eq!(first["nextCursor"], "500");
    let second = envelope(&plugin(
        &[
            "list",
            "file:x",
            "--method",
            "ddr-import",
            "--cursor",
            "500",
        ],
        Some(&big),
    ));
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_eq!(second["nextCursor"], Value::Null);
}

#[test]
fn list_refuses_what_is_not_a_file_import() {
    let tmp = tempfile::tempdir().unwrap();
    let path = file_in(tmp.path(), EXPORT, &fixture(EXPORT));
    let not_file = plugin(
        &["list", "douyin:likes", "--method", "ddr-import"],
        Some(&path),
    );
    assert_eq!(envelope(&not_file)["error"]["code"], "invalid-args");
    let no_input = plugin(&["list", "file:x", "--method", "ddr-import"], None);
    assert_eq!(envelope(&no_input)["error"]["code"], "invalid-args");
    let missing = tmp.path().join("gone.json");
    let gone = list_call(&missing);
    assert_eq!(envelope(&gone)["error"]["code"], "internal");
    // 这种方法没有 fetch、sources
    for args in [
        &["fetch", "m/0", "--out", "/o", "--method", "ddr-import"][..],
        &["sources", "--method", "ddr-import"][..],
    ] {
        let out = plugin(args, Some(&path));
        assert_eq!(envelope(&out)["error"]["code"], "invalid-args", "{args:?}");
    }
}

#[test]
fn other_files_are_refused_with_a_hint() {
    let tmp = tempfile::tempdir().unwrap();
    let cases: [(&str, &[u8]); 5] = [
        ("a.csv", b"\xef\xbb\xbfid,title\n1,invented\n"),
        ("a.zip", b"PK\x03\x04invented"),
        ("a.json", b"[{\"videoId\": \"7431234567890123456\"}]"),
        ("empty.json", b""),
        ("a.html", b"<html></html>"),
    ];
    for (name, bytes) in cases {
        let error = failure(&file_in(tmp.path(), name, bytes));
        assert_eq!(error["code"], "unsupported-input", "{name}: {error}");
        let hint = error["hint"].as_str().unwrap();
        assert!(
            hint.contains("DouyinDataReview") && hint.contains("导出数据"),
            "{hint}"
        );
        assert!(
            error["detail"]
                .as_str()
                .unwrap()
                .contains("field: (top level)"),
            "{name}: {error}"
        );
    }
    // 是 JSON 对象，但没有 records
    let error = failure(&file_in(tmp.path(), "o.json", b"{\"schemaVersion\": 2}"));
    assert_eq!(error["code"], "unsupported-input");
    assert!(error["detail"].as_str().unwrap().contains("field: records"));
}

#[test]
fn a_truncated_file_names_the_problem_without_content() {
    let tmp = tempfile::tempdir().unwrap();
    let whole = fixture(EXPORT);
    let cut = file_in(tmp.path(), "cut.json", &whole[..whole.len() / 2]);
    let error = failure(&cut);
    assert_eq!(error["code"], "unsupported-input", "{error}");
    assert!(
        error["detail"]
            .as_str()
            .unwrap()
            .contains("field: (top level)")
    );
}

#[test]
fn a_changed_format_is_answered_with_the_field() {
    let tmp = tempfile::tempdir().unwrap();
    // 每条的 id 都不成样子
    let error = failure(&file_in(tmp.path(), "c.json", &fixture("changed.json")));
    assert_eq!(error["code"], "upstream-changed", "{error}");
    assert!(
        error["detail"]
            .as_str()
            .unwrap()
            .contains("field: records.liked_videos.videoId"),
        "{error}"
    );
    // 版本号不是认得的那个
    let error = failure(&file_in(
        tmp.path(),
        "v.json",
        &fixture("newer-version.json"),
    ));
    assert_eq!(error["code"], "upstream-changed", "{error}");
    assert!(
        error["detail"]
            .as_str()
            .unwrap()
            .contains("field: schemaVersion")
    );
    // 数组的位置放了别的
    let error = failure(&file_in(
        tmp.path(),
        "s.json",
        br#"{"schemaVersion": 2, "records": {"watch_history": "nope"}}"#,
    ));
    assert_eq!(error["code"], "upstream-changed", "{error}");
    assert!(
        error["detail"]
            .as_str()
            .unwrap()
            .contains("records.watch_history")
    );
    // 顶层的 records 不是对象
    let error = failure(&file_in(
        tmp.path(),
        "r.json",
        br#"{"schemaVersion": 2, "records": [1]}"#,
    ));
    assert_eq!(error["code"], "upstream-changed", "{error}");
}

#[test]
fn hints_follow_the_locale() {
    let tmp = tempfile::tempdir().unwrap();
    let path = file_in(tmp.path(), "a.csv", b"id,title\n");
    let out = Command::new(BIN)
        .env_clear()
        .env("USUYO_LOCALE", "en-US")
        .env("USUYO_INPUT", &path)
        .args(["list", "file:x", "--method", "ddr-import"])
        .output()
        .unwrap();
    let hint = envelope(&out)["error"]["hint"].as_str().unwrap().to_owned();
    assert!(
        hint.starts_with("This does not read as a DouyinDataReview export"),
        "{hint}"
    );
}

#[test]
fn files_over_the_limits_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let path = file_in(tmp.path(), EXPORT, &fixture(EXPORT));
    let refused = |limits: &Limits| {
        let err = list("file:x", Some(&path), None, limits).unwrap_err();
        let failure = err.into_failure(true);
        (failure.code, failure.hint.unwrap_or_default())
    };
    let (code, hint) = refused(&Limits {
        file_bytes: 300,
        records: 1000,
    });
    assert_eq!(code, ErrorCode::UnsupportedInput);
    assert!(hint.contains("超过了插件愿意读的大小"), "{hint}");
    let (code, hint) = refused(&Limits {
        file_bytes: 1 << 20,
        records: 3,
    });
    assert_eq!(code, ErrorCode::UnsupportedInput);
    assert!(hint.contains("超过了插件愿意读的大小"), "{hint}");
    // 刚好够的不拒
    assert!(
        list(
            "file:x",
            Some(&path),
            None,
            &Limits {
                file_bytes: 1 << 20,
                records: 9,
            }
        )
        .is_ok()
    );
}

fn manifest() -> PluginManifest {
    let text = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("usuyo-plugin.yaml"),
    )
    .unwrap();
    let parsed = PluginManifest::from_yaml(&text).unwrap();
    assert!(
        parsed.unknown_fields.is_empty(),
        "{:?}",
        parsed.unknown_fields
    );
    parsed.value
}

#[test]
fn the_manifest_declares_the_ddr_import_method() {
    let m = manifest();
    let PluginKind::Fetch(spec) = &m.kind else {
        panic!("not a fetch plugin");
    };
    let method = spec.method("ddr-import").unwrap();
    assert!(method.built && method.sites.is_empty() && method.visitors.is_empty());
    assert_eq!(
        (method.access, method.login, method.account),
        (Access::OfficialExport, Login::None, Account::None)
    );
    assert!(method.on_by_default());
    let list = method.capabilities.list.as_ref().unwrap();
    // 新旧排的是看的时间，喜欢和收藏没有动作时间，不写 newest-first
    assert_eq!(list.order, ListOrder::Unordered);
    assert!(!list.files.is_empty() && list.reads.is_some());
    assert!(method.capabilities.fetch.is_none());
    let risk = method.risk.as_ref().unwrap();
    assert!(risk.text.contains("DouyinDataReview") && risk.text.contains("不向任何平台发请求"));
    assert_eq!(
        risk.source.url,
        "https://github.com/ZhangYukwiver/DouyinDataReview"
    );
}

fn host() -> (tempfile::TempDir, PluginHost) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("plugins/usuyo-fetch/douyin/0.1.0");
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("usuyo-plugin.yaml");
    std::fs::copy(manifest, dir.join("usuyo-plugin.yaml")).unwrap();
    std::fs::copy(BIN, dir.join("usuyo-fetch-douyin")).unwrap();
    let found = discover(&tmp.path().join("plugins")).unwrap();
    assert!(found.rejected.is_empty(), "{:?}", found.rejected);
    let host = PluginHost::new(
        HostConfig {
            work_dir: tmp.path().join("work"),
            cache_dir: tmp.path().join("cache"),
            locale: "zh-CN".into(),
            limits: CallLimits::default(),
        },
        found.fetch,
    );
    (tmp, host)
}

#[tokio::test]
async fn a_file_is_listed_through_the_host() {
    let (tmp, host) = host();
    let plugin = &host.plugins()[0];
    let input = tmp.path().join("input.json");
    std::fs::write(&input, fixture(EXPORT)).unwrap();
    let page = host
        .list(
            plugin,
            ListRequest {
                method: "ddr-import".into(),
                source: "file:抖音数据_2026-11-12_10-30-00.json".into(),
                cursor: None,
                since: None,
                input_file: Some(input),
            },
            &CallOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), EXPECTED.len());
    assert!(page.items.iter().all(|i| i.reference.is_none()));
    let acted: Vec<bool> = page.items.iter().map(|i| i.acted_at.is_some()).collect();
    assert_eq!(acted, [true, true, false, false, false, false]);
}
