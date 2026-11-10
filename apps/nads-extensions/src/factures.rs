//! Contrôle des factures: finds the IBANs, BICs, SIREN/SIRET and French VAT numbers in a
//! document's text, checks their check digits offline, and remembers each supplier's IBAN so a
//! changed one (the usual invoice fraud) is flagged.
//!
//! Nothing here reaches the network. The checks are the public algorithms: ISO 13616 (IBAN, mod
//! 97), Luhn (SIREN/SIRET, with La Poste's exception) and the French VAT key
//! `(12 + 3 × (SIREN mod 97)) mod 97`.

use serde::{Deserialize, Serialize};

use crate::text::{folded, is_alnum_at, starts_with_at, window_has};

/// At most this many findings of each kind are kept for one document.
const MAX_FINDINGS: usize = 20;

/// The IBAN length of each country in the SWIFT IBAN registry that NadsPDF's users meet most.
/// An IBAN from another country isn't recognised (rather than guessed).
fn iban_length(country: &str) -> Option<usize> {
    Some(match country {
        "NO" => 15,
        "BE" => 16,
        "DK" | "FI" | "FO" | "GL" | "NL" => 18,
        "SI" => 19,
        "AT" | "EE" | "LT" | "LU" | "XK" => 20,
        "CH" | "HR" | "LI" | "LV" => 21,
        "BG" | "DE" | "GB" | "IE" | "VA" | "GE" | "ME" | "RS" | "BH" | "CR" => 22,
        "GI" | "AE" | "IL" | "TL" => 23,
        "AD" | "CZ" | "ES" | "MD" | "PK" | "RO" | "SA" | "SE" | "SK" | "TN" | "VG" => 24,
        "PT" | "ST" => 25,
        "IS" | "TR" => 26,
        "FR" | "GR" | "IT" | "MC" | "MR" | "SM" => 27,
        "AL" | "AZ" | "BY" | "CY" | "DO" | "GT" | "HU" | "LB" | "PL" | "MA" => 28,
        "BR" | "EG" | "PS" | "QA" | "UA" => 29,
        "JO" | "KW" | "MU" => 30,
        "MT" => 31,
        _ => return None,
    })
}

/// An IBAN found in the text, without spaces, upper case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IbanFinding {
    pub iban: String,
    pub problem: Option<IbanProblem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IbanProblem {
    /// The check digits are wrong: a typo, or a number made up.
    Checksum,
    /// Shorter or longer than the country's IBANs.
    Length { expected: usize, found: usize },
}

impl IbanFinding {
    pub fn country(&self) -> &str {
        self.iban.get(..2).unwrap_or_default()
    }
}

/// A SIREN (9 digits) or SIRET (14 digits) found after "SIREN", "SIRET" or "RCS".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanyId {
    pub digits: String,
    pub valid: bool,
}

impl CompanyId {
    pub fn siren(&self) -> &str {
        self.digits.get(..9).unwrap_or_default()
    }
    pub fn is_siret(&self) -> bool {
        self.digits.len() == 14
    }
}

/// A French VAT number, `FR` + 2-digit key + SIREN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VatFinding {
    pub vat: String,
    pub valid: bool,
}

impl VatFinding {
    pub fn siren(&self) -> &str {
        self.vat.get(4..).unwrap_or_default()
    }
}

/// Everything found in one document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The text reads like an invoice (or a credit note, a fee note…).
    pub invoice: bool,
    pub ibans: Vec<IbanFinding>,
    pub ids: Vec<CompanyId>,
    pub vats: Vec<VatFinding>,
    /// The country (letters 5–6) of each BIC found after "BIC" or "SWIFT".
    pub bic_countries: Vec<String>,
}

const INVOICE_WORDS: &[&str] = &[
    "facture",
    "note d'honoraires",
    "avoir n",
    "total ttc",
    "montant ttc",
    "net a payer",
    "invoice",
    "amount due",
    "rechnung",
    "factura",
    "fattura",
];

