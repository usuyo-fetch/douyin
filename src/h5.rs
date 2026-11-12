//! 方法 `h5-token`：分享页不带作品数据时，照 H5 页自己的做法调 `web/api/v2/aweme/iteminfo/`。
//!
//! 来源（MIT 许可证）：移植 wujunwei928/parse-video 的 PR #106（`parser/douyin_h5_detail.go`，未合并，
//! 读的是 PR 里的代码）：分享页里 `douyin_reflow_token` 的 `xsstoken`、`douyin_reflow_webId` 的 `webId`、
//! `usercip`，用 `webId` 的前 16 字节当密钥和 IV 做 AES-128-CBC（PKCS#7），Base64 以后作 `reflow_id` 带给接口。
//! MIT License，Copyright (c) 2022 wujunwei928。
//! 没有移植：PR #102 里用 ByteDance 的 union 接口换匿名 `ttwid`、再去调带签名的 PC 网页详情接口那一段
//! （要自己算签名，不属于这种方法），和设了 Cookie 才用的那一段。
//!
//! 令牌只在这一次调用的内存里：分享页给的 `xsstoken` 约 50 秒过期（页面的 `tccconfig` 里写着），
//! 不存盘，不进输出目录，也不进 `detail`（失败时 `detail` 只带接口的回答）。

use aes::Aes128;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cbc::Encryptor;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockEncryptMut, KeyIvInit};
use serde_json::Value;

use crate::page::{PageError, Parsed, from_item, head, text_of, unavailable};

/// 分享页算出来的一次性上下文。
pub struct Context {
    token: String,
    web_id: String,
    user_cip: Option<String>,
}

/// 从分享页的 HTML 里读 H5 的上下文。读不出来时回原因。
pub fn context(html: &str) -> Result<Context, &'static str> {
    let token = tag_attr(html, "douyin_reflow_token", "xsstoken")
        .ok_or("the share page has no `xsstoken`")?;
    let web_id =
        tag_attr(html, "douyin_reflow_webId", "webId").ok_or("the share page has no `webId`")?;
    if web_id.len() < 16 {
        return Err("the share page's `webId` is shorter than 16 bytes");
    }
    Ok(Context {
        token,
        user_cip: tag_attr(html, "douyin_reflow_webId", "usercip"),
        web_id,
    })
}

impl Context {
    /// 接口的查询参数（不含 `item_ids` 以外的作品信息）。
    pub fn query(&self, id: &str) -> Vec<(&'static str, String)> {
        let mut query = vec![
            ("reflow_source", "reflow_page".to_owned()),
            ("web_id", self.web_id.clone()),
            ("device_id", self.web_id.clone()),
            ("use_new_select_scope", "0".to_owned()),
            ("item_ids", id.to_owned()),
            ("reflow_id", reflow_id(&self.token, &self.web_id)),
        ];
        if let Some(cip) = &self.user_cip {
            query.push(("user_cip", cip.clone()));
        }
        query
    }
}

/// `webId` 的前 16 字节当密钥和 IV，AES-128-CBC、PKCS#7，再 Base64。调用前已查过 `webId` 不短于 16 字节。
pub fn reflow_id(token: &str, web_id: &str) -> String {
    let key = &web_id.as_bytes()[..16];
    let cipher = Encryptor::<Aes128>::new(key.into(), key.into());
    STANDARD.encode(cipher.encrypt_padded_vec_mut::<Pkcs7>(token.as_bytes()))
}

