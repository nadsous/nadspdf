//! NadsPDF's Mise en page tab: the page layout interface of `vendor/designcraft` (menus, tools,
//! panels, styles, tables, master pages…) drawn inside a tab of its own, like the Combine files
//! tab. It runs embedded: NadsPDF's fonts, look and zoom stay in place, and its keyboard is its
//! own while the tab shows. The interface never names DesignCraft (vendor/designcraft/NADSPDF.md).

use designcraft_engine::Session;
use designcraft_ui_egui::DesignApp;
use designcraft_ui_egui::Services;
use serde_json::json;

use crate::PdfCraftApp;

/// The Mise en page tab.
#[derive(Default)]
pub struct LayoutTab {
    pub open: bool,
    pub focused: bool,
    /// The page layout interface, made the first time the tab shows.
    design: Option<Box<DesignApp>>,
    /// Closing with unsaved documents was asked once: the next close discards them.
    close_armed: bool,
}

impl std::fmt::Debug for LayoutTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutTab").field("open", &self.open).field("documents", &self.document_count()).finish_non_exhaustive()
    }
}

/// What the interface reads, writes and pastes with: the files and the clipboard.
fn services() -> Services {
    Services {
        #[cfg(not(target_arch = "wasm32"))]
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        #[cfg(not(target_arch = "wasm32"))]
        write: Some(Box::new(|p: &str, b: &[u8]| crate::editing::write_atomically(p, b).map_err(|e| e.to_string()))),
        #[cfg(not(target_arch = "wasm32"))]
        clipboard_text: Some(Box::new(|| arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok())),
        ..Default::default()
    }
}

impl LayoutTab {
    /// The interface, made on first use.
    pub fn design(&mut self) -> &mut DesignApp {
        self.design.get_or_insert_with(|| {
            let mut d = DesignApp::new(Session::new(), services());
            d.embedded = true;
            d.ui.language = crate::i18n::current().code().to_owned();
            Box::new(d)
        })
    }

    /// How many page layouts are open in the tab.
    pub fn document_count(&self) -> usize {
        self.design.as_ref().map_or(0, |d| d.session.documents().len())
    }

    /// Whether a page layout is open.
    pub fn has_document(&self) -> bool {
        self.document_count() > 0
    }

    /// Whether an open page layout has changes not saved to a file.
    pub fn is_dirty(&self) -> bool {
        self.design.as_ref().is_some_and(|d| d.session.documents().iter().any(|st| st.is_dirty()))
    }

    /// The name the tab shows: the active page layout's, or the mode's.
    pub fn title(&self) -> String {
        self.design
            .as_ref()
            .and_then(|d| d.session.active())
            .map(|st| st.title())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| tl!("Page design").to_owned())
    }

    /// Run a command of the page layout interface (its menus, the engine's commands).
    pub fn run(&mut self, id: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        self.design().run(id, params)
    }
}

/// The interface's colours taken from NadsPDF's theme, so the tab looks like the rest of the
/// app (the red accent, nads.site's panels and text) and follows it from light to dark.
fn nads_tokens(t: &crate::theme::Tokens) -> designcraft_ui_egui::theme::Tokens {
    use designcraft_ui_egui::theme::{Brightness, Tokens};
    let dark = matches!(t.kind, crate::theme::ThemeKind::Dark);
    let base = Tokens::for_brightness(if dark { Brightness::Dark } else { Brightness::Light });
    Tokens {
        app_bar: t.chrome,
        tab_strip: t.titlebar,
        panel: t.panel,
        panel_darker: t.chrome,
        pasteboard: t.pasteboard,
        input: t.field,
        input_border: t.border,
        field_border: t.border,
        border: t.border,
        divider: t.divider,
        section_divider: t.divider,
        text: t.text,
        text_strong: t.text,
        text_dim: t.text_muted,
        text_disabled: t.text_faint,
        icon: t.icon,
        hover: t.hover,
        button: t.field,
        well: t.card,
        tool_active: t.accent_soft,
        accent: t.accent,
        accent_strong: t.accent,
        row_selected: t.accent_soft,
        smart_guide: t.accent,
        ..base
    }
}

