//! The simplified interface ([`PdfCraftApp::lite`](crate::PdfCraftApp)): read, comment, fill in,
//! sign, edit text, images and pages, recognize text (OCR), save and print, and nothing else. The
//! Chrome extension shows it (`?lite=1` on the web).
//!
//! One allowlist decides. `command_enabled` refuses any other command, so no menu, palette,
//! shortcut or control-channel call reaches a hidden tool, and the lists the user sees (menus,
//! palette, All tools, Home, mode bar) leave them out.

/// Tool groups of the engine catalogue shown in the simplified interface.
const GROUPS: &[&str] = &["comment", "fill_sign", "edit", "organize", "scan"];

/// Commands allowed one by one.
const COMMANDS: &[&str] = &[
    "file.open",
    "file.save",
    "file.save_as",
    "file.close",
    "file.close_all",
    "file.revert",
    "file.properties",
    "print.dialog",
    "edit.undo",
    "edit.redo",
    "edit.find",
    "form.fields",
    "form.clear",
    "sign.panel",
    "ocr.recognize",
    "ui.dock.reset",
    "help.shortcuts",
    "help.about",
];

/// Whole families allowed (zoom and layout, comment tools, Fill & Sign, editing, pages), less
/// [`EXCEPT`].
const PREFIXES: &[&str] = &["view.", "comment.", "sign.fill.", "edit.", "page."];

/// Members of [`PREFIXES`] that belong to the full editor.
const EXCEPT: &[&str] = &["view.group_tab", "comment.import", "comment.export", "comment.summarize"];

/// Whether the simplified interface offers this command (or catalogue item) id.
pub(crate) fn allows_command(id: &str) -> bool {
    COMMANDS.contains(&id) || (PREFIXES.iter().any(|p| id.starts_with(p)) && !EXCEPT.contains(&id))
}

/// Whether the simplified interface offers this catalogue tool group.
pub(crate) fn allows_group(id: &str) -> bool {
    GROUPS.contains(&id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_engine::catalog::{TOOL_GROUPS, group};
    use pdfcraft_engine::commands::command;

    #[test]
    fn every_allowed_id_exists() {
        for id in COMMANDS.iter().chain(EXCEPT) {
            assert!(command(id).is_some(), "lite names an unknown command {id}");
        }
        for id in GROUPS {
            assert!(group(id).is_some(), "lite names an unknown tool group {id}");
        }
    }

    #[test]
    fn reading_commenting_and_signing_stay() {
        for id in ["file.save", "print.dialog", "comment.highlight", "sign.fill.signature", "view.fit_one_page", "edit.find"] {
            assert!(allows_command(id), "{id}");
        }
        assert!(allows_group("fill_sign"));
    }

    #[test]
    fn editing_pages_and_ocr_stay() {
        for id in ["edit.edit_text", "edit.text", "edit.image", "edit.watermark", "page.delete", "page.organize", "page.rotate", "ocr.recognize"] {
            assert!(allows_command(id), "{id}");
        }
        for g in ["edit", "organize", "scan"] {
            assert!(allows_group(g), "{g}");
        }
    }

    #[test]
    fn advanced_tools_and_preferences_go() {
        // Preferences is out so the interface language the extension sets stays.
        for id in ["app.preferences", "ocr.recognize_batch", "redact.mark", "tools.js_console", "comment.export", "export.docx"] {
            assert!(!allows_command(id), "{id}");
        }
        for g in TOOL_GROUPS.iter().filter(|g| !GROUPS.contains(&g.id)) {
            assert!(!allows_group(g.id), "{}", g.id);
        }
    }

    /// The app, not only the list: a hidden command can't run (from a shortcut, the palette or the
    /// control channel), a hidden tool panel can't open, and Convert mode falls back to Read.
    #[test]
    fn the_app_refuses_what_lite_hides() {
        let mut app = crate::PdfCraftApp::new();
        let preferences = command("app.preferences").expect("registered");
        let open = command("file.open").expect("registered");
        assert!(app.command_enabled(preferences), "enabled in the full interface");
        app.lite = true;
        assert!(!app.command_enabled(preferences));
        assert!(app.command_enabled(open));
        app.select_tool("redact");
        assert_ne!(app.left, crate::LeftPanel::Tool("redact"));
        app.select_tool("fill_sign");
        assert_eq!(app.left, crate::LeftPanel::Tool("fill_sign"));
        app.switch_mode(crate::Mode::Convert, true);
        assert_eq!(app.mode, crate::Mode::Read);
        app.switch_mode(crate::Mode::Edit, true);
        assert_eq!(app.mode, crate::Mode::Edit);
    }
}
