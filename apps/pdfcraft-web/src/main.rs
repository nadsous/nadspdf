//! NadsPDF in the browser. Build: `cd apps/pdfcraft-web && trunk build --release`
//! (trunk generates the JS loader; no handwritten JS — plan/execution-plan.md §1).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast;
    use pdfcraft_ui_egui::PdfCraftApp;

    let options = eframe::WebOptions { renderer: eframe::Renderer::Glow, ..Default::default() };
    wasm_bindgen_futures::spawn_local(async move {
        let Some(canvas) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("pdfcraft"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        else {
            return;
        };
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
                    // comment, fill in, sign), opening documents in Read mode.
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
                    // prompts; ask the browser to confirm instead while a document has unsaved work (#812).
                    warn_before_unload(app.unsaved_flag.clone());
                    // `?host=parent`: a same-origin page embedding NadsPDF in an iframe (such
                    // as the Nextcloud app) passes files in and takes saves back (`host` below).
                    if query_param("host").as_deref() == Some("parent")
                        && let Some(bridge) = host::Bridge::new()
                    {
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
                    }
                    Ok(Box::new(app))
                }),
            )
            .await;
        if let Err(e) = started {
            eframe::web_sys::console::error_1(&e);
            if query_param("host").as_deref() == Some("parent")
                && let Some(bridge) = host::Bridge::new()
            {
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
                plain = Some(pdf_name(value));
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

/// The `postMessage` bridge to a same-origin parent page. Messages are plain objects with a
/// `type`:
///
/// - `pdfcraft:ready` (to the parent): the app is listening; send files now.
/// - `pdfcraft:open` (from the parent): `{ name, bytes }`, where `bytes` is an `ArrayBuffer` or
///   `Uint8Array` holding a PDF. It opens in a tab, like a `?file=` URL.
/// - `pdfcraft:save` (to the parent): `{ name, saveAs, bytes }` when the user saves; `bytes` is
///   a transferred `ArrayBuffer`. The parent stores the file (and reports its own errors).
/// - `pdfcraft:dirty` (to the parent): `{ dirty }` whenever unsaved work appears or goes away.
/// - `pdfcraft:failed` (to the parent): `{ error }` when the app couldn't start.
///
/// Only the parent window on the app's own origin is heard or answered.
#[cfg(target_arch = "wasm32")]
mod host {
    use eframe::wasm_bindgen::{JsCast, JsValue, closure::Closure};

    #[derive(Clone)]
    pub struct Bridge {
        window: web_sys::Window,
        parent: web_sys::Window,
        origin: String,
    }

    impl Bridge {
        /// `None` when the page isn't framed (there is no parent to talk to).
        pub fn new() -> Option<Self> {
            let window = web_sys::window()?;
            let parent = window.parent().ok().flatten()?;
            if js_sys::Object::is(&parent, &window) {
                return None;
            }
            let origin = window.location().origin().ok()?;
            Some(Self { window, parent, origin })
        }

        fn post(&self, message: &js_sys::Object, transfer: Option<&JsValue>) -> Result<(), String> {
            let sent = match transfer {
                Some(t) => self.parent.post_message_with_transfer(message, &self.origin, &js_sys::Array::of1(t)),
                None => self.parent.post_message(message, &self.origin),
            };
            sent.map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
        }

        pub fn post_ready(&self) {
            if let Some(m) = message(&[("type", "pdfcraft:ready".into()), ("version", env!("CARGO_PKG_VERSION").into())]) {
                let _ = self.post(&m, None);
            }
        }

        /// The app couldn't start (no WebGL, for example): let the parent say so.
        pub fn post_failed(&self, error: &str) {
            if let Some(m) = message(&[("type", "pdfcraft:failed".into()), ("error", error.into())]) {
                let _ = self.post(&m, None);
            }
        }

        /// Hear `pdfcraft:open` from the parent and queue the file like a `?file=` download.
        pub fn listen(&self, inbox: pdfcraft_ui_egui::Inbox, ctx: egui::Context) {
            let parent = self.parent.clone();
            let origin = self.origin.clone();
            let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
                if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                    return;
                }
                let data = event.data();
                if string(&data, "type").as_deref() != Some("pdfcraft:open") {
                    return;
                }
                let name = string(&data, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "document.pdf".to_string());
                let Ok(bytes) = js_sys::Reflect::get(&data, &JsValue::from_str("bytes")) else { return };
                let bytes = if let Some(array) = bytes.dyn_ref::<js_sys::Uint8Array>() {
                    array.to_vec()
                } else if bytes.is_instance_of::<js_sys::ArrayBuffer>() {
                    js_sys::Uint8Array::new(&bytes).to_vec()
                } else {
                    return;
                };
                if let Ok(mut q) = inbox.lock() {
                    q.push((name, bytes));
                }
                ctx.request_repaint();
            });
            if self.window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref()).is_ok() {
                // The listener lives as long as the page.
                on_message.forget();
            }
        }

        pub fn save_fn(&self) -> pdfcraft_ui_egui::HostSaveFn {
            let bridge = self.clone();
            Box::new(move |name: &str, bytes: &[u8], save_as: bool| {
                let buffer = js_sys::Uint8Array::from(bytes).buffer();
                let m =
                    message(&[("type", "pdfcraft:save".into()), ("name", name.into()), ("saveAs", save_as.into()), ("bytes", buffer.clone().into())])
                        .ok_or_else(|| "couldn't build the message".to_string())?;
                bridge.post(&m, Some(&buffer))
            })
        }

        pub fn dirty_fn(&self) -> pdfcraft_ui_egui::HostDirtyFn {
            let bridge = self.clone();
            let mut last = false;
            Box::new(move |dirty: bool| {
                if dirty != last {
                    last = dirty;
                    if let Some(m) = message(&[("type", "pdfcraft:dirty".into()), ("dirty", dirty.into())]) {
                        let _ = bridge.post(&m, None);
                    }
                }
            })
        }
    }

    fn message(fields: &[(&str, JsValue)]) -> Option<js_sys::Object> {
        let m = js_sys::Object::new();
        for (key, value) in fields {
            js_sys::Reflect::set(&m, &JsValue::from_str(key), value).ok()?;
        }
        Some(m)
    }

    fn string(data: &JsValue, key: &str) -> Option<String> {
        js_sys::Reflect::get(data, &JsValue::from_str(key)).ok()?.as_string()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("pdfcraft-web targets wasm32: run `trunk serve` or `trunk build --release` in apps/pdfcraft-web");
}
