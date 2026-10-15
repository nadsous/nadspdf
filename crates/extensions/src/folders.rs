//! Where extensions are installed, shared by the desktop app and `pdfcraft-cli extensions`.

use std::path::{Path, PathBuf};

/// The approvals file, beside the extensions folder.
pub const APPROVALS_FILE: &str = "extension-approvals.json";

/// The per-user extensions folder: `%LOCALAPPDATA%\NadsPDF\Extensions` (Windows),
/// `~/Library/Application Support/NadsPDF/Extensions` (macOS) or `$XDG_DATA_HOME/pdfcraft/extensions`
/// (others, default `~/.local/share`). A portable copy uses its own data folder instead; the app
/// decides that (`ui-egui`'s `extensions_ui::default_folder`).
pub fn user_folder() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support/NadsPDF/Extensions"))
    } else if cfg!(windows) {
        env("LOCALAPPDATA").map(|d| d.join("NadsPDF").join("Extensions"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share"))).map(|d| d.join("pdfcraft/extensions"))
    }
}

/// The approvals kept for the extensions in `folder`.
pub fn approvals_file(folder: &Path) -> PathBuf {
    folder.with_file_name(APPROVALS_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approvals_sit_beside_the_folder() {
        let folder = Path::new("data").join("NadsPDF").join("Extensions");
        assert_eq!(approvals_file(&folder), Path::new("data").join("NadsPDF").join(APPROVALS_FILE));
    }
}
