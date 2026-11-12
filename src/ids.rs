//! 认链接：从一条链接或用户粘贴的分享文字里找出抖音作品的数字 id。不发请求。
//!
//! 认的形状：
//! - `v.douyin.com/<短码>`：App 的分享链接，要展开才知道 id
//! - `www.douyin.com/video/<id>`、`www.douyin.com/note/<id>`
//! - `www.iesdouyin.com/share/video/<id>`；`share/note/<id>`、`share/slides/<id>` 是照记忆加的，没实测
//!
//! id 只收纯数字，主机整个比对，不取「路径最后一段」。

use url::Url;

/// 短码最长多少：分享链接里的短码只有十来个字符，留足余量。
const MAX_CODE: usize = 32;
/// 作品 id 是十进制，十九位上下；留足余量，挡住拼进来的长串。
const MAX_ID: usize = 20;
/// 粘贴的文字最多看这么多字节。
const MAX_TEXT: usize = 4096;

pub const SHORT_LINK_HOST: &str = "v.douyin.com";

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// 作品的数字 id。
    Post(String),
    /// 要展开的短链。
    ShortLink(Url),
    Foreign,
}

/// 输入是一条链接，或带链接的分享文字（「7.87 Pjm:/ 复制打开抖音 https://v.douyin.com/… 复制此链接…」）。
pub fn parse_input(input: &str, is_short_host: impl Fn(&str) -> bool) -> Parsed {
    let Some(url) = find_url(input) else {
        return Parsed::Foreign;
    };
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return Parsed::Foreign;
    };
    let segments: Vec<&str> = url
        .path()
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let post = |id: &str| {
        if is_id(id) {
            Parsed::Post(id.to_owned())
        } else {
            Parsed::Foreign
        }
    };
    match (host.as_str(), segments.as_slice()) {
        ("www.douyin.com" | "douyin.com", ["video" | "note", id]) => post(id),
        ("www.iesdouyin.com" | "iesdouyin.com", ["share", "video" | "note" | "slides", id]) => {
            post(id)
        }
        (h, [code]) if is_short_host(h) && is_code(code) => Parsed::ShortLink(url),
        _ => Parsed::Foreign,
    }
}

/// 整个输入是一条链接就用它；不然取文字里第一个 `http(s)://` 起到空白或非 ASCII 字符为止的一段。
fn find_url(input: &str) -> Option<Url> {
    let text = &input[..floor(input, MAX_TEXT)];
    let text = text.trim();
    if let Ok(url) = Url::parse(text)
        && matches!(url.scheme(), "http" | "https")
    {
        return Some(url);
    }
    let start = ["https://", "http://"]
        .iter()
        .filter_map(|p| text.find(p))
        .min()?;
    let rest = &text[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_graphic() || matches!(c, '"' | '\'' | '<' | '>'))
        .unwrap_or(rest.len());
    let candidate = rest[..end].trim_end_matches(['.', ',', ';', ')', ']', '!']);
    Url::parse(candidate)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
}

fn floor(text: &str, max: usize) -> usize {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn is_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && id.bytes().all(|b| b.is_ascii_digit())
}

fn is_code(code: &str) -> bool {
    code.len() <= MAX_CODE
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// 规范链接：只用于去重和显示，视频和图文用同一种写法。
pub fn canonical_url(id: &str) -> String {
    format!("https://www.douyin.com/video/{id}")
}

/// 是不是抖音自己的主机：展开短链时，跳转只许落在这些主机上。
pub fn is_douyin_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "douyin.com"
        || host.ends_with(".douyin.com")
        || host == "iesdouyin.com"
        || host.ends_with(".iesdouyin.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Parsed {
        parse_input(input, |h| h == SHORT_LINK_HOST)
    }

    fn post(id: &str) -> Parsed {
        Parsed::Post(id.into())
    }

    #[test]
    fn post_links() {
        let id = "7431234567890123456";
        for input in [
            "https://www.douyin.com/video/7431234567890123456",
            "https://www.douyin.com/video/7431234567890123456?previous_page=app_code_link",
            "https://www.douyin.com/note/7431234567890123456/",
            "https://www.iesdouyin.com/share/video/7431234567890123456/?region=CN&mid=1&u_code=0",
            "https://www.iesdouyin.com/share/slides/7431234567890123456",
            "  http://douyin.com/video/7431234567890123456\n",
        ] {
            assert_eq!(parse(input), post(id), "{input}");
        }
    }

    #[test]
    fn share_text() {
        let text = "7.87 Pjm:/ 03/10 e@a.Nq 手冲咖啡三段式 https://v.douyin.com/iRNBho6u/ 复制此链接，打开Dou音搜索，直接观看视频！";
        let Parsed::ShortLink(url) = parse(text) else {
            panic!("{:?}", parse(text))
        };
        assert_eq!(url.as_str(), "https://v.douyin.com/iRNBho6u/");
        assert_eq!(
            parse("看这个 https://www.douyin.com/video/7431234567890123456，真好"),
            post("7431234567890123456")
        );
        assert_eq!(parse("没有链接 7.87 Pjm:/"), Parsed::Foreign);
    }

    #[test]
    fn short_links() {
        assert!(matches!(
            parse("https://v.douyin.com/iRNBho6u/"),
            Parsed::ShortLink(_)
        ));
        assert!(matches!(
            parse("https://v.douyin.com/iRNBho6u"),
            Parsed::ShortLink(_)
        ));
        assert_eq!(parse("https://v.douyin.com/"), Parsed::Foreign);
        assert_eq!(parse("https://v.douyin.com/a/b"), Parsed::Foreign);
        assert_eq!(parse("https://v.douyin.com/a%2Fb"), Parsed::Foreign);
    }

    #[test]
    fn foreign() {
        for input in [
            "https://www.douyin.com/user/MS4wLjABAAAAsample",
            "https://www.douyin.com/video/",
            "https://www.douyin.com/video/abc",
            "https://www.douyin.com/video/7431234567890123456/extra",
            "https://www.douyin.com/video/743123456789012345678",
            "https://evil.example/video/7431234567890123456",
            "https://www.douyin.com.evil.example/video/7431234567890123456",
            "https://notdouyin.com/video/7431234567890123456",
            "https://www.iesdouyin.com/share/user/7431234567890123456",
            "ftp://www.douyin.com/video/7431234567890123456",
            "not a url",
            "",
        ] {
            assert_eq!(parse(input), Parsed::Foreign, "{input}");
        }
    }

    #[test]
    fn douyin_hosts() {
        assert!(is_douyin_host("www.iesdouyin.com"));
        assert!(is_douyin_host("V.douyin.com"));
        assert!(!is_douyin_host("evildouyin.com"));
        assert!(!is_douyin_host("douyin.com.evil.test"));
    }
}