/// Reads `text`.
pub fn analyze(text: &str) -> Report {
    let original: Vec<char> = text.chars().collect();
    let low = folded(text);
    let up: Vec<char> = original.iter().map(char::to_ascii_uppercase).collect();
    Report {
        invoice: window_has(&low, 0, low.len(), INVOICE_WORDS),
        ibans: find_ibans(&original, &up, &low),
        ids: find_company_ids(&low),
        vats: find_vats(&original, &low),
        bic_countries: find_bics(&up, &low),
    }
}

/// The characters of a group-separated number: one space (or no-break space) between groups.
fn is_separator(c: Option<&char>) -> bool {
    matches!(c, Some(' ' | '\u{a0}' | '\u{202f}'))
}

fn find_ibans(original: &[char], up: &[char], low: &[char]) -> Vec<IbanFinding> {
    let mut out: Vec<IbanFinding> = Vec::new();
    let mut i = 0usize;
    while i < up.len() && out.len() < MAX_FINDINGS {
        let start = i;
        i = i.saturating_add(1);
        let at = |k: usize| original.get(start.saturating_add(k)).copied();
        let country_ok = at(0).is_some_and(|c| c.is_ascii_uppercase()) && at(1).is_some_and(|c| c.is_ascii_uppercase());
        let digits_ok = at(2).is_some_and(|c| c.is_ascii_digit()) && at(3).is_some_and(|c| c.is_ascii_digit());
        if !country_ok || !digits_ok || is_alnum_at(original, start.checked_sub(1)) {
            continue;
        }
        let country: String = [at(0), at(1)].into_iter().flatten().collect();
        let Some(expected) = iban_length(&country) else { continue };

        // Collect letters and digits; a separator is allowed only after a group of 4 (how IBANs
        // are printed), so the next words of the text are not swallowed.
        let mut compact = String::new();
        let mut group = 0usize;
        let mut j = start;
        while compact.len() < expected {
            match up.get(j) {
                Some(c) if c.is_ascii_alphanumeric() => {
                    compact.push(*c);
                    group = group.saturating_add(1);
                    j = j.saturating_add(1);
                }
                c if is_separator(c) && group == 4 && up.get(j.saturating_add(1)).is_some_and(char::is_ascii_alphanumeric) => {
                    group = 0;
                    j = j.saturating_add(1);
                }
                _ => break,
            }
        }
        let mut found = compact.len();
        while up.get(j).is_some_and(char::is_ascii_alphanumeric) {
            found = found.saturating_add(1);
            j = j.saturating_add(1);
        }
        let named = window_has(low, start.saturating_sub(40), start, &["iban"]);
        let problem = if found != expected {
            // A short run right after "IBAN" is a cut IBAN; anywhere else, just something else.
            if !window_has(low, start.saturating_sub(15), start, &["iban"]) {
                continue;
            }
            Some(IbanProblem::Length { expected, found })
        } else if iban_mod97(&compact) != Some(1) {
            if !named {
                continue;
            }
            Some(IbanProblem::Checksum)
        } else {
            None
        };
        i = j;
        if !out.iter().any(|f| f.iban == compact) {
            out.push(IbanFinding { iban: compact, problem });
        }
    }
    out
}

/// ISO 13616: the first four characters moved to the end, letters as 10–35, the whole mod 97.
pub fn iban_mod97(compact: &str) -> Option<u32> {
    let mut r: u32 = 0;
    for c in compact.chars().skip(4).chain(compact.chars().take(4)) {
        let v = c.to_digit(36)?;
        r = if v < 10 { (r * 10 + v) % 97 } else { (r * 100 + v) % 97 };
    }
    Some(r)
}

