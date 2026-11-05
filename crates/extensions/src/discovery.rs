//! The extensions installed in a folder: one subfolder each, holding `manifest.json`.
//!
//! A folder that can't be used (no manifest, a refused manifest, a program outside the folder,
//! an id already taken) is listed as [`Entry::Broken`] with the reason, so the Preferences can
//! say why an extension doesn't show up instead of ignoring it.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::manifest::{MANIFEST_FILE, MAX_MANIFEST_BYTES, Manifest};

/// More extension folders than this are not read (the rest are listed as broken).
pub const MAX_EXTENSIONS: usize = 64;
/// A program larger than this is not fingerprinted, so it can't be approved.
pub const MAX_PROGRAM_BYTES: u64 = 1024 * 1024 * 1024;
/// An extension's picture larger than this is left out (its card shows the default one).
pub const MAX_ICON_BYTES: u64 = 256 * 1024;
/// What [`import`] copies at most: files, bytes and folder depth.
pub const MAX_IMPORT_FILES: usize = 4000;
pub const MAX_IMPORT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_IMPORT_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub dir: PathBuf,
    pub manifest: Manifest,
    /// The program, resolved inside `dir`.
    pub executable: PathBuf,
    /// SHA-256 of the manifest and the program, hex: what an approval is bound to.
    pub fingerprint: String,
    /// The manifest's `icon`, as PNG bytes; `None` when it has none, or it can't be used (missing,
    /// outside the folder, too large or not a PNG): the card shows the default picture then.
    pub icon: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Ok(Box<Installed>),
    Broken { dir: PathBuf, error: String },
}

impl Entry {
    pub fn dir(&self) -> &Path {
        match self {
            Entry::Ok(i) => &i.dir,
            Entry::Broken { dir, .. } => dir,
        }
    }
}

/// The extensions under `root`, in folder-name order. A missing `root` means none.
pub fn discover(root: &Path) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(root) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = read.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
    dirs.sort();
    let mut out: Vec<Entry> = Vec::new();
    for (n, dir) in dirs.into_iter().enumerate() {
        if n >= MAX_EXTENSIONS {
            out.push(Entry::Broken { dir, error: format!("more than {MAX_EXTENSIONS} extensions are installed") });
            continue;
        }
        let entry = match load(&dir) {
            Ok(installed) if out.iter().any(|e| matches!(e, Entry::Ok(o) if o.manifest.id == installed.manifest.id)) => {
                Entry::Broken { error: format!("another extension already uses the id {}", installed.manifest.id), dir }
            }
            Ok(installed) => Entry::Ok(Box::new(installed)),
            Err(error) => Entry::Broken { dir, error },
        };
        out.push(entry);
    }
    out
}

/// Read and check one extension folder.
pub fn load(dir: &Path) -> Result<Installed, String> {
    let manifest_path = dir.join(MANIFEST_FILE);
    let bytes = read_capped(&manifest_path, MAX_MANIFEST_BYTES).map_err(|e| format!("{MANIFEST_FILE}: {e}"))?;
    let manifest = Manifest::parse(&bytes)?;
    let root = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
    let executable = dir.join(&manifest.executable).canonicalize().map_err(|e| format!("the program {} can't be found: {e}", manifest.executable))?;
    // Symlinks and junctions are resolved: the real file must still be inside the folder.
    if !executable.starts_with(&root) {
        return Err(format!("the program {} is outside the extension folder", manifest.executable));
    }
    if !executable.is_file() {
        return Err(format!("the program {} is not a file", manifest.executable));
    }
    let fingerprint = fingerprint(&bytes, &executable)?;
    let icon = manifest.icon.as_deref().and_then(|icon| read_icon(&root, &dir.join(icon)));
    Ok(Installed { dir: dir.to_path_buf(), manifest, executable, fingerprint, icon })
}

/// The PNG at `path`, when it is a file inside `root`, at most [`MAX_ICON_BYTES`] and a PNG.
fn read_icon(root: &Path, path: &Path) -> Option<Vec<u8>> {
    let real = path.canonicalize().ok()?;
    if !real.starts_with(root) || !real.is_file() {
        return None;
    }
    let bytes = read_capped(&real, MAX_ICON_BYTES).ok()?;
    bytes.starts_with(b"\x89PNG\r\n\x1a\n").then_some(bytes)
}

