//! `ddr-import` 方法（协议 §3.9）：读 DouyinDataReview 的「导出数据」存下的 JSON 文件，把观看历史、喜欢、收藏
//! 列成抖音链接，内容交给本插件自己的方法去取。不联网，只读你交来的那一个文件，条目不带 `ref`。
//!
//! 文件的形状（`collector/dataExport.mjs`，`schemaVersion` 2）：顶层 `records` 里有三个数组
//! `watch_history`、`liked_videos`、`favorite_videos`，每条有 `videoId`（作品 id）、`title`、`author`、
//! `occurredAt`、`occurredAtSource`、`publishedAt`。身份和链接用插件自己的代码重建，不用记录里的 `url`。
//!
//! 时间：只有观看历史的 `occurredAt` 是你看它的时间（`occurredAtSource` 是 `platform_action` 或
//! `archive_action` 时才取）。喜欢和收藏的 `occurredAt` 来自抖音的播放进度时间，是最后一次播放它的时间，
//! 不是点喜欢或收藏的时间，所以这两种动作的 `actedAt` 一律留空。

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::Deserialize;
use serde_json::Value;
use serde_json::value::RawValue;
use usuyo_plugin_kit::protocol::{
    Access, Author, CallFailure, ErrorCode, Evidence, ListAction, ListItem, ListPage, Rfc3339,
};

use crate::canonical;
use crate::ids::{Parsed, canonical_url, parse_input};

/// 一页最多多少条。
const PAGE: usize = 500;
/// 标题最多多少个字符。
const TITLE_CHARS: usize = 100;
/// DouyinDataReview 给没有描述的作品填的标题，不是作品自己的字。
const PLACEHOLDER_TITLES: [&str; 2] = ["未命名视频", "Untitled video"];
/// 认得的 `schemaVersion`。
const SCHEMA_VERSION: u64 = 2;

/// 交来的文件不可信：读多少、数多少有上限。测试用小的上限，运行用 [`Limits::default`]。
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub file_bytes: u64,
    pub records: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            file_bytes: 256 << 20,
            records: 1_000_000,
        }
    }
}

/// 三种记录：数组名、动作、会不会带真正的动作时间。
const KINDS: [Kind; 3] = [
    Kind {
        key: "watch_history",
        action: ListAction::View,
        acted_at: true,
    },
    Kind {
        key: "liked_videos",
        action: ListAction::Like,
        acted_at: false,
    },
    Kind {
        key: "favorite_videos",
        action: ListAction::Favorite,
        acted_at: false,
    },
];

struct Kind {
    key: &'static str,
    action: ListAction,
    acted_at: bool,
}

#[derive(Deserialize)]
struct Export {
    #[serde(rename = "schemaVersion")]
    schema_version: Option<Value>,
    records: Option<HashMap<String, Box<RawValue>>>,
}

#[derive(Debug, Clone, Copy)]
enum Hint {
    NotAnExport,
    TooLarge,
    Changed,
}

impl Hint {
    fn text(self, zh: bool) -> &'static str {
        match (self, zh) {
            (Hint::NotAnExport, true) => {
                "读不出 DouyinDataReview 的导出文件：请在它的「设置 → 数据」点「导出数据」，勾选观看历史、喜欢或收藏，保存成 JSON 再选这个文件（官方的「个人信息下载」文件不在这里读）"
            }
            (Hint::NotAnExport, false) => {
                "This does not read as a DouyinDataReview export: in its Settings > Data choose Export data, tick watch history, likes or favorites, save as JSON and choose that file (the official personal data download is not read here)"
            }
            (Hint::TooLarge, true) => "这个文件超过了插件愿意读的大小：按时间只导出一部分再选",
            (Hint::TooLarge, false) => {
                "This file is larger than the plugin will read: export only a time range and choose that"
            }
            (Hint::Changed, true) => {
                "DouyinDataReview 的导出文件和这个插件认得的不一样，插件要更新"
            }
            (Hint::Changed, false) => {
                "The DouyinDataReview export differs from what this plugin reads; the plugin needs an update"
            }
        }
    }
}

/// 读文件失败的几种方式；到入口才变成协议的错误。
#[derive(Debug)]
pub enum Refusal {
    InvalidArgs(String),
    NotAnExport { message: String, detail: String },
    TooLarge { message: String, detail: String },
    Changed { message: String, detail: String },
    Internal(String),
}

impl Refusal {
    fn not_an_export(field: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        Refusal::NotAnExport {
            detail: format!("field: {field}\n{message}"),
            message,
        }
    }

    fn changed(field: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        Refusal::Changed {
            detail: format!("field: {field}\n{message}"),
            message,
        }
    }

