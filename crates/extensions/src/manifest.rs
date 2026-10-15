//! `manifest.json`: who the extension is, which program to start and what it may ask for.
//!
//! Every field is checked before anything runs: an unknown field or permission is refused (a
//! typo must not silently drop a restriction), the program must stay inside the extension's
//! folder, and the web origins the extension may open are exact `scheme://host[:port]` values.

use serde::{Deserialize, Serialize};

/// The manifest's file name inside an extension folder.
pub const MANIFEST_FILE: &str = "manifest.json";
/// A manifest is a few hundred bytes; anything this large is not one.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// The protocol version this host speaks (`protocol` in the manifest must match).
pub const PROTOCOL_VERSION: u32 = 1;

const MAX_ID: usize = 64;
const MAX_NAME: usize = 80;
const MAX_VERSION: usize = 32;
const MAX_DESCRIPTION: usize = 400;
const MAX_ORIGINS: usize = 16;

/// What an extension may receive or ask for. The user sees this list before approving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Be told when a document opens or closes (its name, path and page count).
    DocumentEvents,
    /// Read the text of an open document (`doc.text`).
    ReadText,
    /// Know where an open document is saved (`doc.file`).
    ReadFile,
    /// Show a message bar with buttons above a document (`ui.banner.show`).
    Banner,
    /// Open a web page in the browser, only on the origins listed in `open_url`.
    OpenUrl,
}

impl Permission {
    pub const ALL: [Permission; 5] =
        [Permission::DocumentEvents, Permission::ReadText, Permission::ReadFile, Permission::Banner, Permission::OpenUrl];

    /// The manifest spelling (`read_text`).
    pub fn key(self) -> &'static str {
        match self {
            Permission::DocumentEvents => "document_events",
            Permission::ReadText => "read_text",
            Permission::ReadFile => "read_file",
            Permission::Banner => "banner",
            Permission::OpenUrl => "open_url",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Protocol version the extension speaks; must be [`PROTOCOL_VERSION`].
    pub protocol: u32,
    /// Stable identifier: lowercase letters, digits, `.`, `-` and `_` (`com.example.erp`).
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// The program to start, relative to the extension folder (`bin/ext.exe`).
    pub executable: String,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// Origins `shell.open_url` may open (`https://erp.example.com`). Required with `open_url`.
    #[serde(default)]
    pub open_url: Vec<String>,
}

impl Manifest {
    /// Parse and check a manifest. Permissions come back sorted and without duplicates.
    pub fn parse(bytes: &[u8]) -> Result<Manifest, String> {
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(format!("the manifest is larger than {MAX_MANIFEST_BYTES} bytes"));
        }
        let mut m: Manifest = serde_json::from_slice(bytes).map_err(|e| format!("invalid manifest: {e}"))?;
        m.permissions.sort();
        m.permissions.dedup();
        for origin in &mut m.open_url {
            *origin = origin.trim_end_matches('/').to_ascii_lowercase();
        }
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), String> {
        if self.protocol != PROTOCOL_VERSION {
            return Err(format!("protocol {} is not supported (this NadsPDF speaks {PROTOCOL_VERSION})", self.protocol));
        }
        let id_ok = !self.id.is_empty()
            && self.id.len() <= MAX_ID
            && self.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_'))
            && !self.id.starts_with('.');
        if !id_ok {
            return Err(format!("id must be 1–{MAX_ID} lowercase letters, digits, '.', '-' or '_', not starting with '.'"));
        }
        check_text("name", &self.name, 1, MAX_NAME)?;
        check_text("version", &self.version, 1, MAX_VERSION)?;
        check_text("description", &self.description, 0, MAX_DESCRIPTION)?;
        check_executable(&self.executable)?;
        let wants_urls = self.permissions.contains(&Permission::OpenUrl);
        if wants_urls && self.open_url.is_empty() {
            return Err("the open_url permission needs at least one origin in \"open_url\"".into());
        }
        if !wants_urls && !self.open_url.is_empty() {
            return Err("\"open_url\" lists origins but the open_url permission is not requested".into());
        }
        if self.open_url.len() > MAX_ORIGINS {
            return Err(format!("at most {MAX_ORIGINS} origins in \"open_url\""));
        }
        for origin in &self.open_url {
            check_origin(origin)?;
        }
        Ok(())
    }

    pub fn allows(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }

    /// Whether `url` is on one of the manifest's origins: the scheme and authority must be
    /// exactly one of them (`https://erp.example.com.evil.net` and `https://erp.example.com@evil.net`
    /// are other origins).
    pub fn url_allowed(&self, url: &str) -> bool {
        if !self.allows(Permission::OpenUrl) {
            return false;
        }
        match origin_of(url) {
            Some(origin) => self.open_url.contains(&origin),
            None => false,
        }
    }
}

fn check_text(field: &str, value: &str, min: usize, max: usize) -> Result<(), String> {
    let n = value.trim().chars().count();
    if n < min || value.chars().count() > max || value.chars().any(char::is_control) {
        return Err(format!("{field} must be {min}–{max} characters without control characters"));
    }
    Ok(())
}

/// A relative path made of plain components: no root, drive, `..` or `.`.
fn check_executable(path: &str) -> Result<(), String> {
    let bad = path.is_empty()
        || path.len() > 255
        || path.starts_with(['/', '\\'])
        || path.contains(':')
        || path.chars().any(char::is_control)
        || path.split(['/', '\\']).any(|c| c.is_empty() || c == "." || c == "..");
    if bad {
        return Err("executable must be a relative path inside the extension folder (no '..', no drive or root)".into());
    }
    Ok(())
}

