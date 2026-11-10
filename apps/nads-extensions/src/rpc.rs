//! The extension side of protocol 1 (`docs/extensions.md`): read NadsPDF's lines on stdin, ask for
//! each opened document's text, and hand the text and the banner clicks to an [`Extension`].

use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

/// The longest line read from NadsPDF. `doc.text` answers up to 10 pages of 100,000 characters,
/// up to 4 bytes each and escaped in JSON: a longer line is skipped, never kept.
pub const MAX_LINE: usize = 16 * 1024 * 1024;

/// Banners take one line of up to 300 characters.
pub const MAX_BANNER_TEXT: usize = 300;

/// What NadsPDF said, reduced to what the extensions use.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Opened {
        doc: u64,
        name: String,
    },
    Closed {
        doc: u64,
    },
    Action {
        doc: u64,
        banner: String,
        action: String,
    },
    /// The answer to one of our requests; `None` when it was an error.
    Response {
        id: u64,
        result: Option<Value>,
    },
}

/// One line from NadsPDF, or `None` for anything else (other notifications, garbage).
pub fn parse(line: &str) -> Option<Event> {
    let value: Value = serde_json::from_str(line).ok()?;
    let params = value.get("params");
    let doc = || params.and_then(|p| p.get("doc")).and_then(Value::as_u64);
    let text = |k: &str| params.and_then(|p| p.get(k)).and_then(Value::as_str).map(str::to_string);
    match value.get("method").and_then(Value::as_str) {
        Some("document.opened") => Some(Event::Opened { doc: doc()?, name: text("name").unwrap_or_default() }),
        Some("document.closed") => Some(Event::Closed { doc: doc()? }),
        Some("banner.action") => Some(Event::Action { doc: doc()?, banner: text("banner")?, action: text("action")? }),
        Some(_) => None,
        None => {
            let id = value.get("id").and_then(Value::as_u64)?;
            Some(Event::Response { id, result: value.get("result").cloned() })
        }
    }
}

/// The text of a `doc.text` result, pages separated by a blank line.
pub fn page_texts(result: &Value) -> String {
    let pages = result.get("pages").and_then(Value::as_array);
    let texts = pages.into_iter().flatten().filter_map(|p| p.get("text").and_then(Value::as_str));
    texts.collect::<Vec<_>>().join("\n\n")
}

/// A banner button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub id: &'static str,
    pub label: &'static str,
    pub primary: bool,
}

/// Writes requests to NadsPDF, each with a new id.
pub struct Out<W: Write> {
    w: W,
    next: u64,
}

impl<W: Write> Out<W> {
    pub fn new(w: W) -> Self {
        Out { w, next: 1 }
    }

    /// Sends `method` and returns its id.
    pub fn request(&mut self, method: &str, params: Value) -> io::Result<u64> {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.w, "{line}")?;
        self.w.flush()?;
        Ok(id)
    }

    /// Shows (or replaces) the banner `id` above document `doc`.
    pub fn banner(&mut self, doc: u64, id: &str, text: &str, buttons: &[Button]) -> io::Result<u64> {
        let actions: Vec<Value> = buttons.iter().take(3).map(|b| json!({ "id": b.id, "label": b.label, "primary": b.primary })).collect();
        let text = crate::text::one_line(text, MAX_BANNER_TEXT);
        self.request("ui.banner.show", json!({ "doc": doc, "id": id, "text": text, "actions": actions }))
    }

    pub fn hide(&mut self, doc: u64, id: &str) -> io::Result<u64> {
        self.request("ui.banner.hide", json!({ "doc": doc, "id": id }))
    }
}

/// What an extension does with the documents' text and its banners' buttons.
pub trait Extension {
    /// The text of the first pages of `doc` (named `name`) arrived.
    fn on_text<W: Write>(&mut self, out: &mut Out<W>, doc: u64, name: &str, text: &str) -> io::Result<()>;
    /// The user clicked `action` on the banner `banner` of `doc`.
    fn on_action<W: Write>(&mut self, out: &mut Out<W>, doc: u64, banner: &str, action: &str) -> io::Result<()>;
    /// `doc` closed: forget it.
    fn on_closed(&mut self, doc: u64);
}

/// Reads one line of at most `max` bytes into `buf` (without its line ending). `Ok(None)` at the
/// end of input; `Ok(Some(false))` for a line that was too long, skipped.
pub fn read_line_capped(input: &mut impl BufRead, buf: &mut Vec<u8>, max: usize) -> io::Result<Option<bool>> {
    buf.clear();
    let mut too_long = false;
    let mut read_any = false;
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if read_any { Some(!too_long) } else { None });
        }
        read_any = true;
        let (part, found, used) = match chunk.iter().position(|&b| b == b'\n') {
            Some(n) => (chunk.get(..n).unwrap_or_default(), true, n.saturating_add(1)),
            None => (chunk, false, chunk.len()),
        };
        if !too_long {
            if buf.len().saturating_add(part.len()) > max {
                too_long = true;
                buf.clear();
            } else {
                buf.extend_from_slice(part);
            }
        }
        input.consume(used);
        if found {
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(Some(!too_long));
        }
    }
}

