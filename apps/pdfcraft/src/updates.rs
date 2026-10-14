//! Asks GitHub for the latest NadsPDF release (nadsous/nadspdf) (Help ▸ Check for updates, issue #28)
//! and, on Windows, installs it: the release's MSI is downloaded, checked against the SHA-256
//! GitHub publishes for it, and run once NadsPDF has quit. The MSI replaces the installed version
//! in place (MajorUpgrade) and keeps the settings.

use std::time::Duration;

use pdfcraft_ui_egui::updates::{Installer, RELEASES_PAGE, Release};

const LATEST: &str = "https://api.github.com/repos/nadsous/nadspdf/releases/latest";

/// Where a release's files are downloaded from (GitHub then redirects to its file storage).
const DOWNLOADS: &str = "https://github.com/nadsous/nadspdf/releases/download/";

/// The largest installer NadsPDF downloads.
#[cfg(target_os = "windows")]
const MAX_INSTALLER: u64 = 512 << 20;

/// The MSI file name ending for this computer, such as `-windows-x64.msi`.
const MSI_SUFFIX: &str = if cfg!(target_arch = "aarch64") {
    "-windows-arm64.msi"
} else if cfg!(target_arch = "x86") {
    "-windows-x86.msi"
} else {
    "-windows-x64.msi"
};

/// The latest release. The answer is untrusted: its size is capped, and only a page under
/// [`RELEASES_PAGE`] is ever offered for download (anything else falls back to that list).
pub fn latest_release() -> Result<Release, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(os_roots()?).build())
        .build()
        .new_agent();
    let mut response = agent
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("NadsPDF/", env!("CARGO_PKG_VERSION")))
        // Read error statuses ourselves: a 403 from GitHub usually means its anonymous
        // rate limit ran out, and the retry time rides along in a response header.
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|e| format!("couldn't reach GitHub ({e})"))?;
    let status = response.status().as_u16();
    if status == 403 {
        let reset = response.headers().get("x-ratelimit-reset").and_then(|v| v.to_str().ok());
        return Err(rate_limited(reset, now_unix()));
    }
    if !(200..300).contains(&status) {
        return Err(format!("couldn't reach GitHub (http status: {status})"));
    }
    let body = response.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| format!("unreadable answer ({e})"))?;
    parse(&body)
}

/// The certificate authorities the operating system trusts.
fn os_roots() -> Result<ureq::tls::RootCerts, String> {
    let found = rustls_native_certs::load_native_certs();
    let certs: Vec<ureq::tls::Certificate<'static>> = found.certs.iter().map(|c| ureq::tls::Certificate::from_der(c.as_ref()).to_owned()).collect();
    if certs.is_empty() {
        return Err("no trusted certificates found on this system".into());
    }
    Ok(ureq::tls::RootCerts::new_with_certs(&certs))
}

/// The current Unix time in seconds (0 when the clock gives nothing usable).
fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// What to tell the user when GitHub answers 403: its anonymous quota (60 checks an hour,
/// shared by everyone behind the same address) ran out. `reset` is the `x-ratelimit-reset`
/// header, the Unix time the quota refills; `now` is the current Unix time. Anything
/// surprising (no header, not a number, already past) falls back to asking for patience.
fn rate_limited(reset: Option<&str>, now: u64) -> String {
    const FALLBACK: &str = "GitHub is limiting update checks for now (shared hourly quota); try again later";
    let wait = reset.and_then(|r| r.trim().parse::<u64>().ok()).and_then(|reset| reset.checked_sub(now)).filter(|&wait| wait > 0);
    match wait {
        Some(wait) => {
            let minutes = wait.div_ceil(60).max(1);
            let plural = if minutes == 1 { "" } else { "s" };
            format!("GitHub is limiting update checks for now (shared hourly quota); try again in about {minutes} minute{plural}")
        }
        None => FALLBACK.into(),
    }
}

fn parse(body: &str) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("unreadable answer ({e})"))?;
    let version = v["tag_name"].as_str().filter(|t| !t.is_empty() && t.len() <= 64).ok_or("no release found")?.to_string();
    let url = v["html_url"]
        .as_str()
        .filter(|u| u.strip_prefix(RELEASES_PAGE).is_some_and(|rest| rest.starts_with('/') && !rest.contains(['?', '#', '\\'])))
        .unwrap_or(RELEASES_PAGE)
        .to_string();
    let installer = v["assets"].as_array().and_then(|assets| assets.iter().find_map(installer));
    Ok(Release { version, url, installer })
}

/// A release asset NadsPDF can install: this computer's MSI, downloaded from this project's
/// releases, with the digest GitHub computed for it. Anything else is left to the release page.
fn installer(asset: &serde_json::Value) -> Option<Installer> {
    let name = asset["name"].as_str()?;
    let safe = name.len() <= 128 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if !safe || name.contains("..") || !name.starts_with("nadspdf-") || !name.ends_with(MSI_SUFFIX) {
        return None;
    }
    let url = asset["browser_download_url"].as_str()?;
    let rest = url.strip_prefix(DOWNLOADS)?;
    if rest.contains(['?', '#', '\\']) || rest.contains("..") || !rest.ends_with(&format!("/{name}")) {
        return None;
    }
    let sha256 = asset["digest"].as_str()?.strip_prefix("sha256:")?.to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(Installer { name: name.to_string(), url: url.to_string(), sha256 })
}

