use super::{DesignApp, Value, json};
use crate::app::file_dialogs::{Bound, with_path};

pub(super) fn download_document(app: &mut DesignApp) -> Result<Value, String> {
    let name = app.session.active().map(|d| format!("{}.designcraft", d.doc.title)).ok_or("no document")?;
    let ser = app.run("file.serialize", json!({}))?;
    let bytes = designcraft_engine::cmd::base64_decode(ser["base64"].as_str().unwrap_or_default());
    let download = app.services.download.as_mut().ok_or("no download service")?;
    download(&name, &bytes).map_err(|e| format!("could not download {name}: {e}"))?;
    // Marks the document saved; on the web the engine doesn't touch a file system.
    app.run("file.saveAs", json!({"path": name}))
}

/// Asks where to save `name`, then runs `export` with `p` and the chosen path.
#[track_caller]
fn ask_then_export(
    app: &mut DesignApp,
    name: &str,
    p: Value,
    export: impl FnOnce(&mut DesignApp, &Value) -> Result<Value, String> + 'static,
) -> Result<Value, String> {
    app.ask_save(name, Bound::Document, move |app, path| export(app, &with_path(&p, "path", path)))
}

pub(super) fn export_png(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document")?;
    let page = crate::control::page_index(p, crate::canvas::current_page(app).unwrap_or(0), st.doc.page_count())?;
    let Some(path) = p.get("path").and_then(Value::as_str).map(str::to_string) else {
        // The page in view now is the one exported once the dialog answers.
        let name = format!("{}-p{}.png", st.doc.title, page + 1);
        let mut q = if p.is_object() { p.clone() } else { json!({}) };
        q["page"] = json!(page);
        return ask_then_export(app, &name, q, export_png);
    };
    let scale = p.get("scale").and_then(Value::as_f64).unwrap_or(2.0);
    let mut r = designcraft_render::Renderer::new();
    let img = r
        .render_page(
            &st.doc,
            &app.session.cache,
            page,
            scale,
            false,
            &designcraft_render::RenderOptions { printing_only: true, rich_black: app.session.prefs.rich_black_output, ..Default::default() },
        )
        .ok_or("no such page")?;
    let png = img.to_png();
    match app.services.write.as_mut() {
        Some(w) => w(&path, &png).map(|_| json!({"path": path, "width": img.width, "height": img.height})),
        None => Err("no writer".into()),
    }
}

/// File › Export IDML…: ask for a path, export through `file.exportIdml` and write the bytes with
/// the platform writer (a download on the web).
pub(super) fn export_idml(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document")?;
    let Some(path) = p.get("path").and_then(Value::as_str).map(str::to_string) else {
        let name = format!("{}.idml", st.doc.title);
        return ask_then_export(app, &name, p.clone(), export_idml);
    };
    let r = app.run("file.exportIdml", json!({}))?;
    let bytes = designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default());
    match app.services.write.as_mut() {
        Some(w) => w(&path, &bytes).map(|_| json!({"path": path, "bytes": bytes.len()})),
        None => Err("no writer".into()),
    }
}

/// Export EPUB / Text: ask for a path (default extension `ext`), run `cmd` without one and write
/// what it returns (`base64` or `text`) with the platform writer.
/// File › Export EPUB…: export through `file.exportEpub` (the first page as its cover) to `path`,
/// else to a file the user picks.
pub(crate) fn export_epub(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let mut q = if p.is_object() { p.clone() } else { json!({}) };
    if let Some(o) = q.as_object_mut() {
        o.entry("cover").or_insert(json!(true));
    }
    export_bytes(app, &q, "epub", "file.exportEpub")
}

pub(super) fn export_bytes(app: &mut DesignApp, p: &Value, ext: &str, cmd: &str) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document")?;
    let Some(path) = p.get("path").and_then(Value::as_str).map(str::to_string) else {
        let name = format!("{}.{ext}", st.doc.title);
        let (ext, cmd) = (ext.to_string(), cmd.to_string());
        return ask_then_export(app, &name, p.clone(), move |app, q| export_bytes(app, q, &ext, &cmd));
    };
    // The caller's options go to the engine command; the path is ours.
    let mut params = match p {
        Value::Object(o) => Value::Object(o.iter().filter(|(k, _)| k.as_str() != "path").map(|(k, v)| (k.clone(), v.clone())).collect()),
        _ => json!({}),
    };
    if cmd == "file.exportText" {
        let txt = path.to_ascii_lowercase().ends_with(".txt");
        params["format"] = json!(if txt { "txt" } else { "rtf" });
    }
    let r = app.run(cmd, params)?;
    let bytes = match r["text"].as_str() {
        Some(t) => t.as_bytes().to_vec(),
        None => designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default()),
    };
    match app.services.write.as_mut() {
        Some(w) => w(&path, &bytes).map(|_| json!({"path": path, "bytes": bytes.len()})),
        None => Err("no writer".into()),
    }
}

/// View › Separations Preview (`view.separations`): NadsPDF leaves print production out, so the
/// view stays off and says why.
pub(super) fn separations_view(app: &mut DesignApp, _p: &Value) -> Result<Value, String> {
    app.ui.separations = None;
    Err(crate::separations::UNAVAILABLE.into())
}

/// File › Export PDF…: ask for a path, export through `file.exportPdf` and write the bytes with the
/// platform writer (a download on the web).
pub(crate) fn export_pdf(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document")?;
    let Some(path) = p.get("path").and_then(Value::as_str).map(str::to_string) else {
        let name = format!("{}.pdf", st.doc.title);
        return ask_then_export(app, &name, p.clone(), export_pdf);
    };
    let mut params = if p.is_object() { p.clone() } else { json!({}) };
    if let Some(o) = params.as_object_mut() {
        o.remove("path");
        o.entry("bleed").or_insert(json!(true));
        o.entry("tagged").or_insert(json!(true));
        if !app.ui.flattener.is_empty() {
            o.entry("flatten").or_insert(json!(app.ui.flattener));
        }
    }
    let r = app.run("file.exportPdf", params)?;
    let bytes = designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default());
    match app.services.write.as_mut() {
        Some(w) => w(&path, &bytes).map(|_| json!({"path": path, "bytes": bytes.len(), "pages": r["pages"], "warnings": r["warnings"]})),
        None => Err("no writer".into()),
    }
}