/// Digits read from `chars` starting at `from`: one separator (space, no-break space, dot) is
/// allowed between digits; stops after `max` digits.
fn digits_from(chars: &[char], from: usize, max: usize) -> (String, usize) {
    let mut digits = String::new();
    let mut j = from;
    while digits.len() < max {
        match chars.get(j) {
            Some(c) if c.is_ascii_digit() => {
                digits.push(*c);
                j = j.saturating_add(1);
            }
            Some(' ' | '\u{a0}' | '\u{202f}' | '.') if !digits.is_empty() && chars.get(j.saturating_add(1)).is_some_and(char::is_ascii_digit) => {
                j = j.saturating_add(1);
            }
            _ => break,
        }
    }
    (digits, j)
}

fn find_company_ids(low: &[char]) -> Vec<CompanyId> {
    let mut out: Vec<CompanyId> = Vec::new();
    for i in 0..low.len() {
        if out.len() >= MAX_FINDINGS {
            break;
        }
        let Some(word) = ["siret", "siren", "rcs"].into_iter().find(|w| starts_with_at(low, i, w)) else { continue };
        if is_alnum_at(low, i.checked_sub(1)) {
            continue;
        }
        // The number follows within a few words ("RCS Paris B 123 456 789", "SIRET : …").
        let after = i.saturating_add(word.len());
        let Some(first) = (after..after.saturating_add(30).min(low.len())).find(|&k| low.get(k).is_some_and(char::is_ascii_digit)) else {
            continue;
        };
        let (digits, end) = digits_from(low, first, 14);
        if (digits.len() != 9 && digits.len() != 14) || low.get(end).is_some_and(char::is_ascii_digit) {
            continue;
        }
        if !out.iter().any(|f| f.digits == digits) {
            let valid = company_id_valid(&digits);
            out.push(CompanyId { digits, valid });
        }
    }
    out
}

/// Luhn over the digits of `digits` (all ASCII digits).
fn luhn(digits: &str) -> bool {
    let sum: u32 = digits
        .chars()
        .rev()
        .filter_map(|c| c.to_digit(10))
        .enumerate()
        .map(|(k, d)| if k % 2 == 1 { if d * 2 > 9 { d * 2 - 9 } else { d * 2 } } else { d })
        .sum();
    sum.is_multiple_of(10)
}

/// A SIREN or SIRET passes Luhn; La Poste's establishments (SIREN 356 000 000) use the sum of
/// their digits mod 5 instead.
pub fn company_id_valid(digits: &str) -> bool {
    if digits.len() == 14 && digits.starts_with("356000000") {
        let sum: u32 = digits.chars().filter_map(|c| c.to_digit(10)).sum();
        return sum.is_multiple_of(5);
    }
    luhn(digits)
}

fn find_vats(original: &[char], low: &[char]) -> Vec<VatFinding> {
    let mut out: Vec<VatFinding> = Vec::new();
    for i in 0..original.len() {
        if out.len() >= MAX_FINDINGS {
            break;
        }
        if original.get(i) != Some(&'F') || original.get(i.saturating_add(1)) != Some(&'R') || is_alnum_at(original, i.checked_sub(1)) {
            continue;
        }
        if !window_has(low, i.saturating_sub(50), i, &["tva", "vat", "intracom", "ust-id"]) {
            continue;
        }
        let mut from = i.saturating_add(2);
        if is_separator(original.get(from)) {
            from = from.saturating_add(1);
        }
        let (digits, end) = digits_from(original, from, 11);
        // An IBAN (FR76 3000…) goes on: not a VAT number.
        if digits.len() != 11 || original.get(end).is_some_and(char::is_ascii_alphanumeric) {
            continue;
        }
        let vat = format!("FR{digits}");
        let valid = vat_valid(&vat);
        if !out.iter().any(|f| f.vat == vat) {
            out.push(VatFinding { vat, valid });
        }
    }
    out
}

/// `FR` + key + SIREN, the key being `(12 + 3 × (SIREN mod 97)) mod 97` and the SIREN valid.
pub fn vat_valid(vat: &str) -> bool {
    let (Some(key), Some(siren)) = (vat.get(2..4), vat.get(4..)) else { return false };
    let (Ok(key), Ok(n)) = (key.parse::<u64>(), siren.parse::<u64>()) else { return false };
    siren.len() == 9 && luhn(siren) && key == (12 + 3 * (n % 97)) % 97
}