/// Install the extension in folder `src` into the extensions folder `root`, as `root/<id>`:
/// it is checked first (as [`load`] does), then copied, replacing an earlier copy of the same
/// extension. Symbolic links are not followed, and the copy stops past [`MAX_IMPORT_FILES`],
/// [`MAX_IMPORT_BYTES`] or 16 nested folders. An updated program has a new fingerprint, so it
/// must be turned on again.
pub fn import(src: &Path, root: &Path) -> Result<Installed, String> {
    let checked = load(src)?;
    let id = checked.manifest.id.clone();
    std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let real_root = root.canonicalize().map_err(|e| format!("{}: {e}", root.display()))?;
    let real_src = src.canonicalize().map_err(|e| format!("{}: {e}", src.display()))?;
    if real_src.starts_with(&real_root) {
        return Err("this extension is already in the extensions folder".into());
    }
    // Copied beside the target first, so a failed copy leaves the installed one as it was.
    let staging = real_root.join(format!(".import-{id}"));
    let _ = std::fs::remove_dir_all(&staging);
    let mut budget = (MAX_IMPORT_FILES, MAX_IMPORT_BYTES);
    if let Err(e) = copy_tree(&real_src, &staging, 0, &mut budget) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let target = real_root.join(&id);
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|e| format!("the installed copy can't be replaced: {e}"))?;
    }
    std::fs::rename(&staging, &target).map_err(|e| {
        let _ = std::fs::remove_dir_all(&staging);
        format!("the extension can't be put in place: {e}")
    })?;
    load(&target)
}

