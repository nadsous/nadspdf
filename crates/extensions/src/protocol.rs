//! The messages, one JSON-RPC 2.0 object per line.
//!
//! NadsPDF sends *notifications* (events, no `id`) and *responses*; the extension sends
//! *requests* (with an `id`). Every line from an extension is untrusted: [`parse_line`] never
//! fails, it turns anything it can't use into [`Incoming::Invalid`] so the host can answer with
//! an error and keep going.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::manifest::Permission;

/// A line longer than this is refused (and skipped up to its newline).
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
/// `doc.text` reads at most this many pages per request.
pub const MAX_TEXT_PAGES: usize = 50;
/// Pages read when `doc.text` names none.
pub const DEFAULT_TEXT_PAGES: usize = 10;
/// Message bars an extension may show on one document at a time.
pub const MAX_BANNERS_PER_DOC: usize = 2;

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
/// The manifest doesn't ask for the permission the method needs.
pub const PERMISSION_DENIED: i64 = -32001;
/// The request was valid but can't be done now (no such document, no saved file…).
pub const FAILED: i64 = -32002;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    DocText,
    DocFile,
    BannerShow,
    BannerHide,
    OpenUrl,
}

impl Method {
    pub fn from_name(name: &str) -> Option<Method> {
        Some(match name {
            "doc.text" => Method::DocText,
            "doc.file" => Method::DocFile,
            "ui.banner.show" => Method::BannerShow,
            "ui.banner.hide" => Method::BannerHide,
            "shell.open_url" => Method::OpenUrl,
            _ => return None,
        })
    }

    pub fn permission(self) -> Permission {
        match self {
            Method::DocText => Permission::ReadText,
            Method::DocFile => Permission::ReadFile,
            Method::BannerShow | Method::BannerHide => Permission::Banner,
            Method::OpenUrl => Permission::OpenUrl,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Request {
        id: Value,
        method: Method,
        params: Value,
    },
    /// Unusable: answer `error` to `id` when there is one (a notification gets no answer).
    Invalid {
        id: Option<Value>,
        code: i64,
        message: String,
    },
}

pub fn parse_line(line: &str) -> Incoming {
    let invalid = |id: Option<Value>, code, message: String| Incoming::Invalid { id, code, message };
    let value: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return invalid(None, PARSE_ERROR, format!("not JSON: {e}")),
    };
    let Some(obj) = value.as_object() else {
        return invalid(None, INVALID_REQUEST, "a message is a JSON object".into());
    };
    let id = obj.get("id").cloned().filter(|id| id.is_number() || id.is_string());
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return invalid(id, INVALID_REQUEST, "\"jsonrpc\" must be \"2.0\"".into());
    }
    let Some(name) = obj.get("method").and_then(Value::as_str) else {
        return invalid(id, INVALID_REQUEST, "a request needs a \"method\"".into());
    };
    let Some(id) = id else {
        return invalid(None, INVALID_REQUEST, "a request needs a number or string \"id\"".into());
    };
    let Some(method) = Method::from_name(name) else {
        return invalid(Some(id), METHOD_NOT_FOUND, format!("unknown method {name:?}"));
    };
    let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));
    Incoming::Request { id, method, params }
}

/// A notification line (no trailing newline).
pub fn event(method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string()
}

