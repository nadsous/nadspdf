//! Contrôle des factures: the extension program. See `nads_extensions::factures` for the checks.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable, clippy::indexing_slicing)]

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;

use nads_extensions::factures::{History, Known, Report, analyze, short_iban, spaced_id};
use nads_extensions::rpc::{self, Button, Extension, Out};
use nads_extensions::store;

const ID: &str = "site.nads.factures";
const BANNER: &str = "factures";
const KNOWN_FILE: &str = "iban-connus.json";
const MAX_KNOWN_BYTES: u64 = 4 * 1024 * 1024;

const REMEMBER: Button = Button { id: "remember", label: "Mémoriser cet IBAN", primary: true };
const TRUST: Button = Button { id: "trust", label: "C'est le bon IBAN", primary: false };

struct Factures {
    known_path: PathBuf,
    known: Known,
    /// Each open document's IBAN to pay and SIRENs, for the banner's buttons.
    docs: HashMap<u64, (String, Vec<String>)>,
}

/// The banner for a document, and its buttons; `None` when there is nothing to say.
fn banner(report: &Report, history: &History) -> Option<(String, Vec<Button>)> {
    if !report.has_findings() || (!report.invoice && report.ibans.is_empty()) {
        return None;
    }
    let mut problems = report.problems();
    if let History::Changed { siren, before } = history {
        problems.insert(
            0,
            format!("l'IBAN n'est plus celui de la dernière facture de ce fournisseur (SIREN {}, avant {})", spaced_id(siren), short_iban(before)),
        );
    }
    if !problems.is_empty() {
        let mut text = format!("Attention : {}.", problems.join(" ; "));
        let mut buttons = Vec::new();
        if matches!(history, History::Changed { .. }) {
            text.push_str(" Vérifiez par téléphone, à un numéro que vous connaissiez déjà, avant de payer.");
            buttons.push(TRUST);
        }
        return Some((text, buttons));
    }
    let mut parts = Vec::new();
    if let Some(payee) = report.payee_iban() {
        parts.push(format!("IBAN {} valide", short_iban(&payee.iban)));
    }
    if let Some(id) = report.ids.iter().find(|f| f.valid) {
        parts.push(if id.is_siret() { "SIRET valide".to_string() } else { "SIREN valide".to_string() });
    }
    if report.vats.iter().any(|f| f.valid) {
        parts.push("TVA valide".to_string());
    }
    let mut buttons = Vec::new();
    match history {
        History::Known => parts.push("IBAN déjà payé à ce fournisseur".to_string()),
        History::New if report.payee_iban().is_some() && !report.sirens().is_empty() => buttons.push(REMEMBER),
        _ => {}
    }
    Some((format!("Vérifié : {}.", parts.join(" · ")), buttons))
}

impl Factures {
    fn save(&self) {
        let bytes = match serde_json::to_vec_pretty(&self.known) {
            Ok(bytes) => bytes,
            Err(err) => return eprintln!("cannot write the known IBANs: {err}"),
        };
        if let Err(err) = store::write_atomic(&self.known_path, &bytes) {
            eprintln!("cannot write {}: {err}", self.known_path.display());
        }
    }
}

impl Extension for Factures {
    fn on_text<W: Write>(&mut self, out: &mut Out<W>, doc: u64, _name: &str, text: &str) -> io::Result<()> {
        let report = analyze(text);
        let sirens = report.sirens();
        let history = report.payee_iban().map_or(History::New, |p| self.known.check(&p.iban, &sirens));
        if let Some(payee) = report.payee_iban() {
            self.docs.insert(doc, (payee.iban.clone(), sirens));
        }
        if let Some((text, buttons)) = banner(&report, &history) {
            out.banner(doc, BANNER, &text, &buttons)?;
        }
        Ok(())
    }

    fn on_action<W: Write>(&mut self, out: &mut Out<W>, doc: u64, banner: &str, action: &str) -> io::Result<()> {
        if banner != BANNER || !matches!(action, "remember" | "trust") {
            return Ok(());
        }
        let Some((iban, sirens)) = self.docs.get(&doc) else { return Ok(()) };
        self.known.remember(iban, sirens);
        self.save();
        let text = format!("IBAN {} mémorisé : une facture de ce fournisseur avec un autre IBAN sera signalée.", short_iban(iban));
        out.banner(doc, BANNER, &text, &[])?;
        Ok(())
    }

    fn on_closed(&mut self, doc: u64) {
        self.docs.remove(&doc);
    }
}

fn main() {
    let known_path = store::data_dir(ID).join(KNOWN_FILE);
    let known = store::read_capped(&known_path, MAX_KNOWN_BYTES).map(|b| Known::parse(&b)).unwrap_or_default();
    let mut ext = Factures { known_path, known, docs: HashMap::new() };
    if let Err(err) = rpc::run(&mut ext, io::stdin().lock(), io::stdout().lock()) {
        eprintln!("stopped: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banners() {
        let ok = analyze("Facture\nSIRET 732 829 320 00074\nIBAN FR76 3000 6000 0112 3456 7890 189");
        let (text, buttons) = banner(&ok, &History::New).unwrap_or_default();
        assert_eq!(text, "Vérifié : IBAN FR76…0189 valide · SIRET valide.");
        assert_eq!(buttons, [REMEMBER]);
        let (text, buttons) = banner(&ok, &History::Known).unwrap_or_default();
        assert!(text.ends_with("IBAN déjà payé à ce fournisseur."), "{text}");
        assert!(buttons.is_empty());
        let changed = History::Changed { siren: "732829320".into(), before: "FR1420041010050500013M02606".into() };
        let (text, buttons) = banner(&ok, &changed).unwrap_or_default();
        assert!(text.starts_with("Attention : l'IBAN n'est plus celui") && text.contains("FR14…2606"), "{text}");
        assert_eq!(buttons, [TRUST]);
        // A document that is not an invoice and has no IBAN says nothing.
        assert!(banner(&analyze("Extrait Kbis — SIREN 732 829 320"), &History::New).is_none());
        assert!(banner(&analyze("Bonjour"), &History::New).is_none());
    }
}