/// Whether `name` is a file the Mise en page tab opens (by its extension).
pub(crate) fn is_layout_name(name: &str) -> bool {
    let ext = std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    pdfcraft_layout::OPENS.contains(&ext.as_str())
}

/// Standard base64, for handing a file's bytes to the interface.
fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let sym = |i: u32| char::from(ABC.get(i as usize & 63).copied().unwrap_or(b'A'));
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(sym(n >> 18));
        out.push(sym(n >> 12));
        out.push(if chunk.len() > 1 { sym(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { sym(n) } else { '=' });
    }
    out
}

impl PdfCraftApp {
    /// Whether the Mise en page tab is the one shown.
    pub fn layout_showing(&self) -> bool {
        self.active.is_none() && self.layout_tab.open && self.layout_tab.focused
    }

    /// Show the Mise en page tab, opening it if needed.
    pub fn open_layout_tab(&mut self) {
        self.layout_tab.open = true;
        self.layout_tab.focused = true;
        self.combine_tab.focused = false;
        self.active = None;
    }

    /// A mode was chosen while the Mise en page tab showed: the last PDF tab shows instead (Home
    /// without one); the Mise en page tab stays open.
    pub fn leave_layout_tab(&mut self) {
        if self.layout_showing() {
            self.layout_tab.focused = false;
            self.active = self.views.len().checked_sub(1);
        }
    }

    /// Start a new page layout (the interface's New Document dialog).
    pub fn new_layout(&mut self) {
        self.open_layout_tab();
        if let Err(e) = self.layout_tab.run("file.new", json!({})) {
            self.notify_error(e);
        }
    }

    /// Open the sample magazine in the Mise en page tab.
    pub fn open_layout_sample(&mut self) {
        self.open_layout_tab();
        if let Err(e) = self.layout_tab.run("file.newSample", json!({})) {
            self.notify_error(e);
        }
    }

    /// Open a page layout, IDML or InDesign file (`bytes`, named `name`, read from `path`) in
    /// the Mise en page tab, beside the page layouts already open there.
    pub fn open_layout_file(&mut self, name: &str, path: Option<String>, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() {
            return Err(format!("{name}: {}", tl!("the file is empty")));
        }
        self.open_layout_tab();
        let opened = match &path {
            // From a file: linked pictures and a Document Fonts folder beside it are found.
            #[cfg(not(target_arch = "wasm32"))]
            Some(p) => self.layout_tab.run("file.open", json!({ "path": p })),
            _ => self.layout_tab.run("file.openBytes", json!({ "name": name, "base64": base64(bytes) })),
        };
        let v = opened?;
        if let Some(first) = v["warnings"].as_array().and_then(|w| w.first()).and_then(|w| w.as_str()) {
            let more = v["warnings"].as_array().map_or(0, |w| w.len().saturating_sub(1));
            self.notify(if more > 0 { format!("{first} (+{more})") } else { first.to_owned() });
        }
        Ok(())
    }

