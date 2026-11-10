//! Échéances: finds the dates a document asks something for (a payment, an expiry, a hearing, a
//! deadline) and writes them as an iCalendar file (RFC 5545) the user's calendar imports.
//!
//! A date counts only when a deadline word comes before it on its line, or on the line before
//! for a table ("Échéance" above "15/12/2026"). Dates already past, or more than five years
//! ahead, are left out.

use chrono::{DateTime, Datelike, NaiveDate, Utc};

use crate::text::{fnv1a, folded, one_line, starts_with_at};

/// At most this many dates are kept for one document.
pub const MAX_DEADLINES: usize = 20;

/// How far back from a date a deadline word is looked for.
const WINDOW: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Payment,
    Expiry,
    Appointment,
    Deadline,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Payment => "Paiement",
            Kind::Expiry => "Expiration",
            Kind::Appointment => "Rendez-vous",
            Kind::Deadline => "Échéance",
        }
    }
}

/// Folded (lower case, no accents) words that announce a date, and what kind of date.
const KEYWORDS: &[(&str, Kind)] = &[
    ("echeance", Kind::Payment),
    ("a regler", Kind::Payment),
    ("a payer", Kind::Payment),
    ("payable", Kind::Payment),
    ("paiement", Kind::Payment),
    ("prelevement", Kind::Payment),
    ("preleve", Kind::Payment),
    ("due date", Kind::Payment),
    ("payment due", Kind::Payment),
    ("pay by", Kind::Payment),
    ("expire", Kind::Expiry),
    ("expiration", Kind::Expiry),
    ("valable jusqu", Kind::Expiry),
    ("valide jusqu", Kind::Expiry),
    ("fin de validite", Kind::Expiry),
    ("date de fin", Kind::Expiry),
    ("fin de contrat", Kind::Expiry),
    ("resiliation", Kind::Expiry),
    ("renouvellement", Kind::Expiry),
    ("valid until", Kind::Expiry),
    ("expiry", Kind::Expiry),
    ("audience", Kind::Appointment),
    ("convocation", Kind::Appointment),
    ("convoque", Kind::Appointment),
    ("rendez-vous", Kind::Appointment),
    ("rdv", Kind::Appointment),
    ("entretien", Kind::Appointment),
    ("appointment", Kind::Appointment),
    ("hearing", Kind::Appointment),
    ("date limite", Kind::Deadline),
    ("date butoir", Kind::Deadline),
    ("au plus tard", Kind::Deadline),
    ("avant le", Kind::Deadline),
    ("delai", Kind::Deadline),
    ("jusqu'au", Kind::Deadline),
    ("a rendre", Kind::Deadline),
    ("a retourner", Kind::Deadline),
    ("a renvoyer", Kind::Deadline),
    ("deadline", Kind::Deadline),
    ("due by", Kind::Deadline),
    ("no later than", Kind::Deadline),
];

/// Month names (folded), longest first so "juillet" wins over "juil".
const MONTHS: &[(&str, u32)] = &[
    ("septembre", 9),
    ("september", 9),
    ("decembre", 12),
    ("novembre", 11),
    ("december", 12),
    ("february", 2),
    ("november", 11),
    ("fevrier", 2),
    ("janvier", 1),
    ("juillet", 7),
    ("octobre", 10),
    ("january", 1),
    ("october", 10),
    ("august", 8),
    ("avril", 4),
    ("march", 3),
    ("april", 4),
    ("sept", 9),
    ("janv", 1),
    ("fevr", 2),
    ("juil", 7),
    ("mars", 3),
    ("juin", 6),
    ("aout", 8),
    ("june", 6),
    ("july", 7),
    ("fev", 2),
    ("avr", 4),
    ("mai", 5),
    ("oct", 10),
    ("nov", 11),
    ("dec", 12),
    ("jan", 1),
    ("feb", 2),
    ("mar", 3),
    ("apr", 4),
    ("may", 5),
    ("jun", 6),
    ("jul", 7),
    ("aug", 8),
    ("sep", 9),
];

const MONTH_NAMES_FR: [&str; 12] =
    ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];

