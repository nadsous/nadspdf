//! Bake the contributor credits into the binary as static Rust tables, and embed the craft-i18n
//! interface catalogs. Nothing is read at run time.
//!
//! - contributors/contributors.json: commit stats per GitHub username, with a mark for whose people.toml
//!   entry is verified.
//! - contributors/people.toml: the names people chose for themselves. A name is used only when the
//!   JSON says the entry was verified (`people_entry`).
//!
//! A missing or malformed file gives empty tables (or no names) and a warning.
//!
//! - The interface catalogs: NadsPDF's copy of craft-i18n in the repository's `i18n/` folder
//!   (`i18n/README.md`), or a craft-i18n checkout named by `CRAFT_I18N_DIR` (storytold/craft-i18n,
//!   at the SHA in `craft-i18n.rev`). `craft_i18n.rs` holds `CRAFT_I18N`, one (locale, shared
//!   catalog, app catalog) entry per locale that has a `pdfcraft.tsv`. The app catalog is
//!   `pdfcraft.tsv` with the locale's `nadspdf.tsv` merged over it: a NadsPDF row replaces the
//!   row with the same context and source text (craft-i18n would otherwise keep the first of two
//!   and report the second). A folder that isn't a checkout gives a warning and an English-only
//!   interface; with `CRAFT_I18N_REQUIRED=1` (CI and releases) that is an error instead. Recipe:
//!   craft-i18n's `docs/integration.md`.

use std::fmt::Write as _;

use serde_json::Value;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contributors");
    let json = read(&dir.join("contributors.json"), "the About window shows no contributors", |t| {
        serde_json::from_str::<Value>(t).map_err(|e| e.to_string())
    })
    .unwrap_or(Value::Null);
    let people = read(&dir.join("people.toml"), "contributors are shown by username only", |t| t.parse::<toml::Table>().map_err(|e| e.to_string()))
        .unwrap_or_default();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("credits.rs");
    if let Err(e) = std::fs::write(&out, generate(&json, &people)) {
        println!("cargo::warning=writing {}: {e}", out.display());
    }
    let out = out.with_file_name("craft_i18n.rs");
    if let Err(e) = std::fs::write(&out, craft_i18n()) {
        println!("cargo::error=writing {}: {e}", out.display());
    }
}

/// The catalog this app reads in each locale: `locales/<locale>/pdfcraft.tsv`.
const APP_CATALOG: &str = "pdfcraft.tsv";

/// `CRAFT_I18N` from `CRAFT_I18N_DIR`: the `Embedded` list the craft-i18n crate reads.
fn craft_i18n() -> String {
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_REQUIRED");
    let required = std::env::var("CRAFT_I18N_REQUIRED").is_ok_and(|v| v.trim() == "1");
    // NadsPDF's own copy of the catalogs (`i18n/`) unless `CRAFT_I18N_DIR` names another.
    // Not canonicalized: on Windows that gives `\\?\C:\…`, which `include_str!` can't read.
    let dir = match std::env::var_os("CRAFT_I18N_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("i18n"),
    };
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default());
    let entries = match locales(&dir.join("locales"), &out_dir.join("i18n")) {
        Ok(entries) => entries,
        Err(e) => {
            let level = if required { "error" } else { "warning" };
            println!("cargo::{level}=interface catalogs: {e}; the interface is English only");
            Vec::new()
        }
    };
    let mut o = String::from("/// (locale, shared catalog, app catalog), one entry per locale; `\"\"` for a missing file.\n");
    o.push_str("pub static CRAFT_I18N: &craft_i18n::Embedded = &[\n");
    for (code, shared, app) in entries {
        // `{:?}` escapes the path (Windows backslashes) as a Rust string literal.
        let shared = shared.map_or_else(|| "\"\"".to_string(), |p| format!("include_str!({p:?})"));
        let _ = writeln!(o, "    ({code:?}, {shared}, include_str!({app:?})),");
    }
    o.push_str("];\n");
    o
}

/// The catalog NadsPDF adds over `pdfcraft.tsv` in each locale.
const NADS_CATALOG: &str = "nadspdf.tsv";

/// One embedded locale: its code, its `common.tsv` if it has one, and its app catalog.
type Locale = (String, Option<std::path::PathBuf>, std::path::PathBuf);

/// Every locale under `dir` with NadsPDF's catalog, by code; a locale with a `nadspdf.tsv` gets
/// the merged catalog written under `merged`. An error when `dir` isn't a craft-i18n `locales/`
/// directory.
fn locales(dir: &std::path::Path, merged: &std::path::Path) -> Result<Vec<Locale>, String> {
    // Build scripts run in the crate's directory and `include_str!` resolves against the
    // generated file, so a relative path would mean two different places.
    if !dir.is_absolute() {
        return Err(format!("{} is not an absolute path", dir.display()));
    }
    // Before read_dir, so a checkout that appears later reruns the script.
    println!("cargo::rerun-if-changed={}", dir.display());
    let list = std::fs::read_dir(dir).map_err(|e| format!("{} is not a craft-i18n checkout ({e})", dir.display()))?;
    let mut out = Vec::new();
    for entry in list.flatten() {
        let path = entry.path();
        let Some(code) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
        let app = path.join(APP_CATALOG);
        if !app.is_file() {
            continue;
        }
        println!("cargo::rerun-if-changed={}", app.display());
        let shared = Some(path.join("common.tsv")).filter(|p| p.is_file());
        if let Some(p) = &shared {
            println!("cargo::rerun-if-changed={}", p.display());
        }
        let nads = path.join(NADS_CATALOG);
        let app = if nads.is_file() {
            println!("cargo::rerun-if-changed={}", nads.display());
            match merge_catalogs(&app, &nads, &merged.join(format!("{code}.tsv"))) {
                Ok(p) => p,
                Err(e) => {
                    println!("cargo::warning={}: {e}; NadsPDF's own {code} rows are left out", nads.display());
                    app
                }
            }
        } else {
            app
        };
        out.push((code, shared, app));
    }
    if out.is_empty() {
        return Err(format!("{} has no locale with a {APP_CATALOG}", dir.display()));
    }
    out.sort();
    Ok(out)
}

