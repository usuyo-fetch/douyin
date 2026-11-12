//! `canonicalize`：认出抖音作品的身份。`fetch` 用同一个函数，所以两边给的身份一样。

use usuyo_plugin_kit::protocol::{CallFailure, Canonicalized, ErrorCode, Identity, PlatformId};

use crate::api::Douyin;
use crate::ids::{Parsed, canonical_url, parse_input};

/// 输入指向的作品；输入是短链时还带着展开后的完整链接。
pub struct Located {
    pub id: String,
    pub resolved_url: Option<String>,
}

/// 认出输入是哪个作品。`None` 是「不是抖音的作品链接」，不是错误。
/// 只有短链会发请求，请求不带 cookie，只跟随重定向，并且只在抖音自己的主机之间。
pub fn locate(api: &Douyin, input: &str) -> Result<Option<Located>, CallFailure> {
    let endpoints = api.endpoints();
    match parse_input(input, |h| endpoints.is_short_link_host(h)) {
        Parsed::Post(id) => Ok(Some(Located {
            id,
            resolved_url: None,
        })),
        Parsed::Foreign => Ok(None),
        Parsed::ShortLink(url) => {
            let expanded = api.expand_short_link(url)?;
            Ok(
                match parse_input(expanded.as_str(), |h| endpoints.is_short_link_host(h)) {
                    Parsed::Post(id) => Some(Located {
                        id,
                        resolved_url: Some(expanded.into()),
                    }),
                    Parsed::ShortLink(_) | Parsed::Foreign => None,
                },
            )
        }
    }
}

pub fn identity(id: &str) -> Result<Identity, CallFailure> {
    let platform =
        PlatformId::try_from("douyin").map_err(|e| CallFailure::new(ErrorCode::Internal, e))?;
    Ok(Identity::Platform {
        platform,
        content_id: id.to_owned(),
        url: Some(canonical_url(id)),
    })
}

pub fn canonicalize(api: &Douyin, input: &str) -> Result<Canonicalized, CallFailure> {
    Ok(match locate(api, input)? {
        Some(found) => Canonicalized::Match {
            identity: identity(&found.id)?,
            resolved_url: found.resolved_url,
        },
        None => Canonicalized::NoMatch,
    })
}
