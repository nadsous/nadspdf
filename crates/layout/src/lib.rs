//! NadsPDF Mise en page: page layout on DesignCraft's engine (Apache-2.0 OR MIT).
//!
//! Prototype: proves the engine links into NadsPDF and measures what it adds.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// The sample magazine laid out and exported as a PDF.
pub fn sample_pdf() -> Result<Vec<u8>, String> {
    let doc = designcraft_engine::sample::magazine();
    let cache = designcraft_engine::compose::Cache::new();
    designcraft_pdf::export_pdf(&doc, &cache, &designcraft_pdf::PdfOptions::default()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_sample_exports_a_pdf() {
        let pdf = super::sample_pdf().unwrap_or_default();
        assert!(pdf.starts_with(b"%PDF-"), "{} bytes", pdf.len());
    }
}