/// `app` with the rows of `nads` merged over it, written to `out`: an `app` row whose context and
/// source text (the first two columns) a `nads` row has is left out, then every `nads` row
/// follows. Comments and blank lines of `app` are kept, so its line numbers stay readable.
fn merge_catalogs(app: &std::path::Path, nads: &std::path::Path, out: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let read = |p: &std::path::Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let (app_text, nads_text) = (read(app)?, read(nads)?);
    fn key(line: &str) -> Option<(&str, &str)> {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let mut cols = line.split('\t');
        Some((cols.next()?, cols.next()?))
    }
    let nads_text = nads_text.strip_prefix('\u{feff}').unwrap_or(&nads_text);
    let replaced: std::collections::HashSet<(&str, &str)> = nads_text.split('\n').filter_map(key).collect();
    let mut merged = String::with_capacity(app_text.len().saturating_add(nads_text.len()));
    for line in app_text.split('\n') {
        if key(line).is_some_and(|k| replaced.contains(&k)) {
            continue;
        }
        merged.push_str(line);
        merged.push('\n');
    }
    merged.push_str("# NadsPDF's own rows (nadspdf.tsv)\n");
    merged.push_str(nads_text);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(out, merged).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(out.to_path_buf())
}

fn read<T>(path: &std::path::Path, fallback: &str, parse: impl Fn(&str) -> Result<T, String>) -> Option<T> {
    println!("cargo::rerun-if-changed={}", path.display());
    let r = std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| parse(&t));
    if let Err(e) = &r {
        println!("cargo::warning={}: {e}; {fallback}", path.display());
    }
    r.ok()
}

/// A non-empty string field of someone's people.toml entry, as a Rust `Option<&str>` literal.
fn name(v: Option<&str>) -> String {
    match v.map(str::trim).filter(|s| !s.is_empty()) {
        Some(x) => format!("Some({x:?})"),
        None => "None".into(),
    }
}

fn s(v: &Value, k: &str) -> String {
    format!("{:?}", v.get(k).and_then(Value::as_str).unwrap_or(""))
}

fn n(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

fn rows(json: &Value, k: &str) -> Vec<Value> {
    json.get(k).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn generate(json: &Value, people: &toml::Table) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "pub static TOTAL_COMMITS: u64 = {};", n(json, "commits"));
    let _ = writeln!(o, "pub static CONTRIBUTORS: &[Contributor] = &[");
    for c in rows(json, "contributors") {
        let login = c.get("login").and_then(Value::as_str).unwrap_or("");
        if login.is_empty() {
            continue;
        }
        // Names only from a verified, self-chosen people.toml entry.
        let entry = people.get("people").and_then(|p| p.get(login)).filter(|_| c.get("people_entry").and_then(Value::as_bool) == Some(true));
        let field = |k: &str| entry.and_then(|e| e.get(k)).and_then(toml::Value::as_str);
        let synced = entry.and_then(|e| e.get("sync_github_name")).and_then(toml::Value::as_bool) == Some(true);
        let display = field("display_name")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| if synced { c.get("github_name").and_then(Value::as_str) } else { None });
        let _ = writeln!(
            o,
            "    Contributor {{ login: {login:?}, display_name: {}, real_name: {}, prs: {}, commits: {}, lines_added: {}, lines_deleted: {}, binary_added: {}, binary_deleted: {}, first_commit: {}, last_commit: {} }},",
            name(display),
            name(field("real_name")),
            n(&c, "prs"),
            n(&c, "commits"),
            n(&c, "lines_added"),
            n(&c, "lines_deleted"),
            n(&c, "binary_added"),
            n(&c, "binary_deleted"),
            s(&c, "first_commit"),
            s(&c, "last_commit"),
        );
    }
    let _ = writeln!(o, "];");
    let _ = writeln!(o, "pub static MODELS: &[Model] = &[");
    for m in rows(json, "models") {
        let _ = writeln!(
            o,
            "    Model {{ company: {}, model: {}, version: {}, commits: {}, lines_added: {}, lines_deleted: {} }},",
            s(&m, "company"),
            s(&m, "model"),
            s(&m, "version"),
            n(&m, "commits"),
            n(&m, "lines_added"),
            n(&m, "lines_deleted"),
        );
    }
    let _ = writeln!(o, "];");
    o
}
