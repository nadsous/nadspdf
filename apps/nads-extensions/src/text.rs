//! Small text helpers shared by the extensions. Everything works on `char`s, never on byte
//! positions, so no input can split a character.

/// `c` lowercased and without its accent, as one `char`, so a folded text has exactly as many
/// characters as the original and positions carry over.
pub fn fold(c: char) -> char {
    let lower = c.to_lowercase().next().unwrap_or(c);
    match lower {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        '’' | '‘' | '`' | '´' => '\'',
        '\u{a0}' | '\u{202f}' | '\u{2009}' | '\t' => ' ',
        other => other,
    }
}

/// The folded characters of `text` (see [`fold`]).
pub fn folded(text: &str) -> Vec<char> {
    text.chars().map(fold).collect()
}

/// Whether `hay` holds `needle` (already folded) starting at `at`.
pub fn starts_with_at(hay: &[char], at: usize, needle: &str) -> bool {
    let mut i = at;
    for n in needle.chars() {
        if hay.get(i) != Some(&n) {
            return false;
        }
        i = i.saturating_add(1);
    }
    true
}

/// Whether one of `needles` (already folded) occurs inside `hay[from..to]`.
pub fn window_has(hay: &[char], from: usize, to: usize, needles: &[&str]) -> bool {
    let to = to.min(hay.len());
    (from..to).any(|i| needles.iter().any(|n| starts_with_at(hay, i, n) && i.saturating_add(n.chars().count()) <= to))
}

/// Whether the character at `i` (if any) is a letter or a digit.
pub fn is_alnum_at(hay: &[char], i: Option<usize>) -> bool {
    i.and_then(|i| hay.get(i)).is_some_and(|c| c.is_alphanumeric())
}

/// `text` on one line of at most `max` characters: control characters become spaces, runs of
/// spaces shrink to one, and a cut text ends with "…". Banners refuse anything else.
pub fn one_line(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    let mut last_space = true;
    for c in text.chars() {
        let c = if c.is_control() || c.is_whitespace() { ' ' } else { c };
        if c == ' ' && last_space {
            continue;
        }
        if count.saturating_add(1) >= max {
            out.push('…');
            return out;
        }
        last_space = c == ' ';
        out.push(c);
        count = count.saturating_add(1);
    }
    out.trim_end().to_string()
}

/// A 64-bit FNV-1a hash, for stable identifiers (not for security).
pub fn fnv1a(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_keeps_one_char_per_char() {
        let text = "Échéance À RÉGLER İstanbul ß";
        assert_eq!(folded(text).len(), text.chars().count());
        assert_eq!(folded("Échéance").iter().collect::<String>(), "echeance");
    }

    #[test]
    fn one_line_is_short_and_has_no_control_characters() {
        assert_eq!(one_line("a\n\n b\tc ", 300), "a b c");
        let long = one_line(&"é".repeat(500), 300);
        assert_eq!(long.chars().count(), 300);
        assert!(long.ends_with('…'));
        assert_eq!(one_line("", 10), "");
    }

    #[test]
    fn windows_stay_inside_the_text() {
        let hay = folded("IBAN : FR76");
        assert!(window_has(&hay, 0, 99, &["iban"]));
        assert!(!window_has(&hay, 0, 3, &["iban"]));
        assert!(!window_has(&hay, 50, 99, &["iban"]));
        assert!(!is_alnum_at(&hay, None));
        assert!(!is_alnum_at(&hay, Some(500)));
    }
}
