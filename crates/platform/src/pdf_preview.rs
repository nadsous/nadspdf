//! PDF previews in Outlook and File Explorer's preview pane (Windows).
//!
//! Outlook previews an attachment, and Explorer a selected file, through the preview handler
//! registered for its type (`.pdf\shellex\{8895b1c6-…}`). Windows ships one for PDFs with Edge,
//! the "Microsoft PDF Previewer", but leaves it unregistered for `.pdf` unless Edge is the default
//! PDF app, so with NadsPDF (or another reader without a previewer) Outlook says it can't preview
//! the file. [`ensure`] points `.pdf` at that previewer for the current user, which needs no
//! administrator, and only when no previewer is registered at all: one installed by another
//! reader is left alone.

/// The shell extension key that names a type's preview handler.
pub const PREVIEW_HANDLER: &str = "{8895b1c6-b41f-4c1c-a562-0d564250836f}";
/// Windows' "Microsoft PDF Previewer", installed with Edge.
pub const EDGE_PDF_PREVIEWER: &str = "{3A84F9C2-6164-485C-A7D9-4B27F8AC009E}";
/// Where [`ensure`] writes, under `HKEY_CURRENT_USER`.
pub const KEY: &str = "Software\\Classes\\.pdf\\shellex\\{8895b1c6-b41f-4c1c-a562-0d564250836f}";

/// What [`ensure`] found or did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// `.pdf` had no previewer; it now uses Windows' PDF previewer.
    Registered,
    /// A previewer was already registered (by us earlier, or by another reader).
    AlreadySet,
    /// Windows' PDF previewer isn't installed, so there is nothing to point at.
    NoPreviewer,
    /// Not Windows.
    Unsupported,
}

/// Give PDFs a preview in Outlook and Explorer, if they have none. Best effort and idempotent.
pub fn ensure() -> Result<Outcome, String> {
    imp::ensure()
}

#[cfg(windows)]
mod imp {
    use super::{EDGE_PDF_PREVIEWER, KEY, Outcome, PREVIEW_HANDLER};
    use winsafe::{HKEY, RegistryValue, co};

    fn has_value(root: &HKEY, path: &str) -> bool {
        root.RegGetValue(Some(path), None, co::RRF::RT_REG_SZ).is_ok_and(|v| matches!(v, RegistryValue::Sz(s) if !s.trim().is_empty()))
    }

    pub fn ensure() -> Result<Outcome, String> {
        // Any previewer for `.pdf`, the user's or the machine's, wins over ours.
        let machine = format!("Software\\Classes\\.pdf\\shellex\\{PREVIEW_HANDLER}");
        if has_value(&HKEY::CURRENT_USER, KEY) || has_value(&HKEY::LOCAL_MACHINE, &machine) {
            return Ok(Outcome::AlreadySet);
        }
        // The previewer's DLL must be there: an Edge removed by hand leaves a dangling CLSID.
        let server = format!("Software\\Classes\\CLSID\\{EDGE_PDF_PREVIEWER}\\InProcServer32");
        let dll = match HKEY::LOCAL_MACHINE.RegGetValue(Some(&server), None, co::RRF::RT_REG_SZ) {
            Ok(RegistryValue::Sz(path)) => path,
            _ => return Ok(Outcome::NoPreviewer),
        };
        if !std::path::Path::new(dll.trim_matches('"')).is_file() {
            return Ok(Outcome::NoPreviewer);
        }
        let (key, _) = HKEY::CURRENT_USER
            .RegCreateKeyEx(KEY, None, co::REG_OPTION::NON_VOLATILE, co::KEY::WRITE, None)
            .map_err(|e| format!("HKEY_CURRENT_USER\\{KEY}: {e}"))?;
        key.RegSetValueEx(None, RegistryValue::Sz(EDGE_PDF_PREVIEWER.to_string())).map_err(|e| format!("HKEY_CURRENT_USER\\{KEY}: {e}"))?;
        Ok(Outcome::Registered)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Outcome;

    pub fn ensure() -> Result<Outcome, String> {
        Ok(Outcome::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_the_pdf_preview_handler_slot() {
        assert!(KEY.ends_with(PREVIEW_HANDLER));
        assert!(KEY.starts_with("Software\\Classes\\.pdf\\shellex\\"));
    }
}