/// A date the document asks something for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deadline {
    pub date: NaiveDate,
    pub kind: Kind,
    /// The words around it, on one line.
    pub context: String,
}

/// The dates in `text` from `today` to five years ahead, earliest first, one per day.
pub fn find(text: &str, today: NaiveDate) -> Vec<Deadline> {
    let original: Vec<char> = text.chars().collect();
    let low = folded(text);
    let last_day = today.checked_add_months(chrono::Months::new(60)).unwrap_or(NaiveDate::MAX);
    let mut out: Vec<Deadline> = Vec::new();
    let mut i = 0usize;
    // Where the words of the next date may start: after the previous date on the same line.
    let mut floor = 0usize;
    while i < low.len() && out.len() < MAX_DEADLINES {
        let starts_word = i == 0 || !low.get(i.wrapping_sub(1)).is_some_and(|c| c.is_alphanumeric());
        let found = if starts_word { parse_date(&low, i, today) } else { None };
        let Some((date, end)) = found else {
            if low.get(i) == Some(&'\n') {
                floor = i.saturating_add(1);
            }
            i = i.saturating_add(1);
            continue;
        };
        let start_of_line = line_start(&low, i);
        let from = floor.max(start_of_line).max(i.saturating_sub(WINDOW));
        let mut hit = keyword_before(&low, from, i);
        let mut context_from = hit.map_or(from, |(at, _)| at);
        // A table cell: the date alone on its line, its label on the line above.
        let alone = low.get(start_of_line..i).is_some_and(|before| before.iter().all(|c| !c.is_alphanumeric()));
        if hit.is_none() && alone && start_of_line > 0 {
            let above_end = start_of_line.saturating_sub(1);
            let above_start = line_start(&low, above_end).max(above_end.saturating_sub(WINDOW));
            hit = keyword_before(&low, above_start, above_end);
            context_from = hit.map_or(context_from, |(at, _)| at);
        }
        if let Some((_, kind)) = hit
            && date >= today
            && date <= last_day
            && !out.iter().any(|d| d.date == date)
        {
            let context: String = original.get(context_from..end).unwrap_or_default().iter().collect();
            out.push(Deadline { date, kind, context: one_line(&context, 120) });
        }
        floor = end;
        i = end.max(i.saturating_add(1));
    }
    out.sort_by_key(|d| d.date);
    out
}

fn line_start(low: &[char], at: usize) -> usize {
    let mut k = at.min(low.len());
    while k > 0 && low.get(k.saturating_sub(1)) != Some(&'\n') {
        k = k.saturating_sub(1);
    }
    k
}

/// The deadline word in `low[from..to]`: the nearest specific one (payment, expiry,
/// appointment), else the nearest generic one. Its position and kind.
fn keyword_before(low: &[char], from: usize, to: usize) -> Option<(usize, Kind)> {
    let mut specific: Option<(usize, Kind)> = None;
    let mut generic: Option<(usize, Kind)> = None;
    for k in from..to.min(low.len()) {
        let word_start = k == 0 || !low.get(k.wrapping_sub(1)).is_some_and(|c| c.is_alphanumeric());
        if !word_start {
            continue;
        }
        for &(word, kind) in KEYWORDS {
            if starts_with_at(low, k, word) && k.saturating_add(word.chars().count()) <= to {
                let slot = if kind == Kind::Deadline { &mut generic } else { &mut specific };
                *slot = Some((k, kind));
            }
        }
    }
    specific.or(generic)
}

/// Up to `max` ASCII digits at `at`: their value, count and end.
fn number(low: &[char], at: usize, max: usize) -> Option<(u32, usize, usize)> {
    let mut value: u32 = 0;
    let mut n = 0usize;
    let mut k = at;
    while n < max {
        let Some(d) = low.get(k).and_then(|c| c.to_digit(10)) else { break };
        value = value.saturating_mul(10).saturating_add(d);
        n = n.saturating_add(1);
        k = k.saturating_add(1);
    }
    if low.get(k).is_some_and(char::is_ascii_digit) || n == 0 {
        return None;
    }
    Some((value, n, k))
}

