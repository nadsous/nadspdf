//! File Explorer's right-click menu (Windows): "Convert to PDF", "Combine" and "Open" entries on
//! the files NadsPDF reads, without taking over any file type's default app.
//!
//! The entries are static verbs under `Software\Classes\SystemFileAssociations\.<ext>\shell`,
//! which Windows shows whatever app is the type's default. The app writes them per user
//! (`HKEY_CURRENT_USER`, Preferences or `--register-shell`), the installer per machine under
//! the same key names (packaging/windows/pdfcraft.wxs); Windows merges both, the user's copy
//! winning, so a file never shows an entry twice. On Windows 11 they are under "Show more
//! options": the first-level menu only takes packaged apps' `IExplorerCommand` entries.
//!
//! Explorer starts one process per selected file for a static verb, whatever its
//! `MultiSelectModel`; the single-instance hand-off gathers them into one window, and for
//! `--combine` into one Combine files list. [`entries`] is the whole registration as data, so it
//! is tested without touching the registry; [`register`], [`unregister`] and [`status`] apply it.

/// Where every key [`entries`] names lives, under `HKEY_CURRENT_USER` or `HKEY_LOCAL_MACHINE`.
pub const CLASSES: &str = "Software\\Classes";

/// What each entry is called in the menu, in the user's language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Labels {
    /// On documents, images and text: open them as a new PDF.
    pub convert: String,
    /// On PDFs and convertible files: add them to one Combine files list.
    pub combine: String,
    /// On PDFs: open them.
    pub open: String,
}

/// The menu entries, by key name (`shell\<verb>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Convert,
    Combine,
    Open,
}

impl Verb {
    pub const ALL: [Verb; 3] = [Verb::Convert, Verb::Combine, Verb::Open];

    /// The verb's key name. Stable: the installer writes the same ones, and [`unregister`]
    /// finds earlier registrations by them.
    pub fn key(self) -> &'static str {
        match self {
            Verb::Convert => "NadsPDF.Convert",
            Verb::Combine => "NadsPDF.Combine",
            Verb::Open => "NadsPDF.Open",
        }
    }

    /// The extensions (lowercase, without the dot) whose menu shows the verb.
    pub fn extensions(self) -> Vec<&'static str> {
        match self {
            Verb::Convert => CONVERTIBLE.to_vec(),
            Verb::Combine => std::iter::once("pdf").chain(CONVERTIBLE).collect(),
            Verb::Open => vec!["pdf"],
        }
    }

    /// `Document` starts one launch per file and hides the entry beyond 15 files; `Player` keeps
    /// it for any number (still one launch per file for a command-line verb).
    fn multi_select(self) -> &'static str {
        match self {
            Verb::Combine => "Player",
            Verb::Convert | Verb::Open => "Document",
        }
    }

    fn label(self, labels: &Labels) -> &str {
        match self {
            Verb::Convert => &labels.convert,
            Verb::Combine => &labels.combine,
            Verb::Open => &labels.open,
        }
    }

    /// The command line, `%1` being the clicked file.
    fn command(self, exe: &str) -> String {
        match self {
            Verb::Combine => format!("\"{exe}\" --combine \"%1\""),
            Verb::Convert | Verb::Open => format!("\"{exe}\" \"%1\""),
        }
    }
}

/// The types the app converts to PDF as it opens them (`pdfcraft_engine::CONVERTIBLE`; a test in
/// the desktop app keeps the two in step).
pub const CONVERTIBLE: [&str; 20] = [
    "png", "jpg", "jpeg", "tif", "tiff", "gif", "bmp", "jp2", "j2k", "jpx", "txt", "text", "docx", "xlsx", "xlsm", "ods", "csv", "tsv", "html", "htm",
];

/// One registry value: `name` `None` is the key's default value. Every value is a string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Relative to [`CLASSES`].
    pub key: String,
    pub name: Option<&'static str>,
    pub value: String,
}

/// `SystemFileAssociations\.<ext>\shell\<verb>`, relative to [`CLASSES`].
pub fn verb_key(ext: &str, verb: Verb) -> String {
    format!("SystemFileAssociations\\.{ext}\\shell\\{}", verb.key())
}

/// Every verb key the registration owns, relative to [`CLASSES`]: what [`unregister`] removes.
pub fn verb_keys() -> Vec<String> {
    Verb::ALL.iter().flat_map(|&v| v.extensions().into_iter().map(move |ext| verb_key(ext, v))).collect()
}

/// The whole registration for the program at `exe` (an absolute path).
pub fn entries(exe: &str, labels: &Labels) -> Vec<Entry> {
    let mut out = Vec::new();
    for verb in Verb::ALL {
        for ext in verb.extensions() {
            let key = verb_key(ext, verb);
            let value = |name: Option<&'static str>, value: String| Entry { key: key.clone(), name, value };
            out.push(value(Some("MUIVerb"), verb.label(labels).to_string()));
            out.push(value(Some("Icon"), format!("\"{exe}\",0")));
            out.push(value(Some("MultiSelectModel"), verb.multi_select().to_string()));
            out.push(Entry { key: format!("{key}\\command"), name: None, value: verb.command(exe) });
        }
    }
    out
}

