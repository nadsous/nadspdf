//! Extensions end to end: an approved extension starts with the app, hears about the open
//! document, reads its text, shows a message bar, hears the click on its button, and may open
//! only the web origins its manifest lists. The test binary is its own extension: copied into
//! the extensions folder as `demo-extension`, it sees its new name and plays the extension.

#![cfg(not(target_arch = "wasm32"))]

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;
use serde_json::{Value, json};

const CHILD: &str = "demo-extension";
const ALLOWED: &str = "https://erp.example.com/interventions/42";

fn is_child() -> bool {
    std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).as_deref() == Some(CHILD)
}

/// A two-page PDF whose pages say "Page 1" and "Page 2".
fn fixture() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [4 0 R 6 0 R] /Count 2 /MediaBox [0 0 200 300] >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << /Font << /F1 3 0 R >> >> >>".to_string(),
        stream("BT /F1 24 Tf 20 150 Td (Page 1) Tj ET"),
        "<< /Type /Page /Parent 2 0 R /Contents 7 0 R /Resources << /Font << /F1 3 0 R >> >> >>".to_string(),
        stream("BT /F1 24 Tf 20 150 Td (Page 2) Tj ET"),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

fn stream(body: &str) -> String {
    format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len())
}

/// The extension: answers events and responses one line at a time.
fn child() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut say = |v: Value| {
        writeln!(out, "{v}").unwrap();
        out.flush().unwrap();
    };
    let request = |id: u64, method: &str, params: Value| json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let mut doc = Value::Null;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { return };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        match (msg["method"].as_str(), msg["id"].as_u64()) {
            (Some("document.opened"), _) => {
                doc = msg["params"]["doc"].clone();
                say(request(1, "doc.text", json!({ "doc": doc })));
            }
            (None, Some(1)) => {
                let text = msg["result"]["pages"][0]["text"].as_str().unwrap_or_default();
                let seen = if text.contains("Page 1") { "text read" } else { "no text" };
                say(request(
                    2,
                    "ui.banner.show",
                    json!({ "doc": doc, "id": "di", "text": seen, "actions": [{ "id": "open", "label": "Open the job", "primary": true }] }),
                ));
            }
            (Some("banner.action"), _) if msg["params"]["action"] == "open" => {
                say(request(3, "shell.open_url", json!({ "url": ALLOWED })));
                say(request(4, "shell.open_url", json!({ "url": "https://evil.example.net/" })));
            }
            (None, Some(4)) => {
                let denied = msg["error"]["code"] == -32001;
                let text = if denied { "other site refused" } else { "other site opened" };
                say(request(5, "ui.banner.show", json!({ "doc": doc, "id": "di", "text": text, "actions": [] })));
            }
            _ => {}
        }
    }
}

struct Installed {
    root: PathBuf,
    folder: PathBuf,
    approvals: PathBuf,
}

impl Drop for Installed {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Copy this test binary into an extensions folder of its own and, with `approve`, approve it.
fn install(name: &str, approve: bool) -> Installed {
    let root = std::env::temp_dir().join(format!("pdfcraft-extensions-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let folder = root.join("Extensions");
    let dir = folder.join("demo");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = format!("{CHILD}{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(std::env::current_exe().unwrap(), dir.join(&exe)).unwrap();
    let manifest = json!({
        "protocol": 1, "id": "com.example.demo", "name": "Demo ERP", "version": "1.0.0", "executable": exe,
        "permissions": ["document_events", "read_text", "banner", "open_url"],
        "open_url": ["https://erp.example.com"],
    });
    std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
    let approvals = root.join("extension-approvals.json");
    if approve {
        let installed = pdfcraft_extensions::discovery::load(&dir).unwrap();
        let mut a = pdfcraft_extensions::Approvals::default();
        a.approve(&installed);
        a.save(&approvals).unwrap();
    }
    Installed { root, folder, approvals }
}

fn harness(installed: &Installed) -> Harness<'static, PdfCraftApp> {
    let (folder, approvals) = (installed.folder.clone(), installed.approvals.clone());
    Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("doc.pdf", None, fixture()).unwrap();
        app.enable_extensions(&cc.egui_ctx, folder, approvals);
        app
    })
}

fn step_until(h: &mut Harness<'static, PdfCraftApp>, what: impl Fn(&PdfCraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !what(h.state()) {
        assert!(Instant::now() < deadline, "timed out; banners: {:?}", h.state().extensions.shown_banners());
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn banner_text(app: &PdfCraftApp) -> Option<String> {
    app.extensions.shown_banners().first().map(|b| b.text.clone())
}

#[test]
fn an_approved_extension_reads_text_shows_a_bar_and_opens_only_its_sites() {
    if is_child() {
        child();
        return;
    }
    let installed = install("approved", true);
    let mut h = harness(&installed);

    step_until(&mut h, |app| banner_text(app).is_some());
    assert_eq!(banner_text(h.state()).as_deref(), Some("text read"), "doc.text answered with the page's text");
    h.run_steps(2);

    h.get_by_label("Open the job").click();
    step_until(&mut h, |app| banner_text(app).as_deref() == Some("other site refused"));
    assert_eq!(h.state().last_opened_url.as_deref(), Some(ALLOWED), "only the listed origin opened");
}

#[test]
fn an_extension_that_is_not_approved_does_not_start() {
    if is_child() {
        return;
    }
    let installed = install("unapproved", false);
    let mut h = harness(&installed);
    step_until(&mut h, |app| !app.extensions.entries().is_empty());
    for _ in 0..50 {
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(h.state().extensions.shown_banners().is_empty());
    assert!(!h.state().extensions.is_running("com.example.demo"));
}
