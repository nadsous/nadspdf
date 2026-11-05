//! A document an assistant writes (a report, a letter, a filled-in template), from the Markdown
//! it answers in to the HTML NadsPDF's converter lays out as PDF pages: headings, paragraphs,
//! bulleted and numbered lists, bold, and tables.

/// `markdown` as a complete HTML page titled `title`. Everything is escaped: the text can't add
/// markup of its own.
pub fn to_html(title: &str, markdown: &str) -> String {
    let mut body = String::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut list: Option<&'static str> = None;
    let mut table: Vec<Vec<String>> = Vec::new();
    for raw in markdown.lines() {
        let line = raw.trim();
        let is_row = line.starts_with('|');
        if !is_row && !table.is_empty() {
            push_table(&mut body, &table);
            table.clear();
        }
        let item = list_item(line);
        if item.is_none() && !paragraph.is_empty() && (line.is_empty() || heading(line).is_some() || is_row || is_rule(line)) {
            push_paragraph(&mut body, &paragraph);
            paragraph.clear();
        }
        if let Some(kind) = list
            && item.is_none_or(|(k, _)| k != kind)
        {
            body.push_str(&format!("</{kind}>\n"));
            list = None;
        }
        if line.is_empty() || is_rule(line) {
            continue;
        }
        if let Some((level, text)) = heading(line) {
            body.push_str(&format!("<h{level}>{}</h{level}>\n", inline(text)));
        } else if is_row {
            let cells: Vec<String> = line.trim_matches('|').split('|').map(|c| c.trim().to_string()).collect();
            // The |---|:---:| line under a header row only says how columns align.
            if !cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))) {
                table.push(cells);
            }
        } else if let Some((kind, text)) = item {
            if list.is_none() {
                body.push_str(&format!("<{kind}>\n"));
                list = Some(kind);
            }
            body.push_str(&format!("<li>{}</li>\n", inline(text)));
        } else {
            paragraph.push(line);
        }
    }
    if !paragraph.is_empty() {
        push_paragraph(&mut body, &paragraph);
    }
    if let Some(kind) = list {
        body.push_str(&format!("</{kind}>\n"));
    }
    if !table.is_empty() {
        push_table(&mut body, &table);
    }
    format!("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{}</title></head><body>\n{body}</body></html>\n", escape(title))
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|c| *c == '#').count();
    let text = line.get(level..)?;
    (1..=6).contains(&level).then_some(())?;
    text.starts_with(' ').then(|| (level.min(3), text.trim()))
}

fn is_rule(line: &str) -> bool {
    line.len() >= 3 && (line.chars().all(|c| c == '-') || line.chars().all(|c| c == '*') || line.chars().all(|c| c == '_'))
}

/// A list item: `- `, `* `, `• ` (bulleted) or `1. `, `1) ` (numbered), and its text.
fn list_item(line: &str) -> Option<(&'static str, &str)> {
    for bullet in ["- ", "* ", "• "] {
        if let Some(text) = line.strip_prefix(bullet) {
            return Some(("ul", text.trim()));
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 3 {
        let rest = line.get(digits..)?;
        if let Some(text) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some(("ol", text.trim()));
        }
    }
    None
}

fn push_paragraph(body: &mut String, lines: &[&str]) {
    let html: Vec<String> = lines.iter().map(|l| inline(l)).collect();
    body.push_str(&format!("<p>{}</p>\n", html.join("<br>")));
}

fn push_table(body: &mut String, rows: &[Vec<String>]) {
    body.push_str("<table>\n");
    for (i, row) in rows.iter().enumerate() {
        let tag = if i == 0 { "th" } else { "td" };
        body.push_str("<tr>");
        for cell in row {
            body.push_str(&format!("<{tag}>{}</{tag}>", inline(cell)));
        }
        body.push_str("</tr>\n");
    }
    body.push_str("</table>\n");
}

/// One line's text, escaped, with `**bold**` as bold; the backticks of `code` are dropped.
fn inline(text: &str) -> String {
    let text = text.replace('`', "");
    let mut out = String::new();
    for (i, part) in text.split("**").enumerate() {
        // Odd parts sit between a pair of `**`; a lone `**` at the end stays as written.
        if i % 2 == 1 {
            out.push_str(&format!("<b>{}</b>", escape(part)));
        } else {
            out.push_str(&escape(part));
        }
    }
    if text.matches("**").count() % 2 == 1 {
        // Unbalanced: write it back as plain text rather than leave a bold run open.
        return escape(&text);
    }
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_becomes_headings_paragraphs_lists_and_tables() {
        let md = "# Rapport d'intervention\n\nClient : **Dupont**\nAdresse : 3 rue <B>\n\n## Travaux\n- Remplacement du disjoncteur\n- Test différentiel 30 mA\n\n1. Couper\n2. Vérifier\n\n| Circuit | Mesure |\n|---|:-:|\n| Prises | 0,45 MΩ |\n\n---\nFin **non fermé";
        let html = to_html("Rapport & co", md);
        assert!(html.contains("<title>Rapport &amp; co</title>"));
        assert!(html.contains("<h1>Rapport d'intervention</h1>"));
        assert!(html.contains("<p>Client : <b>Dupont</b><br>Adresse : 3 rue &lt;B&gt;</p>"), "{html}");
        assert!(html.contains("<h2>Travaux</h2>\n<ul>\n<li>Remplacement du disjoncteur</li>\n<li>Test différentiel 30 mA</li>\n</ul>"), "{html}");
        assert!(html.contains("<ol>\n<li>Couper</li>\n<li>Vérifier</li>\n</ol>"), "{html}");
        assert!(html.contains("<tr><th>Circuit</th><th>Mesure</th></tr>\n<tr><td>Prises</td><td>0,45 MΩ</td></tr>"), "{html}");
        assert!(html.contains("<p>Fin **non fermé</p>"), "an unbalanced ** stays as written: {html}");
        assert!(!html.contains("---"));
    }

    #[test]
    fn empty_and_odd_input_is_harmless() {
        assert!(to_html("", "").contains("<body>\n</body>"));
        let html = to_html("t", "#\n####### x\n#nospace\n- \n12345. trop\n|\n");
        assert!(!["<h1>", "<h2>", "<h3>"].iter().any(|h| html.contains(h)), "{html}");
    }
}
