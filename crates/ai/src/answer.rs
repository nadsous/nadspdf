//! What an answer holds beyond its text: Süleyman's table and Selim's proposed actions, read
//! from the ```json block the instructions ask for. Anything malformed is left out (the text
//! still shows), and sizes are capped.

use serde_json::Value;

const MAX_ROWS: usize = 20_000;
const MAX_COLUMNS: usize = 100;
const MAX_CELL: usize = 4_000;
const MAX_ACTIONS: usize = 50;

/// One table cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Empty,
    Text(String),
    Number(f64),
}

impl Cell {
    pub fn as_text(&self) -> String {
        match self {
            Cell::Empty => String::new(),
            Cell::Text(t) => t.clone(),
            Cell::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
        }
    }
}

/// A table to show and export.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    pub notes: String,
}

/// One action Selim proposes.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Highlight { text: String },
    Redact { text: String },
    Bookmark { title: String, page: usize },
    GoToPage { page: usize },
    ExtractPages { pages: String },
}

/// The answer split for display: the text without its JSON block, and what the block held.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    pub text: String,
    pub table: Option<Table>,
    pub actions: Vec<Action>,
    /// Actions left out (unknown, or missing what they need).
    pub skipped: usize,
}

/// The first ```json block of `text` (or a bare ``` block holding JSON), and `text` without it.
fn json_block(text: &str) -> Option<(Value, String)> {
    let start = text.find("```")?;
    let after = text.get(start + 3..)?;
    let body_start = after.find('\n').map_or(0, |n| n + 1);
    let body = after.get(body_start..)?;
    let end = body.find("```")?;
    let json = body.get(..end)?;
    let value: Value = serde_json::from_str(json.trim()).ok()?;
    let rest_start = start + 3 + body_start + end + 3;
    let without = format!(
        "{}{}",
        text.get(..start).unwrap_or_default().trim_end(),
        text.get(rest_start..).map(|r| format!("\n{}", r.trim_start())).unwrap_or_default()
    );
    Some((value, without.trim().to_string()))
}

fn cell(v: &Value) -> Cell {
    match v {
        Value::Null => Cell::Empty,
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()).map_or(Cell::Empty, Cell::Number),
        Value::String(s) => Cell::Text(s.chars().take(MAX_CELL).collect()),
        Value::Bool(b) => Cell::Text(b.to_string()),
        other => Cell::Text(other.to_string().chars().take(MAX_CELL).collect()),
    }
}

fn text_of(v: &Value) -> Option<String> {
    v.as_str().map(|s| s.trim().chars().take(MAX_CELL).collect::<String>()).filter(|s| !s.is_empty())
}

fn page_of(v: &Value) -> Option<usize> {
    v.as_u64().and_then(|p| usize::try_from(p).ok()).filter(|p| *p >= 1)
}

fn table(v: &Value) -> Option<Table> {
    let columns: Vec<String> =
        v["columns"].as_array()?.iter().take(MAX_COLUMNS).map(|c| c.as_str().map_or_else(|| c.to_string(), str::to_string)).collect();
    if columns.is_empty() {
        return None;
    }
    let rows = v["rows"]
        .as_array()?
        .iter()
        .take(MAX_ROWS)
        .filter_map(|r| r.as_array())
        .map(|r| {
            let mut row: Vec<Cell> = r.iter().take(columns.len()).map(cell).collect();
            row.resize(columns.len(), Cell::Empty);
            row
        })
        .collect();
    Some(Table { title: text_of(&v["title"]).unwrap_or_default(), columns, rows, notes: text_of(&v["notes"]).unwrap_or_default() })
}

fn actions(v: &Value) -> (Vec<Action>, usize) {
    let Some(list) = v["actions"].as_array() else { return (Vec::new(), 0) };
    let mut out = Vec::new();
    let mut skipped = 0;
    for a in list {
        let action = match a["action"].as_str() {
            Some("highlight") => text_of(&a["text"]).map(|text| Action::Highlight { text }),
            Some("redact") => text_of(&a["text"]).map(|text| Action::Redact { text }),
            Some("bookmark") => text_of(&a["title"]).zip(page_of(&a["page"])).map(|(title, page)| Action::Bookmark { title, page }),
            Some("go_to_page") => page_of(&a["page"]).map(|page| Action::GoToPage { page }),
            Some("extract_pages") => text_of(&a["pages"]).map(|pages| Action::ExtractPages { pages }),
            _ => None,
        };
        match action {
            Some(action) if out.len() < MAX_ACTIONS => out.push(action),
            _ => skipped += 1,
        }
    }
    (out, skipped)
}

/// Split an answer into its text and what its JSON block holds.
pub fn parse(text: &str) -> Parsed {
    let Some((value, without)) = json_block(text) else { return Parsed { text: text.trim().to_string(), ..Parsed::default() } };
    let (acts, skipped) = actions(&value);
    Parsed { text: without, table: table(&value), actions: acts, skipped }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_is_read_and_the_block_removed_from_the_text() {
        let answer = "Voici les factures.\n```json\n{\"title\":\"Factures\",\"columns\":[\"N°\",\"Montant (EUR)\",\"Page\"],\"rows\":[[\"F-1\",1234.5,2],[\"F-2\",null],[\"F-3\",7,4,\"extra\"]],\"notes\":\"\"}\n```\nBonne lecture.";
        let p = parse(answer);
        assert_eq!(p.text, "Voici les factures.\nBonne lecture.");
        let t = p.table.unwrap();
        assert_eq!(t.columns.len(), 3);
        assert_eq!(t.rows[0][1], Cell::Number(1234.5));
        assert_eq!(t.rows[1], vec![Cell::Text("F-2".into()), Cell::Empty, Cell::Empty], "short rows are padded");
        assert_eq!(t.rows[2].len(), 3, "long rows are cut");
        assert_eq!(Cell::Number(7.0).as_text(), "7");
    }

    #[test]
    fn only_known_complete_actions_are_kept() {
        let answer = "```json\n{\"actions\":[{\"action\":\"highlight\",\"text\":\"31 mars\"},{\"action\":\"bookmark\",\"title\":\"Prix\"},{\"action\":\"delete_everything\"},{\"action\":\"go_to_page\",\"page\":0},{\"action\":\"extract_pages\",\"pages\":\"2-3\"}]}\n```";
        let p = parse(answer);
        assert_eq!(p.actions, vec![Action::Highlight { text: "31 mars".into() }, Action::ExtractPages { pages: "2-3".into() }]);
        assert_eq!(p.skipped, 3);
    }

    #[test]
    fn plain_or_broken_answers_are_only_text() {
        assert_eq!(parse("Rien à extraire.").text, "Rien à extraire.");
        let p = parse("```json\n{broken\n```");
        assert!(p.table.is_none() && p.actions.is_empty());
    }
}
