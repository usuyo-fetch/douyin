//! 真的二进制：不需要网络的子命令，以及清单经宿主的校验和调用。

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;
use usuyo_plugin::protocol::{Access, Account, Canonicalized, ErrorCode, Identity, Login};
use usuyo_plugin::{
    CallLimits, CallOptions, FetchInput, FetchRequest, HostConfig, PluginError, PluginHost,
    discover,
};

const BIN: &str = env!("CARGO_BIN_EXE_usuyo-fetch-douyin");

/// 清空环境后运行，和宿主一样。
fn run(args: &[&str]) -> (i32, Value) {
    let output = Command::new(BIN).args(args).env_clear().output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json = if stdout.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&stdout).unwrap()
    };
    (output.status.code().unwrap(), json)
}

#[test]
fn canonicalize_needs_no_network_for_full_links() {
    let (code, out) = run(&[
        "canonicalize",
        "https://www.douyin.com/note/7431234567890123456",
    ]);
    assert_eq!(code, 0);
    assert_eq!(out["ok"], true);
    assert_eq!(out["match"], true);
    assert_eq!(out["identity"]["platform"], "douyin");
    assert_eq!(out["identity"]["contentId"], "7431234567890123456");
    assert_eq!(
        out["identity"]["url"],
        "https://www.douyin.com/video/7431234567890123456"
    );
    assert!(out.get("resolvedUrl").is_none());

    let (code, out) = run(&[
        "canonicalize",
        "https://example.com/video/7431234567890123456",
    ]);
    assert_eq!((code, &out["match"]), (0, &Value::Bool(false)));
}

#[test]
fn health_does_not_touch_the_network() {
    for args in [&["health"][..], &["health", "--probe"][..]] {
        let (code, out) = run(args);
        assert_eq!(code, 0);
        assert_eq!(
            (out["ready"].as_bool(), out["protocol"].as_u64()),
            (Some(true), Some(1))
        );
        assert_eq!(out["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(out["login"], serde_json::json!([]));
        assert_eq!(out["checks"], serde_json::json!([]));
    }
}

#[test]
fn bad_usage_exits_2() {
    for args in [
        &[][..],
        &["nope"][..],
        &["fetch", "x"][..],
        &["fetch", "x", "--out", "/o"][..],
        &["sources"][..],
        &["canonicalize"][..],
    ] {
        let (code, out) = run(args);
        assert_eq!((code, out), (2, Value::Null), "{args:?}");
    }
}

#[test]
fn each_call_goes_to_its_method() {
    // 方法和调用对不上的回 invalid-args，不发请求
    for args in [
        &["sources", "--method", "share-page"][..],
        &["sources", "--method", "h5-token"][..],
        &["list", "douyin:likes", "--method", "userscript"][..],
        &[
            "fetch",
            "https://www.douyin.com/video/7431234567890123456",
            "--out",
            "/o",
            "--method",
            "export",
        ][..],
        &[
            "fetch",
            "https://www.douyin.com/video/7431234567890123456",
            "--out",
            "/o",
            "--method",
            "userscript",
        ][..],
    ] {
        let (code, out) = run(args);
        assert_eq!(
            (code, &out["error"]["code"]),
            (1, &Value::from("invalid-args")),
            "{args:?}"
        );
    }
    // 不是抖音的链接：不发请求，在读到凭据之前就能判定
    let tmp = tempfile::tempdir().unwrap();
    let (code, out) = run(&[
        "fetch",
        "https://example.com/x",
        "--out",
        tmp.path().to_str().unwrap(),
        "--method",
        "share-page",
    ]);
    assert_eq!(code, 1);
    assert_eq!(out["error"]["code"], "unsupported-input");
}

/// 清单装成宿主认的样子：`<root>/usuyo-fetch/douyin/<version>/`。
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
            locale: usuyo_plugin::Locale::new("zh-CN"),
            limits: CallLimits::default(),
        },
        found.fetch,
    );
    (tmp, host)
}