fn find_bics(up: &[char], low: &[char]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for i in 0..low.len() {
        if out.len() >= MAX_FINDINGS {
            break;
        }
        let Some(word) = ["bic", "swift"].into_iter().find(|w| starts_with_at(low, i, w)) else { continue };
        if is_alnum_at(low, i.checked_sub(1)) || is_alnum_at(low, Some(i.saturating_add(word.len()))) {
            continue;
        }
        let after = i.saturating_add(word.len());
        let Some(first) = (after..after.saturating_add(12).min(up.len())).find(|&k| up.get(k).is_some_and(char::is_ascii_alphanumeric)) else {
            continue;
        };
        let mut token = String::new();
        let mut j = first;
        while let Some(c) = up.get(j).filter(|c| c.is_ascii_alphanumeric()) {
            token.push(*c);
            j = j.saturating_add(1);
            if token.len() > 11 {
                break;
            }
        }
        let shape = token.chars().take(6).all(|c| c.is_ascii_uppercase());
        if (token.len() == 8 || token.len() == 11) && shape {
            out.push(token.get(4..6).unwrap_or_default().to_string());
        }
    }
    out.dedup();
    out
}

/// `FR76 3000 … 0189`: enough of an IBAN to recognise it in a one-line banner.
pub fn short_iban(iban: &str) -> String {
    let head: String = iban.chars().take(4).collect();
    let n = iban.chars().count();
    let tail: String = iban.chars().skip(n.saturating_sub(4)).collect();
    format!("{head}…{tail}")
}

/// `123 456 789` or `123 456 789 00012`.
pub fn spaced_id(digits: &str) -> String {
    let mut out = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && k % 3 == 0 && k <= 9 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

impl Report {
    /// The SIRENs of the document: from its valid SIREN/SIRET and VAT numbers, without repeats.
    pub fn sirens(&self) -> Vec<String> {
        let ids = self.ids.iter().filter(|f| f.valid).map(|f| f.siren().to_string());
        let vats = self.vats.iter().filter(|f| f.valid).map(|f| f.siren().to_string());
        let mut out: Vec<String> = Vec::new();
        for s in ids.chain(vats) {
            if !s.is_empty() && !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }

    /// The first valid IBAN: the one to pay.
    pub fn payee_iban(&self) -> Option<&IbanFinding> {
        self.ibans.iter().find(|f| f.problem.is_none())
    }

    /// Whether there is anything to say about this document.
    pub fn has_findings(&self) -> bool {
        !self.ibans.is_empty() || !self.ids.is_empty() || !self.vats.is_empty()
    }

    /// What is wrong, most serious first, in French.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for f in &self.ibans {
            match f.problem {
                Some(IbanProblem::Checksum) => out.push(format!("l'IBAN {} est faux (clé de contrôle)", short_iban(&f.iban))),
                Some(IbanProblem::Length { expected, found }) => {
                    out.push(format!("l'IBAN {} a {found} caractères au lieu de {expected}", short_iban(&f.iban)));
                }
                None => {}
            }
        }
        for f in self.ids.iter().filter(|f| !f.valid) {
            let kind = if f.is_siret() { "SIRET" } else { "SIREN" };
            out.push(format!("le {kind} {} est faux", spaced_id(&f.digits)));
        }
        for f in self.vats.iter().filter(|f| !f.valid) {
            out.push(format!("le n° de TVA {} est faux", f.vat));
        }
        let id_sirens: Vec<&str> = self.ids.iter().filter(|f| f.valid).map(CompanyId::siren).collect();
        if !id_sirens.is_empty() {
            for f in self.vats.iter().filter(|f| f.valid && !id_sirens.contains(&f.siren())) {
                out.push(format!("le n° de TVA {} ne correspond à aucun SIREN de la facture", f.vat));
            }
        }
        if let Some(payee) = self.payee_iban() {
            let country = payee.country();
            if let [bic] = self.bic_countries.as_slice()
                && bic != country
                && self.ibans.len() == 1
            {
                out.push(format!("le BIC est d'un autre pays ({bic}) que l'IBAN ({country})"));
            }
            let french = !self.sirens().is_empty();
            if self.invoice && french && !matches!(country, "FR" | "MC") {
                out.push(format!("IBAN étranger ({country}) pour une entreprise française : à vérifier"));
            }
        }
        out
    }
}

