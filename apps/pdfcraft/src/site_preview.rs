//! Vision Bubble (`pdfcraft_ui_egui::link_bubble`): a preview of the website a link points to,
//! asked for only when the user turned the extension on.
//!
//! Only the site's **home page** is read (`https://<host>/`), never the link's own address: a
//! link in a document can be a one-time action (an unsubscribe, a confirmation, a password reset)
//! that a request would carry out. From the page's head come its title, description, preview
//! image (`og:image`) and the icon it declares; each is one HTTPS request without cookies, a
//! short timeout, at most three redirects and a size cap, and only to plain host names.

use std::time::Duration;

use pdfcraft_ui_egui::link_bubble::SitePreview;

/// The most bytes read of the page, of its preview image and of its icon.
const MAX_PAGE: u64 = 512 * 1024;
const MAX_IMAGE: u64 = 3 * 1024 * 1024;
const MAX_ICON: u64 = 256 * 1024;
/// The longest title and description kept, in characters.
const MAX_TITLE: usize = 200;
const MAX_DESCRIPTION: usize = 400;

/// What a page's head says about it.
#[derive(Debug, Default, PartialEq)]
struct Head {
    title: String,
    description: String,
    image: Option<String>,
    icon: Option<String>,
}

/// The preview of `host`'s home page (blocking; runs on its own thread).
pub fn fetch(host: &str) -> Result<SitePreview, String> {
    if !crate::favicon::plain_host(host) {
        return Err(format!("{host:?} is not a website name"));
    }
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .max_redirects(3)
        .https_only(true)
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(crate::updates::os_roots()?).build())
        .build()
        .new_agent();
    // `whole`: a picture must arrive complete (a larger one is refused); a page is read up to
    // `limit` only, as its head comes first.
    let get = |url: &str, limit: u64, whole: bool| -> Result<(Vec<u8>, String), String> {
        let mut response =
            agent.get(url).header("User-Agent", concat!("NadsPDF/", env!("CARGO_PKG_VERSION"))).call().map_err(|e| format!("{host}: {e}"))?;
        let kind = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_ascii_lowercase();
        let body = if whole {
            response.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| format!("{host}: {e}"))?
        } else {
            let mut out = Vec::new();
            std::io::Read::read_to_end(&mut std::io::Read::take(response.body_mut().as_reader(), limit), &mut out)
                .map_err(|e| format!("{host}: {e}"))?;
            out
        };
        Ok((body, kind))
    };
    let (page, kind) = get(&format!("https://{host}/"), MAX_PAGE, false)?;
    if !kind.is_empty() && !kind.contains("html") {
        return Err(format!("{host}: the home page is not HTML"));
    }
    let head = parse(&String::from_utf8_lossy(&page), host);
    // The image and the icon are extras: the preview stands without them.
    let image = head.image.as_deref().and_then(|u| get(u, MAX_IMAGE, true).ok()).map(|(b, _)| b);
    let icon = head.icon.as_deref().and_then(|u| get(u, MAX_ICON, true).ok()).map(|(b, _)| b);
    Ok(SitePreview { title: head.title, description: head.description, image, icon })
}

