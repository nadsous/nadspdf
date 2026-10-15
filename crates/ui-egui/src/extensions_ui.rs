//! Extensions in the desktop app ([`pdfcraft_extensions`]): starting the approved ones, telling
//! them about documents, answering their requests, the message bars they show above a document
//! and their Preferences section.
//!
//! Off unless the desktop app calls [`crate::PdfCraftApp::enable_extensions`]: tests and the web
//! build never look for extensions. Even then an extension runs only once the user turned it on
//! in Preferences, and only while its program and manifest are the ones they approved.
//!
//! Open and closed documents are found by comparing the session's documents with the ones seen
//! last frame, so every way a document comes or goes (a tab, a reload, a torn-off window, a
//! combine that replaces it) is reported once.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use pdfcraft_engine::DocId;
pub use pdfcraft_extensions::folders::approvals_file;
use pdfcraft_extensions::protocol::BannerAction;
use pdfcraft_extensions::{Approvals, Entry, Installed, Permission};

use crate::PdfCraftApp;
use crate::theme::Tokens;

/// A message bar an extension shows above one document.
#[derive(Debug, Clone, PartialEq)]
pub struct Banner {
    pub ext: String,
    pub doc: DocId,
    pub id: String,
    pub text: String,
    pub actions: Vec<BannerAction>,
}

#[derive(Default)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct Extensions {
    folder: Option<PathBuf>,
    approvals_path: Option<PathBuf>,
    pub(crate) entries: Vec<Entry>,
    approvals: Approvals,
    /// Why an extension isn't running (it stopped, or couldn't start), by id.
    status: BTreeMap<String, String>,
    /// Documents already reported as opened.
    known: BTreeSet<u64>,
    pub(crate) banners: Vec<Banner>,
    /// The extension whose "Turn on" waits for the user's confirmation in Preferences.
    confirm: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    native: Option<Native>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Native {
    ctx: egui::Context,
    host: pdfcraft_extensions::host::Host,
    scan: Option<std::sync::mpsc::Receiver<Vec<Entry>>>,
    text_tx: std::sync::mpsc::Sender<TextDone>,
    text_rx: std::sync::mpsc::Receiver<TextDone>,
    /// `doc.text` reads under way, by extension (at most [`MAX_TEXT_READS`] each).
    reading: BTreeMap<String, usize>,
}

/// Background `doc.text` reads one extension may have under way at once.
#[cfg(not(target_arch = "wasm32"))]
const MAX_TEXT_READS: usize = 2;

#[cfg(not(target_arch = "wasm32"))]
struct TextDone {
    ext: String,
    id: serde_json::Value,
    result: Result<serde_json::Value, String>,
}

impl Extensions {
    pub fn enabled(&self) -> bool {
        self.folder.is_some()
    }

    pub fn folder(&self) -> Option<&std::path::Path> {
        self.folder.as_deref()
    }

    /// The extension folders found, usable or not (empty until the folder has been read).
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The message bars showing, on every document.
    pub fn shown_banners(&self) -> &[Banner] {
        &self.banners
    }

