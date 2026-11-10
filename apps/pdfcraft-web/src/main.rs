//! NadsPDF in the browser. Build: `cd apps/pdfcraft-web && trunk build --release`
//! (trunk generates the JS loader; no handwritten JS — plan/execution-plan.md §1).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(any(target_arch = "wasm32", test))]
mod api;

#[cfg(target_arch = "wasm32")]
fn main() {
    use pdfcraft_ui_egui::PdfCraftApp;

    let options = eframe::WebOptions { renderer: eframe::Renderer::Glow, ..Default::default() };
    wasm_bindgen_futures::spawn_local(async move {
        let Some(canvas) = app_canvas() else { return };
        let started = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(|cc| {
                    let mut app = PdfCraftApp::new();
                    // "printcraft": settings saved under the app's former name, PrintCraft.
                    if let Some(json) = cc.storage.and_then(|s| s.get_string("pdfcraft").or_else(|| s.get_string("printcraft"))) {
                        app.restore(&json);
                    }
                    // `?lang=fr`: the host's interface language, over the saved one (the web has
                    // no system language: "auto" is English there).
                    if let Some(lang) = query_param("lang") {
                        let _ = app.set_option("language", &lang);
                    }
                    // `?lite=1`: the simplified interface the Chrome extension shows (read,
                    // comment, fill in, sign, edit, OCR), opening documents in Read mode.
                    if query_param("lite").is_some_and(|v| v != "0") {
                        app.lite = true;
                        let _ = app.set_option("mode", "read");
                    }
                    // `?file=<url>` opens a PDF from a URL (same-origin or CORS-enabled); the
                    // Chrome extension's redirect passes it as `#file=<url>` (`source::file_url`).
                    let hash = web_sys::window().and_then(|w| w.location().hash().ok()).unwrap_or_default();
                    if let Some(url) = source::file_url(&hash).or_else(|| query_param("file")) {
                        let inbox = app.startup_inbox.clone();
                        let failed = app.failed_inbox.clone();
                        let ctx = cc.egui_ctx.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let mut name = source::name_from_url(&url);
                            set_page_title(&name);
                            match fetch_bytes(&url).await {
                                Ok((bytes, served_name)) => {
                                    if let Some(served) = served_name {
                                        name = served;
                                        set_page_title(&name);
                                    }
                                    if let Ok(mut q) = inbox.lock() {
                                        q.push((name, bytes));
                                    }
                                }
                                Err(e) => {
                                    eframe::web_sys::console::error_1(&format!("NadsPDF: could not fetch {url}: {e}").into());
                                    // Shown in the app too, not only in the console (#173).
                                    if let Ok(mut q) = failed.lock() {
                                        q.push((name, e));
                                    }
                                }
                            }
                            ctx.request_repaint();
                        });
                    }
                    // eframe never raises a close request on a browser reload or tab close, so `guard_quit` never
                    // prompts; ask the browser to confirm instead while a document has unsaved work or an OCR review is open (#812).
                    warn_before_unload(app.unsaved_flag.clone());
                    // Host modes (`host` below, over `craft-web-host`): `?host=parent`, a
                    // same-origin page embedding NadsPDF in an iframe (such as the Nextcloud
                    // app), or `data-host="page"` on the canvas, a host that runs the module in
                    // its own document.
                    if let Some(bridge) = app_canvas().as_ref().and_then(host::Bridge::connect) {
                        // `?author=`: the host's name for the user signs their comments,
                        // unless they chose a name in NadsPDF (the web default is "Guest").
                        if let Some(author) = query_param("author").map(|a| a.trim().to_string()).filter(|a| !a.is_empty())
                            && app.comment_prefs.author == "Guest"
                        {
                            app.comment_prefs.author = author.chars().take(200).collect();
                        }
                        bridge.listen(app.startup_inbox.clone(), cc.egui_ctx.clone());
                        app.host_save = Some(bridge.save_fn());
                        app.host_dirty = Some(bridge.dirty_fn());
                        bridge.post_ready();
                    } else if query_param("host").as_deref() == Some("parent") {
                        eframe::web_sys::console::warn_1(
                            &"NadsPDF: ?host=parent needs a framing page on the same origin; saves download instead".into(),
                        );
                    }
                    // `?control`: `window.pdfcraft`, the page's command API (`api`). Opt-in: the control
                    // channel keeps the accessibility tree of every frame.
                    if query_param("control").is_some() {
                        api::install(app.attach_control(&cc.egui_ctx));
                    }
                    Ok(Box::new(app))
                }),
            )
            .await;
        if let Err(e) = started {
            eframe::web_sys::console::error_1(&e);
            if let Some(bridge) = app_canvas().as_ref().and_then(host::Bridge::connect) {
                bridge.post_failed(&e.as_string().unwrap_or_else(|| format!("{e:?}")));
            }
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn warn_before_unload(unsaved: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    use eframe::wasm_bindgen::{JsCast, closure::Closure};
    let Some(window) = web_sys::window() else { return };
    let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        if unsaved.load(std::sync::atomic::Ordering::Relaxed) {
            event.prevent_default();
        }
    });
    if window.add_event_listener_with_callback("beforeunload", handler.as_ref().unchecked_ref()).is_ok() {
        handler.forget();
    }
}