/// Copy folder `from` to `to` within `budget` (files, bytes left), skipping symbolic links.
fn copy_tree(from: &Path, to: &Path, depth: usize, budget: &mut (usize, u64)) -> Result<(), String> {
    if depth > MAX_IMPORT_DEPTH {
        return Err(format!("the extension has folders nested more than {MAX_IMPORT_DEPTH} deep"));
    }
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let entries = std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        let dest = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &dest, depth + 1, budget)?;
        } else if kind.is_file() {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            budget.0 = budget.0.checked_sub(1).ok_or(format!("the extension has more than {MAX_IMPORT_FILES} files"))?;
            budget.1 = budget.1.checked_sub(size).ok_or("the extension is larger than 2 GiB".to_string())?;
            std::fs::copy(entry.path(), &dest).map_err(|e| format!("{}: {e}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn read_capped(path: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("larger than {max} bytes")));
    }
    Ok(bytes)
}

fn fingerprint(manifest: &[u8], program: &Path) -> Result<String, String> {
    let mut hash = Sha256::new();
    hash.update((manifest.len() as u64).to_le_bytes());
    hash.update(manifest);
    let file = std::fs::File::open(program).map_err(|e| format!("the program can't be read: {e}"))?;
    let mut reader = file.take(MAX_PROGRAM_BYTES.saturating_add(1));
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("the program can't be read: {e}"))?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n as u64);
        if total > MAX_PROGRAM_BYTES {
            return Err(format!("the program is larger than {} MiB", MAX_PROGRAM_BYTES / (1024 * 1024)));
        }
        hash.update(buf.get(..n).unwrap_or_default());
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nadspdf-ext-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn install(root: &Path, folder: &str, id: &str, exe: &str) -> PathBuf {
        let dir = root.join(folder);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin/ext.exe"), b"program").unwrap();
        let manifest = format!(r#"{{"protocol":1,"id":"{id}","name":"Ext","version":"1","executable":"{exe}"}}"#);
        std::fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
        dir
    }

    #[test]
    fn missing_root_lists_nothing() {
        assert!(discover(Path::new("/definitely/not/here/nadspdf")).is_empty());
    }

    #[test]
    fn lists_good_and_broken_folders() {
        let root = scratch("list");
        install(&root, "a", "com.a", "bin/ext.exe");
        install(&root, "b", "com.b", "bin/missing.exe");
        std::fs::create_dir_all(root.join("c")).unwrap();
        std::fs::write(root.join("not-a-folder.txt"), b"x").unwrap();
        let entries = discover(&root);
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[0], Entry::Ok(i) if i.manifest.id == "com.a" && i.fingerprint.len() == 64));
        assert!(matches!(&entries[1], Entry::Broken { error, .. } if error.contains("can't be found")));
        assert!(matches!(&entries[2], Entry::Broken { error, .. } if error.contains(MANIFEST_FILE)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_duplicate_id_is_broken() {
        let root = scratch("dup");
        install(&root, "a", "com.same", "bin/ext.exe");
        install(&root, "b", "com.same", "bin/ext.exe");
        let entries = discover(&root);
        assert!(matches!(&entries[0], Entry::Ok(_)));
        assert!(matches!(&entries[1], Entry::Broken { error, .. } if error.contains("already uses")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_fingerprint_follows_the_program() {
        let root = scratch("fp");
        let dir = install(&root, "a", "com.a", "bin/ext.exe");
        let before = load(&dir).unwrap().fingerprint;
        std::fs::write(dir.join("bin/ext.exe"), b"another program").unwrap();
        let after = load(&dir).unwrap().fingerprint;
        assert_ne!(before, after);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A manifest's picture is read when it is a PNG inside the folder; anything else is left
    /// out without making the extension unusable.
    #[test]
    fn an_icon_is_read_only_when_it_is_a_png_inside_the_folder() {
        let root = scratch("icon");
        let png = b"\x89PNG\r\n\x1a\nrest".to_vec();
        for (folder, icon, bytes, kept) in
            [("good", "icon.png", png.clone(), true), ("fake", "icon.png", b"GIF89a".to_vec(), false), ("missing", "nope.png", Vec::new(), false)]
        {
            let dir = root.join(folder);
            std::fs::create_dir_all(dir.join("bin")).unwrap();
            std::fs::write(dir.join("bin/ext.exe"), b"program").unwrap();
            if !bytes.is_empty() {
                std::fs::write(dir.join("icon.png"), &bytes).unwrap();
            }
            let manifest = format!(r#"{{"protocol":1,"id":"com.{folder}","name":"Ext","version":"1","executable":"bin/ext.exe","icon":"{icon}"}}"#);
            std::fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
            let loaded = load(&dir).unwrap();
            assert_eq!(loaded.icon.is_some(), kept, "{folder}");
        }
        // A path out of the folder is refused by the manifest itself.
        let bad = r#"{"protocol":1,"id":"com.x","name":"X","version":"1","executable":"x.exe","icon":"../icon.png"}"#;
        assert!(Manifest::parse(bad.as_bytes()).is_err());
        let not_png = r#"{"protocol":1,"id":"com.x","name":"X","version":"1","executable":"x.exe","icon":"icon.svg"}"#;
        assert!(Manifest::parse(not_png.as_bytes()).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Importing copies a checked extension into the folder under its id, replaces an earlier
    /// copy, and refuses a folder that is not an extension or is already in the folder.
    #[test]
    fn import_copies_a_checked_extension_under_its_id_and_replaces_an_older_copy() {
        let work = scratch("import");
        let (src, root) = (work.join("downloads"), work.join("extensions"));
        let ext = install(&src, "my-ext-v1", "com.example.erp", "bin/ext.exe");
        let installed = import(&ext, &root).unwrap();
        assert_eq!(installed.dir, root.canonicalize().unwrap().join("com.example.erp"));
        assert_eq!(discover(&root).len(), 1);
        // A newer program replaces the installed copy, with a new fingerprint.
        std::fs::write(ext.join("bin/ext.exe"), b"program v2").unwrap();
        let again = import(&ext, &root).unwrap();
        assert_ne!(again.fingerprint, installed.fingerprint);
        assert_eq!(std::fs::read(again.dir.join("bin/ext.exe")).unwrap(), b"program v2");
        assert_eq!(discover(&root).len(), 1, "replaced, not added");
        // Not an extension: nothing is copied.
        let empty = src.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(import(&empty, &root).is_err());
        assert!(!root.join(".import-com.example.erp").exists());
        // Already installed: importing it from there is refused.
        assert!(import(&again.dir, &root).is_err());
        let _ = std::fs::remove_dir_all(&work);
    }
}
