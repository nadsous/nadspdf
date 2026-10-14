//! Vaults: named groups of files from anywhere on the computer, each with a colour and an icon
//! (like spaces in a browser). A vault only lists absolute paths: its files stay where they are,
//! and creating, editing, emptying or deleting a vault never moves, copies or deletes one.
//! A file may sit in several vaults.
//!
//! This module is the model: validation, editing and the settings format, with no egui. The Home
//! view and its dialogs are in `vaults_ui.rs`. Settings are untrusted input, so [`Vaults::restore`]
//! keeps only what it can validate and caps every count.

use std::time::SystemTime;

/// At most this many vaults.
pub const MAX_VAULTS: usize = 100;
/// At most this many files per vault.
pub const MAX_FILES: usize = 2000;
/// Longest vault name, in characters.
pub const MAX_NAME_CHARS: usize = 60;
/// Longest file path accepted, from a picker, a drop or settings.
const MAX_PATH_CHARS: usize = 4096;

/// The colours a vault can take (sRGB). The first is the nads red.
pub const COLORS: [[u8; 3]; 9] = [
    [0xEF, 0x3E, 0x36],
    [0xF5, 0x8A, 0x1F],
    [0xEA, 0xB3, 0x08],
    [0x22, 0xC5, 0x5E],
    [0x14, 0xB8, 0xA6],
    [0x3B, 0x82, 0xF6],
    [0x8B, 0x5C, 0xF6],
    [0xEC, 0x48, 0x99],
    [0x94, 0xA3, 0xB8],
];

/// The icons a vault can take (bundled Lucide icons).
pub const ICONS: [&str; 16] = [
    "folder",
    "file-text",
    "star",
    "bookmark",
    "lock",
    "house",
    "book-open",
    "tag",
    "signature",
    "globe",
    "camera",
    "clock",
    "paperclip",
    "sparkles",
    "user-round",
    "presentation",
];

/// A vault. `id` is stable while the app runs (not saved).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vault {
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
    pub icon: &'static str,
    /// Absolute paths, in the order they were added.
    pub files: Vec<String>,
}

/// Why a vault change was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultError {
    /// The name is empty after trimming.
    EmptyName,
    /// [`MAX_VAULTS`] reached.
    TooManyVaults,
    /// No vault has this id.
    NoSuchVault,
}

/// What adding files did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Added {
    pub added: usize,
    /// Already in the vault.
    pub duplicates: usize,
    /// Not an absolute path to a supported file.
    pub rejected: usize,
    /// Left out because the vault holds [`MAX_FILES`].
    pub over_limit: usize,
}

/// What a vault's file was like when last checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStat {
    pub exists: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// The vaults, in the user's order.
#[derive(Clone, Debug, Default)]
pub struct Vaults {
    pub list: Vec<Vault>,
    next_id: u64,
}

/// `name` trimmed and cut to [`MAX_NAME_CHARS`] (control characters dropped); `None` if empty.
pub fn clean_name(name: &str) -> Option<String> {
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    let name: String = name.trim().chars().take(MAX_NAME_CHARS).collect();
    let name = name.trim_end().to_string();
    (!name.is_empty()).then_some(name)
}

/// `path` as a vault file: trimmed and absolute; `None` if it can't be one.
pub fn clean_path(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() || path.chars().count() > MAX_PATH_CHARS || path.contains('\0') {
        return None;
    }
    std::path::Path::new(path).is_absolute().then(|| path.to_string())
}

/// Whether `path` names a file NadsPDF opens: a PDF, or a format it converts to PDF.
pub fn is_supported(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf") || pdfcraft_engine::CONVERTIBLE.iter().any(|c| c.eq_ignore_ascii_case(e)))
}

/// Whether two paths name the same file (Windows paths ignore case and separator style).
pub fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        let norm = |p: &str| p.replace('/', "\\").to_lowercase();
        norm(a) == norm(b)
    } else {
        a == b
    }
}