#[tokio::test]
async fn the_manifest_loads_and_matches_douyin_links() {
    let (_tmp, host) = host();
    let plugin = &host.plugins()[0];
    assert_eq!(plugin.id().as_str(), "usuyo-fetch/douyin");
    assert_eq!(plugin.header.version.to_string(), env!("CARGO_PKG_VERSION"));
    let spec = &plugin.spec;
    // routes.md 第 7 节的每一行（client-hook 那行在另一个组织），加上 mediacrawler，顺序照清单
    let ids: Vec<&str> = spec.methods.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "share-page",
            "h5-token",
            "export",
            "userscript",
            "ddr-import",
            "iphone",
            "android",
            "mediacrawler"
        ]
    );
    let share = spec.method("share-page").unwrap();
    assert_eq!(
        (share.access, share.login, share.account),
        (Access::PublicWeb, Login::None, Account::None)
    );
    assert!(share.built && share.sites.is_empty() && share.capabilities.fetch.is_some());
    // 带页面算出的参数，层级是 private-api：宿主默认关着
    let h5 = spec.method("h5-token").unwrap();
    assert_eq!(
        (h5.access, h5.login, h5.account),
        (Access::PrivateApi, Login::None, Account::None)
    );
    assert!(h5.built && h5.sites.is_empty() && h5.capabilities.fetch.is_some());
    assert!(!h5.on_by_default() && share.on_by_default());
    let userscript = spec.method("userscript").unwrap();
    assert_eq!(
        (userscript.access, userscript.account),
        (Access::BrowserSession, Account::OwnBrowser)
    );
    assert!(userscript.built && userscript.delivers.is_some());
    let ddr = spec.method("ddr-import").unwrap();
    assert!(ddr.built && ddr.capabilities.list.is_some());
    assert_eq!(
        (ddr.access, ddr.login, ddr.account),
        (Access::OfficialExport, Login::None, Account::None)
    );
    for (id, access, account) in [
        ("export", Access::OfficialExport, Account::None),
        ("iphone", Access::VisionUi, Account::OwnPhone),
        ("android", Access::VisionUi, Account::OwnPhone),
    ] {
        let m = spec.method(id).unwrap();
        assert!(!m.built, "{id}");
        assert_eq!((m.access, m.account), (access, account), "{id}");
    }
    for hit in [
        "https://v.douyin.com/iRNBho6u/",
        "https://www.douyin.com/video/7431234567890123456",
        "https://www.douyin.com/note/7431234567890123456",
        "https://www.iesdouyin.com/share/video/7431234567890123456/",
    ] {
        let url = url::Url::parse(hit).unwrap();
        assert_eq!(host.candidates(&url).len(), 1, "{hit}");
    }
    let url = url::Url::parse("https://www.douyin.com/user/self").unwrap();
    assert!(host.candidates(&url).is_empty());

    // 经宿主调一次：输出要过宿主的 schema 校验
    let url = url::Url::parse("https://www.douyin.com/video/7431234567890123456").unwrap();
    let canonical = host
        .canonicalize(host.candidates(&url)[0], &url, &CallOptions::default())
        .await
        .unwrap();
    let Canonicalized::Match {
        identity: Identity::Platform { content_id, .. },
        ..
    } = canonical
    else {
        panic!("{canonical:?}")
    };
    assert_eq!(content_id, "7431234567890123456");
}

#[tokio::test]
async fn a_link_that_is_not_a_post_is_turned_away_through_the_host() {
    let (_tmp, host) = host();
    let plugin = &host.plugins()[0];
    let url = url::Url::parse("https://www.douyin.com/video/not-a-number").unwrap();
    let request = FetchRequest {
        method: "share-page".into(),
        input: FetchInput::Url(url),
        want: None,
        input_file: None,
    };
    let err = host
        .fetch(plugin, request, &CallOptions::default())
        .await
        .unwrap_err();
    let PluginError::Call { failure, .. } = err else {
        panic!("{err:?}")
    };
    assert_eq!(failure.code, ErrorCode::UnsupportedInput);
}