/// 读接口的回答：`{status_code, item_list: [...], filter_list?}`，条目的形状和分享页的 `item_list[0]` 一样。
pub fn parse_answer(raw: &[u8], id: &str) -> Result<Parsed, PageError> {
    let text = String::from_utf8_lossy(raw);
    let detail = || head(&text).to_owned();
    let Ok(root) = serde_json::from_slice::<Value>(raw) else {
        return Err(PageError::Missing {
            what: "the item info answer is not JSON",
            detail: detail(),
        });
    };
    if root["status_code"].as_i64() != Some(0) {
        return Err(PageError::Missing {
            what: "the item info answer's `status_code` is not 0",
            detail: detail(),
        });
    }
    let Some(item) = root["item_list"].as_array().and_then(|a| a.first()) else {
        let filtered = root["filter_list"]
            .as_array()
            .and_then(|a| a.iter().find(|e| text_of(&e["aweme_id"]) == id));
        return Err(match filtered {
            Some(entry) => unavailable(entry, detail()),
            None => PageError::Missing {
                what: "the item info answer has neither an item nor a filter entry for this id",
                detail: detail(),
            },
        });
    };
    if text_of(&item["aweme_id"]) != id {
        return Err(PageError::Missing {
            what: "the item info answer is for a different post",
            detail: detail(),
        });
    }
    Ok(Parsed {
        post: from_item(item),
        raw: text.into_owned(),
    })
}

/// 找 `id` 是 `element_id` 的那个标签，取它的 `attr` 属性值。值可以带引号，也可以不带
/// （实测的分享页写成 `<div id=douyin_reflow_token xsstoken=… >`）。
fn tag_attr(html: &str, element_id: &str, attr: &str) -> Option<String> {
    let marks = [
        format!("id={element_id}"),
        format!("id='{element_id}'"),
        format!("id=\"{element_id}\""),
    ];
    let at = marks
        .iter()
        .filter_map(|m| html.find(m.as_str()).map(|i| (i, m.len())))
        .min()?;
    let start = html[..at.0].rfind('<')?;
    let end = at.0 + html[at.0..].find('>')?;
    let tag = &html[start..end];
    let lower = tag.to_ascii_lowercase();
    let key = format!(" {}=", attr.to_ascii_lowercase());
    let from = lower.find(&key)? + key.len();
    let rest = &tag[from..];
    let value = match rest.chars().next()? {
        q @ ('\'' | '"') => rest[1..].split(q).next()?,
        _ => rest.split_whitespace().next()?,
    };
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "<div id='douyin_reflow_location' location={\"a\":1} ><div id=douyin_reflow_token xsstoken=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef ><div id='douyin_reflow_webId' webId=7700000000000000001 usercip=203.0.113.7 >";

    #[test]
    fn reads_the_context_in_the_measured_shape() {
        let c = context(PAGE).unwrap();
        assert_eq!(c.web_id, "7700000000000000001");
        assert_eq!(c.user_cip.as_deref(), Some("203.0.113.7"));
        assert_eq!(c.token.len(), 96);
        assert_eq!(
            context("<div id=\"douyin_reflow_token\" xsstoken=\"abc\"><div id=\"douyin_reflow_webId\" webId=\"1234567890123456\">")
                .unwrap()
                .token,
            "abc"
        );
        assert!(context("<p>nothing</p>").is_err());
        assert!(
            context(
                "<div id=douyin_reflow_token xsstoken=a ><div id=douyin_reflow_webId webId=123 >"
            )
            .is_err()
        );
    }

    /// 期望值用 `openssl enc -aes-128-cbc -K <webId 前 16 字节的十六进制> -iv <同> -base64` 算的。
    #[test]
    fn reflow_id_is_aes_128_cbc_with_the_web_id_as_key_and_iv() {
        let c = context(PAGE).unwrap();
        assert_eq!(
            reflow_id(&c.token, &c.web_id),
            "2HXMN/+tXNUwa0w3ui6cXAybo/LeIwkoQJosEMvaCwiZYTBJ+u4KEBp/2/XQKP/HG0qFGxiDxWrTVRaIbeH02nlGxJKBIaV1ze9UyleDg6Tec5Ja6dlbpL1y3zSGA/g6Kunt7JB3H18/rmRxS9FgFA=="
        );
    }
}