    pub fn into_failure(self, zh: bool) -> CallFailure {
        let with = |code, message: String, hint: Hint, detail: String| {
            CallFailure {
                hint: Some(hint.text(zh).to_owned()),
                ..CallFailure::new(code, message)
            }
            .with_detail(&detail)
        };
        match self {
            Refusal::InvalidArgs(m) => {
                CallFailure::new(ErrorCode::InvalidArgs, format!("invalid arguments: {m}"))
            }
            Refusal::NotAnExport { message, detail } => with(
                ErrorCode::UnsupportedInput,
                format!("unsupported input: {message}"),
                Hint::NotAnExport,
                detail,
            ),
            Refusal::TooLarge { message, detail } => with(
                ErrorCode::UnsupportedInput,
                format!("unsupported input: {message}"),
                Hint::TooLarge,
                detail,
            ),
            Refusal::Changed { message, detail } => with(
                ErrorCode::UpstreamChanged,
                format!("DouyinDataReview export format changed: {message}"),
                Hint::Changed,
                detail,
            ),
            Refusal::Internal(m) => CallFailure::new(ErrorCode::Internal, m),
        }
    }
}

fn read_input(path: &Path, limits: &Limits) -> Result<Vec<u8>, Refusal> {
    let file =
        File::open(path).map_err(|e| Refusal::Internal(format!("cannot open the input: {e}")))?;
    let len = file
        .metadata()
        .map_err(|e| Refusal::Internal(format!("cannot read the input: {e}")))?
        .len();
    if len > limits.file_bytes {
        return Err(Refusal::TooLarge {
            message: format!(
                "the file is {len} bytes, over the {} byte limit",
                limits.file_bytes
            ),
            detail: "field: (whole file)".into(),
        });
    }
    let mut bytes = Vec::new();
    file.take(limits.file_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Refusal::Internal(format!("cannot read the input: {e}")))?;
    Ok(bytes)
}

/// 一条记录列成的条目，和排序用的时间。
struct Entry {
    item: ListItem,
    acted: Option<i64>,
}

