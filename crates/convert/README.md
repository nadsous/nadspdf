# pdfcraft-convert

Layer L4 (beside `pdfcraft-create`, which it uses for embedded images and text decoding).
Converts Office and text documents to PDF in pure Rust: no LibreOffice, no Chromium, no other
external program.

- `convert(name, bytes) -> Result<pdfcraft_cos::Document, ConvertError>`: the extension of
  `name` picks the format; `convert_with_report` also returns a `Report` (pages written,
  characters escaped, images skipped, notes on what was left out).
- `supported_extensions()` / `EXTENSIONS`: `docx xlsx xlsm ods csv tsv html htm txt text`.
- `handles(name)`: also true for `.xls`, `.doc` and `.ppt`, which are refused with an error
  that says what to do (save as `.xlsx` / `.docx`).

## Formats

- **Word (`.docx`)**: `word/document.xml` with `styles.xml`, `numbering.xml` and the
  relationships. Paragraphs and runs (bold, italic, underline, from the run, its character style
  and its paragraph style), headings (by the style's built-in name `heading N` / `Title`, which
  Word keeps in English whatever the language, its outline level, or ids such as `Titre1`),
  bullet and numbered lists (decimal, letters, roman; nested levels), alignment, tabs, line and
  page breaks, section breaks, landscape sections, tables (grid widths, column spans, vertical
  merges, header rows repeated on each page; nested tables flattened as `a | b` lines), embedded
  PNG/JPEG/… images at their drawing size, scaled to the text width. Field codes, deleted text and
  hidden text are left out. Not carried over: fonts and colours, headers and footers, footnotes,
  text boxes, floating image positions, images inside table cells, charts, comments.
- **Excel (`.xlsx`, `.xlsm`) and OpenDocument (`.ods`)**: every visible sheet on its own page
  under its name, as a table with the first row as header. Shared, inline and rich strings,
  numbers (general format, percentages, dates and times from the cell's number format), booleans.
  Empty rows and columns are left out; a sheet too wide for portrait turns landscape, and its text
  is shrunk (down to 6 pt) and then its columns are printed in groups. Formulas show their cached
  values. Not carried over: cell formatting, merged cells, charts, images, hidden rows/columns.
- **CSV / TSV**: delimiter detected (`,` `;` tab), RFC 4180 quotes; text decoded as UTF-8/UTF-16
  (BOM) or Windows-1252.
- **HTML (`.html`, `.htm`)**: a lenient tag scanner (no DOM): `h1`–`h6`, `p`/`div` and other
  blocks, `br`, `b/strong`, `i/em`, `u`, `ul/ol/li` (with `start`), `table/tr/td/th` (`colspan`,
  `thead`), `pre`, `blockquote`, `align`/`text-align`; named and numeric entities. Scripts,
  styles and comments are skipped; images, CSS layout and links are not rendered.
- **Plain text**: one paragraph per line, form feeds start pages.

## Layout

A small flow engine (`layout.rs`): A4 pages, 2 cm margins, Helvetica / Helvetica-Bold /
Helvetica-Oblique / Helvetica-BoldOblique (standard 14, WinAnsiEncoding, exact widths from
`pdfcraft_fonts::Std14`), greedy word wrap (words longer than a line break by character), headings
kept with what follows, hanging list markers, bordered tables with shaded header rows and rows that
split across pages. Every Western European character (French `é è à ç œ « » ’ … –` included) is
drawn as itself; typographic characters WinAnsi lacks get a close equivalent (narrow no-break space,
ligatures, arrows); anything else is written as `[U+XXXX]` and counted, never as `?`.

## Limits

Inputs over 256 MB, zip members inflating past 64 MB (512 MB in all), XML parts over 8 million
nodes, DTDs, more than 300 000 non-empty cells, 100 000 rows, 1 024 columns, 500 images or 10 000
pages are refused or cut, with a note (and, for cut sheets, a visible line in the PDF).