#[cfg(target_arch = "wasm32")]
fn query_param(key: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search).ok()?.get(key)
}

/// The browser tab shows the document's name, like Chrome's own viewer, instead of "NadsPDF".
#[cfg(target_arch = "wasm32")]
fn set_page_title(name: &str) {
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        document.set_title(&format!("{name} – NadsPDF"));
    }
}

/// The file's bytes, and the name the server gives it (`Content-Disposition`), if any.
#[cfg(target_arch = "wasm32")]
async fn fetch_bytes(url: &str) -> Result<(Vec<u8>, Option<String>), String> {
    use eframe::wasm_bindgen::JsCast;
    // `fetch` refuses `file:` URLs; an extension with file access may still read them through XHR.
    if url.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) {
        return read_local(url).await.map(|bytes| (bytes, None));
    }
    let window = web_sys::window().ok_or("no window")?;
    let resp = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url)).await.map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| "not a response")?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let served_name = resp.headers().get("content-disposition").ok().flatten().and_then(|h| source::name_from_disposition(&h));
    let buf = wasm_bindgen_futures::JsFuture::from(resp.array_buffer().map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))?;
    Ok((js_sys::Uint8Array::new(&buf).to_vec(), served_name))
}

/// A `file:` URL read with `XMLHttpRequest`, the only browser API that can (and only from an
/// extension the user let read local files).
#[cfg(target_arch = "wasm32")]
async fn read_local(url: &str) -> Result<Vec<u8>, String> {
    use eframe::wasm_bindgen::{JsCast, JsValue, closure::Closure};
    const NO_ACCESS: &str = "NadsPDF ne peut pas lire les fichiers de l'ordinateur. Dans la page des extensions, ouvrez \
                             « Détails » sur NadsPDF et activez « Autoriser l'accès aux URL de fichier ».";
    let js_err = |e: JsValue| e.as_string().unwrap_or_else(|| format!("{e:?}"));
    let xhr = web_sys::XmlHttpRequest::new().map_err(js_err)?;
    xhr.open_with_async("GET", url, true).map_err(js_err)?;
    xhr.set_response_type(web_sys::XmlHttpRequestResponseType::Arraybuffer);
    let done = js_sys::Promise::new(&mut |resolve: js_sys::Function, reject: js_sys::Function| {
        let onload = Closure::once(move || {
            let _ = resolve.call0(&JsValue::NULL);
        });
        let onerror = Closure::once(move || {
            let _ = reject.call0(&JsValue::NULL);
        });
        xhr.set_onload(Some(onload.as_ref().unchecked_ref()));
        xhr.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        // One request per page load: the handlers live as long as the page.
        onload.forget();
        onerror.forget();
    });
    xhr.send().map_err(|_| NO_ACCESS.to_string())?;
    wasm_bindgen_futures::JsFuture::from(done).await.map_err(|_| NO_ACCESS.to_string())?;
    // A `file:` read answers status 0; anything else is an HTTP-style failure.
    match xhr.status() {
        Ok(0 | 200) => {}
        Ok(status) => return Err(format!("HTTP {status}")),
        Err(e) => return Err(js_err(e)),
    }
    let body = xhr.response().map_err(js_err)?;
    if !body.is_instance_of::<js_sys::ArrayBuffer>() {
        return Err(NO_ACCESS.to_string());
    }
    Ok(js_sys::Uint8Array::new(&body).to_vec())
}

