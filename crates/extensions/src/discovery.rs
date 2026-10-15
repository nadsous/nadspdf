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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub dir: PathBuf,
    pub manifest: Manifest,
    /// The program, resolved inside `dir`.
    pub executable: PathBuf,
    /// SHA-256 of the manifest and the program, hex: what an approval is bound to.
    pub fingerprint: String,
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
    Ok(Installed { dir: dir.to_path_buf(), manifest, executable, fingerprint })
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
}