/// The IBANs already paid, with the SIRENs printed on the invoice they came from. Kept by the
/// extension in its data folder (`iban-connus.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Known {
    #[serde(default)]
    pub entries: Vec<KnownEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownEntry {
    pub iban: String,
    pub sirens: Vec<String>,
}

/// What the remembered IBANs say about a document's IBAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum History {
    /// Already paid to one of these companies.
    Known,
    /// Every company on the document is known, and one of them was paid on another IBAN.
    Changed { siren: String, before: String },
    /// Nothing to compare with (a new supplier, or no SIREN on the document).
    New,
}

/// Remembered IBANs beyond this are dropped, oldest first.
pub const MAX_KNOWN: usize = 5_000;

impl Known {
    pub fn parse(bytes: &[u8]) -> Known {
        serde_json::from_slice(bytes).unwrap_or_else(|err| {
            eprintln!("iban-connus.json is unreadable ({err}): starting empty");
            Known::default()
        })
    }

    /// Remembers `iban` as paid to the companies `sirens`.
    pub fn remember(&mut self, iban: &str, sirens: &[String]) {
        if sirens.is_empty() {
            return;
        }
        match self.entries.iter_mut().find(|e| e.iban == iban) {
            Some(entry) => {
                for s in sirens {
                    if !entry.sirens.contains(s) {
                        entry.sirens.push(s.clone());
                    }
                }
            }
            None => self.entries.push(KnownEntry { iban: iban.to_string(), sirens: sirens.to_vec() }),
        }
        let over = self.entries.len().saturating_sub(MAX_KNOWN);
        self.entries.drain(..over);
    }

    /// A SIREN printed on invoices paid to different companies' IBANs (typically the user's own
    /// company, on every invoice it receives) says nothing about who is paid.
    fn shared(&self, siren: &str) -> bool {
        let others: Vec<Vec<&String>> =
            self.entries.iter().filter(|e| e.sirens.iter().any(|s| s == siren)).map(|e| e.sirens.iter().filter(|s| *s != siren).collect()).collect();
        others
            .iter()
            .enumerate()
            .any(|(k, a)| !a.is_empty() && others.iter().skip(k.saturating_add(1)).any(|b| !b.is_empty() && !a.iter().any(|s| b.contains(s))))
    }