/// Where a startup PDF comes from and what to call it. Plain string handling, so it is tested
/// on the host as well as built for the browser.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod source {
    /// Longest name kept, in characters: a URL can be any length (#161).
    const MAX_NAME: usize = 200;

    /// The document address in a `#file=<url>` fragment, as the Chrome extension's redirect
    /// writes it. The redirect can't encode the address, so it is taken whole: a `?file=` query
    /// would split a signed URL's own `&` parameters off. An encoded address is decoded.
    pub fn file_url(hash: &str) -> Option<String> {
        let raw = hash.strip_prefix('#').unwrap_or(hash).strip_prefix("file=")?;
        let url = if raw.contains("://") { raw.to_string() } else { percent_decode(raw) };
        (!url.trim().is_empty()).then_some(url)
    }

    /// The file name a URL ends in, decoded (`Mon%20devis.pdf` → `Mon devis.pdf`). Addresses
    /// without one (`https://arxiv.org/pdf/2301.00001`) get `.pdf` added, or are `document.pdf`.
    pub fn name_from_url(url: &str) -> String {
        let path = url.split(['?', '#']).next().unwrap_or_default();
        // Leave out the scheme and host: `https://example.com/` names no file.
        let path = match path.split_once("://") {
            Some((_, rest)) => rest.split_once('/').map(|(_, p)| p).unwrap_or_default(),
            None => path,
        };
        let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
        pdf_name(&percent_decode(last))
    }

    /// The name in a `Content-Disposition` header: `filename*=UTF-8''…` (RFC 6266) first, then
    /// `filename=`, quoted or not.
    pub fn name_from_disposition(header: &str) -> Option<String> {
        let mut plain = None;
        for param in header.split(';') {
            let Some((key, value)) = param.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            if key.eq_ignore_ascii_case("filename*") {
                // charset'language'percent-encoded; only UTF-8 (and ASCII) are decoded as text.
                let mut parts = value.splitn(3, '\'');
                let (Some(charset), Some(_), Some(encoded)) = (parts.next(), parts.next(), parts.next()) else { continue };
                if charset.eq_ignore_ascii_case("utf-8") || charset.eq_ignore_ascii_case("us-ascii") {
                    return Some(pdf_name(&percent_decode(encoded)));
                }
            } else if key.eq_ignore_ascii_case("filename") {
                let value = value.strip_prefix('"').map(|v| v.split('"').next().unwrap_or_default()).unwrap_or(value);
                // Servers often percent-encode this one too (`Quittance%20de%20loyer.pdf`), and
                // Chrome shows it decoded; a value that isn't valid UTF-8 once decoded stays as sent.
                let decoded = percent_decode(value);
                plain = Some(pdf_name(if decoded.contains('\u{FFFD}') { value } else { &decoded }));
            }
        }
        plain
    }

    /// A safe tab name: no folders or control characters, bounded, ending in `.pdf`.
    fn pdf_name(raw: &str) -> String {
        let name: String = raw.chars().filter(|c| !c.is_control()).map(|c| if matches!(c, '/' | '\\') { '_' } else { c }).take(MAX_NAME).collect();
        let name = name.trim();
        if name.is_empty() || name.chars().all(|c| c == '.') {
            "document.pdf".to_string()
        } else if name.to_ascii_lowercase().ends_with(".pdf") {
            name.to_string()
        } else {
            format!("{name}.pdf")
        }
    }

    /// `%XX` escapes decoded as UTF-8 (invalid sequences become U+FFFD); a stray `%` stays.
    fn percent_decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while let Some(&b) = bytes.get(i) {
            let hex = |at: usize| bytes.get(at).and_then(|h| char::from(*h).to_digit(16));
            match (b, hex(i + 1), hex(i + 2)) {
                // Both digits are < 16, so the value always fits a byte.
                (b'%', Some(hi), Some(lo)) => {
                    out.push(u8::try_from(hi * 16 + lo).unwrap_or(b'%'));
                    i += 3;
                }
                _ => {
                    out.push(b);
                    i += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn signed_urls_keep_their_own_query() {
            let url = "https://bucket.s3.amazonaws.com/a.pdf?X-Amz-Expires=60&X-Amz-Signature=ab%2Bc";
            assert_eq!(file_url(&format!("#file={url}")).as_deref(), Some(url));
        }

        #[test]
        fn encoded_addresses_are_decoded() {
            assert_eq!(file_url("#file=https%3A%2F%2Fexample.com%2Fa.pdf").as_deref(), Some("https://example.com/a.pdf"));
        }

        #[test]
        fn other_fragments_are_not_files() {
            assert_eq!(file_url(""), None);
            assert_eq!(file_url("#page=3"), None);
            assert_eq!(file_url("#file="), None);
        }

        #[test]
        fn names_come_from_the_path() {
            assert_eq!(name_from_url("https://example.com/docs/Mon%20devis.pdf?v=2#page=1"), "Mon devis.pdf");
            assert_eq!(name_from_url("https://example.com/r?next=/x/y.pdf"), "r.pdf");
            assert_eq!(name_from_url("https://arxiv.org/pdf/2301.00001"), "2301.00001.pdf");
            assert_eq!(name_from_url("https://example.com/"), "document.pdf");
            assert_eq!(name_from_url("file:///C:/Users/me/Rapport%C3%A9t%C3%A9.PDF"), "Rapportété.PDF");
            assert_eq!(name_from_url("https://example.com/%2E%2E"), "document.pdf");
            assert_eq!(name_from_url("https://example.com/a%2Fb%00.pdf"), "a_b.pdf");
        }

        #[test]
        fn names_from_content_disposition() {
            assert_eq!(name_from_disposition("inline; filename=\"Facture 12.pdf\"").as_deref(), Some("Facture 12.pdf"));
            assert_eq!(name_from_disposition("inline; filename=plan; filename*=UTF-8''Plan%20%C3%A9tage.pdf").as_deref(), Some("Plan étage.pdf"));
            assert_eq!(name_from_disposition("INLINE; FILENAME=x.pdf").as_deref(), Some("x.pdf"));
            assert_eq!(name_from_disposition("inline; filename=\"Quittance%20de%20loyer.pdf\"").as_deref(), Some("Quittance de loyer.pdf"));
            assert_eq!(name_from_disposition("inline; filename=\"r%E9el.pdf\"").as_deref(), Some("r%E9el.pdf"));
            assert_eq!(name_from_disposition("inline"), None);
            assert_eq!(name_from_disposition("inline; filename*=broken"), None);
            assert_eq!(name_from_disposition("inline; filename*=broken; filename=ok.pdf").as_deref(), Some("ok.pdf"));
        }

        #[test]
        fn malformed_escapes_never_panic() {
            for s in ["%", "%4", "%zz", "%%41", "é%", "%C3", "%FF%FE"] {
                let _ = percent_decode(s);
                let _ = name_from_url(&format!("https://h/{s}"));
            }
            assert_eq!(percent_decode("%%41"), "%A");
            assert_eq!(name_from_url(&format!("https://h/{}", "a".repeat(1000))).chars().count(), MAX_NAME + 4);
        }
    }
}

/// The app's canvas: the one element `index.html` and every embedding page must provide, with
/// the id the app looks up.
#[cfg(target_arch = "wasm32")]
fn app_canvas() -> Option<web_sys::HtmlCanvasElement> {
    use eframe::wasm_bindgen::JsCast;
    web_sys::window()?.document()?.get_element_by_id("pdfcraft").and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
}

/// Host modes: how the page embedding PdfCraft passes files in and takes saves back. The messages
/// and the two transports are `craft-web-host`'s (storytold/craft-libs), the same contract as the
/// other Craft apps' bridges, with the `pdfcraft:` prefix. This module is PdfCraft's part of it.
///
/// - `?host=parent` is the `postMessage` bridge for a same-origin page that embeds PdfCraft in an
///   iframe (such as the Nextcloud app). With a parent on another origin there is no bridge: the
///   app runs as if unframed and saves download (#2167). `data-host="page"` on the app's canvas
///   is for a host that loads the module into its own document, with no iframe: the same messages
///   travel as DOM `CustomEvent`s on that canvas. `?host=parent` wins when both are present.
/// - `pdfcraft:ready` once the app is listening; send files now. PdfCraft reads bytes, so it
///   says `file: false` and a host sends `bytes`.
/// - `pdfcraft:open` (from the host): `{ name, bytes }` holding a PDF. It opens in a tab, like a
///   `?file=` URL; a file over [`host::MAX_OPEN_BYTES`] is refused before it is copied.
/// - `pdfcraft:save` (to the host): `{ name, saveAs, bytes }` when the user saves. The host stores
///   the file (and reports its own errors).
/// - `pdfcraft:dirty` whenever unsaved work appears or goes away, `pdfcraft:failed` when the app
///   couldn't start.
#[cfg(target_arch = "wasm32")]
mod host {
    use std::rc::Rc;

    use craft_web_host::web::{FileData, Host, Message};

    /// The app id in the host messages (`pdfcraft:ready`).
    const APP: &str = "pdfcraft";

    /// The name a file gets when the host sent none (the crate says `document` for a blank name).
    const DEFAULT_NAME: &str = "document.pdf";

    /// The largest file the host may hand over to open, checked before it is copied (the app's own readers have their own limits).
    const MAX_OPEN_BYTES: u64 = 512 << 20;

    #[derive(Clone)]
    pub struct Bridge {
        host: Rc<Host>,
    }

    impl Bridge {
        /// The bridge for this page, or `None` when PdfCraft isn't hosted: no `?host=parent` and
        /// no `data-host="page"` on the canvas, an unframed `?host=parent` page, or a parent on
        /// another origin (messages only go to this origin, so the browser would drop every save
        /// without an error while the document showed as saved, #2167).
        pub fn connect(canvas: &web_sys::HtmlCanvasElement) -> Option<Self> {
            Some(Self { host: Rc::new(Host::connect(APP, canvas)?) })
        }

        pub fn post_ready(&self) {
            let _ = self.host.post_ready(env!("CARGO_PKG_VERSION"), false);
        }

        /// The app couldn't start (no WebGL, for example): let the host say so.
        pub fn post_failed(&self, error: &str) {
            let _ = self.host.post_failed(error, None);
        }

        /// Hear `pdfcraft:open` and queue the file like a `?file=` download.
        pub fn listen(&self, inbox: pdfcraft_ui_egui::Inbox, ctx: egui::Context) {
            self.host.listen(move |message| {
                if let Message::Open { name, data } = message {
                    queue_open(&name, &data, &inbox, &ctx);
                }
            });
        }

        pub fn save_fn(&self) -> pdfcraft_ui_egui::HostSaveFn {
            let host = self.host.clone();
            Box::new(move |name: &str, bytes: &[u8], save_as: bool| host.post_save(name, bytes, None, save_as))
        }

        pub fn dirty_fn(&self) -> pdfcraft_ui_egui::HostDirtyFn {
            let host = self.host.clone();
            // A host starts out clean: the first report is a change only when it is dirty.
            let mut last = false;
            Box::new(move |dirty: bool| {
                if dirty != last {
                    last = dirty;
                    host.post_dirty(dirty);
                }
            })
        }
    }

    /// The `pdfcraft:open` payload, `{ name, bytes }` where `bytes` holds a PDF: queue it like a
    /// `?file=` download and wake the app. The crate has cleaned the name (and cut it to 255
    /// characters); a blank one comes as `document` and becomes `document.pdf`.
    fn queue_open(name: &str, data: &FileData, inbox: &pdfcraft_ui_egui::Inbox, ctx: &egui::Context) {
        let name = if name == "document" { DEFAULT_NAME } else { name }.to_string();
        let FileData::Bytes(view) = data else {
            eframe::web_sys::console::warn_1(&format!("PdfCraft: host file {name} came as a File; PdfCraft reads bytes only; not opened").into());
            return;
        };
        // A file from the host is untrusted input: refuse an oversized one before copying it into the app's memory.
        if data.size() > MAX_OPEN_BYTES as f64 {
            eframe::web_sys::console::warn_1(
                &format!("PdfCraft: host file is {} bytes, over the {} MiB host open limit; not opened", data.size(), MAX_OPEN_BYTES >> 20).into(),
            );
            return;
        }
        if let Ok(mut q) = inbox.lock() {
            q.push((name, view.to_vec()));
        }
        ctx.request_repaint();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("pdfcraft-web targets wasm32: run `trunk serve` or `trunk build --release` in apps/pdfcraft-web");
}