    pub fn is_running(&self, id: &str) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.native.as_ref().is_some_and(|n| n.host.is_running(id))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = id;
            false
        }
    }

    fn installed(&self, id: &str) -> Option<&Installed> {
        self.entries.iter().find_map(|e| match e {
            Entry::Ok(i) if i.manifest.id == id => Some(&**i),
            _ => None,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_approvals(&self) -> Result<(), String> {
        match &self.approvals_path {
            Some(path) => self.approvals.save(path).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }
}

/// The extensions folder: `Extensions` in the portable data folder in portable mode, otherwise
/// the per-user one ([`pdfcraft_extensions::folders::user_folder`]). The approvals live beside it.
pub fn default_folder() -> Option<PathBuf> {
    match crate::portable::data_dir() {
        Some(dir) => Some(dir.join("Extensions")),
        None => pdfcraft_extensions::folders::user_folder(),
    }
}

/// The permissions as the user reads them before turning an extension on.
pub fn permission_label(p: Permission) -> &'static str {
    match p {
        Permission::DocumentEvents => tl!("Know when you open or close a document (its name and number of pages)"),
        Permission::ReadText => tl!("Read the text of the documents you open"),
        Permission::ReadFile => tl!("Know where the documents you open are saved"),
        Permission::Banner => tl!("Show a message bar with buttons above a document"),
        Permission::OpenUrl => tl!("Open web pages on the sites it lists"),
    }
}

impl PdfCraftApp {
    /// Look for extensions in `folder`, with the user's approvals kept in `approvals`. The
    /// folder is read on a worker thread; approved extensions start when it has been read.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_extensions(&mut self, ctx: &egui::Context, folder: PathBuf, approvals: PathBuf) {
        let ext = &mut self.extensions;
        ext.approvals = Approvals::load(&approvals);
        ext.approvals_path = Some(approvals);
        ext.folder = Some(folder);
        let waker = ctx.clone();
        let wake: pdfcraft_extensions::host::Wake = std::sync::Arc::new(move || waker.request_repaint());
        let (text_tx, text_rx) = std::sync::mpsc::channel();
        ext.native = Some(Native {
            ctx: ctx.clone(),
            host: pdfcraft_extensions::host::Host::new(wake),
            scan: None,
            text_tx,
            text_rx,
            reading: BTreeMap::new(),
        });
        rescan(self);
    }
}

/// Read the extensions folder again (on a worker thread).
#[cfg(not(target_arch = "wasm32"))]
pub fn rescan(app: &mut PdfCraftApp) {
    let ext = &mut app.extensions;
    let (Some(folder), Some(native)) = (ext.folder.clone(), ext.native.as_mut()) else { return };
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = native.ctx.clone();
    let spawned = std::thread::Builder::new().name("pdfcraft-extensions-scan".into()).spawn(move || {
        let _ = tx.send(pdfcraft_extensions::discover(&folder));
        ctx.request_repaint();
    });
    match spawned {
        Ok(_) => native.scan = Some(rx),
        Err(e) => log::warn!("extensions: the folder could not be read: {e}"),
    }
}

/// Once a frame: the folder scan, documents opened and closed, the extensions' requests and the
/// answers of background reads.
#[cfg(not(target_arch = "wasm32"))]
pub fn pump(app: &mut PdfCraftApp) {
    if app.extensions.native.is_none() {
        return;
    }
    scanned(app);
    documents(app);
    let messages = match app.extensions.native.as_mut() {
        Some(n) => n.host.poll(),
        None => return,
    };
    for m in messages {
        match m.message {
            pdfcraft_extensions::host::Message::Exited(reason) => {
                log::info!("extension {}: {reason}", m.ext);
                app.extensions.status.insert(m.ext.clone(), reason);
                app.extensions.banners.retain(|b| b.ext != m.ext);
            }
            pdfcraft_extensions::host::Message::Line(line) => handle(app, &m.ext, line),
        }
    }
    let done: Vec<TextDone> = match app.extensions.native.as_mut() {
        Some(n) => {
            let done: Vec<TextDone> = n.text_rx.try_iter().collect();
            for d in &done {
                if let Some(count) = n.reading.get_mut(&d.ext) {
                    *count = count.saturating_sub(1);
                }
            }
            done
        }
        None => Vec::new(),
    };
    for d in done {
        let line = match d.result {
            Ok(v) => pdfcraft_extensions::protocol::result(&d.id, v),
            Err(e) => pdfcraft_extensions::protocol::error(&d.id, pdfcraft_extensions::protocol::FAILED, &e),
        };
        send(app, &d.ext, &line);
    }
}

#[cfg(target_arch = "wasm32")]
pub fn pump(_app: &mut PdfCraftApp) {}

#[cfg(not(target_arch = "wasm32"))]
fn send(app: &mut PdfCraftApp, ext: &str, line: &str) {
    if let Some(n) = app.extensions.native.as_mut() {
        n.host.send(ext, line);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn scanned(app: &mut PdfCraftApp) {
    let Some(native) = app.extensions.native.as_mut() else { return };
    let Some(rx) = &native.scan else { return };
    let Ok(entries) = rx.try_recv() else { return };
    native.scan = None;
    app.extensions.entries = entries;
    let approved: Vec<Installed> = app
        .extensions
        .entries
        .iter()
        .filter_map(|e| match e {
            Entry::Ok(i) if app.extensions.approvals.is_approved(i) => Some((**i).clone()),
            _ => None,
        })
        .collect();
    for ext in approved {
        start(app, &ext);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn start(app: &mut PdfCraftApp, ext: &Installed) {
    let hello = serde_json::json!({
        "host": "NadsPDF",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": pdfcraft_extensions::manifest::PROTOCOL_VERSION,
    });
    let Some(native) = app.extensions.native.as_mut() else { return };
    if native.host.is_running(&ext.manifest.id) {
        return;
    }
    match native.host.start(ext, hello) {
        Ok(()) => {
            app.extensions.status.remove(&ext.manifest.id);
            // It missed the documents that were already open.
            if ext.manifest.allows(Permission::DocumentEvents) {
                let ids: Vec<DocId> = app.session.docs().iter().map(|d| d.id).collect();
                for id in ids {
                    if let Some(line) = opened_event(app, id, &ext.manifest) {
                        send(app, &ext.manifest.id, &line);
                    }
                }
            }
        }
        Err(e) => {
            log::warn!("extension {}: {e}", ext.manifest.id);
            app.extensions.status.insert(ext.manifest.id.clone(), e);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn stop(app: &mut PdfCraftApp, id: &str) {
    if let Some(n) = app.extensions.native.as_mut() {
        n.host.stop(id);
    }
    app.extensions.banners.retain(|b| b.ext != id);
}

#[cfg(not(target_arch = "wasm32"))]
fn running_with(app: &PdfCraftApp, permission: Permission) -> Vec<pdfcraft_extensions::Manifest> {
    let Some(native) = app.extensions.native.as_ref() else { return Vec::new() };
    app.extensions
        .entries
        .iter()
        .filter_map(|e| match e {
            Entry::Ok(i) if native.host.is_running(&i.manifest.id) && i.manifest.allows(permission) => Some(i.manifest.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn opened_event(app: &PdfCraftApp, id: DocId, manifest: &pdfcraft_extensions::Manifest) -> Option<String> {
    let doc = app.session.get(id)?;
    let path = if manifest.allows(Permission::ReadFile) { doc.path.clone() } else { None };
    Some(pdfcraft_extensions::protocol::event(
        "document.opened",
        serde_json::json!({ "doc": id.0, "name": doc.name, "pages": doc.info.pages.len(), "path": path }),
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn documents(app: &mut PdfCraftApp) {
    let now: BTreeSet<u64> = app.session.docs().iter().map(|d| d.id.0).collect();
    if now == app.extensions.known {
        return;
    }
    let opened: Vec<u64> = now.difference(&app.extensions.known).copied().collect();
    let closed: Vec<u64> = app.extensions.known.difference(&now).copied().collect();
    app.extensions.known = now;
    app.extensions.banners.retain(|b| !closed.contains(&b.doc.0));
    let listeners = running_with(app, Permission::DocumentEvents);
    for m in &listeners {
        for &id in &opened {
            if let Some(line) = opened_event(app, DocId(id), m) {
                send(app, &m.id, &line);
            }
        }
        for &id in &closed {
            send(app, &m.id, &pdfcraft_extensions::protocol::event("document.closed", serde_json::json!({ "doc": id })));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn handle(app: &mut PdfCraftApp, ext: &str, line: pdfcraft_extensions::protocol::Incoming) {
    use pdfcraft_extensions::protocol::{self as p, Incoming, Method};
    let (id, method, params) = match line {
        Incoming::Request { id, method, params } => (id, method, params),
        Incoming::Invalid { id: Some(id), code, message } => {
            send(app, ext, &p::error(&id, code, &message));
            return;
        }
        Incoming::Invalid { id: None, message, .. } => {
            log::info!("extension {ext}: ignored a message: {message}");
            return;
        }
    };
    let Some(manifest) = app.extensions.installed(ext).map(|i| i.manifest.clone()) else { return };
    let needed = method.permission();
    if !manifest.allows(needed) {
        let message = format!("{} needs the {} permission in the manifest", method_name(method), needed.key());
        send(app, ext, &p::error(&id, p::PERMISSION_DENIED, &message));
        return;
    }
    let answer: Result<Option<serde_json::Value>, (i64, String)> = match method {
        Method::DocText => doc_text(app, ext, &id, &params).map(|()| None),
        Method::DocFile => doc_file(app, &params).map(Some),
        Method::BannerShow => banner_show(app, ext, &params).map(Some),
        Method::BannerHide => banner_hide(app, ext, &params).map(Some),
        Method::OpenUrl => open_url(app, &manifest, &params).map(Some),
    };
    match answer {
        Ok(Some(v)) => send(app, ext, &p::result(&id, v)),
        Ok(None) => {}
        Err((code, message)) => send(app, ext, &p::error(&id, code, &message)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn method_name(m: pdfcraft_extensions::protocol::Method) -> &'static str {
    use pdfcraft_extensions::protocol::Method;
    match m {
        Method::DocText => "doc.text",
        Method::DocFile => "doc.file",
        Method::BannerShow => "ui.banner.show",
        Method::BannerHide => "ui.banner.hide",
        Method::OpenUrl => "shell.open_url",
    }
}

#[cfg(not(target_arch = "wasm32"))]
type Answer<T> = Result<T, (i64, String)>;

#[cfg(not(target_arch = "wasm32"))]
fn bad_params(e: String) -> (i64, String) {
    (pdfcraft_extensions::protocol::INVALID_PARAMS, e)
}

#[cfg(not(target_arch = "wasm32"))]
fn no_document(doc: u64) -> (i64, String) {
    (pdfcraft_extensions::protocol::FAILED, format!("document {doc} is not open"))
}

/// `doc.text`: read on a worker thread, answered by [`pump`] when it is done.
#[cfg(not(target_arch = "wasm32"))]
fn doc_text(app: &mut PdfCraftApp, ext: &str, id: &serde_json::Value, params: &serde_json::Value) -> Answer<()> {
    use pdfcraft_extensions::protocol::{TextParams, params as decode};
    const MAX_PAGE_CHARS: usize = 100_000;
    let asked: TextParams = decode(params).map_err(bad_params)?;
    let doc = app.session.get(DocId(asked.doc)).ok_or_else(|| no_document(asked.doc))?;
    let pages = asked.pages(doc.info.pages.len()).map_err(bad_params)?;
    let bytes = doc.bytes.clone();
    let config = pdfcraft_render::RenderConfig { password: doc.password.as_deref().map(std::sync::Arc::from), ..Default::default() };
    let Some(native) = app.extensions.native.as_mut() else { return Ok(()) };
    let reading = native.reading.entry(ext.to_string()).or_insert(0);
    if *reading >= MAX_TEXT_READS {
        return Err((pdfcraft_extensions::protocol::FAILED, format!("at most {MAX_TEXT_READS} doc.text requests at a time; wait for an answer")));
    }
    let (tx, ctx) = (native.text_tx.clone(), native.ctx.clone());
    let (ext, id, doc_id) = (ext.to_string(), id.clone(), asked.doc);
    let job = move || {
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut renderer = pdfcraft_render::PageRenderer::new(bytes, config);
            let out: Vec<serde_json::Value> = pages
                .iter()
                .map(|&p| {
                    let r = renderer.render(pdfcraft_render::RenderRequest {
                        page: p,
                        kind: pdfcraft_render::RequestKind::Text,
                        scale: 1.0,
                        ..Default::default()
                    });
                    let text: String = r.text.map(|t| t.plain_text()).unwrap_or_default().chars().take(MAX_PAGE_CHARS).collect();
                    serde_json::json!({ "page": p.saturating_add(1), "text": text })
                })
                .collect();
            serde_json::json!({ "doc": doc_id, "pages": out })
        }));
        let result = read.map_err(|_| "the document's text could not be read".to_string());
        let _ = tx.send(TextDone { ext, id, result });
        ctx.request_repaint();
    };
    let spawned = std::thread::Builder::new().name("pdfcraft-extension-text".into()).spawn(job);
    match spawned {
        Ok(_) => {
            *reading = reading.saturating_add(1);
            Ok(())
        }
        Err(e) => Err((pdfcraft_extensions::protocol::FAILED, format!("the text could not be read: {e}"))),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn doc_file(app: &PdfCraftApp, params: &serde_json::Value) -> Answer<serde_json::Value> {
    use pdfcraft_extensions::protocol::{DocParams, params as decode};
    let asked: DocParams = decode(params).map_err(bad_params)?;
    let doc = app.session.get(DocId(asked.doc)).ok_or_else(|| no_document(asked.doc))?;
    let Some(path) = doc.path.clone() else {
        return Err((pdfcraft_extensions::protocol::FAILED, "the document is not saved in a file yet".into()));
    };
    Ok(serde_json::json!({ "doc": asked.doc, "name": doc.name, "path": path, "dirty": doc.dirty }))
}

#[cfg(not(target_arch = "wasm32"))]
fn banner_show(app: &mut PdfCraftApp, ext: &str, params: &serde_json::Value) -> Answer<serde_json::Value> {
    use pdfcraft_extensions::protocol::{BannerShow, MAX_BANNERS_PER_DOC, params as decode};
    let asked: BannerShow = decode(params).map_err(bad_params)?;
    asked.validate().map_err(bad_params)?;
    if app.session.get(DocId(asked.doc)).is_none() {
        return Err(no_document(asked.doc));
    }
    let doc = DocId(asked.doc);
    let banners = &mut app.extensions.banners;
    let banner = Banner { ext: ext.to_string(), doc, id: asked.id.clone(), text: asked.text, actions: asked.actions };
    if let Some(existing) = banners.iter_mut().find(|b| b.ext == ext && b.doc == doc && b.id == asked.id) {
        *existing = banner;
    } else {
        if banners.iter().filter(|b| b.ext == ext && b.doc == doc).count() >= MAX_BANNERS_PER_DOC {
            return Err((pdfcraft_extensions::protocol::FAILED, format!("at most {MAX_BANNERS_PER_DOC} message bars per document")));
        }
        banners.push(banner);
    }
    if let Some(ctx) = &app.ctx {
        ctx.request_repaint();
    }
    Ok(serde_json::json!({ "shown": true }))
}

#[cfg(not(target_arch = "wasm32"))]
fn banner_hide(app: &mut PdfCraftApp, ext: &str, params: &serde_json::Value) -> Answer<serde_json::Value> {
    use pdfcraft_extensions::protocol::{BannerHide, params as decode};
    let asked: BannerHide = decode(params).map_err(bad_params)?;
    let before = app.extensions.banners.len();
    app.extensions.banners.retain(|b| !(b.ext == ext && b.doc.0 == asked.doc && b.id == asked.id));
    Ok(serde_json::json!({ "hidden": app.extensions.banners.len() < before }))
}

#[cfg(not(target_arch = "wasm32"))]
fn open_url(app: &mut PdfCraftApp, manifest: &pdfcraft_extensions::Manifest, params: &serde_json::Value) -> Answer<serde_json::Value> {
    use pdfcraft_extensions::protocol::{UrlParams, params as decode};
    let asked: UrlParams = decode(params).map_err(bad_params)?;
    if !manifest.url_allowed(&asked.url) {
        return Err((pdfcraft_extensions::protocol::PERMISSION_DENIED, "this address is not on an origin listed in the manifest's open_url".into()));
    }
    app.open_url(&asked.url);
    Ok(serde_json::json!({ "opened": true }))
}

/// Draw the extensions' message bars for `doc` above its pages. A button tells its extension.
pub fn banners(app: &mut PdfCraftApp, doc: DocId, ui: &mut egui::Ui, t: &Tokens) {
    if app.extensions.banners.is_empty() {
        return;
    }
    let shown: Vec<Banner> = app.extensions.banners.iter().filter(|b| b.doc == doc).cloned().collect();
    let mut clicked: Vec<(String, String, String)> = Vec::new();
    let mut dismissed: Vec<(String, String)> = Vec::new();
    for b in &shown {
        let name = app.extensions.installed(&b.ext).map_or(b.ext.clone(), |i| i.manifest.name.clone());
        let text = format!("{name} · {}", b.text);
        crate::canvas::notice_bar(ui, t, "info", t.accent_text, text, |ui| {
            if crate::icons::button(ui, "x", 22.0, false, tl!("Dismiss")).clicked() {
                dismissed.push((b.ext.clone(), b.id.clone()));
            }
            for a in b.actions.iter().rev() {
                if crate::widgets::pill_button(ui, &a.label, a.primary).clicked() {
                    clicked.push((b.ext.clone(), b.id.clone(), a.id.clone()));
                }
            }
        });
    }
    for (ext, id) in dismissed {
        app.extensions.banners.retain(|b| !(b.ext == ext && b.doc == doc && b.id == id));
        #[cfg(target_arch = "wasm32")]
        let _ = (ext, id);
        #[cfg(not(target_arch = "wasm32"))]
        send(app, &ext, &pdfcraft_extensions::protocol::event("banner.dismissed", serde_json::json!({ "doc": doc.0, "banner": id })));
    }
    for (ext, banner, action) in clicked {
        #[cfg(not(target_arch = "wasm32"))]
        send(
            app,
            &ext,
            &pdfcraft_extensions::protocol::event("banner.action", serde_json::json!({ "doc": doc.0, "banner": banner, "action": action })),
        );
        #[cfg(target_arch = "wasm32")]
        let _ = (ext, banner, action);
    }
}

/// Preferences ▸ Extensions. Hidden while none is installed.
#[cfg(not(target_arch = "wasm32"))]
pub fn preferences_section(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) {
    if !app.extensions.enabled() || app.extensions.entries.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tl!("Extensions")).font(crate::theme::semibold(13.0)));
    let entries = app.extensions.entries.clone();
    for entry in &entries {
        egui::Frame::new().fill(t.hover).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match entry {
                Entry::Ok(ext) => extension_row(ui, app, t, ext),
                Entry::Broken { dir, error } => {
                    let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.label(egui::RichText::new(folder).strong());
                    ui.label(
                        egui::RichText::new(crate::i18n::fmt(tl!("This extension can't be used: {e}"), &[("e", error)])).small().color(t.text_muted),
                    );
                }
            }
        });
        ui.add_space(4.0);
    }
    if let Some(folder) = app.extensions.folder() {
        ui.label(
            egui::RichText::new(crate::i18n::fmt(tl!("Extensions folder: {path}"), &[("path", &folder.display().to_string())]))
                .small()
                .color(t.text_muted),
        );
    }
}

#[cfg(target_arch = "wasm32")]
pub fn preferences_section(_ui: &mut egui::Ui, _app: &mut PdfCraftApp, _t: &Tokens) {}

#[cfg(not(target_arch = "wasm32"))]
fn extension_row(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens, ext: &Installed) {
    let id = ext.manifest.id.clone();
    let approved = app.extensions.approvals.is_approved(ext);
    let running = app.extensions.native.as_ref().is_some_and(|n| n.host.is_running(&id));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&ext.manifest.name).strong());
        ui.label(egui::RichText::new(&ext.manifest.version).color(t.text_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if approved {
                if crate::widgets::pill_button(ui, tl!("Turn off"), false).clicked() {
                    app.extensions.approvals.revoke(&id);
                    if let Err(e) = app.extensions.save_approvals() {
                        app.notify_fmt("Couldn't save the extension settings: {e}", &[("e", &e)]);
                    }
                    stop(app, &id);
                    app.extensions.status.remove(&id);
                }
            } else if app.extensions.confirm.as_deref() != Some(id.as_str()) && crate::widgets::pill_button(ui, tl!("Turn on…"), true).clicked() {
                app.extensions.confirm = Some(id.clone());
            }
        });
    });
    if !ext.manifest.description.is_empty() {
        ui.label(egui::RichText::new(&ext.manifest.description).small());
    }
    let state = if running {
        tl!("On").to_string()
    } else if let Some(reason) = app.extensions.status.get(&id) {
        crate::i18n::fmt(tl!("Stopped: {e}"), &[("e", reason)])
    } else if approved {
        tl!("On at the next start").to_string()
    } else {
        tl!("Off").to_string()
    };
    ui.label(egui::RichText::new(state).small().color(t.text_muted));
    if app.extensions.confirm.as_deref() == Some(id.as_str()) {
        ui.add_space(6.0);
        ui.label(crate::i18n::fmt(
            tl!("{name} is a program that runs on this computer, with your rights. It will be able to:"),
            &[("name", &ext.manifest.name)],
        ));
        if ext.manifest.permissions.is_empty() {
            ui.label(tl!("• Nothing beyond running."));
        }
        for p in &ext.manifest.permissions {
            ui.label(format!("• {}", permission_label(*p)));
        }
        for origin in &ext.manifest.open_url {
            ui.label(egui::RichText::new(format!("    {origin}")).small().color(t.text_muted));
        }
        ui.label(egui::RichText::new(ext.executable.display().to_string()).small().color(t.text_muted));
        ui.label(egui::RichText::new(tl!("Only turn on extensions you trust.")).small().color(t.text_muted));
        ui.horizontal(|ui| {
            if crate::widgets::pill_button(ui, tl!("Turn on"), true).clicked() {
                app.extensions.confirm = None;
                app.extensions.approvals.approve(ext);
                if let Err(e) = app.extensions.save_approvals() {
                    app.notify_fmt("Couldn't save the extension settings: {e}", &[("e", &e)]);
                }
                start(app, ext);
            }
            if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                app.extensions.confirm = None;
            }
        });
    }
}