    pub fn check(&self, iban: &str, sirens: &[String]) -> History {
        if sirens.is_empty() {
            return History::New;
        }
        let known_for = |s: &String| self.entries.iter().filter(|e| e.sirens.contains(s)).map(|e| e.iban.as_str()).collect::<Vec<_>>();
        if sirens.iter().any(|s| known_for(s).contains(&iban)) {
            return History::Known;
        }
        // A company never seen means a new supplier, not a changed IBAN.
        if sirens.iter().any(|s| known_for(s).is_empty()) {
            return History::New;
        }
        for s in sirens.iter().filter(|s| !self.shared(s)) {
            if let Some(before) = known_for(s).last() {
                return History::Changed { siren: s.clone(), before: (*before).to_string() };
            }
        }
        History::New
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic numbers with correct check digits, made up for these tests.
    const IBAN_FR: &str = "FR76 3000 6000 0112 3456 7890 189";
    const IBAN_FR_2: &str = "FR14 2004 1010 0505 0001 3M02 606";
    const IBAN_DE: &str = "DE89 3704 0044 0532 0130 00";

    #[test]
    fn iban_check_digits() {
        assert_eq!(iban_mod97("FR7630006000011234567890189"), Some(1));
        assert_eq!(iban_mod97("FR1420041010050500013M02606"), Some(1));
        assert_eq!(iban_mod97("DE89370400440532013000"), Some(1));
        assert_ne!(iban_mod97("FR7630006000011234567890188"), Some(1));
        assert_eq!(iban_mod97("FR76-!"), None);
        assert_eq!(iban_mod97(""), Some(0));
    }

    #[test]
    fn ibans_are_found_in_print_and_stop_at_the_next_word() {
        let r = analyze(&format!("Facture n° 12\nIBAN : {IBAN_FR} BIC : AGRIFRPP\nTotal TTC 120 €"));
        assert!(r.invoice);
        assert_eq!(r.ibans, vec![IbanFinding { iban: "FR7630006000011234567890189".into(), problem: None }]);
        assert_eq!(r.bic_countries, vec!["FR".to_string()]);
        assert!(r.problems().is_empty(), "{:?}", r.problems());
    }

    #[test]
    fn a_wrong_iban_is_reported_only_when_named() {
        let wrong = "FR76 3000 6000 0112 3456 7890 188";
        let r = analyze(&format!("IBAN {wrong}"));
        assert_eq!(r.ibans.first().and_then(|f| f.problem), Some(IbanProblem::Checksum));
        assert!(r.problems().iter().any(|p| p.contains("clé de contrôle")), "{:?}", r.problems());
        // The same characters in a reference number elsewhere are not an IBAN.
        assert!(analyze(&format!("Référence de dossier lointaine, sans rapport : {wrong}")).ibans.is_empty());
    }

    #[test]
    fn a_cut_iban_is_reported() {
        let r = analyze("IBAN : FR76 3000 6000 0112 3456 789");
        assert_eq!(r.ibans.first().and_then(|f| f.problem), Some(IbanProblem::Length { expected: 27, found: 23 }));
    }

    #[test]
    fn several_ibans_and_countries() {
        let r = analyze(&format!("IBAN {IBAN_FR}\nou IBAN {IBAN_DE}\nou {IBAN_FR_2}"));
        let found: Vec<&str> = r.ibans.iter().map(|f| f.iban.as_str()).collect();
        assert_eq!(found, ["FR7630006000011234567890189", "DE89370400440532013000", "FR1420041010050500013M02606"]);
        assert!(r.ibans.iter().all(|f| f.problem.is_none()));
    }

    #[test]
    fn siren_siret_and_vat() {
        // 732 829 320 passes Luhn; its VAT key is (12 + 3 × (732829320 mod 97)) mod 97 = 44.
        let r = analyze("SIRET : 732 829 320 00074 — RCS Nanterre B 732 829 320 — TVA intracommunautaire FR44 732829320");
        assert_eq!(r.ids.iter().map(|f| (f.digits.as_str(), f.valid)).collect::<Vec<_>>(), [("73282932000074", true), ("732829320", true)]);
        assert_eq!(r.vats, vec![VatFinding { vat: "FR44732829320".into(), valid: true }]);
        assert_eq!(r.sirens(), vec!["732829320".to_string()]);
        assert!(r.problems().is_empty(), "{:?}", r.problems());

        let bad = analyze("SIRET 732 829 321 00074, TVA FR45732829320");
        let problems = bad.problems();
        assert!(problems.iter().any(|p| p.contains("SIRET 732 829 321 00074 est faux")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("TVA FR45732829320 est faux")), "{problems:?}");
    }

    #[test]
    fn la_poste_sirets_use_their_own_rule() {
        // Digits sum to 15 (a multiple of 5), though Luhn refuses it.
        assert!(company_id_valid("35600000000010"));
        assert!(!luhn("35600000000010"));
        assert!(!company_id_valid("35600000000011"));
    }

    #[test]
    fn a_vat_number_is_not_mistaken_for_an_iban_nor_the_reverse() {
        let r = analyze(&format!("TVA FR44732829320\nIBAN {IBAN_FR}"));
        assert_eq!(r.vats.len(), 1);
        assert_eq!(r.ibans.len(), 1);
        assert!(r.ibans.iter().all(|f| f.problem.is_none()));
    }

    #[test]
    fn the_words_after_a_number_are_not_read_as_an_iban() {
        // Without the groups-of-4 rule, "FR44732829320SIREN732829320" (27 characters, near the
        // word IBAN) would be reported as a wrong French IBAN.
        let r = analyze("IBAN : voir RIB joint. TVA FR44 732829320 SIREN 732829320");
        assert!(r.ibans.is_empty(), "{:?}", r.ibans);
        assert!(r.problems().is_empty(), "{:?}", r.problems());
    }

    #[test]
    fn fraud_signals() {
        let foreign = analyze(&format!("Facture\nSIREN 732 829 320\nIBAN {IBAN_DE}\nBIC COBADEFFXXX"));
        assert!(foreign.problems().iter().any(|p| p.contains("IBAN étranger (DE)")), "{:?}", foreign.problems());
        let mismatch = analyze(&format!("IBAN {IBAN_FR}\nBIC COBADEFFXXX"));
        assert!(mismatch.problems().iter().any(|p| p.contains("BIC est d'un autre pays (DE)")), "{:?}", mismatch.problems());
    }

    #[test]
    fn hostile_text_never_panics() {
        for text in ["", "FR", "FR7", "IBAN FR76", "SIRET", "RCS 1", "TVA FR", "BIC", "€€€FR76 ", "ＦＲ７６", &"IBAN FR76 3000 ".repeat(1000)]
        {
            let r = analyze(text);
            let _ = r.problems();
            let _ = r.sirens();
        }
        assert_eq!(short_iban(""), "…");
        assert_eq!(short_iban("FR7630006000011234567890189"), "FR76…0189");
        assert_eq!(spaced_id("73282932000074"), "732 829 320 00074");
    }

    #[test]
    fn a_changed_iban_is_flagged_but_not_a_new_supplier() {
        let me = "111111118".to_string();
        let a = "222222226".to_string();
        let b = "333333334".to_string();
        let mut known = Known::default();
        known.remember("FR_A", &[me.clone(), a.clone()]);
        assert_eq!(known.check("FR_A", &[me.clone(), a.clone()]), History::Known);
        // Supplier A now asks to be paid elsewhere.
        assert_eq!(known.check("FR_X", &[me.clone(), a.clone()]), History::Changed { siren: me.clone(), before: "FR_A".into() });
        assert_eq!(known.check("FR_X", std::slice::from_ref(&a)), History::Changed { siren: a.clone(), before: "FR_A".into() });
        // A first invoice from supplier B: B is unknown, so nothing changed.
        assert_eq!(known.check("FR_B", &[me.clone(), b.clone()]), History::New);
        known.remember("FR_B", &[me.clone(), b.clone()]);
        // My own SIREN is now on two suppliers' invoices: it no longer identifies who is paid…
        assert!(known.shared(&me));
        // …but A's still does.
        assert_eq!(known.check("FR_X", &[me.clone(), a.clone()]), History::Changed { siren: a.clone(), before: "FR_A".into() });
        assert_eq!(known.check("FR_B", &[me.clone(), b.clone()]), History::Known);
        assert_eq!(known.check("FR_A", &[]), History::New);
        // A confirmed change is accepted from then on.
        known.remember("FR_X", &[me, a.clone()]);
        assert_eq!(known.check("FR_X", &[a]), History::Known);
    }

    #[test]
    fn the_store_survives_garbage_and_stays_bounded() {
        assert_eq!(Known::parse(b"{nope"), Known::default());
        assert_eq!(Known::parse(b"{}"), Known::default());
        let mut known = Known::default();
        for n in 0..MAX_KNOWN + 3 {
            known.remember(&format!("IBAN{n}"), &["1".to_string()]);
        }
        assert_eq!(known.entries.len(), MAX_KNOWN);
        assert_eq!(known.entries.first().map(|e| e.iban.as_str()), Some("IBAN3"));
    }
}
