//! Reading members of a zip container (DOCX, XLSX, ODS), with size limits.

use std::io::{Cursor, Read};

use crate::ConvertError;

/// Most bytes one member may inflate to.
const MAX_ENTRY: u64 = 64 << 20;
/// Most bytes all members read from one file may inflate to.
const MAX_TOTAL: u64 = 512 << 20;

pub(crate) struct Archive<'a> {
    zip: zip::ZipArchive<Cursor<&'a [u8]>>,
    budget: u64,
    what: &'static str,
}

impl<'a> Archive<'a> {
    /// Open `bytes` as a zip container; `what` names the format for error messages.
    pub fn open(bytes: &'a [u8], what: &'static str) -> Result<Self, ConvertError> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ConvertError::Malformed(format!("this isn't a valid {what} file: its zip container can't be read ({e})")))?;
        Ok(Self { zip, budget: MAX_TOTAL, what })
    }

    /// The member's name as stored: `name` itself, or a member whose name differs only in case.
    fn find(&self, name: &str) -> Option<String> {
        let name = name.trim_start_matches('/');
        if self.zip.index_for_name(name).is_some() {
            return Some(name.to_string());
        }
        self.zip.file_names().find(|n| n.eq_ignore_ascii_case(name)).map(str::to_string)
    }

    /// The bytes of member `name`, or `None` when the container has no such member.
    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, ConvertError> {
        let Some(name) = self.find(name) else { return Ok(None) };
        let what = self.what;
        let bad = |e: String| ConvertError::Malformed(format!("this {what} file is damaged: {name}: {e}"));
        let file = self.zip.by_name(&name).map_err(|e| bad(e.to_string()))?;
        let cap = MAX_ENTRY.min(self.budget);
        if file.size() > cap {
            return Err(ConvertError::TooLarge(format!("this {what} file is too large to convert ({name} unpacks to {} bytes)", file.size())));
        }
        let mut out = Vec::new();
        file.take(cap.saturating_add(1)).read_to_end(&mut out).map_err(|e| bad(e.to_string()))?;
        let len = u64::try_from(out.len()).unwrap_or(u64::MAX);
        if len > cap {
            return Err(ConvertError::TooLarge(format!("this {what} file is too large to convert ({name} unpacks to more than {cap} bytes)")));
        }
        self.budget = self.budget.saturating_sub(len);
        Ok(Some(out))
    }

    /// Member `name` as text (UTF-8; a byte order mark is dropped).
    pub fn text(&mut self, name: &str) -> Result<Option<String>, ConvertError> {
        Ok(self.read(name)?.map(|b| {
            let s = String::from_utf8_lossy(&b).into_owned();
            s.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(s)
        }))
    }
}

/// `target` (a relationship target) resolved against the folder `base` (`word/`, `xl/`), with
/// `..` steps applied; an absolute target (`/xl/media/a.png`) is taken from the root.
pub(crate) fn resolve(base: &str, target: &str) -> String {
    let target = target.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    let joined = if let Some(abs) = target.strip_prefix('/') { abs.to_string() } else { format!("{base}{target}") };
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}