fn text(record: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    match record.get(key)? {
        Value::String(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// 作品 id：先 `videoId`，没有再从记录里的链接认；过一遍本插件认链接的代码，直播（`live-…`）和不成样子的不要。
fn video_id(record: &serde_json::Map<String, Value>) -> Option<String> {
    let candidate = text(record, "videoId")
        .map(|id| canonical_url(&id))
        .or_else(|| text(record, "url"))?;
    match parse_input(&candidate, |_| false) {
        Parsed::Post(id) => Some(id),
        Parsed::ShortLink(_) | Parsed::Foreign => None,
    }
}

fn rfc3339(value: Option<&Value>) -> Option<Timestamp> {
    value?.as_str()?.trim().parse::<Timestamp>().ok()
}

fn entries(export: Export, limits: &Limits) -> Result<Vec<ListItem>, Refusal> {
    let version = export.schema_version.as_ref().and_then(Value::as_u64);
    if version != Some(SCHEMA_VERSION) {
        return Err(Refusal::changed(
            "schemaVersion",
            format!(
                "expected {SCHEMA_VERSION}, found {:?}",
                export.schema_version
            ),
        ));
    }
    let records = export
        .records
        .ok_or_else(|| Refusal::not_an_export("records", "the file has no `records`"))?;
    let mut out: Vec<Entry> = Vec::new();
    // 同一动作下同一作品只留一条；观看历史留最近的一次
    let mut seen: HashMap<(&'static str, String), usize> = HashMap::new();
    let (mut total, mut unreadable, mut live) = (0usize, 0usize, 0usize);
    let mut first_problem: Option<(String, String)> = None;
    for kind in &KINDS {
        let Some(raw) = records.get(kind.key) else {
            continue;
        };
        let list: Vec<Box<RawValue>> = serde_json::from_str(raw.get()).map_err(|e| {
            Refusal::changed(
                &format!("records.{}", kind.key),
                format!("not an array: {e}"),
            )
        })?;
        total += list.len();
        if total > limits.records {
            return Err(Refusal::TooLarge {
                message: format!("more than {} records", limits.records),
                detail: "field: (records)".into(),
            });
        }
        for raw in list {
            let Ok(record) = serde_json::from_str::<serde_json::Map<String, Value>>(raw.get())
            else {
                unreadable += 1;
                first_problem.get_or_insert((
                    format!("records.{}", kind.key),
                    "a record is not an object".into(),
                ));
                continue;
            };
            if text(&record, "videoId").is_some_and(|id| id.starts_with("live-"))
                || record.get("mediaType").and_then(Value::as_str) == Some("live")
            {
                live += 1;
                continue;
            }
            let Some(id) = video_id(&record) else {
                unreadable += 1;
                first_problem.get_or_insert((
                    format!("records.{}.videoId", kind.key),
                    "a record has no usable video id".into(),
                ));
                continue;
            };
            let acted = if kind.acted_at
                && matches!(
                    record.get("occurredAtSource").and_then(Value::as_str),
                    Some("platform_action" | "archive_action")
                ) {
                rfc3339(record.get("occurredAt"))
            } else {
                None
            };
            let title = text(&record, "title")
                .filter(|t| !PLACEHOLDER_TITLES.contains(&t.as_str()))
                .and_then(|t| {
                    let line = t.lines().map(str::trim).find(|l| !l.is_empty())?;
                    Some(line.chars().take(TITLE_CHARS).collect::<String>())
                });
            let entry = Entry {
                item: ListItem {
                    url: Some(canonical_url(&id)),
                    reference: None,
                    identity: Some(
                        canonical::identity(&id)
                            .map_err(|e| Refusal::Internal(e.message.to_string()))?,
                    ),
                    title,
                    author: text(&record, "author").map(|name| Author {
                        name,
                        platform_id: None,
                        url: None,
                    }),
                    published_at: rfc3339(record.get("publishedAt")).map(Rfc3339::from_timestamp),
                    action: Some(kind.action),
                    acted_at: acted.map(Rfc3339::from_timestamp),
                },
                acted: acted.map(|t| t.as_second()),
            };
            match seen.get(&(kind.key, id.clone())) {
                Some(&at) => {
                    if entry.acted > out[at].acted {
                        out[at] = entry;
                    }
                }
                None => {
                    seen.insert((kind.key, id), out.len());
                    out.push(entry);
                }
            }
        }
    }
    if unreadable > 0 {
        let (field, problem) = first_problem.unwrap_or_default();
        eprintln!("{unreadable} records were skipped: {field}: {problem}");
        if out.is_empty() && live == 0 {
            return Err(Refusal::changed(
                &field,
                format!("none of the records is readable: {problem}"),
            ));
        }
    }
    if live > 0 {
        eprintln!("{live} live-stream records are not listed: they have no post to fetch");
    }
    // 有动作时间的新的在前，没有的在后；同样的保持文件里的先后
    out.sort_by_key(|e| std::cmp::Reverse(e.acted));
    Ok(out.into_iter().map(|e| e.item).collect())
}

/// 列一页。`cursor` 是从第几条开始（插件自己的不透明字符串）。
pub fn list(
    source: &str,
    input: Option<&Path>,
    cursor: Option<&str>,
    limits: &Limits,
) -> Result<ListPage, Refusal> {
    if !source.starts_with("file:") {
        return Err(Refusal::InvalidArgs(format!(
            "source `{source}` is not a file import"
        )));
    }
    let input = input
        .ok_or_else(|| Refusal::InvalidArgs("USUYO_INPUT is not set for a file import".into()))?;
    let start = match cursor {
        None => 0,
        Some(c) => c
            .parse::<usize>()
            .map_err(|_| Refusal::InvalidArgs(format!("cursor `{c}` is not ours")))?,
    };
    let fetched_at = Rfc3339::from_timestamp(Timestamp::now());
    let bytes = read_input(input, limits)?;
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
    if bytes.iter().copied().find(|b| !b.is_ascii_whitespace()) != Some(b'{') {
        return Err(Refusal::not_an_export(
            "(top level)",
            "the file is not a JSON object",
        ));
    }
    let export: Export = serde_json::from_slice(bytes).map_err(|e| match e.classify() {
        serde_json::error::Category::Data => {
            Refusal::changed("(top level)", format!("the export cannot be read: {e}"))
        }
        _ => Refusal::not_an_export("(top level)", format!("not a valid JSON file: {e}")),
    })?;
    let all = entries(export, limits)?;
    let end = start.saturating_add(PAGE).min(all.len());
    let items = all.get(start..end).unwrap_or_default().to_vec();
    Ok(ListPage {
        items,
        next_cursor: (end < all.len()).then(|| end.to_string()),
        evidence: Evidence {
            fetched_at,
            access: Access::OfficialExport,
            site: None,
            upstream: None,
            status: None,
        },
    })
}

/// 入口用的一次调用：交来的文件在 `USUYO_INPUT`，`hint` 的语言看 `USUYO_LOCALE`。
pub fn list_call(source: &str, cursor: Option<&str>) -> Result<ListPage, CallFailure> {
    let zh = std::env::var("USUYO_LOCALE")
        .ok()
        .is_none_or(|l| l.to_ascii_lowercase().starts_with("zh"));
    let input = std::env::var_os("USUYO_INPUT").map(PathBuf::from);
    list(source, input.as_deref(), cursor, &Limits::default())
        .map_err(|refusal| refusal.into_failure(zh))
}
