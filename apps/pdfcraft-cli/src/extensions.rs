//! `pdfcraft-cli extensions list|check` (NadsPDF): what the desktop app would find in the
//! extensions folder, without starting anything (docs/extensions.md).

use std::path::{Path, PathBuf};

use crate::options::positional;
use crate::{CliError, OptionSpec, flag, stdout_line, validate_options};

const OPTIONS: &[OptionSpec] = &[OptionSpec { name: "--dir", takes_value: true }];

/// `extensions list` and `extensions check`: what the desktop app would find, without starting
/// anything (docs/extensions.md).
pub(super) fn run(args: &[String]) -> Result<(), CliError> {
    use pdfcraft_extensions::{Approvals, Entry, discovery, folders};
    validate_options("extensions", args, OPTIONS)?;
    let rest = positional(args, OPTIONS);
    let describe = |entry: &Entry, approvals: Option<&Approvals>| match entry {
        Entry::Ok(ext) => {
            let m = &ext.manifest;
            let mut v = serde_json::json!({
                "folder": ext.dir, "ok": true, "id": m.id, "name": m.name, "version": m.version,
                "description": m.description, "executable": ext.executable,
                "permissions": m.permissions.iter().map(|p| p.key()).collect::<Vec<_>>(),
                "open_url": m.open_url, "fingerprint": ext.fingerprint,
            });
            if let (Some(a), Some(obj)) = (approvals, v.as_object_mut()) {
                obj.insert("approved".into(), serde_json::Value::Bool(a.is_approved(ext)));
            }
            v
        }
        Entry::Broken { dir, error } => serde_json::json!({ "folder": dir, "ok": false, "error": error }),
    };
    let pretty = |v: &serde_json::Value| serde_json::to_string_pretty(v).unwrap_or_default();
    match rest.as_slice() {
        ["list"] => {
            let folder = match flag(args, "--dir") {
                Some(dir) => PathBuf::from(dir),
                None => folders::user_folder().ok_or("extensions: no per-user data folder; pass --dir")?,
            };
            let approvals_path = folders::approvals_file(&folder);
            let approvals = Approvals::load(&approvals_path);
            let entries: Vec<serde_json::Value> = discovery::discover(&folder).iter().map(|e| describe(e, Some(&approvals))).collect();
            let out = serde_json::json!({ "folder": folder, "approvals": approvals_path, "extensions": entries });
            stdout_line(format_args!("{}", pretty(&out)))
        }
        ["check", folder] => {
            if flag(args, "--dir").is_some() {
                return Err("extensions check: --dir is for list; name the extension folder itself".into());
            }
            let entry = match discovery::load(Path::new(folder)) {
                Ok(ext) => Entry::Ok(Box::new(ext)),
                Err(error) => return Err(format!("extensions check: {folder}: {error}").into()),
            };
            stdout_line(format_args!("{}", pretty(&describe(&entry, None))))
        }
        _ => Err("usage: pdfcraft-cli extensions list [--dir DIR] | pdfcraft-cli extensions check <folder>".into()),
    }
}