/// Runs `ext` until NadsPDF closes our stdin.
pub fn run<E: Extension>(ext: &mut E, input: impl BufRead, output: impl Write) -> io::Result<()> {
    let mut input = input;
    let mut out = Out::new(output);
    // Our `doc.text` requests still unanswered: id → (document, name).
    let mut pending: HashMap<u64, (u64, String)> = HashMap::new();
    let mut buf = Vec::new();
    while let Some(complete) = read_line_capped(&mut input, &mut buf, MAX_LINE)? {
        if !complete {
            eprintln!("skipped a line longer than {MAX_LINE} bytes");
            continue;
        }
        let Ok(line) = std::str::from_utf8(&buf) else { continue };
        match parse(line) {
            Some(Event::Opened { doc, name }) => {
                let id = out.request("doc.text", json!({ "doc": doc }))?;
                pending.insert(id, (doc, name));
            }
            Some(Event::Closed { doc }) => {
                pending.retain(|_, (d, _)| *d != doc);
                ext.on_closed(doc);
            }
            Some(Event::Action { doc, banner, action }) => ext.on_action(&mut out, doc, &banner, &action)?,
            Some(Event::Response { id, result }) => {
                if let Some((doc, name)) = pending.remove(&id) {
                    match result {
                        Some(result) => ext.on_text(&mut out, doc, &name, &page_texts(&result))?,
                        None => eprintln!("could not read the text of document {doc}"),
                    }
                }
            }
            None => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_and_answers_parse() {
        let opened = r#"{"jsonrpc":"2.0","method":"document.opened","params":{"doc":3,"name":"f.pdf","pages":2,"path":null}}"#;
        assert_eq!(parse(opened), Some(Event::Opened { doc: 3, name: "f.pdf".into() }));
        let action = r#"{"jsonrpc":"2.0","method":"banner.action","params":{"doc":3,"banner":"b","action":"a"}}"#;
        assert_eq!(parse(action), Some(Event::Action { doc: 3, banner: "b".into(), action: "a".into() }));
        let error = r#"{"jsonrpc":"2.0","id":4,"error":{"code":-32002,"message":"x"}}"#;
        assert_eq!(parse(error), Some(Event::Response { id: 4, result: None }));
        assert_eq!(parse(r#"{"jsonrpc":"2.0","method":"host.started","params":{}}"#), None);
        assert_eq!(parse("{nope"), None);
        assert_eq!(parse(r#"{"method":"document.opened","params":{"doc":-1}}"#), None);
    }

    #[test]
    fn long_lines_are_skipped_whole() {
        let data = format!("{}\nshort\r\nlast", "x".repeat(50));
        let mut input = io::BufReader::with_capacity(8, data.as_bytes());
        let mut buf = Vec::new();
        assert_eq!(read_line_capped(&mut input, &mut buf, 10).ok(), Some(Some(false)));
        assert_eq!(read_line_capped(&mut input, &mut buf, 10).ok(), Some(Some(true)));
        assert_eq!(buf, b"short");
        assert_eq!(read_line_capped(&mut input, &mut buf, 10).ok(), Some(Some(true)));
        assert_eq!(buf, b"last");
        assert_eq!(read_line_capped(&mut input, &mut buf, 10).ok(), Some(None));
    }

    struct Echo(Vec<(u64, String)>);

    impl Extension for Echo {
        fn on_text<W: Write>(&mut self, _: &mut Out<W>, doc: u64, _: &str, text: &str) -> io::Result<()> {
            self.0.push((doc, text.to_string()));
            Ok(())
        }
        fn on_action<W: Write>(&mut self, _: &mut Out<W>, _: u64, _: &str, _: &str) -> io::Result<()> {
            Ok(())
        }
        fn on_closed(&mut self, _: u64) {}
    }

    #[test]
    fn an_opened_document_is_read_and_its_text_handed_over() {
        let input = [
            r#"{"jsonrpc":"2.0","method":"host.started","params":{"host":"NadsPDF","version":"0.8.1","protocol":1}}"#,
            r#"{"jsonrpc":"2.0","method":"document.opened","params":{"doc":7,"name":"a.pdf","pages":2,"path":null}}"#,
            "garbage",
            r#"{"jsonrpc":"2.0","id":1,"result":{"doc":7,"pages":[{"page":1,"text":"un"},{"page":2,"text":"deux"}]}}"#,
            r#"{"jsonrpc":"2.0","id":1,"result":{"doc":7,"pages":[]}}"#,
        ]
        .join("\n");
        let mut sent = Vec::new();
        let mut ext = Echo(Vec::new());
        assert!(run(&mut ext, input.as_bytes(), &mut sent).is_ok());
        assert_eq!(ext.0, vec![(7, "un\n\ndeux".to_string())], "answered once, the repeat ignored");
        let sent = String::from_utf8(sent).unwrap_or_default();
        assert!(sent.contains(r#""method":"doc.text""#) && sent.contains(r#""doc":7"#), "{sent}");
    }
}