/// Whether the menu entries are there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Not registered.
    Off,
    /// Registered for this user (Preferences or `--register-shell`).
    User,
    /// Registered for every user by the installer: the app can't remove it.
    Machine,
    /// Not a Windows build.
    Unsupported,
}

/// A path the registration can name: absolute, and nothing that would break the quoted command.
fn check_exe(exe: &std::path::Path) -> Result<String, String> {
    let s = exe.to_str().ok_or_else(|| format!("{} isn't a path Windows can start", exe.display()))?;
    if !exe.is_absolute() || s.is_empty() || s.contains('"') {
        return Err(format!("{s} isn't an absolute path"));
    }
    Ok(s.to_string())
}

/// Add the entries for `exe` for the current user. Idempotent: earlier entries (from another
/// copy of the app, too) are replaced. On failure nothing half-written is left behind.
pub fn register(exe: &std::path::Path, labels: &Labels) -> Result<(), String> {
    let exe = check_exe(exe)?;
    imp::register(&entries(&exe, labels))
}

/// Remove this user's entries. Idempotent; the installer's per-machine ones stay.
pub fn unregister() -> Result<(), String> {
    imp::unregister()
}

/// What is registered now.
pub fn status() -> Status {
    imp::status()
}

#[cfg(windows)]
mod imp {
    use super::{CLASSES, Entry, Status, Verb, verb_key, verb_keys};
    use winsafe::{HKEY, RegistryValue, co};

    pub fn register(entries: &[Entry]) -> Result<(), String> {
        unregister()?;
        let written = entries.iter().try_for_each(|e| {
            let (key, _) = HKEY::CURRENT_USER
                .RegCreateKeyEx(&format!("{CLASSES}\\{}", e.key), None, co::REG_OPTION::NON_VOLATILE, co::KEY::WRITE, None)
                .map_err(|err| format!("HKEY_CURRENT_USER\\{CLASSES}\\{}: {err}", e.key))?;
            key.RegSetValueEx(e.name, RegistryValue::Sz(e.value.clone())).map_err(|err| format!("HKEY_CURRENT_USER\\{CLASSES}\\{}: {err}", e.key))
        });
        if written.is_err() {
            let _ = unregister();
        }
        written
    }

    pub fn unregister() -> Result<(), String> {
        for key in verb_keys() {
            match HKEY::CURRENT_USER.RegDeleteTree(Some(&format!("{CLASSES}\\{key}"))) {
                Ok(()) => {}
                Err(co::ERROR::FILE_NOT_FOUND | co::ERROR::PATH_NOT_FOUND) => {}
                Err(err) => return Err(format!("HKEY_CURRENT_USER\\{CLASSES}\\{key}: {err}")),
            }
            // `.<ext>\shell` and `.<ext>`, which registering may have created: removed once
            // nothing is left in them (anyone else's verb or value keeps them).
            let shell = key.rsplit_once('\\').map(|(parent, _)| parent);
            let ext = shell.and_then(|s| s.rsplit_once('\\')).map(|(parent, _)| parent);
            for parent in [shell, ext].into_iter().flatten() {
                remove_if_empty(&format!("{CLASSES}\\{parent}"));
            }
        }
        Ok(())
    }

    /// Delete `path` under `HKEY_CURRENT_USER` if it has neither subkeys nor values. Best effort.
    fn remove_if_empty(path: &str) {
        let Ok(key) = HKEY::CURRENT_USER.RegOpenKeyEx(Some(path), co::REG_OPTION::default(), co::KEY::READ) else { return };
        let (mut keys, mut values) = (0u32, 0u32);
        let counted = key.RegQueryInfoKey(None, Some(&mut keys), None, None, Some(&mut values), None, None, None, None).is_ok();
        drop(key);
        if counted && keys == 0 && values == 0 {
            let _ = HKEY::CURRENT_USER.RegDeleteKey(path);
        }
    }

    /// The Combine entry on PDFs stands for the whole registration.
    pub fn status() -> Status {
        let probe = format!("{CLASSES}\\{}\\command", verb_key("pdf", Verb::Combine));
        let has = |root: &HKEY| root.RegGetValue(Some(&probe), None, co::RRF::RT_REG_SZ).is_ok();
        if has(&HKEY::CURRENT_USER) {
            Status::User
        } else if has(&HKEY::LOCAL_MACHINE) {
            Status::Machine
        } else {
            Status::Off
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Entry, Status};

    pub fn register(_: &[Entry]) -> Result<(), String> {
        Err("the File Explorer menu is only on Windows".to_string())
    }

    pub fn unregister() -> Result<(), String> {
        Ok(())
    }

    pub fn status() -> Status {
        Status::Unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> Labels {
        Labels { convert: "Convertir en PDF avec NadsPDF".into(), combine: "Fusionner avec NadsPDF".into(), open: "Ouvrir avec NadsPDF".into() }
    }

    const EXE: &str = r"C:\Program Files\NadsPDF\nadspdf.exe";

    fn value<'a>(entries: &'a [Entry], key: &str, name: Option<&str>) -> Option<&'a str> {
        entries.iter().find(|e| e.key == key && e.name == name).map(|e| e.value.as_str())
    }

