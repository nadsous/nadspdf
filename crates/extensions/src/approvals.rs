//! What the user allowed: one approval per extension id, bound to the fingerprint of its
//! manifest and program. A new program, or a manifest asking for something else, no longer
//! matches, so the extension stays off until the user approves it again.
//!
//! The file is read leniently: a missing or corrupt file means nothing is approved (which is
//! always safe), never an error that keeps the app from starting.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::discovery::Installed;
use crate::manifest::Permission;

/// Approvals beyond this many are dropped on load (a hand-edited or hostile file).
const MAX_APPROVALS: usize = 256;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approvals {
    #[serde(default)]
    pub approved: BTreeMap<String, Approval>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub fingerprint: String,
    pub version: String,
    pub permissions: Vec<Permission>,
}

impl Approvals {
    pub fn load(path: &Path) -> Approvals {
        let Ok(bytes) = std::fs::read(path) else { return Approvals::default() };
        match serde_json::from_slice::<Approvals>(&bytes) {
            Ok(mut a) => {
                while a.approved.len() > MAX_APPROVALS {
                    a.approved.pop_last();
                }
                a
            }
            Err(e) => {
                log::warn!("extension approvals {} ignored: {e}", path.display());
                Approvals::default()
            }
        }
    }

    /// Write atomically (a temporary file beside it, then a rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    pub fn is_approved(&self, ext: &Installed) -> bool {
        self.approved.get(&ext.manifest.id).is_some_and(|a| a.fingerprint == ext.fingerprint && a.permissions == ext.manifest.permissions)
    }

    pub fn approve(&mut self, ext: &Installed) {
        self.approved.insert(
            ext.manifest.id.clone(),
            Approval { fingerprint: ext.fingerprint.clone(), version: ext.manifest.version.clone(), permissions: ext.manifest.permissions.clone() },
        );
    }

    pub fn revoke(&mut self, id: &str) -> bool {
        self.approved.remove(id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;

    fn installed(fingerprint: &str, permissions: &str) -> Installed {
        let text = format!(r#"{{"protocol":1,"id":"com.a","name":"A","version":"1","executable":"a.exe","permissions":[{permissions}]}}"#);
        Installed {
            dir: "x".into(),
            manifest: Manifest::parse(text.as_bytes()).unwrap(),
            executable: "x/a.exe".into(),
            fingerprint: fingerprint.into(),
        }
    }

    #[test]
    fn an_approval_is_bound_to_the_fingerprint() {
        let mut a = Approvals::default();
        let ext = installed("aaa", r#""banner""#);
        assert!(!a.is_approved(&ext));
        a.approve(&ext);
        assert!(a.is_approved(&ext));
        assert!(!a.is_approved(&installed("bbb", r#""banner""#)));
        assert!(!a.is_approved(&installed("aaa", r#""banner","read_text""#)));
        assert!(a.revoke("com.a"));
        assert!(!a.is_approved(&ext));
    }

    #[test]
    fn a_corrupt_or_missing_file_approves_nothing() {
        let dir = std::env::temp_dir().join(format!("nadspdf-approvals-{}", std::process::id()));
        let path = dir.join("approvals.json");
        assert_eq!(Approvals::load(&path), Approvals::default());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(Approvals::load(&path), Approvals::default());
        let mut a = Approvals::default();
        a.approve(&installed("aaa", ""));
        a.save(&path).unwrap();
        assert_eq!(Approvals::load(&path), a);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
