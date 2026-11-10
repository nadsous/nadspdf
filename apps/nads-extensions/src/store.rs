//! Where an extension keeps its own files, outside its program folder (importing a new version
//! replaces that folder): `<NadsPDF data>/ExtensionData/<id>/`. NadsPDF starts an extension in its
//! folder, `<NadsPDF data>/Extensions/<id>/`, so the data folder is found from there.

use std::path::{Path, PathBuf};

/// The data folder of the extension `id`, created if needed. Falls back to the temporary folder
/// when the extension doesn't run from an extensions folder (a test, a manual run).
pub fn data_dir(id: &str) -> PathBuf {
    let from_cwd = std::env::current_dir().ok().and_then(|cwd| cwd.parent().and_then(Path::parent).map(|data| data.join("ExtensionData").join(id)));
    let dir = from_cwd.unwrap_or_else(|| std::env::temp_dir().join("NadsPDF-ExtensionData").join(id));
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create {}: {err}", dir.display());
    }
    dir
}

/// Reads `path` when it is at most `max` bytes; `None` when missing, unreadable or too big.
pub fn read_capped(path: &Path, max: u64) -> Option<Vec<u8>> {
    let len = std::fs::metadata(path).ok()?.len();
    if len > max {
        eprintln!("{} is larger than {max} bytes: ignored", path.display());
        return None;
    }
    std::fs::read(path).ok()
}

/// Writes `bytes` to `path` through a temporary file, so a crash never leaves half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