    #[test]
    fn convert_is_on_every_convertible_type_and_opens_the_file() {
        let entries = entries(EXE, &labels());
        for ext in ["docx", "xlsx", "xlsm", "ods", "csv", "tsv", "html", "htm", "txt", "png", "jpg", "jpeg"] {
            let key = format!(r"SystemFileAssociations\.{ext}\shell\NadsPDF.Convert");
            assert_eq!(value(&entries, &key, Some("MUIVerb")), Some("Convertir en PDF avec NadsPDF"), "{ext}");
            assert_eq!(value(&entries, &key, Some("Icon")), Some(r#""C:\Program Files\NadsPDF\nadspdf.exe",0"#));
            assert_eq!(value(&entries, &key, Some("MultiSelectModel")), Some("Document"));
            assert_eq!(value(&entries, &format!(r"{key}\command"), None), Some(r#""C:\Program Files\NadsPDF\nadspdf.exe" "%1""#));
        }
        assert!(value(&entries, r"SystemFileAssociations\.pdf\shell\NadsPDF.Convert", Some("MUIVerb")).is_none(), "a PDF is one already");
    }

    #[test]
    fn combine_is_on_pdfs_and_convertible_types_and_takes_any_number() {
        let entries = entries(EXE, &labels());
        for ext in ["pdf", "docx", "png", "csv"] {
            let key = format!(r"SystemFileAssociations\.{ext}\shell\NadsPDF.Combine");
            assert_eq!(value(&entries, &key, Some("MUIVerb")), Some("Fusionner avec NadsPDF"));
            assert_eq!(value(&entries, &key, Some("MultiSelectModel")), Some("Player"));
            assert_eq!(value(&entries, &format!(r"{key}\command"), None), Some(r#""C:\Program Files\NadsPDF\nadspdf.exe" --combine "%1""#));
        }
    }

    #[test]
    fn open_is_on_pdfs_only() {
        let entries = entries(EXE, &labels());
        let keys: Vec<&str> =
            entries.iter().filter(|e| e.key.contains("NadsPDF.Open") && e.name == Some("MUIVerb")).map(|e| e.key.as_str()).collect();
        assert_eq!(keys, [r"SystemFileAssociations\.pdf\shell\NadsPDF.Open"]);
        assert_eq!(
            value(&entries, r"SystemFileAssociations\.pdf\shell\NadsPDF.Open\command", None),
            Some(r#""C:\Program Files\NadsPDF\nadspdf.exe" "%1""#)
        );
    }

    #[test]
    fn unregister_owns_exactly_the_keys_register_writes() {
        let entries = entries(EXE, &labels());
        let keys = verb_keys();
        assert_eq!(keys.len(), 20 + 21 + 1);
        for e in &entries {
            assert!(keys.iter().any(|k| e.key == *k || e.key == format!(r"{k}\command")), "{} is removed", e.key);
            assert!(!e.key.contains(".."), "no key escapes its verb");
        }
        assert_eq!(entries.len(), keys.len() * 4, "three values and a command per verb");
    }

    /// The installer writes exactly these entries per machine, with its French labels.
    #[test]
    fn the_installer_writes_the_same_entries() {
        let wxs = include_str!("../../../packaging/windows/pdfcraft.wxs").replace("\r\n", "\n");
        let entries = entries("[#PdfcraftExe]", &labels());
        for e in &entries {
            let name = e.name.map(|n| format!(" Name=\"{n}\"")).unwrap_or_default();
            let row =
                format!(r#"<RegistryValue Root="HKLM" Key="{CLASSES}\{}"{name} Value="{}" Type="string""#, e.key, e.value.replace('"', "&quot;"));
            assert!(wxs.contains(&row), "pdfcraft.wxs lacks {row}");
        }
        assert_eq!(wxs.matches(r"\shell\NadsPDF.").count(), entries.len(), "and nothing more");
    }

    #[test]
    fn only_absolute_quotable_paths_are_registered() {
        assert!(check_exe(std::path::Path::new("nadspdf.exe")).is_err());
        assert!(check_exe(std::path::Path::new("")).is_err());
        let abs = std::env::temp_dir().join("nadspdf.exe");
        assert_eq!(check_exe(&abs), Ok(abs.to_string_lossy().into_owned()));
        let quoted = std::env::temp_dir().join("a\"b.exe");
        assert!(check_exe(&quoted).is_err());
    }
}