pub fn result(id: &Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

pub fn error(id: &Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

/// Decode a request's params, with an error message the extension can act on.
pub fn params<T: for<'de> Deserialize<'de>>(params: &Value) -> Result<T, String> {
    serde_json::from_value(params.clone()).map_err(|e| format!("invalid params: {e}"))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocParams {
    pub doc: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextParams {
    pub doc: u64,
    /// 1-based page numbers; the first [`DEFAULT_TEXT_PAGES`] pages when absent.
    #[serde(default)]
    pub pages: Option<Vec<u64>>,
}

impl TextParams {
    /// 0-based pages to read, given the document's page count.
    pub fn pages(&self, count: usize) -> Result<Vec<usize>, String> {
        let Some(asked) = &self.pages else {
            return Ok((0..count.min(DEFAULT_TEXT_PAGES)).collect());
        };
        if asked.len() > MAX_TEXT_PAGES {
            return Err(format!("at most {MAX_TEXT_PAGES} pages per request"));
        }
        let mut out = Vec::with_capacity(asked.len());
        for &p in asked {
            let index = usize::try_from(p).ok().and_then(|p| p.checked_sub(1)).filter(|&i| i < count);
            match index {
                Some(i) if !out.contains(&i) => out.push(i),
                Some(_) => {}
                None => return Err(format!("page {p} is not in the document (1–{count})")),
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerAction {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerShow {
    pub doc: u64,
    /// The extension's own name for the banner: showing the same id again replaces it.
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub actions: Vec<BannerAction>,
}

impl BannerShow {
    pub fn validate(&self) -> Result<(), String> {
        check_token("id", &self.id)?;
        check_label("text", &self.text, 300)?;
        if self.actions.len() > 3 {
            return Err("at most 3 actions".into());
        }
        for (k, a) in self.actions.iter().enumerate() {
            check_token("action id", &a.id)?;
            check_label("action label", &a.label, 40)?;
            if self.actions.iter().take(k).any(|b| b.id == a.id) {
                return Err(format!("action id {:?} is used twice", a.id));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BannerHide {
    pub doc: u64,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UrlParams {
    pub url: String,
}

fn check_token(field: &str, value: &str) -> Result<(), String> {
    let ok = !value.is_empty() && value.len() <= 64 && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    if !ok {
        return Err(format!("{field} must be 1–64 letters, digits, '.', '-' or '_'"));
    }
    Ok(())
}

fn check_label(field: &str, value: &str, max: usize) -> Result<(), String> {
    let n = value.chars().count();
    if value.trim().is_empty() || n > max || value.chars().any(char::is_control) {
        return Err(format!("{field} must be 1–{max} characters on one line"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_request() {
        let got = parse_line(r#"{"jsonrpc":"2.0","id":7,"method":"doc.text","params":{"doc":1}}"#);
        assert_eq!(got, Incoming::Request { id: json!(7), method: Method::DocText, params: json!({"doc":1}) });
    }

    #[test]
    fn bad_lines_become_errors_with_the_id_when_known() {
        assert!(matches!(parse_line("{nope"), Incoming::Invalid { id: None, code: PARSE_ERROR, .. }));
        assert!(matches!(parse_line("[1,2]"), Incoming::Invalid { id: None, code: INVALID_REQUEST, .. }));
        assert!(matches!(parse_line(r#"{"id":1,"method":"doc.text"}"#), Incoming::Invalid { id: Some(_), code: INVALID_REQUEST, .. }));
        assert!(matches!(
            parse_line(r#"{"jsonrpc":"2.0","id":"a","method":"fs.delete"}"#),
            Incoming::Invalid { id: Some(_), code: METHOD_NOT_FOUND, .. }
        ));
        assert!(matches!(parse_line(r#"{"jsonrpc":"2.0","method":"doc.text"}"#), Incoming::Invalid { id: None, .. }));
        assert!(matches!(parse_line(r#"{"jsonrpc":"2.0","id":{"x":1},"method":"doc.text"}"#), Incoming::Invalid { id: None, .. }));
    }

    #[test]
    fn writes_messages() {
        assert_eq!(event("document.closed", json!({"doc":2})), r#"{"jsonrpc":"2.0","method":"document.closed","params":{"doc":2}}"#);
        let r: Value = serde_json::from_str(&error(&json!(3), FAILED, "no")).unwrap();
        assert_eq!(r["error"]["code"], FAILED);
        assert_eq!(r["id"], 3);
    }

    #[test]
    fn text_pages_are_bounded_and_one_based() {
        let p = |pages: Option<Vec<u64>>| TextParams { doc: 1, pages };
        assert_eq!(p(None).pages(3).unwrap(), vec![0, 1, 2]);
        assert_eq!(p(None).pages(40).unwrap().len(), DEFAULT_TEXT_PAGES);
        assert_eq!(p(Some(vec![2, 2, 1])).pages(3).unwrap(), vec![1, 0]);
        assert!(p(Some(vec![0])).pages(3).is_err());
        assert!(p(Some(vec![4])).pages(3).is_err());
        assert!(p(Some(vec![u64::MAX])).pages(3).is_err());
        assert!(p(Some((1..=51).collect())).pages(100).is_err());
        assert!(p(None).pages(0).unwrap().is_empty());
    }

    #[test]
    fn banners_are_checked() {
        let banner = |id: &str, text: &str, actions: Vec<(&str, &str)>| BannerShow {
            doc: 1,
            id: id.into(),
            text: text.into(),
            actions: actions.into_iter().map(|(id, label)| BannerAction { id: id.into(), label: label.into(), primary: false }).collect(),
        };
        assert!(banner("di", "DI BE-2026-0042 trouvée", vec![("open", "Ouvrir la DI")]).validate().is_ok());
        assert!(banner("", "x", vec![]).validate().is_err());
        assert!(banner("a b", "x", vec![]).validate().is_err());
        assert!(banner("di", " ", vec![]).validate().is_err());
        assert!(banner("di", "two\nlines", vec![]).validate().is_err());
        assert!(banner("di", &"x".repeat(301), vec![]).validate().is_err());
        assert!(banner("di", "x", vec![("a", "A"), ("a", "B")]).validate().is_err());
        assert!(banner("di", "x", vec![("a", "A"), ("b", "B"), ("c", "C"), ("d", "D")]).validate().is_err());
    }

    #[test]
    fn params_refuse_unknown_keys() {
        assert!(params::<DocParams>(&json!({"doc": 1, "path": "C:/x"})).is_err());
        assert_eq!(params::<DocParams>(&json!({"doc": 1})).unwrap(), DocParams { doc: 1 });
    }
}
