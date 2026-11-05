//! A website's icon for the link bubble (`pdfcraft_ui_egui::link_bubble`), asked for only when
//! the user turned on Preferences ▸ Show websites' icons.
//!
//! One HTTPS request to `https://<host>/favicon.ico`: no cookies, a short timeout, at most three
//! redirects and 256 KiB. `host` comes from a document, so it is checked again here.

use std::time::Duration;

/// The most bytes read from the answer.
const MAX_BYTES: u64 = 256 * 1024;

/// Whether `host` is a plain DNS name (letters, digits, `-`, `.`), so nothing else can be put
/// into the request's address.
fn plain_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.contains('.')
        && host.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        && !host.starts_with(['.', '-'])
}

/// The icon `https://<host>/favicon.ico` answers with (blocking).
pub fn fetch(host: &str) -> Result<Vec<u8>, String> {
    if !plain_host(host) {
        return Err(format!("{host:?} is not a website name"));
    }
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(6)))
        .max_redirects(3)
        .https_only(true)
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(crate::updates::os_roots()?).build())
        .build()
        .new_agent();
    let mut response = agent
        .get(&format!("https://{host}/favicon.ico"))
        .header("User-Agent", concat!("NadsPDF/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| format!("{host}: {e}"))?;
    response.body_mut().with_config().limit(MAX_BYTES).read_to_vec().map_err(|e| format!("{host}: {e}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_plain_host_names_are_asked() {
        assert!(super::plain_host("nads.site"));
        for bad in ["", "localhost", "evil.com/x", "a.com:80", "a.com@b.com", "-a.com", "A.COM", "a .com"] {
            assert!(!super::plain_host(bad), "{bad}");
            assert!(super::fetch(bad).is_err(), "{bad} is refused before any request");
        }
    }
}
