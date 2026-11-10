//! `cargo xtask extensions`: builds NadsPDF's own extensions (`apps/nads-extensions`) in release
//! and lays each one out as an importable folder, `dist/extensions/<id>/` (manifest + program),
//! checked the way NadsPDF checks an extension before importing it.

use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use serde_json::json;

use crate::gates::{root, run_args, target_dir};

struct Extension {
    id: &'static str,
    name: &'static str,
    binary: &'static str,
    description: &'static str,
}

const EXTENSIONS: &[Extension] = &[
    Extension {
        id: "site.nads.factures",
        name: "Contrôle des factures",
        binary: "nads-factures",
        description: "Vérifie hors ligne l'IBAN, le BIC, le SIRET et le n° de TVA des factures que vous ouvrez, et signale un IBAN qui a changé depuis la dernière facture d'un fournisseur.",
    },
    Extension {
        id: "site.nads.echeances",
        name: "Échéances",
        binary: "nads-echeances",
        description: "Repère les dates à retenir d'un document (paiement, expiration, audience, date limite) et les ajoute à votre agenda.",
    },
];

/// Both read only the text of the documents you open and show message bars: no file path, no
/// web page.
const PERMISSIONS: [&str; 3] = ["document_events", "read_text", "banner"];

pub fn run(_: &[String]) -> anyhow::Result<()> {
    run_args(&["build", "--release", "-p", "nads-extensions", "--bins"])?;
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).context("Cargo.toml")?;
    let version = crate::version::read(&manifest).map_err(|e| anyhow!(e))?;
    let out = root().join("dist").join("extensions");
    for ext in EXTENSIONS {
        let dir = package(ext, &version, &out)?;
        let installed = pdfcraft_extensions::discovery::load(&dir).map_err(|e| anyhow!("{}: {e}", dir.display()))?;
        println!("{}  {}  fingerprint {}", ext.id, dir.display(), installed.fingerprint);
    }
    println!("\nImport each folder in NadsPDF: Home ▸ Extensions ▸ Import an extension…");
    Ok(())
}

fn package(ext: &Extension, version: &str, out: &Path) -> anyhow::Result<PathBuf> {
    let dir = out.join(ext.id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("clearing {}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let exe = format!("{}{}", ext.binary, std::env::consts::EXE_SUFFIX);
    let built = target_dir().join("release").join(&exe);
    std::fs::copy(&built, dir.join(&exe)).with_context(|| format!("copying {}", built.display()))?;
    let manifest = json!({
        "protocol": 1,
        "id": ext.id,
        "name": ext.name,
        "version": version,
        "description": ext.description,
        "executable": exe,
        "permissions": PERMISSIONS,
    });
    std::fs::write(dir.join("manifest.json"), serde_json::to_string_pretty(&manifest)? + "\n")?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every manifest this command writes passes NadsPDF's own manifest check.
    #[test]
    fn manifests_are_accepted() {
        for ext in EXTENSIONS {
            let manifest = json!({
                "protocol": 1, "id": ext.id, "name": ext.name, "version": "1.0.0",
                "description": ext.description, "executable": "x.exe", "permissions": PERMISSIONS,
            });
            let parsed = pdfcraft_extensions::Manifest::parse(manifest.to_string().as_bytes());
            assert!(parsed.is_ok(), "{}: {parsed:?}", ext.id);
        }
    }
}