fn skip_spaces(low: &[char], mut at: usize) -> usize {
    while low.get(at) == Some(&' ') {
        at = at.saturating_add(1);
    }
    at
}

fn month_at(low: &[char], at: usize) -> Option<(u32, usize)> {
    MONTHS.iter().find_map(|&(name, month)| {
        let end = at.saturating_add(name.len());
        let whole = starts_with_at(low, at, name) && !low.get(end).is_some_and(|c| c.is_alphabetic());
        whole.then(|| (month, if low.get(end) == Some(&'.') { end.saturating_add(1) } else { end }))
    })
}

/// The date written at `at` and where it ends: `15/12/2026`, `15.12.26`, `2026-12-15`,
/// `15 décembre 2026`, `1er mars`, `December 15, 2026`. A day and month without a year is the
/// next such day from `today`.
fn parse_date(low: &[char], at: usize, today: NaiveDate) -> Option<(NaiveDate, usize)> {
    let ends_well = |end: usize| !low.get(end).is_some_and(|c| c.is_alphanumeric());
    if let Some((first, len, end)) = number(low, at, 4) {
        // 2026-12-15
        if len == 4 {
            let sep = *low.get(end).filter(|c| matches!(c, '-' | '/'))?;
            let (m, _, e2) = number(low, end.saturating_add(1), 2)?;
            (low.get(e2) == Some(&sep)).then_some(())?;
            let (d, _, e3) = number(low, e2.saturating_add(1), 2)?;
            let date = NaiveDate::from_ymd_opt(i32::try_from(first).ok()?, m, d)?;
            return ends_well(e3).then_some((date, e3));
        }
        if len > 2 {
            return None;
        }
        // 15/12/2026, 15.12.26, 15-12-2026
        if let Some(&sep) = low.get(end).filter(|c| matches!(c, '/' | '.' | '-'))
            && let Some((m, _, e2)) = number(low, end.saturating_add(1), 2)
            && low.get(e2) == Some(&sep)
        {
            let (y, ylen, e3) = number(low, e2.saturating_add(1), 4)?;
            let year = match ylen {
                4 => y,
                2 => y.saturating_add(2000),
                _ => return None,
            };
            let date = NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, m, first)?;
            return ends_well(e3).then_some((date, e3));
        }
        // 15 décembre 2026, 1er mars
        let mut k = end;
        if first == 1 && starts_with_at(low, k, "er") {
            k = k.saturating_add(2);
        }
        let k2 = skip_spaces(low, k);
        if k2 == k {
            return None;
        }
        let (month, after_month) = month_at(low, k2)?;
        let k3 = skip_spaces(low, after_month);
        if k3 > after_month
            && let Some((y, 4, e)) = number(low, k3, 4)
        {
            let date = NaiveDate::from_ymd_opt(i32::try_from(y).ok()?, month, first)?;
            return ends_well(e).then_some((date, e));
        }
        let this_year = NaiveDate::from_ymd_opt(today.year(), month, first)?;
        let date = if this_year >= today { this_year } else { NaiveDate::from_ymd_opt(today.year().saturating_add(1), month, first)? };
        return Some((date, after_month));
    }
    // December 15, 2026
    let (month, after_month) = month_at(low, at)?;
    let k = skip_spaces(low, after_month);
    let (d, _, mut e) = number(low, k, 2)?;
    for suffix in ["st", "nd", "rd", "th"] {
        if starts_with_at(low, e, suffix) {
            e = e.saturating_add(2);
        }
    }
    if low.get(e) == Some(&',') {
        e = e.saturating_add(1);
    }
    let k = skip_spaces(low, e);
    let (y, 4, end) = number(low, k, 4)? else { return None };
    let date = NaiveDate::from_ymd_opt(i32::try_from(y).ok()?, month, d)?;
    ends_well(end).then_some((date, end))
}

/// `15 décembre 2026`, `1er mars 2027`.
pub fn date_fr(date: NaiveDate) -> String {
    let month = usize::try_from(date.month0()).ok().and_then(|m| MONTH_NAMES_FR.get(m)).copied().unwrap_or_default();
    let day = if date.day() == 1 { "1er".to_string() } else { date.day().to_string() };
    format!("{day} {month} {}", date.year())
}