    /// Open in page layout: the active PDF becomes a page layout in the Mise en page tab, its
    /// pages' artwork locked behind editable text frames (`pdfcraft_layout::Layout::from_pdf`).
    /// The PDF itself is untouched.
    pub fn layout_from_active_pdf(&mut self) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(doc) = self.session.get(id) else { return };
        let stem = doc.name.rsplit_once('.').map_or(doc.name.as_str(), |(s, _)| s).to_owned();
        let Some((artwork, pages)) = doc.layout_sources() else {
            return self.notify_tr("This PDF's security doesn't allow copying its content, so it can't be opened in page layout.");
        };
        let pages: Vec<pdfcraft_layout::PdfPage> = pages
            .into_iter()
            .map(|p| pdfcraft_layout::PdfPage {
                width: p.width,
                height: p.height,
                blocks: p
                    .blocks
                    .into_iter()
                    .map(|b| pdfcraft_layout::PdfBlock {
                        text: b.text,
                        rect: b.rect,
                        style: pdfcraft_layout::TextStyle { size: b.size, bold: b.bold, color: b.color },
                        rotation: b.rotation,
                    })
                    .collect(),
            })
            .collect();
        let made = pdfcraft_layout::Layout::from_pdf(&artwork, &format!("{stem}.pdf"), &pages).and_then(|l| l.save_bytes());
        let r =
            made.map_err(|e| e.to_string()).and_then(|bytes| self.open_layout_file(&format!("{stem}.{}", pdfcraft_layout::EXTENSION), None, &bytes));
        if let Err(e) = r {
            self.notify_error(e);
        }
    }

    /// Undo in the page layout interface.
    pub fn layout_undo(&mut self) -> bool {
        self.layout_tab.run("edit.undo", json!({})).is_ok()
    }

    pub fn layout_redo(&mut self) -> bool {
        self.layout_tab.run("edit.redo", json!({})).is_ok()
    }

    /// Save the active page layout (`as_new`: Save As).
    pub fn save_layout(&mut self, as_new: bool) {
        let id = if as_new { "file.saveAs" } else { "file.save" };
        if let Err(e) = self.layout_tab.run(id, json!({})) {
            self.notify_error(e);
        }
    }

    /// The tab's close button: unsaved page layouts are kept the first time, with a word on how
    /// to keep them; asked again, they are discarded.
    pub fn request_close_layout_tab(&mut self) {
        if self.layout_tab.is_dirty() && !self.layout_tab.close_armed {
            self.layout_tab.close_armed = true;
            self.open_layout_tab();
            self.notify_tr("This page layout has unsaved changes. Save it (Ctrl+S), or close it again to discard them.");
            return;
        }
        self.close_layout_tab();
    }

    /// Quitting with unsaved page layouts: the first time the window stays open on them with a
    /// word on how to keep them (`true`); asked again, NadsPDF quits.
    pub(crate) fn hold_quit_for_layout(&mut self) -> bool {
        if !self.layout_tab.is_dirty() || self.layout_tab.close_armed {
            return false;
        }
        self.layout_tab.close_armed = true;
        self.open_layout_tab();
        self.notify_tr("This page layout has unsaved changes. Save it (Ctrl+S), or close again to quit without saving.");
        true
    }

    /// Close the Mise en page tab and its page layouts; the last PDF tab shows, if any.
    pub fn close_layout_tab(&mut self) {
        let showing = self.layout_showing();
        self.layout_tab = LayoutTab::default();
        if showing {
            self.active = self.views.len().checked_sub(1);
        }
    }
}

/// The Mise en page tab's page: the whole page layout interface.
pub(crate) fn page(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let design = app.layout_tab.design();
    // The interface speaks NadsPDF's language, also after it changes in Preferences.
    let lang = crate::i18n::current().code();
    if design.ui.language != lang {
        design.ui.language = lang.to_owned();
    }
    designcraft_ui_egui::theme::store(&ctx, &nads_tokens(&crate::theme::Tokens::get(&ctx)));
    design.logic(&ctx);
    design.ui(ui);
    #[cfg(not(target_arch = "wasm32"))]
    design.show_file_dialogs::<eframe::Frame>(None);
    // Anything done in the tab may make it unsaved again: closing asks again.
    if !app.layout_tab.is_dirty() {
        app.layout_tab.close_armed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_files_are_known_by_their_extension() {
        for name in ["Affiche.nadsmep", "a.IDML", "b.indd", "c.indt", "old.designcraft"] {
            assert!(is_layout_name(name), "{name}");
        }
        for name in ["a.pdf", "a.docx", "nadsmep", "a.nadsmep.pdf"] {
            assert!(!is_layout_name(name), "{name}");
        }
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }
}