/// The colour as `#RRGGBB`.
pub fn color_hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// `#RRGGBB` (or `RRGGBB`) as a colour.
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// The bundled icon called `name`, or the folder icon.
pub fn icon_named(name: &str) -> &'static str {
    ICONS.iter().copied().find(|i| *i == name).unwrap_or("folder")
}

/// The file name of `path`, for rows and messages.
pub fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// Look `path` up on disk (follows links, like opening the file does).
pub fn stat(path: &str) -> FileStat {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => FileStat { exists: true, size: m.len(), modified: m.modified().ok() },
        _ => FileStat { exists: false, size: 0, modified: None },
    }
}

impl Vaults {
    pub fn get(&self, id: u64) -> Option<&Vault> {
        self.list.iter().find(|v| v.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Result<&mut Vault, VaultError> {
        self.list.iter_mut().find(|v| v.id == id).ok_or(VaultError::NoSuchVault)
    }

    /// Where the vault `id` is in the list.
    pub fn index(&self, id: u64) -> Option<usize> {
        self.list.iter().position(|v| v.id == id)
    }

    /// The first vault called `name` (ignoring case).
    pub fn find_by_name(&self, name: &str) -> Option<u64> {
        let name = name.trim().to_lowercase();
        self.list.iter().find(|v| v.name.to_lowercase() == name).map(|v| v.id)
    }

    /// Add a vault at the end and return its id.
    pub fn create(&mut self, name: &str, color: [u8; 3], icon: &str) -> Result<u64, VaultError> {
        let name = clean_name(name).ok_or(VaultError::EmptyName)?;
        if self.list.len() >= MAX_VAULTS {
            return Err(VaultError::TooManyVaults);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.list.push(Vault { id, name, color, icon: icon_named(icon), files: Vec::new() });
        Ok(id)
    }

    /// Change a vault's name, colour and icon.
    pub fn update(&mut self, id: u64, name: &str, color: [u8; 3], icon: &str) -> Result<(), VaultError> {
        let name = clean_name(name).ok_or(VaultError::EmptyName)?;
        let v = self.get_mut(id)?;
        v.name = name;
        v.color = color;
        v.icon = icon_named(icon);
        Ok(())
    }

    /// Remove a vault. Its files are not touched.
    pub fn delete(&mut self, id: u64) -> Option<Vault> {
        let i = self.index(id)?;
        Some(self.list.remove(i))
    }

    /// Move the vault `id` to position `to` (clamped to the list).
    pub fn move_to(&mut self, id: u64, to: usize) {
        let Some(from) = self.index(id) else { return };
        let v = self.list.remove(from);
        let to = to.min(self.list.len());
        self.list.insert(to, v);
    }

    /// Move the vault `id` one place earlier (`-1`) or later (`1`).
    pub fn shift(&mut self, id: u64, by: isize) {
        let Some(from) = self.index(id) else { return };
        let to = from.saturating_add_signed(by).min(self.list.len().saturating_sub(1));
        self.move_to(id, to);
    }

    /// Add `paths` to the vault `id`, skipping duplicates, unsupported files and anything past
    /// [`MAX_FILES`].
    pub fn add_files<S: AsRef<str>>(&mut self, id: u64, paths: &[S]) -> Result<Added, VaultError> {
        let v = self.get_mut(id)?;
        let mut out = Added::default();
        for p in paths {
            let Some(p) = clean_path(p.as_ref()).filter(|p| is_supported(p)) else {
                out.rejected += 1;
                continue;
            };
            if v.files.iter().any(|f| same_path(f, &p)) {
                out.duplicates += 1;
            } else if v.files.len() >= MAX_FILES {
                out.over_limit += 1;
            } else {
                v.files.push(p);
                out.added += 1;
            }
        }
        Ok(out)
    }

    /// Take `path` out of the vault `id`. The file is not touched.
    pub fn remove_file(&mut self, id: u64, path: &str) -> bool {
        let Ok(v) = self.get_mut(id) else { return false };
        let before = v.files.len();
        v.files.retain(|f| !same_path(f, path));
        v.files.len() != before
    }

    /// Point the vault's entry for `old` at `new` instead (a moved file found again), in place.
    pub fn relocate(&mut self, id: u64, old: &str, new: &str) -> bool {
        let Some(new) = clean_path(new) else { return false };
        let Ok(v) = self.get_mut(id) else { return false };
        if v.files.iter().any(|f| same_path(f, &new)) {
            v.files.retain(|f| !same_path(f, old));
            return true;
        }
        match v.files.iter_mut().find(|f| same_path(f, old)) {
            Some(f) => {
                *f = new;
                true
            }
            None => false,
        }
    }

    /// Move `path` from the vault `from` into the vault `to` (separating it from the first).
    /// Returns false, changing nothing, when it isn't in `from` or `to` is full.
    pub fn move_file(&mut self, from: u64, to: u64, path: &str) -> bool {
        if from == to || self.get(to).is_none() || !self.get(from).is_some_and(|v| v.files.iter().any(|f| same_path(f, path))) {
            return false;
        }
        match self.add_files(to, &[path]) {
            Ok(a) if a.added == 1 || a.duplicates == 1 => self.remove_file(from, path),
            _ => false,
        }
    }

    /// The vaults as settings JSON.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.list.iter().map(|v| serde_json::json!({ "name": v.name, "color": color_hex(v.color), "icon": v.icon, "files": v.files })).collect(),
        )
    }

    /// Restore vaults from settings. Anything malformed is skipped: a vault without a usable
    /// name, an unknown colour or icon (defaults instead), relative or duplicate paths, and
    /// counts past the caps. A file missing right now is kept (its drive may be offline).
    pub fn restore(&mut self, value: &serde_json::Value) {
        let Some(items) = value.as_array() else { return };
        let mut out = Vaults::default();
        for item in items {
            if out.list.len() >= MAX_VAULTS {
                break;
            }
            let Some(name) = item.get("name").and_then(serde_json::Value::as_str) else { continue };
            let color = item.get("color").and_then(serde_json::Value::as_str).and_then(parse_color).unwrap_or(COLORS[0]);
            let icon = item.get("icon").and_then(serde_json::Value::as_str).unwrap_or("folder");
            let Ok(id) = out.create(name, color, icon) else { continue };
            if let Some(files) = item.get("files").and_then(serde_json::Value::as_array) {
                let paths: Vec<&str> = files.iter().filter_map(serde_json::Value::as_str).take(MAX_FILES.saturating_mul(2)).collect();
                let _ = out.add_files(id, &paths);
            }
        }
        *self = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> &'static str {
        if cfg!(windows) { "C:\\" } else { "/" }
    }

    fn p(name: &str) -> String {
        format!("{}{name}", root())
    }

    #[test]
    fn names_are_trimmed_capped_and_required() {
        assert_eq!(clean_name("  Work \n"), Some("Work".into()));
        assert_eq!(clean_name("   "), None);
        assert_eq!(clean_name(&"é".repeat(200)).map(|n| n.chars().count()), Some(MAX_NAME_CHARS));
        let mut v = Vaults::default();
        assert_eq!(v.create(" ", COLORS[0], "folder"), Err(VaultError::EmptyName));
        for i in 0..MAX_VAULTS {
            v.create(&format!("v{i}"), COLORS[1], "star").unwrap();
        }
        assert_eq!(v.create("one more", COLORS[0], "folder"), Err(VaultError::TooManyVaults));
    }

    #[test]
    fn files_are_absolute_supported_unique_and_capped() {
        let mut v = Vaults::default();
        let id = v.create("Work", COLORS[0], "nonexistent-icon").unwrap();
        assert_eq!(v.get(id).unwrap().icon, "folder", "unknown icons fall back");
        let a = p("a.pdf");
        let added = v.add_files(id, &[a.as_str(), a.as_str(), "relative.pdf", &p("notes.exe"), &p("sheet.XLSX"), "", "x\0y"]).unwrap();
        assert_eq!(added, Added { added: 2, duplicates: 1, rejected: 4, over_limit: 0 });
        let many: Vec<String> = (0..MAX_FILES + 5).map(|i| p(&format!("f{i}.pdf"))).collect();
        let added = v.add_files(id, &many).unwrap();
        assert_eq!(v.get(id).unwrap().files.len(), MAX_FILES);
        assert_eq!(added.over_limit, 7);
        assert_eq!(v.add_files(999, &[a.as_str()]), Err(VaultError::NoSuchVault));
    }

    #[test]
    fn reorder_move_relocate_and_delete() {
        let mut v = Vaults::default();
        let a = v.create("A", COLORS[0], "folder").unwrap();
        let b = v.create("B", COLORS[1], "star").unwrap();
        let c = v.create("C", COLORS[2], "lock").unwrap();
        v.move_to(c, 0);
        assert_eq!(v.list.iter().map(|x| x.id).collect::<Vec<_>>(), [c, a, b]);
        v.shift(c, 1);
        v.shift(b, 5);
        v.shift(a, -9);
        assert_eq!(v.list.iter().map(|x| x.id).collect::<Vec<_>>(), [a, c, b]);
        let f = p("contract.pdf");
        v.add_files(a, &[f.as_str()]).unwrap();
        assert!(v.move_file(a, b, &f));
        assert!(v.get(a).unwrap().files.is_empty());
        assert_eq!(v.get(b).unwrap().files, std::slice::from_ref(&f));
        assert!(!v.move_file(a, b, &f), "it is no longer in A");
        assert!(!v.move_file(b, b, &f));
        let moved = p("moved.pdf");
        assert!(v.relocate(b, &f, &moved));
        assert_eq!(v.get(b).unwrap().files, std::slice::from_ref(&moved));
        assert!(v.remove_file(b, &moved));
        assert!(v.delete(b).is_some());
        assert!(v.get(b).is_none());
        assert!(v.update(c, "Renamed", COLORS[5], "globe").is_ok());
        assert_eq!(v.get(c).map(|x| (x.name.as_str(), x.color, x.icon)), Some(("Renamed", COLORS[5], "globe")));
    }

    #[test]
    fn settings_round_trip_and_garbage_is_ignored() {
        let mut v = Vaults::default();
        let a = v.create("Taxes", [1, 2, 3], "lock").unwrap();
        v.add_files(a, &[p("t.pdf")]).unwrap();
        let mut back = Vaults::default();
        back.restore(&v.to_json());
        assert_eq!(back.list.len(), 1);
        assert_eq!(back.list[0].name, "Taxes");
        assert_eq!(back.list[0].color, [1, 2, 3]);
        assert_eq!(back.list[0].icon, "lock");
        assert_eq!(back.list[0].files, [p("t.pdf")]);
        let garbage = serde_json::json!([
            5,
            {"name": ""},
            {"name": "ok", "color": "#zzzzzz", "icon": 7, "files": [1, "rel.pdf", p("x.pdf"), p("x.pdf")]},
            {"name": "é", "color": "#ÉÉÉ"},
        ]);
        back.restore(&garbage);
        assert_eq!(back.list.len(), 2);
        assert_eq!(back.list[0].color, COLORS[0]);
        assert_eq!(back.list[0].icon, "folder");
        assert_eq!(back.list[0].files, [p("x.pdf")]);
        back.restore(&serde_json::json!("nope"));
        assert_eq!(back.list.len(), 2, "a non-list keeps what is there");
    }

    #[test]
    fn colours_parse_only_well_formed_hex() {
        assert_eq!(parse_color("#EF3E36"), Some([0xEF, 0x3E, 0x36]));
        assert_eq!(parse_color("ef3e36"), Some([0xEF, 0x3E, 0x36]));
        assert_eq!(parse_color("#EF3E3"), None);
        assert_eq!(parse_color("#ÉF3E3"), None);
        assert_eq!(color_hex([0xEF, 0x3E, 0x36]), "#EF3E36");
    }

    #[test]
    fn missing_files_stat_as_missing() {
        let missing = std::env::temp_dir().join(format!("pdfcraft-no-such-vault-file-{}.pdf", std::process::id()));
        assert!(!stat(&missing.to_string_lossy()).exists);
    }
}