/// RFC 5545 text: backslash, semicolon, comma and line breaks escaped, other controls dropped.
fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// One content line, folded at 75 bytes without splitting a character.
fn push_line(out: &mut String, line: &str) {
    let mut width = 0usize;
    for c in line.chars() {
        let w = c.len_utf8();
        if width.saturating_add(w) > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width = width.saturating_add(w);
    }
    out.push_str("\r\n");
}

/// The document's name without `.pdf`, for event titles.
pub fn title_of(doc_name: &str) -> &str {
    let lower = doc_name.to_ascii_lowercase();
    if lower.ends_with(".pdf") { doc_name.get(..doc_name.len().saturating_sub(4)).unwrap_or(doc_name) } else { doc_name }
}

/// An iCalendar file with one all-day event per deadline, each with a reminder the day before at
/// 9:00. The ids are derived from the document and the date, so importing it again updates the
/// events instead of adding them twice.
pub fn ics(deadlines: &[Deadline], doc_name: &str, now: DateTime<Utc>) -> String {
    let mut out = String::new();
    for line in ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//NadsPDF//Echeances//FR", "CALSCALE:GREGORIAN", "METHOD:PUBLISH"] {
        push_line(&mut out, line);
    }
    let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let title = title_of(doc_name);
    for d in deadlines {
        let day = d.date.format("%Y%m%d").to_string();
        let next = d.date.succ_opt().unwrap_or(d.date).format("%Y%m%d").to_string();
        let uid = fnv1a(&format!("{doc_name}|{day}|{}", d.kind.label()));
        let summary = escape(&format!("{} — {title}", d.kind.label()));
        let description = escape(&format!("Trouvé dans « {doc_name} » : {}", d.context));
        push_line(&mut out, "BEGIN:VEVENT");
        push_line(&mut out, &format!("UID:{uid:016x}-{day}@echeances.nadspdf"));
        push_line(&mut out, &format!("DTSTAMP:{stamp}"));
        push_line(&mut out, &format!("DTSTART;VALUE=DATE:{day}"));
        push_line(&mut out, &format!("DTEND;VALUE=DATE:{next}"));
        push_line(&mut out, &format!("SUMMARY:{summary}"));
        push_line(&mut out, &format!("DESCRIPTION:{description}"));
        push_line(&mut out, "TRANSP:TRANSPARENT");
        push_line(&mut out, "BEGIN:VALARM");
        push_line(&mut out, "ACTION:DISPLAY");
        push_line(&mut out, &format!("DESCRIPTION:{summary}"));
        push_line(&mut out, "TRIGGER:-PT15H");
        push_line(&mut out, "END:VALARM");
        push_line(&mut out, "END:VEVENT");
    }
    push_line(&mut out, "END:VCALENDAR");
    out
}