/// Read the head of `html`, served by `host`: its title, description, preview image and icon,
/// with relative addresses made absolute and anything that isn't plain HTTPS to a plain host
/// left out.
fn parse(html: &str, host: &str) -> Head {
    // Everything wanted is in the head; a page without `</head>` is read to the size cap.
    let lower = html.to_ascii_lowercase();
    let end = lower.find("</head>").unwrap_or(html.len());
    let (html, lower) = (html.get(..end).unwrap_or(html), lower.get(..end).unwrap_or(&lower));
    let mut meta: Vec<(String, String)> = Vec::new();
    let mut icons: Vec<(u8, String)> = Vec::new();
    let mut title_tag = String::new();
    let mut at = 0usize;
    while let Some(found) = lower.get(at..).and_then(|s| s.find('<')) {
        let start = at.saturating_add(found);
        let Some(close) = lower.get(start..).and_then(|s| s.find('>')) else { break };
        let close = start.saturating_add(close);
        let (Some(tag), Some(tag_lower)) = (html.get(start + 1..close), lower.get(start + 1..close)) else { break };
        at = close.saturating_add(1);
        let name = tag_lower.split(|c: char| c.is_ascii_whitespace() || c == '/').next().unwrap_or_default();
        match name {
            "meta" => {
                let a = attributes(tag);
                let key = a.iter().find(|(k, _)| k == "property" || k == "name").map(|(_, v)| v.to_ascii_lowercase());
                let content = a.iter().find(|(k, _)| k == "content").map(|(_, v)| v.clone());
                if let (Some(key), Some(content)) = (key, content) {
                    meta.push((key, content));
                }
            }
            "link" => {
                let a = attributes(tag);
                let rel = a.iter().find(|(k, _)| k == "rel").map(|(_, v)| v.to_ascii_lowercase()).unwrap_or_default();
                let href = a.iter().find(|(k, _)| k == "href").map(|(_, v)| v.clone());
                // A larger, sharper icon first.
                let rank = if rel.contains("apple-touch-icon") {
                    0
                } else if rel.split_ascii_whitespace().any(|r| r == "icon") {
                    1
                } else {
                    continue;
                };
                if let Some(href) = href {
                    icons.push((rank, href));
                }
            }
            "title" if title_tag.is_empty() => {
                let text_end = lower.get(at..).and_then(|s| s.find("</title")).map_or(at, |e| at.saturating_add(e));
                title_tag = html.get(at..text_end).unwrap_or_default().to_string();
            }
            _ => {}
        }
    }
    let first = |keys: &[&str]| keys.iter().find_map(|k| meta.iter().find(|(m, v)| m == k && !v.trim().is_empty()).map(|(_, v)| v.clone()));
    let title = first(&["og:title", "twitter:title"]).unwrap_or(title_tag);
    let description = first(&["og:description", "description", "twitter:description"]).unwrap_or_default();
    let image = first(&["og:image:secure_url", "og:image", "twitter:image"]).and_then(|u| absolute(&decode_entities(&u), host));
    icons.sort_by_key(|(rank, _)| *rank);
    let icon = icons.iter().find_map(|(_, href)| absolute(&decode_entities(href), host));
    Head { title: clean(&title, MAX_TITLE), description: clean(&description, MAX_DESCRIPTION), image, icon }
}

/// A tag's attributes, names lowercase, values with their entities decoded.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = tag.char_indices().peekable();
    // Skip the tag name.
    while chars.peek().is_some_and(|(_, c)| !c.is_ascii_whitespace()) {
        chars.next();
    }
    while out.len() < 64 {
        while chars.peek().is_some_and(|(_, c)| c.is_ascii_whitespace() || *c == '/') {
            chars.next();
        }
        let mut name = String::new();
        while let Some(&(_, c)) = chars.peek() {
            if c.is_ascii_whitespace() || c == '=' || c == '/' {
                break;
            }
            name.push(c.to_ascii_lowercase());
            chars.next();
        }
        if name.is_empty() {
            break;
        }
        while chars.peek().is_some_and(|(_, c)| c.is_ascii_whitespace()) {
            chars.next();
        }
        let mut value = String::new();
        if chars.peek().is_some_and(|(_, c)| *c == '=') {
            chars.next();
            while chars.peek().is_some_and(|(_, c)| c.is_ascii_whitespace()) {
                chars.next();
            }
            match chars.peek().map(|(_, c)| *c) {
                Some(q @ ('"' | '\'')) => {
                    chars.next();
                    for (_, c) in chars.by_ref() {
                        if c == q {
                            break;
                        }
                        value.push(c);
                    }
                }
                _ => {
                    while let Some(&(_, c)) = chars.peek() {
                        if c.is_ascii_whitespace() {
                            break;
                        }
                        value.push(c);
                        chars.next();
                    }
                }
            }
        }
        out.push((name, decode_entities(&value)));
    }
    out
}