/// Whether this copy can install updates itself: Windows, installed by the MSI. A portable copy
/// (portable.txt beside the exe) and a Microsoft Store copy are left alone.
pub fn can_install() -> bool {
    cfg!(target_os = "windows")
        && std::env::current_exe().is_ok_and(|exe| !from_store(&exe) && exe.parent().is_some_and(|dir| !dir.join("portable.txt").exists()))
}

/// Whether this copy runs from the Microsoft Store's package folder.
pub fn running_from_store() -> bool {
    std::env::current_exe().is_ok_and(|exe| from_store(&exe))
}

fn from_store(exe: &std::path::Path) -> bool {
    exe.components().any(|c| c.as_os_str().eq_ignore_ascii_case("WindowsApps"))
}

/// Download `installer`, check its digest and start it so that it runs once NadsPDF has quit,
/// then opens the new version.
pub fn install(installer: &Installer) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let dir = std::env::temp_dir().join("NadsPDF-update");
        std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {} ({e})", dir.display()))?;
        let msi = dir.join(&installer.name);
        download(installer, &msi)?;
        run_after_quit(&msi)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = installer;
        Err("installing updates is only supported on Windows".into())
    }
}

#[cfg(target_os = "windows")]
fn download(installer: &Installer, to: &std::path::Path) -> Result<(), String> {
    use sha2::Digest;
    use std::io::{Read, Write};

    let agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(20 * 60)))
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(os_roots()?).build())
        .build()
        .new_agent();
    let mut response = agent
        .get(&installer.url)
        .header("User-Agent", concat!("NadsPDF/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| format!("couldn't download {} ({e})", installer.name))?;
    let mut body = response.body_mut().with_config().limit(MAX_INSTALLER).reader();
    let partial = to.with_extension("part");
    let mut file = std::fs::File::create(&partial).map_err(|e| format!("couldn't write {} ({e})", partial.display()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = body.read(&mut buf).map_err(|e| format!("the download of {} stopped ({e})", installer.name))?;
        let chunk = buf.get(..n).unwrap_or_default();
        if chunk.is_empty() {
            break;
        }
        hasher.update(chunk);
        file.write_all(chunk).map_err(|e| format!("couldn't write {} ({e})", partial.display()))?;
    }
    file.sync_all().map_err(|e| format!("couldn't write {} ({e})", partial.display()))?;
    drop(file);
    let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != installer.sha256 {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("{} doesn't match the digest GitHub published for it; nothing was installed", installer.name));
    }
    let _ = std::fs::remove_file(to);
    std::fs::rename(&partial, to).map_err(|e| format!("couldn't write {} ({e})", to.display()))
}

/// Start a hidden PowerShell that waits for this process to end, runs the MSI with a progress
/// bar only (`/passive`; Windows asks for administrator rights) and opens NadsPDF again if it
/// installed. It outlives NadsPDF.
#[cfg(target_os = "windows")]
fn run_after_quit(msi: &std::path::Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = std::env::current_exe().map_err(|e| format!("couldn't find NadsPDF's own path ({e})"))?;
    let script = after_quit_script(std::process::id(), msi, &exe);
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(drop)
        .map_err(|e| format!("couldn't start the installer ({e})"))
}

/// The PowerShell run by [`run_after_quit`]. Paths go in single-quoted strings, where only `'`
/// is special (doubled); Windows paths can't contain `"`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn after_quit_script(pid: u32, msi: &std::path::Path, exe: &std::path::Path) -> String {
    let quote = |p: &std::path::Path| format!("'{}'", p.display().to_string().replace('\'', "''"));
    format!(
        "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; \
         $p = Start-Process -FilePath msiexec.exe -ArgumentList @('/i', ('\"' + {msi} + '\"'), '/passive', '/norestart') -Wait -PassThru; \
         if ($p.ExitCode -eq 0 -or $p.ExitCode -eq 3010) {{ Start-Process -FilePath {exe} }}",
        msi = quote(msi),
        exe = quote(exe),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_and_only_our_release_pages_are_offered() {
        let r = parse(r#"{"tag_name":"v0.2.0","html_url":"https://github.com/nadsous/nadspdf/releases/tag/v0.2.0"}"#).unwrap();
        assert_eq!(r, Release { version: "v0.2.0".into(), url: "https://github.com/nadsous/nadspdf/releases/tag/v0.2.0".into(), installer: None });
        for elsewhere in ["https://example.com/pdfcraft.exe", "https://github.com/nadsous/nadspdf/releases.evil/x", "javascript:alert(1)"] {
            let r = parse(&format!(r#"{{"tag_name":"v9.9.9","html_url":"{elsewhere}"}}"#)).unwrap();
            assert_eq!(r.url, RELEASES_PAGE, "{elsewhere}");
        }
        assert!(parse(r#"{"message":"Not Found"}"#).is_err());
        assert!(parse("<html>").is_err());
    }

    fn asset(name: &str, url: &str, digest: &str) -> serde_json::Value {
        serde_json::json!({ "name": name, "browser_download_url": url, "digest": digest })
    }

    #[test]
    fn only_this_computers_msi_from_our_releases_with_a_digest_is_installed() {
        let name = format!("nadspdf-0.8.0{MSI_SUFFIX}");
        let url = format!("{DOWNLOADS}v0.8.0/{name}");
        let digest = format!("sha256:{}", "AB".repeat(32));
        let found = installer(&asset(&name, &url, &digest)).unwrap();
        assert_eq!(found, Installer { name: name.clone(), url: url.clone(), sha256: "ab".repeat(32) });

        let body = serde_json::json!({
            "tag_name": "v0.8.0",
            "html_url": "https://github.com/nadsous/nadspdf/releases/tag/v0.8.0",
            "assets": [asset("nadspdf-0.8.0-chrome-extension.zip", &format!("{DOWNLOADS}v0.8.0/x.zip"), &digest), asset(&name, &url, &digest)],
        });
        assert_eq!(parse(&body.to_string()).unwrap().installer, Some(found));

        let other = if MSI_SUFFIX == "-windows-x64.msi" { "nadspdf-0.8.0-windows-arm64.msi" } else { "nadspdf-0.8.0-windows-x64.msi" };
        let other_url = format!("{DOWNLOADS}v0.8.0/{other}");
        let md5 = format!("md5:{}", "ab".repeat(32));
        let elsewhere = format!("https://evil.example/v0.8.0/{name}");
        let wrong_file = format!("{DOWNLOADS}v0.8.0/other.msi");
        let climbing = format!("{DOWNLOADS}../../x/{name}");
        let query = format!("{url}?x=1");
        let cases: [(&str, &str, &str); 10] = [
            (&name, &url, "sha256:abc"),
            (&name, &url, ""),
            (&name, &url, &md5),
            (&name, &elsewhere, &digest),
            (&name, &wrong_file, &digest),
            (&name, &climbing, &digest),
            (&name, &query, &digest),
            (other, &other_url, &digest),
            ("nadspdf-..\\..\\x-windows-x64.msi", &url, &digest),
            ("evil-0.8.0-windows-x64.msi", &url, &digest),
        ];
        for (n, u, d) in cases {
            assert_eq!(installer(&asset(n, u, d)), None, "{n} {u} {d}");
        }
        assert_eq!(installer(&serde_json::json!({ "name": 3 })), None);
    }

    #[test]
    fn the_installer_script_quotes_paths() {
        let script = after_quit_script(
            42,
            std::path::Path::new(r"C:\Users\O'Neil\AppData\Local\Temp\NadsPDF-update\a.msi"),
            std::path::Path::new(r"C:\Program Files\NadsPDF\nadspdf.exe"),
        );
        assert!(script.starts_with("Wait-Process -Id 42 "), "{script}");
        assert!(script.contains(r"'C:\Users\O''Neil\AppData\Local\Temp\NadsPDF-update\a.msi'"), "{script}");
        assert!(script.contains(r"Start-Process -FilePath 'C:\Program Files\NadsPDF\nadspdf.exe'"), "{script}");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn store_copies_are_recognised_by_their_folder() {
        assert!(from_store(std::path::Path::new(r"C:\Program Files\WindowsApps\nads.NadsPDF_0.7.1.0_x64__abc\nadspdf.exe")));
        assert!(!from_store(std::path::Path::new(r"C:\Program Files\NadsPDF\nadspdf.exe")));
    }

    #[test]
    fn rate_limit_messages_name_the_wait() {
        let limited = "GitHub is limiting update checks for now (shared hourly quota); try again in about 20 minutes";
        assert_eq!(rate_limited(Some("1800001200"), 1800000000), limited);
        // Rounds up: 61 seconds out reads as 2 minutes.
        assert_eq!(
            rate_limited(Some("1800000061"), 1800000000),
            "GitHub is limiting update checks for now (shared hourly quota); try again in about 2 minutes"
        );
        assert_eq!(
            rate_limited(Some("1800000060"), 1800000000),
            "GitHub is limiting update checks for now (shared hourly quota); try again in about 1 minute"
        );
        // No header, not a number, or already past: ask for patience instead of a time.
        let later = "GitHub is limiting update checks for now (shared hourly quota); try again later";
        for bad in [None, Some(""), Some("soon"), Some("1799999999"), Some("1800000000")] {
            assert_eq!(rate_limited(bad, 1800000000), later);
        }
    }

    /// Live: asks GitHub over TLS with the OS's roots (`cargo test -p pdfcraft -- --ignored`).
    #[test]
    #[ignore = "needs network access"]
    fn github_answers_with_the_latest_release() {
        let r = latest_release().unwrap();
        assert!(pdfcraft_ui_egui::updates::is_newer(&r.version, "0.0.0"), "{r:?}");
        assert!(r.url.starts_with(RELEASES_PAGE), "{r:?}");
    }
}
