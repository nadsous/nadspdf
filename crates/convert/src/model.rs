//! The flow model every reader produces and the layout engine sets: paragraphs of styled runs,
//! list items, headings, tables and images, in reading order.

/// Character formatting of a run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// Text in one style. `'\n'` is a forced line break and `'\t'` a tab.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Run {
    pub text: String,
    pub style: Style,
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), style: Style::default() }
    }

    pub fn styled(text: impl Into<String>, style: Style) -> Self {
        Self { text: text.into(), style }
    }
}

/// What a paragraph is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    Body,
    /// The document title (larger than a level-1 heading).
    Title,
    /// Heading level 1–6.
    Heading(u8),
    /// A list item at nesting `level` (0-based), drawn after `marker` (`•`, `1.`, `a)`, …).
    ListItem {
        level: u8,
        marker: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Para {
    pub kind: Kind,
    pub runs: Vec<Run>,
    pub align: Align,
    /// Extra left indent, in steps (block quotes).
    pub indent: u8,
}

impl Para {
    pub fn new(kind: Kind, runs: Vec<Run>) -> Self {
        Self { kind, runs, align: Align::Left, indent: 0 }
    }

    /// Whether the paragraph has no visible text.
    pub fn is_blank(&self) -> bool {
        self.runs.iter().all(|r| r.text.trim().is_empty())
    }
}

/// One table cell: its paragraphs (each a list of runs) and how many grid columns it spans.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Cell {
    pub paras: Vec<Vec<Run>>,
    pub span: usize,
}

impl Cell {
    pub fn text(text: impl Into<String>) -> Self {
        Self { paras: vec![vec![Run::plain(text)]], span: 1 }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Row {
    pub cells: Vec<Cell>,
    /// A header row: set in bold, shaded, and repeated at the top of each page the table runs onto.
    pub header: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Table {
    pub rows: Vec<Row>,
    /// Relative column widths the source gives (Word's grid), if any.
    pub widths: Option<Vec<f64>>,
    /// Font size of the cell text.
    pub size: f64,
    /// Spread the columns over the full text width (documents) rather than sizing them to their
    /// content (spreadsheets).
    pub fill: bool,
    /// When the columns can't fit across the page even at the smallest size, print them in
    /// groups one after another (spreadsheets), rather than squeezing them.
    pub split_columns: bool,
}

impl Table {
    /// Number of grid columns.
    pub fn columns(&self) -> usize {
        self.rows.iter().map(|r| r.cells.iter().map(|c| c.span.max(1)).fold(0usize, usize::saturating_add)).max().unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Block {
    Para(Para),
    Table(Table),
    /// An image file (PNG, JPEG, …) shown at `size` points when the source gives one.
    Image {
        name: String,
        bytes: Vec<u8>,
        size: Option<(f64, f64)>,
    },
    PageBreak,
    /// Start a new page in this orientation (each spreadsheet sheet picks its own).
    Section {
        landscape: bool,
    },
}

/// A whole source document, ready to lay out.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Source {
    pub blocks: Vec<Block>,
    /// Notes about what the conversion could not carry over (shown in the report).
    pub notes: Vec<String>,
}

impl Source {
    pub fn push_para(&mut self, kind: Kind, runs: Vec<Run>) {
        self.blocks.push(Block::Para(Para::new(kind, runs)));
    }
}