/// `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&#39;`, `&nbsp;` and numeric references; anything
/// else is left as written.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(rest.get(..amp).unwrap_or_default());
        let after = rest.get(amp..).unwrap_or_default();
        let Some(semi) = after.get(..after.len().min(12)).and_then(|s| s.find(';')) else {
            out.push('&');
            rest = after.get(1..).unwrap_or_default();
            continue;
        };
        let entity = after.get(1..semi).unwrap_or_default();
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32)
                .filter(|c| !c.is_control()),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = after.get(semi + 1..).unwrap_or_default();
            }
            None => {
                out.push('&');
                rest = after.get(1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    out
}

/// One line of text: entities decoded, whitespace runs collapsed, at most `max` characters.
fn clean(text: &str, max: usize) -> String {
    let text = decode_entities(text);
    let words: Vec<&str> = text.split_whitespace().collect();
    let line = words.join(" ");
    if line.chars().count() > max { format!("{}…", line.chars().take(max.saturating_sub(1)).collect::<String>()) } else { line }
}

/// `href` from a page of `host` as an absolute `https://` address to a plain host name, or `None`
/// (plain HTTP, data:, javascript:, an IP address…).
fn absolute(href: &str, host: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() || href.len() > 2048 || href.chars().any(char::is_control) {
        return None;
    }
    let url = if let Some(rest) = href.strip_prefix("//") {
        format!("https://{rest}")
    } else if href.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://")) {
        format!("https://{}", href.get(8..)?)
    } else if href.contains(':') && !href.starts_with('/') && href.find(':') < href.find('/').or(Some(usize::MAX)) {
        // Another scheme (http:, data:, javascript:…).
        return None;
    } else if href.starts_with('/') {
        format!("https://{host}{href}")
    } else {
        format!("https://{host}/{href}")
    };
    let rest = url.get(8..)?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let target = authority.to_ascii_lowercase();
    // A port is fine; user info is not, and the name must be a plain one.
    if target.contains('@') {
        return None;
    }
    let name = target.split(':').next()?;
    crate::favicon::plain_host(name).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_head_gives_the_title_description_image_and_best_icon() {
        let html = r#"<!doctype html><html><head>
            <title>  Fallback   title </title>
            <meta property="og:title" content="Nads &amp; Co">
            <meta name='description' content="Outils PDF &#8211; libres">
            <meta property=og:image content=/img/card.png>
            <link rel="icon" href="/favicon-32.png">
            <link rel="apple-touch-icon" href="https://cdn.nads.site/touch.png">
            </head><body><meta property="og:title" content="not in the head"></body>"#;
        let head = parse(html, "nads.site");
        assert_eq!(
            head,
            Head {
                title: "Nads & Co".into(),
                description: "Outils PDF – libres".into(),
                image: Some("https://nads.site/img/card.png".into()),
                icon: Some("https://cdn.nads.site/touch.png".into()),
            }
        );
        let plain = parse("<head><TITLE>Only\n a title</TITLE>", "a.com");
        assert_eq!((plain.title.as_str(), plain.image, plain.icon), ("Only a title", None, None));
    }

    #[test]
    fn only_https_addresses_to_plain_hosts_are_followed() {
        assert_eq!(absolute("img.png", "a.com").as_deref(), Some("https://a.com/img.png"));
        assert_eq!(absolute("//cdn.a.com/x.png", "a.com").as_deref(), Some("https://cdn.a.com/x.png"));
        assert_eq!(absolute("HTTPS://b.org:8443/i.jpg", "a.com").as_deref(), Some("https://b.org:8443/i.jpg"));
        for bad in [
            "http://a.com/x.png",
            "data:image/png;base64,AA",
            "javascript:alert(1)",
            "https://192.168.1.1/x.png",
            "https://user@a.com/x",
            "https://localhost/x",
            "",
        ] {
            assert_eq!(absolute(bad, "a.com"), None, "{bad}");
        }
    }

    #[test]
    fn odd_markup_is_read_without_trouble() {
        for html in [
            "",
            "<",
            "<meta",
            "<meta content=>",
            "<title>",
            "&",
            "&#xFFFFFFFF;",
            "<link rel=icon href>",
            "<meta name=\"description\" content=\"é&#0;\">",
        ] {
            let _ = parse(html, "a.com");
        }
        assert_eq!(decode_entities("a &unknown; &#65;&#x42; &"), "a &unknown; AB &");
        assert_eq!(clean(&"x".repeat(500), 10).chars().count(), 10);
    }

    #[test]
    fn hosts_are_checked_before_any_request() {
        for bad in ["", "localhost", "10.0.0.1", "evil.com/x"] {
            assert!(fetch(bad).is_err(), "{bad}");
        }
    }
}
