//! File Explorer's right-click menu (Windows): "Convert to PDF with NadsPDF", "Combine with
//! NadsPDF" and "Open with NadsPDF", in the interface language, and Preferences' switch for them.
//! The registry side is `pdfcraft_platform::explorer_menu`.

use pdfcraft_platform::explorer_menu as menu;
pub use pdfcraft_platform::explorer_menu::{Status, status, unregister};

/// The entries' names in the current interface language ([`crate::i18n::set_current`]).
pub fn labels() -> menu::Labels {
    menu::Labels {
        convert: tl!("Convert to PDF with NadsPDF").to_string(),
        combine: tl!("Combine with NadsPDF").to_string(),
        open: tl!("Open with NadsPDF").to_string(),
    }
}

/// Add the entries for this user, pointing at this program, named in the current language.
pub fn register() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("can't find this program's path: {e}"))?;
    menu::register(&exe, &labels())
}

/// Preferences ▸ "Show NadsPDF in the File Explorer right-click menu". The registry is read once
/// and remembered (egui's temporary memory), then again after each change.
#[cfg(windows)]
pub(crate) fn preferences_row(ui: &mut egui::Ui, app: &mut crate::PdfCraftApp, t: &crate::theme::Tokens) {
    let id = egui::Id::new("explorer-menu-status");
    let status = match ui.ctx().data(|d| d.get_temp::<menu::Status>(id)) {
        Some(status) => status,
        None => {
            let status = menu::status();
            ui.ctx().data_mut(|d| d.insert_temp(id, status));
            status
        }
    };
    let machine = status == menu::Status::Machine;
    let mut on = status != menu::Status::Off;
    let changed = ui.add_enabled(!machine, crate::widgets::checkbox(&mut on, tl!("Show NadsPDF in the File Explorer right-click menu"))).changed();
    if changed {
        let result = if on { register() } else { menu::unregister() };
        if let Err(e) = result {
            log::warn!("File Explorer menu: {e}");
            app.notify_fmt("Couldn't change the File Explorer menu: {e}", &[("e", &e)]);
        }
        ui.ctx().data_mut(|d| d.insert_temp(id, menu::status()));
    }
    let note = if machine {
        tl!("Added for all users when NadsPDF was installed.")
    } else {
        tl!("Convert to PDF, Combine and Open on documents, images and PDFs. On Windows 11, under Show more options.")
    };
    ui.label(egui::RichText::new(note).small().color(t.text_muted));
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_entries_are_named_in_the_interface_language() {
        let fr = crate::i18n::Lang::from_code("fr").expect("French");
        crate::i18n::set_current(fr);
        let labels = super::labels();
        crate::i18n::set_current(crate::i18n::Lang::EN);
        assert_eq!(labels.convert, "Convertir en PDF avec NadsPDF");
        assert_eq!(labels.combine, "Fusionner avec NadsPDF");
        assert_eq!(labels.open, "Ouvrir avec NadsPDF");
        assert_eq!(super::labels().combine, "Combine with NadsPDF");
    }

    #[test]
    fn convert_is_offered_on_exactly_the_types_the_app_converts() {
        assert_eq!(pdfcraft_platform::explorer_menu::CONVERTIBLE, pdfcraft_engine::CONVERTIBLE);
    }
}
