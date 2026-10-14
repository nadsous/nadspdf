//! pdfcraft-convert — convert Office and text documents to PDF in pure Rust (L4). See the README.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod docx;
mod html;
mod layout;
mod model;
mod sheet;
mod xml;
mod zipfile;

pub use layout::Report;
use model::{Block, Kind, Run, Source};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConvertError {
    /// The file type is not one this crate converts.
    #[error("{0}")]
    Unsupported(String),
    /// The file is damaged or not what its name says.
    #[error("{0}")]
    Malformed(String),
    /// The file is larger than a conversion accepts.
    #[error("{0}")]
    TooLarge(String),
    /// Writing the PDF failed.
    #[error("{0}")]
    Pdf(String),
}

/// File extensions [`convert`] accepts (lower case, without the dot).
pub const EXTENSIONS: [&str; 10] = ["docx", "xlsx", "xlsm", "ods", "csv", "tsv", "html", "htm", "txt", "text"];

/// File extensions [`convert`] accepts (lower case, without the dot).
pub fn supported_extensions() -> &'static [&'static str] {
    &EXTENSIONS
}

/// Extensions that are recognised but refused with an explanation (binary Office formats).
const REFUSED: [&str; 3] = ["xls", "doc", "ppt"];

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// Whether `name` is a file this crate handles: one it converts ([`EXTENSIONS`]) or a binary
/// Office format it refuses with an explanation (`.xls`, `.doc`, `.ppt`).
pub fn handles(name: &str) -> bool {
    let ext = extension(name);
    EXTENSIONS.contains(&ext.as_str()) || REFUSED.contains(&ext.as_str())
}

/// Most bytes a source file may have.
const MAX_INPUT: usize = 256 << 20;

/// Convert the file `name` (its extension picks the format) to a new PDF document.
pub fn convert(name: &str, bytes: &[u8]) -> Result<pdfcraft_cos::Document, ConvertError> {
    convert_with_report(name, bytes).map(|(doc, _)| doc)
}

/// [`convert`], also reporting what could not be carried over exactly.
pub fn convert_with_report(name: &str, bytes: &[u8]) -> Result<(pdfcraft_cos::Document, Report), ConvertError> {
    if bytes.len() > MAX_INPUT {
        return Err(ConvertError::TooLarge(format!("{name} is larger than {} MB", MAX_INPUT >> 20)));
    }
    let source = match extension(name).as_str() {
        "docx" => docx::read(bytes)?,
        "xlsx" | "xlsm" => sheet::xlsx(bytes)?,
        "ods" => sheet::ods(bytes)?,
        "csv" => sheet::csv(bytes, None)?,
        "tsv" => sheet::csv(bytes, Some('\t'))?,
        "html" | "htm" => html::read(&pdfcraft_create::decode_text(bytes)),
        "txt" | "text" => plain_text(&pdfcraft_create::decode_text(bytes)),
        "xls" => {
            return Err(ConvertError::Unsupported(
                "the .xls format (Excel 97-2003) isn't supported: save the workbook as .xlsx and convert that".into(),
            ));
        }
        "doc" => {
            return Err(ConvertError::Unsupported(
                "the .doc format (Word 97-2003) isn't supported: save the document as .docx and convert that".into(),
            ));
        }
        "ppt" => return Err(ConvertError::Unsupported("PowerPoint presentations aren't supported".into())),
        other => {
            return Err(ConvertError::Unsupported(format!(
                "can't convert {}: use one of {}",
                if other.is_empty() { "a file without an extension".to_string() } else { format!(".{other} files") },
                EXTENSIONS.map(|e| format!(".{e}")).join(", ")
            )));
        }
    };
    let title = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let title = title.rsplit_once('.').map_or(title, |(s, _)| s);
    layout::render(title, &source)
}

/// Plain text: one paragraph per line, blank lines kept as space, form feeds start pages.
fn plain_text(text: &str) -> Source {
    let mut source = Source::default();
    for (i, page) in text.replace("\r\n", "\n").replace('\r', "\n").split('\u{c}').enumerate() {
        if i > 0 {
            source.blocks.push(Block::PageBreak);
        }
        let page = page.strip_suffix('\n').unwrap_or(page);
        for line in page.split('\n') {
            source.push_para(Kind::Body, vec![Run::plain(line)]);
        }
    }
    source
}

/// How a character is written in Helvetica (WinAnsiEncoding).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mapped {
    /// As this character (itself, or the nearest one WinAnsi has).
    Char(char),
    /// As these characters (a ligature spelled out, an arrow as `->`).
    Str(&'static str),
    /// Not at all (zero-width and control characters).
    Drop,
    /// As `[U+XXXX]`: there is no faithful substitute.
    Escape,
}

/// The way `c` is written in Helvetica: itself when WinAnsiEncoding has it (every French,
/// German, Spanish… letter, `« » ’ … – — œ €`), a close equivalent for common typographic
/// characters it lacks, and otherwise its code point, as `from_text_escaped` does, never `?`.
pub(crate) fn map_char(c: char) -> Mapped {
    match c {
        // A soft hyphen is invisible unless the line breaks there.
        '\u{ad}' | '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}' | '\u{fe00}'..='\u{fe0f}' => Mapped::Drop,
        _ if pdfcraft_fonts::win_ansi_byte(c).is_some() => Mapped::Char(c),
        '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => Mapped::Drop,
        // Narrow and other fixed-width no-break spaces (French « », ; : ! ?) keep words together.
        '\u{202f}' | '\u{2007}' => Mapped::Char('\u{a0}'),
        '\u{2061}'..='\u{2064}' => Mapped::Drop,
        '\u{2000}'..='\u{200a}' | '\u{205f}' | '\u{3000}' => Mapped::Char(' '),
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2043}' | '\u{2212}' | '\u{fe63}' | '\u{ff0d}' => Mapped::Char('-'),
        '\u{2015}' => Mapped::Char('\u{2014}'),
        '\u{2032}' => Mapped::Char('\''),
        '\u{2033}' => Mapped::Char('"'),
        '\u{2044}' | '\u{2215}' => Mapped::Char('/'),
        '\u{2024}' => Mapped::Char('.'),
        '\u{2025}' => Mapped::Str(".."),
        '\u{2027}' | '\u{22c5}' => Mapped::Char('\u{b7}'),
        // Bullets, including the private-use one Word's Symbol-font bullets use.
        '\u{25cf}' | '\u{25aa}' | '\u{25a0}' | '\u{25e6}' | '\u{25cb}' | '\u{2023}' | '\u{2219}' | '\u{f0b7}' | '\u{f0a7}' | '\u{2981}' => {
            Mapped::Char('\u{2022}')
        }
        '\u{fb00}' => Mapped::Str("ff"),
        '\u{fb01}' => Mapped::Str("fi"),
        '\u{fb02}' => Mapped::Str("fl"),
        '\u{fb03}' => Mapped::Str("ffi"),
        '\u{fb04}' => Mapped::Str("ffl"),
        '\u{2190}' => Mapped::Str("<-"),
        '\u{2192}' | '\u{f0e0}' => Mapped::Str("->"),
        '\u{2194}' => Mapped::Str("<->"),
        '\u{21d2}' => Mapped::Str("=>"),
        '\u{2264}' => Mapped::Str("<="),
        '\u{2265}' => Mapped::Str(">="),
        '\u{2260}' => Mapped::Str("!="),
        '\u{2248}' => Mapped::Char('~'),
        _ => Mapped::Escape,
    }
}

#[cfg(test)]
mod tests;
