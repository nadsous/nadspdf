//! One HTTPS request to the provider, with the user's key (desktop only, feature `net`).
//!
//! The key goes only to the provider's own address ([`Provider::endpoint`]), over TLS checked
//! against the system's trusted certificates. Answers are capped in size.

use std::time::Duration;

use crate::provider::{body, error_message, parse_reply};
use crate::{Provider, Reply, Request};

/// The longest an answer may take, and the most bytes read from it.
const TIMEOUT: Duration = Duration::from_secs(180);
const MAX_ANSWER: u64 = 8 * 1024 * 1024;

fn os_roots() -> Result<ureq::tls::RootCerts, String> {
    let found = rustls_native_certs::load_native_certs();
    let certs: Vec<ureq::tls::Certificate<'static>> = found.certs.iter().map(|c| ureq::tls::Certificate::from_der(c.as_ref()).to_owned()).collect();
    if certs.is_empty() {
        return Err("no trusted certificates found on this system".into());
    }
    Ok(ureq::tls::RootCerts::new_with_certs(&certs))
}

/// Send `request` to `provider` with `model` and `key` (blocking).
pub fn send(provider: Provider, model: &str, key: &str, request: &Request) -> Result<Reply, String> {
    let key = key.trim();
    if key.is_empty() || key.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("the API key is missing or malformed: paste it again in Preferences ▸ Assistant.".into());
    }
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .https_only(true)
        .max_redirects(0)
        .http_status_as_error(false)
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(os_roots()?).build())
        .build()
        .new_agent();
    let payload = body(provider, model, request).to_string();
    let call = agent
        .post(provider.endpoint())
        .header("Content-Type", "application/json")
        .header("User-Agent", concat!("NadsPDF/", env!("CARGO_PKG_VERSION")));
    let call = match provider {
        Provider::Anthropic => call.header("x-api-key", key).header("anthropic-version", "2023-06-01"),
        Provider::OpenRouter => call
            .header("Authorization", &format!("Bearer {key}"))
            // OpenRouter shows which app made a request on the user's own activity page.
            .header("HTTP-Referer", "https://github.com/nadsous/nadspdf")
            .header("X-Title", "NadsPDF"),
        Provider::Mistral => call.header("Authorization", &format!("Bearer {key}")),
    };
    let mut response = call.send(payload.as_bytes()).map_err(|e| format!("{} couldn't be reached ({e})", provider.label()))?;
    let status = response.status().as_u16();
    let text = response.body_mut().with_config().limit(MAX_ANSWER).read_to_string().map_err(|e| format!("the answer couldn't be read ({e})"))?;
    if !(200..300).contains(&status) {
        return Err(error_message(provider, status, &text));
    }
    parse_reply(provider, &text)
}