/// A file name made of `name`'s letters and digits only (no path, no shell character).
pub fn safe_file_stem(name: &str) -> String {
    let stem: String = title_of(name).chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).take(60).collect();
    let stem = stem.trim_matches('-').to_string();
    if stem.is_empty() { "document".to_string() } else { stem }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
    }

    const TODAY: (i32, u32, u32) = (2026, 11, 10);

    fn today() -> NaiveDate {
        day(TODAY.0, TODAY.1, TODAY.2)
    }

    fn dates(text: &str) -> Vec<(NaiveDate, Kind)> {
        find(text, today()).into_iter().map(|d| (d.date, d.kind)).collect()
    }

    #[test]
    fn the_usual_ways_of_writing_a_date() {
        assert_eq!(dates("À régler avant le 15/12/2026."), [(day(2026, 12, 15), Kind::Payment)]);
        assert_eq!(dates("Date limite : 15.12.26"), [(day(2026, 12, 15), Kind::Deadline)]);
        assert_eq!(dates("Expire le 2027-03-01"), [(day(2027, 3, 1), Kind::Expiry)]);
        assert_eq!(dates("Vous êtes convoqué à l'audience du 1er février 2027 à 9 h"), [(day(2027, 2, 1), Kind::Appointment)]);
        assert_eq!(dates("Payment due: December 15, 2026"), [(day(2026, 12, 15), Kind::Payment)]);
        assert_eq!(dates("Échéance : 3 janv. 2027"), [(day(2027, 1, 3), Kind::Payment)]);
    }

    #[test]
    fn a_date_without_year_is_the_next_one() {
        assert_eq!(dates("à payer au plus tard le 20 décembre"), [(day(2026, 12, 20), Kind::Payment)]);
        assert_eq!(dates("à payer au plus tard le 5 janvier"), [(day(2027, 1, 5), Kind::Payment)]);
    }

    #[test]
    fn only_dates_that_ask_for_something_and_are_ahead() {
        let text = "Facture du 01/11/2026\nDate d'émission : 02/11/2026\nÉchéance : 01/12/2026   Émise le 03/12/2026\nAvant le 01/01/2020\nValable jusqu'au 01/01/2040";
        assert_eq!(dates(text), [(day(2026, 12, 1), Kind::Payment)]);
    }

    #[test]
    fn a_table_label_above_its_date() {
        assert_eq!(dates("Date d'échéance\n15/01/2027\n"), [(day(2027, 1, 15), Kind::Payment)]);
        assert!(dates("Montant\n15/01/2027\n").is_empty());
    }

    #[test]
    fn earliest_first_and_one_per_day() {
        let text = "Rendez-vous le 20/01/2027. Paiement avant le 05/12/2026. Rappel : paiement le 05/12/2026.";
        assert_eq!(dates(text), [(day(2026, 12, 5), Kind::Payment), (day(2027, 1, 20), Kind::Appointment)]);
    }

    #[test]
    fn not_dates() {
        for text in [
            "Échéance : 31/02/2027",
            "Échéance : 12/2027",
            "Échéance 1.234.567",
            "Échéance 2026-13-01",
            "Échéance mars",
            "Échéance : 15/12/202",
            "Échéance : 150/12/2026",
        ] {
            assert!(dates(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn hostile_text_never_panics() {
        for text in
            ["", "\n", "1", "1er", "Échéance 1er", "December", "December 1,", "9999-99-99", "Échéance : 31/12/9999", &"Échéance 01/".repeat(2000)]
        {
            let _ = find(text, today());
        }
        let _ = find("Échéance 01/01/2027", NaiveDate::MAX);
        let _ = find("Échéance 01/01/2027", NaiveDate::MIN);
    }

    #[test]
    fn calendar_file() {
        let found = find("À régler, sans frais; ni retard\\, avant le 15/12/2026", today());
        let now = DateTime::<Utc>::from_timestamp(1_794_300_000, 0).unwrap_or_default();
        let text = ics(&found, "Facture EDF.pdf", now);
        assert!(text.starts_with("BEGIN:VCALENDAR\r\n") && text.ends_with("END:VCALENDAR\r\n"), "{text}");
        assert!(text.contains("DTSTART;VALUE=DATE:20261215\r\n") && text.contains("DTEND;VALUE=DATE:20261216\r\n"), "{text}");
        assert!(text.contains("SUMMARY:Paiement — Facture EDF\r\n"), "{text}");
        assert!(text.contains("\\;") && text.contains("\\,") && text.contains("\\\\"), "{text}");
        for line in text.split("\r\n") {
            assert!(line.len() <= 75, "{line:?}");
        }
        // The same document and date give the same id.
        assert_eq!(ics(&found, "Facture EDF.pdf", now), text);
    }

    #[test]
    fn names() {
        assert_eq!(date_fr(day(2026, 12, 15)), "15 décembre 2026");
        assert_eq!(date_fr(day(2027, 3, 1)), "1er mars 2027");
        assert_eq!(title_of("Facture.PDF"), "Facture");
        assert_eq!(title_of("é.pdf"), "é");
        assert_eq!(safe_file_stem("../Facture & co.pdf"), "Facture---co");
        assert_eq!(safe_file_stem("日本.pdf"), "document");
    }
}
