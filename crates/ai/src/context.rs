//! Which pages of the document go with a question, within a size budget.
//!
//! A short document goes whole. A long one sends the pages the question names ("page 12",
//! "p. 3-5"), then the pages sharing the most words with the question, then the first pages,
//! until the budget is used, and keeps them in page order. The user sees which pages were sent.

use std::collections::BTreeSet;

/// The text sent and which pages it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    /// The first message's document part: a line saying which pages, then `<document>` … `</document>`.
    pub text: String,
    /// The pages included (0-based).
    pub pages: Vec<usize>,
    /// Whether some pages were left out.
    pub partial: bool,
}

/// Lowercase, accents folded, the way two words are compared.
fn fold(word: &str) -> String {
    word.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ş' => 's',
            'ğ' => 'g',
            'ı' => 'i',
            c => c,
        })
        .collect()
}

fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3).map(fold).collect()
}

/// Page numbers (1-based) the question names: "page 12", "p. 3-5", "pages 2 et 7".
fn named_pages(question: &str, count: usize) -> Vec<usize> {
    let lower = fold(question);
    let mut out = Vec::new();
    let mut rest = lower.as_str();
    while let Some(at) = rest.find(['p']) {
        let after = rest.get(at..).unwrap_or_default();
        let tail = after
            .strip_prefix("pages")
            .or_else(|| after.strip_prefix("page"))
            .or_else(|| after.strip_prefix("pp."))
            .or_else(|| after.strip_prefix("p."));
        rest = after.get(1..).unwrap_or_default();
        let Some(tail) = tail else { continue };
        // Numbers and ranges right after it: "3-5", "2, 7", "2 et 7".
        let spec: String = tail.chars().take_while(|c| c.is_ascii_digit() || matches!(c, ' ' | ',' | '-' | '–' | 'e' | 't' | '&')).collect();
        for part in spec.split([',', '&']).flat_map(|p| p.split(" et ")) {
            let mut ends = part.split(['-', '–']).map(|n| n.trim().parse::<usize>().ok());
            match (ends.next().flatten(), ends.next().flatten()) {
                (Some(a), Some(b)) if a >= 1 && b >= a => out.extend((a..=b.min(a.saturating_add(50))).filter(|p| *p <= count)),
                (Some(a), None) if a >= 1 && a <= count => out.push(a),
                _ => {}
            }
        }
    }
    out
}

/// `pages` as ranges for people: "1–5, 9".
pub fn describe(pages: &[usize]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut iter = pages.iter().map(|p| p + 1).peekable();
    while let Some(start) = iter.next() {
        let mut end = start;
        while iter.peek() == Some(&(end + 1)) {
            end += 1;
            iter.next();
        }
        out.push(if start == end { start.to_string() } else { format!("{start}–{end}") });
    }
    out.join(", ")
}

/// The context for `question` from `pages` (each page's text, in order), at most `budget`
/// characters of page text.
pub fn build(pages: &[String], question: &str, budget: usize) -> Context {
    let total: usize = pages.iter().map(|p| p.chars().count()).sum();
    let chosen: Vec<usize> = if total <= budget {
        (0..pages.len()).collect()
    } else {
        let q = words(question);
        let mut order: Vec<usize> = named_pages(question, pages.len()).into_iter().map(|p| p - 1).collect();
        // Each shared word counts by its rarity in the document: one on every page ("page")
        // tells nothing, one on a single page ("résiliation") tells a lot.
        let page_words: Vec<BTreeSet<String>> = pages.iter().map(|t| words(t)).collect();
        let n = pages.len().max(1) as f64;
        let weight = |w: &String| {
            let df = page_words.iter().filter(|p| p.contains(w)).count().max(1) as f64;
            (n / df).ln()
        };
        let mut scored: Vec<(f64, usize)> = page_words.iter().enumerate().map(|(i, p)| (p.intersection(&q).map(weight).sum::<f64>(), i)).collect();
        // Most telling pages first, earlier pages first among equals.
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        order.extend(scored.into_iter().filter(|(s, _)| *s > 0.0).map(|(_, i)| i));
        order.extend(0..pages.len());
        let mut picked = BTreeSet::new();
        let mut used = 0usize;
        for i in order {
            if picked.contains(&i) {
                continue;
            }
            let len = pages.get(i).map_or(0, |p| p.chars().count());
            if used.saturating_add(len) > budget {
                continue;
            }
            used = used.saturating_add(len);
            picked.insert(i);
        }
        picked.into_iter().collect()
    };
    let partial = chosen.len() < pages.len();
    let mut text = if partial {
        format!("The document has {} pages; these are included: {}.\n<document>\n", pages.len(), describe(&chosen))
    } else {
        format!("The document has {} pages, all included.\n<document>\n", pages.len())
    };
    for &i in &chosen {
        text.push_str(&format!("[Page {}]\n", i + 1));
        text.push_str(pages.get(i).map(String::as_str).unwrap_or_default().trim());
        text.push_str("\n\n");
    }
    text.push_str("</document>");
    Context { text, pages: chosen, partial }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_document_goes_whole() {
        let pages = vec!["Contrat de location".to_string(), "Loyer : 800 €".to_string()];
        let c = build(&pages, "Quel est le loyer ?", 10_000);
        assert_eq!(c.pages, [0, 1]);
        assert!(!c.partial && c.text.contains("[Page 2]\nLoyer : 800 €") && c.text.ends_with("</document>"));
    }

    #[test]
    fn a_long_document_sends_the_named_and_matching_pages_in_order() {
        let mut pages: Vec<String> = (0..40).map(|i| format!("Page {i} filler text about nothing in particular")).collect();
        pages[25] = "Clause de résiliation : préavis de trois mois".into();
        let c = build(&pages, "Que dit la résiliation ? Voir aussi page 3", 120);
        assert!(c.partial);
        assert!(c.pages.contains(&2), "page 3 is named");
        assert!(c.pages.contains(&25), "the page sharing words with the question");
        assert!(c.pages.windows(2).all(|w| w[0] < w[1]), "in page order");
        assert!(c.text.starts_with("The document has 40 pages; these are included:"));
    }

    #[test]
    fn page_ranges_are_read_and_described() {
        assert_eq!(named_pages("résume les pages 3-5 et la p. 9", 20), [3, 4, 5, 9]);
        assert_eq!(named_pages("page 999", 20), Vec::<usize>::new());
        assert_eq!(describe(&[0, 1, 2, 4, 7, 8]), "1–3, 5, 8–9");
        assert_eq!(describe(&[]), "");
    }
}
