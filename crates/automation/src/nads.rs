//! NadsPDF's own tools: document conversion (`doc_convert`) and strong compression by
//! rasterizing pages (`doc_rasterize_compress`). Kept apart from the upstream tools so merges from
//! PdfCraft don't collide with them.

use serde_json::{Value, json};

use super::Automation;
use super::error::{Result, ToolError, failed};
use super::json::{Args, summary};
use super::paths::write_atomic;

impl Automation {
    /// Compress ▸ Rasterize pages: every page as a JPEG at a level's (or the given) resolution
    /// and quality, written only when smaller.
    pub(crate) fn doc_rasterize_compress(&mut self, a: &Args) -> Result<Value> {
        use pdfcraft_engine::rasterize::RasterLevel;
        let id = self.doc(a)?.id;
        let before = self.doc(a)?.bytes.len();
        let path = self.resolve(a.str("path")?, true)?;
        let level = match a.get("level").map(|v| v.as_str()) {
            None => RasterLevel::default(),
            Some(Some(l)) => RasterLevel::from_id(l).ok_or_else(|| ToolError::InvalidArgs(format!("unknown level {l:?} (light, medium, strong)")))?,
            Some(None) => return Err(ToolError::InvalidArgs("level must be a string (light, medium, strong)".into())),
        };
        let mut settings = level.settings();
        if let Some(v) = a.get("dpi") {
            let dpi = v.as_f64().filter(|d| d.is_finite() && (36.0..=600.0).contains(d));
            settings.dpi = dpi.ok_or_else(|| ToolError::InvalidArgs("dpi must be a number from 36 to 600".into()))?;
        }
        if let Some(v) = a.get("quality") {
            let q = v.as_u64().filter(|q| (1..=100).contains(q));
            settings.quality = q.ok_or_else(|| ToolError::InvalidArgs("quality must be an integer from 1 to 100".into()))? as u8;
        }
        let bytes = self.session.rasterized_bytes(id, settings).map_err(failed)?;
        let written = bytes.len() < before;
        if written {
            write_atomic(&path, &bytes)?;
        }
        Ok(json!({
            "path": path.to_string_lossy(),
            "written": written,
            "level": level.id(),
            "dpi": settings.dpi,
            "quality": settings.quality,
            "bytes_before": before,
            "bytes_after": bytes.len(),
        }))
    }

    /// Convert an Office, spreadsheet, CSV, HTML or text file to a new, unsaved PDF.
    pub(crate) fn doc_convert(&mut self, a: &Args) -> Result<Value> {
        let path = self.resolve(a.str("path")?, false)?;
        let bytes = std::fs::read(&path).map_err(|e| failed(format!("{}: {e}", path.display())))?;
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // The readers parse untrusted bytes: a panic in one must not take the session down.
        let converted = pdfcraft_engine::guard(|| self.session.create_from_document(&file, &bytes))
            .map_err(|_| failed(format!("{file}: the file could not be read")))?;
        let (pdf, report) = converted.map_err(|e| match e {
            pdfcraft_engine::EditError::Convert(pdfcraft_engine::ConvertError::Unsupported(m)) => ToolError::InvalidArgs(m),
            e => failed(format!("{file}: {e}")),
        })?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
        let name = a.opt_str("name")?.map(str::to_owned).unwrap_or_else(|| format!("{stem}.pdf"));
        let id = self.session.open_new(name, pdf).map_err(failed)?;
        let mut out = summary(self.session.get(id).ok_or_else(|| failed("the document vanished"))?);
        out["conversion"] = json!({
            "pages": report.pages,
            "escaped": report.escaped,
            "images_skipped": report.images_skipped,
            "notes": report.notes,
        });
        Ok(out)
    }
}