fn check_origin(origin: &str) -> Result<(), String> {
    let ok = origin_of(origin).is_some_and(|o| o == origin) && !origin.ends_with("://");
    let secure = origin.starts_with("https://") || is_loopback(origin);
    if !ok || !secure {
        return Err(format!("origin {origin:?} must be https://host[:port] (http only for localhost), without a path"));
    }
    Ok(())
}

fn is_loopback(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("http://") else { return false };
    let host = rest.split(':').next().unwrap_or(rest);
    matches!(host, "localhost" | "127.0.0.1")
}

/// `scheme://authority` of an http(s) URL, lowercased; `None` for anything else.
fn origin_of(url: &str) -> Option<String> {
    if url.len() > 4096 || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    let (scheme, rest) = lower.split_once("://")?;
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest.get(..end)?;
    if authority.is_empty() || authority.contains('@') || authority.contains('\\') {
        return None;
    }
    Some(format!("{scheme}://{authority}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(extra: &str) -> String {
        format!(r#"{{"protocol":1,"id":"com.example.erp","name":"ERP","version":"1.0.0","executable":"bin/ext.exe"{extra}}}"#)
    }

    #[test]
    fn parses_a_minimal_manifest() {
        let m = Manifest::parse(manifest("").as_bytes()).unwrap();
        assert_eq!(m.id, "com.example.erp");
        assert!(m.permissions.is_empty());
    }

    #[test]
    fn sorts_and_dedups_permissions() {
        let m = Manifest::parse(manifest(r#","permissions":["banner","read_text","banner"]"#).as_bytes()).unwrap();
        assert_eq!(m.permissions, vec![Permission::ReadText, Permission::Banner]);
    }

    #[test]
    fn refuses_unknown_fields_and_permissions() {
        assert!(Manifest::parse(manifest(r#","permisions":["banner"]"#).as_bytes()).is_err());
        assert!(Manifest::parse(manifest(r#","permissions":["network"]"#).as_bytes()).is_err());
    }

    #[test]
    fn refuses_another_protocol() {
        let text = manifest("").replace(r#""protocol":1"#, r#""protocol":2"#);
        assert!(Manifest::parse(text.as_bytes()).unwrap_err().contains("protocol 2"));
    }

    #[test]
    fn refuses_bad_ids() {
        for id in ["", "Com.Example", "a b", ".hidden", "x/y", &"a".repeat(65)] {
            let text = manifest("").replace("com.example.erp", id);
            assert!(Manifest::parse(text.as_bytes()).is_err(), "{id:?}");
        }
    }

    #[test]
    fn executable_stays_inside_the_folder() {
        for exe in ["../evil.exe", "bin/../../x", "/usr/bin/sh", "\\\\host\\share\\x.exe", "C:\\x.exe", "", "./x", "bin//x"] {
            let text = manifest("").replace("bin/ext.exe", &exe.replace('\\', "\\\\"));
            assert!(Manifest::parse(text.as_bytes()).is_err(), "{exe:?}");
        }
        let nested = manifest("").replace("bin/ext.exe", "bin\\\\ext.exe");
        assert!(Manifest::parse(nested.as_bytes()).is_ok());
    }

    #[test]
    fn open_url_needs_origins_and_the_permission() {
        assert!(Manifest::parse(manifest(r#","permissions":["open_url"]"#).as_bytes()).is_err());
        assert!(Manifest::parse(manifest(r#","open_url":["https://erp.example.com"]"#).as_bytes()).is_err());
    }

    #[test]
    fn origins_are_exact() {
        for bad in ["http://erp.example.com", "https://erp.example.com/path", "ftp://x", "https://", "https://a@b", "javascript:alert(1)"] {
            let text = manifest(&format!(r#","permissions":["open_url"],"open_url":["{bad}"]"#));
            assert!(Manifest::parse(text.as_bytes()).is_err(), "{bad:?}");
        }
        for good in ["https://erp.example.com", "https://ERP.example.com/", "http://localhost:3000", "https://erp.example.com:8443"] {
            let text = manifest(&format!(r#","permissions":["open_url"],"open_url":["{good}"]"#));
            assert!(Manifest::parse(text.as_bytes()).is_ok(), "{good:?}");
        }
    }

    #[test]
    fn url_allowed_matches_scheme_and_authority_only() {
        let m = Manifest::parse(manifest(r#","permissions":["open_url"],"open_url":["https://erp.example.com"]"#).as_bytes()).unwrap();
        assert!(m.url_allowed("https://erp.example.com/interventions/42"));
        assert!(m.url_allowed("https://ERP.example.com?x=1"));
        assert!(m.url_allowed("https://erp.example.com"));
        for bad in [
            "https://erp.example.com.evil.net/",
            "https://erp.example.com@evil.net/",
            "http://erp.example.com/",
            "https://erp.example.com:444/",
            "https://evil.net/https://erp.example.com",
            "https://erp.example.com/ x",
            "file:///C:/x",
        ] {
            assert!(!m.url_allowed(bad), "{bad:?}");
        }
    }

    #[test]
    fn refuses_oversized_manifests() {
        let big = manifest(&format!(r#","description":"{}""#, "x".repeat(70_000)));
        assert!(Manifest::parse(big.as_bytes()).unwrap_err().contains("larger"));
    }
}
