//! Window chrome: tab strip (with the integrated macOS title bar), mode bar, right rail.

use egui::{Align, Align2, Color32, CornerRadius, Layout, Rect, Sense, Stroke, pos2, vec2};

use crate::canvas::{DocView, Fit, PageLayout};
use crate::tab_groups::{self, GroupAction, Slot, TabGroups};
use crate::theme::{self, ThemePreference, Tokens};
use crate::{Dialog, Mode, PdfCraftApp, PropsTab, RightPanel, icons, widgets, window_controls};

/// Height of the tab strip when it doubles as the macOS title bar (#789). AppKit draws the traffic
/// lights itself, centred in the standard 28 pt title-bar band at the top of the window, and moving
/// them takes AppKit calls the workspace forbids (`unsafe`). So on macOS the strip is that band:
/// the lights sit vertically centred in it, and the controls beside them shrink to fit, as in
/// WordCraft (storytold/wordcraft#255).
pub const MAC_TITLE_BAR: f32 = 28.0;
/// The traffic lights as AppKit lays them out: button diameter, centre-to-centre pitch and the
/// first button's left edge (8–10 pt depending on the macOS version; the larger value keeps the
/// gap after them from shrinking).
const MAC_LIGHT: f32 = 12.0;
const MAC_LIGHT_PITCH: f32 = 20.0;
const MAC_LIGHTS_LEFT: f32 = 10.0;
/// Where the strip's content starts to the right of the traffic lights: the end of the three
/// lights plus the same gap the lights keep from the top of the window.
pub const MAC_CONTENT_LEFT: f32 = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT + (MAC_TITLE_BAR - MAC_LIGHT) / 2.0;
/// The strip and its controls everywhere else.
const TAB_STRIP: f32 = 38.0;
const TAB_HEIGHT: f32 = 30.0;
const STRIP_BUTTON: f32 = 28.0;

pub fn tab_strip(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    // macOS: the strip is the 28 pt title bar, so the traffic lights are centred in it; tabs and
    // buttons shrink to keep a margin above and below.
    let mac = cfg!(target_os = "macos") && app.integrated_titlebar;
    let left = if mac { MAC_CONTENT_LEFT as i8 } else { 8 };
    let (tab_h, button) = if mac { (MAC_TITLE_BAR - 2.0, 24.0) } else { (TAB_HEIGHT, STRIP_BUTTON) };
    egui::Panel::top("tab_strip")
        .exact_size(if mac { MAC_TITLE_BAR } else { TAB_STRIP })
        // No line under the strip: the active tab runs on into the mode bar, which shares its colour.
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.titlebar).inner_margin(egui::Margin { left, right: 10, top: 0, bottom: 0 }))
        .show(ui, |ui| {
            let full = ui.max_rect();
            app.strip_rect = full;
            let drag = ui.interact(full, ui.id().with("titledrag"), Sense::click_and_drag());
            if drag.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
            // GNOME on Wayland: the strip is the header bar, with the window buttons at its ends.
            let fullscreen = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
            let controls = app
                .window_controls
                .clone()
                .filter(|_| app.draws_window_frame() && !fullscreen)
                .unwrap_or_else(|| window_controls::Layout { left: Vec::new(), right: Vec::new() });
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if !controls.left.is_empty() {
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        window_controls::buttons(ui, &controls.left);
                    });
                    ui.add_space(window_controls::SIDE_GAP - 4.0);
                }
                // The nads.site brand, as on the site's top bar, opens the main menu.
                main_menu(app, ui, button);
                ui.add_space(2.0);
                if icons::button(ui, "house", button, app.active.is_none() && !app.combine_showing(), tl!("Home")).clicked() {
                    app.active = None;
                    app.combine_tab.focused = false;
                }
                // Keep the two icon buttons and the gaps outside the scrolling tabs.
                let controls_width = 2.0 * button + 50.0 + window_controls::width(&controls.right);
                // Open stays in sight after the tabs: its icon, text and padding, and the gaps around it.
                let open_width = ui.fonts_mut(|f| f.layout_no_wrap(tl!("Open").into(), theme::medium(13.0), t.text).size().x) + 38.0 + 12.0;
                let mut tabs_width = (ui.available_width() - controls_width - open_width).max(0.0);
                // Tabs shrink to share the strip (names end in "…") and scroll only once they reach
                // their narrowest; then arrows either side say so and step through them.
                // Tab groups: each chip sits before its tabs, and a folded group hides them.
                let docs: Vec<pdfcraft_engine::DocId> = app.views.iter().map(|v| v.id).collect();
                app.tab_groups.prune(&docs);
                let mut slots = app.tab_groups.layout(&docs, app.active);
                // Bubbled documents float over the window instead.
                slots.retain(|s| !matches!(*s, Slot::Tab(i) if docs.get(i).is_some_and(|d| app.bubbles.contains(*d))));
                let gap = ui.spacing().item_spacing.x;
                let mut natural: Vec<f32> = Vec::with_capacity(slots.len() + 1);
                for slot in &slots {
                    match *slot {
                        Slot::Tab(i) => {
                            if let Some(doc) = app.views.get(i).and_then(|v| app.session.get(v.id)) {
                                natural.push(tab_natural_width(ui, &t, &doc.display_name()));
                            }
                        }
                        // Chips keep their width: what they take comes out of the tabs' share.
                        Slot::Chip(g) => tabs_width = (tabs_width - chip_width(ui, &app.tab_groups, g) - gap).max(0.0),
                    }
                }
                if app.combine_tab.open {
                    natural.push(tab_natural_width(ui, &t, tl!("Combine files")));
                }
                let overflow = tabs_overflow(&natural, tabs_width, gap, TAB_MIN_WIDTH);
                if overflow {
                    tabs_width = (tabs_width - 2.0 * (TAB_ARROW + gap)).max(0.0);
                }
                let cap = tab_cap(&natural, tabs_width, gap, TAB_MIN_WIDTH);
                // Where the tabs were scrolled to last frame, and how far they can go.
                let scroll_id = ui.id().with("tab_scroll");
                let (offset, max_offset) = ui.data(|d| d.get_temp::<(f32, f32)>(scroll_id)).unwrap_or((0.0, 0.0));
                let mut scroll_to = None;
                if overflow && arrow(ui, "chevron-left", offset > 0.5, tl!("Scroll tabs left")) {
                    scroll_to = Some((offset - (TAB_MIN_WIDTH + gap)).max(0.0));
                }
                let state = (app.active, app.views.len(), app.combine_showing());
                let changed = ui.data_mut(|data| {
                    let id = ui.id().with("active_tab");
                    let changed = data.get_temp::<(Option<usize>, usize, bool)>(id) != Some(state);
                    data.insert_temp(id, state);
                    changed
                });
                let mut action = None;
                let mut close = None;
                let mut group_action = None;
                let mut tear = None;
                let mut drag_start = None;
                let mut tab_rects = Vec::new();
                ui.scope(|ui| {
                    ui.style_mut().always_scroll_the_only_direction = true;
                    let mut area = egui::ScrollArea::horizontal().id_salt("document_tabs").max_width(tabs_width).auto_shrink([true, true]);
                    if let Some(x) = ui.data_mut(|d| d.remove_temp::<f32>(scroll_id.with("to"))) {
                        area = area.horizontal_scroll_offset(x);
                    }
                    let out = area.show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            for slot in &slots {
                                let i = match *slot {
                                    Slot::Chip(g) => {
                                        if let Some(a) = group_chip(ui, &t, &mut app.tab_groups, g, tab_h) {
                                            group_action = Some(a);
                                        }
                                        continue;
                                    }
                                    Slot::Tab(i) => i,
                                };
                                let Some(view_id) = app.views.get(i).map(|v| v.id) else { continue };
                                let Some(doc) = app.session.get(view_id) else { continue };
                                let (name, dirty) = (doc.display_name(), doc.dirty);
                                // A tab showing the document's title names its file on hover.
                                let file = (name != doc.name).then_some(doc.name.as_str());
                                let response = tab(
                                    ui,
                                    ui.id().with(("document-tab", view_id)),
                                    &t,
                                    "file-text",
                                    &name,
                                    file,
                                    dirty,
                                    app.active == Some(i),
                                    &mut close,
                                    i,
                                    cap,
                                    tab_h,
                                    true,
                                );
                                tab_rects.push((i, response.rect));
                                if response.drag_started() {
                                    drag_start = Some(view_id);
                                }
                                if changed && app.active == Some(i) && !app.combine_showing() {
                                    response.scroll_to_me(Some(Align::Center));
                                }
                                let id = view_id;
                                let group = app.tab_groups.group_of(id);
                                if let Some(g) = group.and_then(|g| app.tab_groups.get(g)) {
                                    // The group's colour runs under its tabs.
                                    let r = response.rect;
                                    ui.painter().hline(r.x_range().shrink(4.0), r.bottom() - 1.5, Stroke::new(3.0, tab_groups::color(g.color)));
                                }
                                let reveal = cfg!(not(target_arch = "wasm32")) && doc.path.is_some();
                                response.context_menu(|ui| {
                                    if ui.button(tl!("Close Tab")).clicked() {
                                        action = Some(crate::tab_context::Action::Close(id));
                                        ui.close();
                                    }
                                    if ui.add_enabled(!app.closed_tabs.is_empty(), egui::Button::new(tl!("Reopen Closed Tab"))).clicked() {
                                        action = Some(crate::tab_context::Action::Reopen);
                                        ui.close();
                                    }
                                    let label = if cfg!(target_os = "macos") { tl!("Reveal in Finder") } else { tl!("Reveal in File Explorer") };
                                    if ui.add_enabled(reveal, egui::Button::new(label)).clicked() {
                                        action = Some(crate::tab_context::Action::Reveal(id));
                                        ui.close();
                                    }
                                    if ui
                                        .button(tl!("Float as a bubble"))
                                        .on_hover_text(tl!("Show this PDF in a small window over the others"))
                                        .clicked()
                                    {
                                        action = Some(crate::tab_context::Action::Float(id));
                                        ui.close();
                                    }
                                    if app.open_in_new_window.is_some()
                                        && ui
                                            .button(tl!("Open in New Window"))
                                            .on_hover_text(tl!("Or drag the tab out of the strip, to another screen for instance"))
                                            .clicked()
                                    {
                                        tear = Some(id);
                                        ui.close();
                                    }
                                    ui.separator();
                                    tab_group_menu(ui, &app.tab_groups, id, group, &mut group_action);
                                });
                                if response.clicked() {
                                    app.active = Some(i);
                                }
                            }
                            if let Some(i) = close
                                && let Some(view) = app.views.get(i)
                            {
                                action.get_or_insert(crate::tab_context::Action::Close(view.id));
                            }
                            match action {
                                Some(crate::tab_context::Action::Float(id)) => app.float_tab(id, ui.ctx().content_rect()),
                                Some(action) => app.tab_action(action),
                                None => {}
                            }
                            app.tab_rects = std::mem::take(&mut tab_rects);
                            if let Some(id) = drag_start.take() {
                                app.start_tab_drag(id);
                            }
                            if let Some(id) = tear.take() {
                                // Offset from this window, so the new one doesn't hide exactly behind it.
                                let at = crate::tear_off::screen_px(ui.ctx(), egui::pos2(48.0, 48.0));
                                app.tear_off(id, at);
                            }
                            if let Some(a) = group_action.take() {
                                app.tab_group_action(a);
                            }
                            if app.combine_tab.open {
                                // After the document tabs; its index can't clash with theirs.
                                let mut close = None;
                                let response = tab(
                                    ui,
                                    ui.id().with("combine-tab"),
                                    &t,
                                    "files",
                                    tl!("Combine files"),
                                    None,
                                    false,
                                    app.combine_showing(),
                                    &mut close,
                                    usize::MAX,
                                    cap,
                                    tab_h,
                                    false,
                                );
                                if changed && app.combine_showing() {
                                    response.scroll_to_me(Some(Align::Center));
                                }
                                if response.clicked() {
                                    app.open_combine_tab();
                                }
                                if close.is_some() {
                                    app.close_combine_tab();
                                }
                            }
                        });
                    });
                    let max = (out.content_size.x - out.inner_rect.width()).max(0.0);
                    ui.data_mut(|d| d.insert_temp(scroll_id, (out.state.offset.x, max)));
                });
                if overflow && arrow(ui, "chevron-right", offset < max_offset - 0.5, tl!("Scroll tabs right")) {
                    scroll_to = Some((offset + TAB_MIN_WIDTH + gap).min(max_offset));
                }
                if let Some(x) = scroll_to {
                    ui.data_mut(|d| d.insert_temp(scroll_id.with("to"), x));
                    ui.ctx().request_repaint();
                }
                ui.add_space(4.0);
                let tip = crate::commands::command_tip(ui.ctx(), tl!("Open a PDF ({key})"), "file.open");
                if widgets::ghost_button_sized(ui, "plus", tl!("Open"), tab_h).on_hover_text(tip).clicked() {
                    app.open_dialog();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !controls.right.is_empty() {
                        // Laid out left to right inside the right-to-left run, so they read in
                        // the desktop's order.
                        let size = egui::vec2(window_controls::width(&controls.right) - window_controls::SIDE_GAP, window_controls::BUTTON);
                        ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            window_controls::buttons(ui, &controls.right);
                        });
                        ui.add_space(window_controls::SIDE_GAP - 4.0);
                    }
                    let (icon, label) = match app.theme_preference {
                        ThemePreference::System => ("monitor", tl!("Use system setting")),
                        ThemePreference::Light => ("sun", tl!("Light gray")),
                        ThemePreference::Dark => ("moon", tl!("Dark gray")),
                    };
                    let tip = format!("{}: {label}", tl!("Display theme"));
                    let response = icons::button(ui, icon, button, false, &tip);
                    // A click cycles System, Light, Dark; a right-click opens the menu.
                    if response.clicked() {
                        app.execute(match app.theme_preference {
                            ThemePreference::System => "view.theme.light",
                            ThemePreference::Light => "view.theme.dark",
                            ThemePreference::Dark => "view.theme.system",
                        });
                    }
                    egui::Popup::context_menu(&response).id(response.id.with("theme_menu")).show(|ui| appearance_menu(app, ui));
                    if icons::button(ui, "circle-help", button, false, tl!("Keyboard shortcuts")).clicked() {
                        app.dialog = Some(Dialog::Shortcuts);
                    }
                });
            });
        });
}

/// Both theme entry points use the same choices and command path.
fn theme_menu(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    for (preference, command, label) in [
        (ThemePreference::System, "view.theme.system", "Use system setting"),
        (ThemePreference::Light, "view.theme.light", "Light gray"),
        (ThemePreference::Dark, "view.theme.dark", "Dark gray"),
    ] {
        if ui.add(widgets::radio(app.theme_preference == preference, tl!(label))).clicked() {
            app.execute(command);
            ui.close();
        }
    }
    ui.separator();
    if ui.button(tl!("Appearance settings…")).clicked() {
        app.dialog = Some(Dialog::Preferences);
        ui.close();
    }
}

/// The right-click menu on the title bar's theme button.
fn appearance_menu(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    if ui.button(tl!("Appearance settings…")).clicked() {
        app.dialog = Some(Dialog::Preferences);
        ui.close();
    }
}

/// A tab's chrome around its name: the icon on the left, the close button on the right.
const TAB_CHROME: f32 = 64.0;
/// The narrowest a tab shrinks to before the strip scrolls: room for most of a name.
const TAB_MIN_WIDTH: f32 = 180.0;
/// The arrows either side of tabs that don't fit.
const TAB_ARROW: f32 = 24.0;

/// Whether tabs of `natural` widths, `gap` apart, are wider than `budget` even at `min` each.
fn tabs_overflow(natural: &[f32], budget: f32, gap: f32, min: f32) -> bool {
    let narrowest: f32 = natural.iter().map(|w| w.min(min)).sum();
    narrowest + gap * natural.len().saturating_sub(1) as f32 > budget
}

/// A scroll arrow beside the tabs; dimmed (and inert) at its end. True when clicked.
fn arrow(ui: &mut egui::Ui, icon: &str, enabled: bool, tip: &str) -> bool {
    ui.add_enabled_ui(enabled, |ui| icons::button(ui, icon, TAB_ARROW, false, tip)).inner.clicked()
}

/// A tab name, at most 28 characters (longer names end in "…").
fn tab_name(name: &str) -> String {
    if name.chars().count() > 28 { format!("{}…", name.chars().take(27).collect::<String>()) } else { name.to_string() }
}

/// The width a tab takes when nothing has to shrink.
fn tab_natural_width(ui: &egui::Ui, t: &Tokens, name: &str) -> f32 {
    let label = crate::bidi::visual(&tab_name(name)).into_owned();
    ui.fonts_mut(|f| f.layout_no_wrap(label, theme::regular(13.0), t.text).size().x) + TAB_CHROME
}

/// The widest any tab may be so that tabs of `natural` widths, `gap` apart, fit in `budget`:
/// narrow tabs keep their width and the rest share what is left equally (as browsers do).
/// `None` when every tab fits as it is; never below `min` (then the strip scrolls).
fn tab_cap(natural: &[f32], budget: f32, gap: f32, min: f32) -> Option<f32> {
    let n = natural.len();
    if n == 0 {
        return None;
    }
    let mut left = budget - gap * (n - 1) as f32;
    if natural.iter().sum::<f32>() <= left {
        return None;
    }
    let mut sorted = natural.to_vec();
    sorted.sort_by(f32::total_cmp);
    for (k, w) in sorted.iter().enumerate() {
        let share = left / (n - k) as f32;
        if *w > share {
            return Some(share.max(min));
        }
        left -= w;
    }
    None
}

/// The chip's text: the group's name, and how many tabs it hides while folded.
fn chip_text(groups: &TabGroups, id: u32) -> String {
    let Some(g) = groups.get(id) else { return String::new() };
    let name = tab_name(&g.name);
    if !g.collapsed {
        return name;
    }
    let n = groups.members(id).len();
    if name.is_empty() { n.to_string() } else { format!("{name} · {n}") }
}

/// A chip's height inside the strip.
const CHIP_HEIGHT: f32 = 22.0;
/// A chip with no name: a swatch of its colour.
const CHIP_EMPTY: f32 = 18.0;
/// A chip while its name is typed.
const CHIP_EDITING: f32 = 132.0;

/// The width group `id`'s chip takes in the strip.
fn chip_width(ui: &egui::Ui, groups: &TabGroups, id: u32) -> f32 {
    if groups.renaming == Some(id) {
        return CHIP_EDITING;
    }
    let text = chip_text(groups, id);
    if text.is_empty() {
        return CHIP_EMPTY;
    }
    ui.fonts_mut(|f| f.layout_no_wrap(text, theme::medium(12.0), Color32::WHITE).size().x) + 18.0
}

/// A group's chip: a click folds or unfolds the group, a double-click renames it, and the
/// right-click menu renames, recolours, ungroups or closes it.
fn group_chip(ui: &mut egui::Ui, t: &Tokens, groups: &mut TabGroups, id: u32, height: f32) -> Option<GroupAction> {
    let color = tab_groups::color(groups.get(id)?.color);
    let width = chip_width(ui, groups, id);
    let height = ui.available_height().max(height);
    let (slot, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let rect = Rect::from_center_size(slot.center(), vec2(width, CHIP_HEIGHT));
    if groups.renaming == Some(id) {
        let g = groups.get_mut(id)?;
        let edit = egui::TextEdit::singleline(&mut g.name)
            .hint_text(tl!("Group name"))
            .char_limit(tab_groups::MAX_NAME)
            .font(theme::medium(12.0))
            .margin(vec2(6.0, 2.0))
            .desired_width(rect.width() - 12.0);
        let resp = ui.put(rect, edit);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, tl!("Group name")));
        ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.5, color), egui::StrokeKind::Outside);
        // Enter, Escape or a click elsewhere ends the edit; the name is kept as typed.
        if resp.lost_focus() {
            groups.renaming = None;
        } else if !resp.has_focus() {
            resp.request_focus();
        }
        return None;
    }
    let g = groups.get(id)?;
    let text = chip_text(groups, id);
    let a11y = if g.name.is_empty() { tab_groups::color_name(g.color).to_string() } else { g.name.clone() };
    let resp = ui.interact(rect, ui.id().with(("tab-group", id)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &a11y));
    let press = theme::Press::track(ui, &resp);
    let fill = if resp.hovered() { color.gamma_multiply(0.85) } else { color };
    ui.painter().rect_filled(rect.translate(press.offset()), CornerRadius::same(6), fill);
    if !text.is_empty() {
        // Dark text reads on every palette colour.
        ui.painter().text(rect.center() + press.offset(), Align2::CENTER_CENTER, text, theme::medium(12.0), Color32::from_rgb(0x11, 0x11, 0x13));
    }
    // The colour also runs under the chip, joining the line under the group's tabs.
    ui.painter().hline(slot.x_range().shrink(2.0), slot.bottom() - 1.5, Stroke::new(3.0, color));
    let mut action = None;
    let resp = theme::hand(resp).on_hover_text(tl!("Click to fold or unfold the group"));
    if resp.double_clicked() {
        action = Some(GroupAction::Rename(id));
    } else if resp.clicked() {
        action = Some(GroupAction::Toggle(id));
    }
    resp.context_menu(|ui| {
        if ui.button(tl!("Rename Group")).clicked() {
            action = Some(GroupAction::Rename(id));
            ui.close();
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for c in 0..tab_groups::COLORS.len() {
                let Ok(c) = u8::try_from(c) else { continue };
                let (r, swatch) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                let current = groups.get(id).is_some_and(|g| g.color == c);
                ui.painter().circle_filled(r.center(), 7.0, tab_groups::color(c));
                if current || swatch.hovered() {
                    ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(1.5, t.text));
                }
                swatch.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tab_groups::color_name(c)));
                if theme::hand(swatch).on_hover_text(tab_groups::color_name(c)).clicked() {
                    action = Some(GroupAction::Recolor(id, c));
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui.button(tl!("Ungroup")).clicked() {
            action = Some(GroupAction::Ungroup(id));
            ui.close();
        }
        if ui.button(tl!("Close Group")).clicked() {
            action = Some(GroupAction::Close(id));
            ui.close();
        }
    });
    action
}

/// The tab menu's group entries for the tab showing `doc`, which is in group `current`.
fn tab_group_menu(ui: &mut egui::Ui, groups: &TabGroups, doc: pdfcraft_engine::DocId, current: Option<u32>, action: &mut Option<GroupAction>) {
    if ui.button(tl!("Add Tab to New Group")).clicked() {
        *action = Some(GroupAction::New(doc));
        ui.close();
    }
    let others: Vec<&tab_groups::TabGroup> = groups.groups().iter().filter(|g| Some(g.id) != current).collect();
    if !others.is_empty() {
        ui.menu_button(tl!("Add Tab to Group"), |ui| {
            for g in others {
                let name = if g.name.is_empty() { tab_groups::color_name(g.color) } else { g.name.as_str() };
                let dot = egui::RichText::new("●").color(tab_groups::color(g.color));
                let resp = ui.horizontal(|ui| {
                    ui.label(dot);
                    ui.button(name)
                });
                if resp.inner.clicked() {
                    *action = Some(GroupAction::Add(doc, g.id));
                    ui.close();
                }
            }
        });
    }
    if current.is_some() && ui.button(tl!("Remove from Group")).clicked() {
        *action = Some(GroupAction::Remove(doc));
        ui.close();
    }
}

#[allow(clippy::too_many_arguments)]
fn tab(
    ui: &mut egui::Ui,
    id: egui::Id,
    t: &Tokens,
    icon: &str,
    name: &str,
    file: Option<&str>,
    dirty: bool,
    active: bool,
    close: &mut Option<usize>,
    index: usize,
    cap: Option<f32>,
    height: f32,
    draggable: bool,
) -> egui::Response {
    let font = theme::regular(13.0);
    let mut label = tab_name(name);
    // A shrunk tab shows as much of its name as fits (the tooltip has all of it).
    if let Some(cap) = cap {
        let room = (cap - TAB_CHROME).max(0.0);
        label = ui.fonts_mut(|f| {
            crate::panels::ellipsized_prefix(&label, |s| f.layout_no_wrap(crate::bidi::visual(s).into_owned(), font.clone(), t.text).size().x <= room)
        });
    }
    // Painted text only: the accessibility name below keeps the logical order.
    let label = crate::bidi::visual(&label).into_owned();
    let text_w = ui.fonts_mut(|f| f.layout_no_wrap(label.clone(), font.clone(), t.text).size().x);
    let width = cap.map_or(text_w + TAB_CHROME, |cap| (text_w + TAB_CHROME).min(cap));
    // Full strip height so the active tab sits on the mode bar instead of floating in the title bar.
    let height = ui.available_height().max(height);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let a11y = if dirty { format!("{name} (edited)") } else { name.to_string() };
    let sense = if draggable { Sense::click_and_drag() } else { Sense::click() };
    let resp = craft_ui::tabs::Tab::new(id, &a11y, active).sense(sense).focus_stroke(Stroke::new(1.0, t.accent)).show_at(ui, rect, |ui, resp| {
        let press = theme::Press::track(ui, resp);
        let bg = if active {
            press.fill(t.chrome, t.chrome.lerp_to_gamma(t.hover, 0.65), t.hover)
        } else {
            press.fill(Color32::TRANSPARENT, t.hover, t.pressed)
        };
        if bg.a() > 0 {
            ui.painter().rect_filled(rect, CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 }, bg);
        }
        let icon_rect = Rect::from_min_size(rect.min + vec2(6.0, (height - 16.0) / 2.0), vec2(16.0, 16.0));
        if icon == "file-text" {
            // A document tab shows the NadsPDF mark rather than a generic PDF file icon;
            // background tabs show it faded.
            let tint = if active { Color32::WHITE } else { Color32::WHITE.gamma_multiply(0.45) };
            egui::Image::from_bytes("bytes://nads-n.svg", include_bytes!("../../../assets/app-icon/nads-n.svg"))
                .tint(tint)
                .paint_at(ui, Rect::from_center_size(icon_rect.center(), vec2(14.0, 14.0)));
        } else {
            icons::paint(ui, icon_rect, icon, 15.0, if active { t.accent } else { t.text_muted });
        }
        ui.painter().text(rect.min + vec2(28.0, rect.height() / 2.0), Align2::LEFT_CENTER, label, font, if active { t.text } else { t.text_muted });
    });
    let x_rect = Rect::from_center_size(rect.right_center() - vec2(16.0, 0.0), vec2(20.0, 20.0));
    let x = ui.interact(x_rect, id.with("close"), Sense::click());
    let x_press = theme::Press::track(ui, &x);
    x_press.wash(ui, x_rect, 4, false);
    // Unsaved changes: a dot where the close button sits, until the tab is hovered.
    if dirty && !resp.hovered() && !x.hovered() {
        ui.painter().circle_filled(x_rect.center(), 4.0, if active { t.text } else { t.text_muted });
    } else if active || resp.hovered() || x.hovered() {
        icons::paint(ui, x_rect.translate(x_press.offset()), "x", 13.0, t.text_muted);
    }
    if theme::hand(x).clicked() {
        *close = Some(index);
    }
    let shown = match file {
        Some(file) => std::borrow::Cow::Owned(format!("{}\n{}", crate::bidi::visual(name), crate::bidi::visual(file))),
        None => crate::bidi::visual(name),
    };
    theme::hand(resp).on_hover_text(if dirty {
        crate::i18n::fmt(tl!("{name} — unsaved changes"), &[("name", shown.as_ref())])
    } else {
        shown.into_owned()
    })
}

/// The modes of the mode bar: (mode, icon, label).
const MODES: [(Mode, &str, &str); 5] = [
    (Mode::AllTools, "layout-grid", "All tools"),
    (Mode::Read, "book-open", "Read"),
    (Mode::Edit, "file-pen-line", "Edit"),
    (Mode::Convert, "arrow-left-right", "Convert"),
    (Mode::Sign, "signature", "E-Sign"),
];

/// Height of the mode bar: a [`widgets::SEARCH_HEIGHT`] row and 10 px above and below it.
const MODE_BAR: f32 = widgets::SEARCH_HEIGHT + 2.0 * 10.0;

/// `rect` filled with `top` fading to `bottom`.
fn vertical_gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    for (pos, color) in [(rect.left_top(), top), (rect.right_top(), top), (rect.right_bottom(), bottom), (rect.left_bottom(), bottom)] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// Least space between the modes and the search box.
const MODE_BAR_GAP: f32 = 16.0;

pub fn mode_bar(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    // nads.site's top bar: pill groups, with 10 px of air above and below them. The bar starts in
    // the active tab's colour, so the tab runs on into it, and fades to the title bar's colour
    // across its whole width.
    egui::Panel::top("mode_bar").exact_size(MODE_BAR).frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(10, 0))).show(ui, |ui| {
        let bar = ui.max_rect();
        vertical_gradient(ui.painter(), bar.expand2(vec2(10.0, 0.0)), t.chrome, t.chrome.lerp_to_gamma(t.titlebar, 0.7));
        let modes: Vec<(Mode, &str, &str)> = MODES.into_iter().filter(|&(mode, _, _)| app.offered_mode(mode) == mode).collect();
        let pills: Vec<(&str, bool)> = modes.iter().map(|&(mode, _, label)| (tl!(label), app.mode == mode)).collect();
        let modes_width = widgets::mode_pills_width(ui, &pills);
        // The modes sit at the left end; the search box and the document buttons at the right end,
        // the search box no wider than the room the modes leave it.
        let modes_right = bar.left() + modes_width;
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(Layout::right_to_left(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let has_doc = app.active.is_some();
            widgets::pill_group(ui, |ui| {
                // The group lays out right to left like the bar: Print is added first, so
                // it reads Properties, Save, Print.
                ui.add_enabled_ui(has_doc, |ui| {
                    let tip = crate::commands::command_tip(ui.ctx(), tl!("Print ({key})"), "print.dialog");
                    if icons::button(ui, "printer", widgets::PILL_HEIGHT, false, &tip).clicked() {
                        app.run_command("print.dialog");
                    }
                    let tip = crate::commands::command_tip(ui.ctx(), tl!("Save ({key})"), "file.save");
                    if icons::button(ui, "save", widgets::PILL_HEIGHT, false, &tip).clicked() {
                        app.run_command("file.save");
                    }
                    let tip = crate::commands::command_tip(ui.ctx(), tl!("Document properties ({key})"), "file.properties");
                    if icons::button(ui, "info", widgets::PILL_HEIGHT, false, &tip).clicked() {
                        app.dialog = Some(Dialog::Properties(PropsTab::Description));
                    }
                });
            });
            let room = ui.available_rect_before_wrap().right() - modes_right - MODE_BAR_GAP;
            if widgets::search_box(ui, tl!("Find tools and commands"), room.clamp(200.0, 340.0)).clicked() {
                app.palette_open = true;
            }
        });
        let slot = Rect::from_min_size(bar.left_top(), vec2(modes_width, bar.height()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(slot).layout(Layout::left_to_right(Align::Center)), |ui| {
            widgets::pill_group(ui, |ui| {
                for &(mode, icon, label) in &modes {
                    if widgets::mode_pill(ui, icon, tl!(label), app.mode == mode).clicked() {
                        app.switch_mode(mode, true);
                    }
                }
            });
        });
    });
}

/// The logo at the strip's left end: it is the main menu's button (File, Edit, Pages, View…).
fn logo_button(ui: &mut egui::Ui, t: &Tokens, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size + 8.0, size), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tl!("Menu")));
    let press = theme::Press::track(ui, &resp);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    press.wash(ui, rect, 8, open);
    let mark = Rect::from_center_size(rect.center() - vec2(3.0, 0.0) + press.offset(), vec2(18.0, 18.0));
    egui::Image::from_bytes("bytes://nads-n.svg", include_bytes!("../../../assets/app-icon/nads-n.svg")).paint_at(ui, mark);
    // A small chevron says the logo opens a menu.
    let tint = if resp.hovered() || open { t.text } else { t.text_faint };
    icons::paint(ui, Rect::from_center_size(pos2(mark.right() + 6.0, mark.bottom() - 2.0), vec2(9.0, 9.0)), "chevron-down", 9.0, tint);
    theme::hand(resp).on_hover_text(tl!("Menu"))
}

fn main_menu(app: &mut PdfCraftApp, ui: &mut egui::Ui, size: f32) {
    let t = Tokens::get(ui.ctx());
    let resp = logo_button(ui, &t, size);
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(230.0);
        ui.menu_button(tl!("File"), |ui| crate::commands::registry_menu(app, ui, "File"));
        ui.menu_button(tl!("Edit"), |ui| crate::commands::registry_menu(app, ui, "Edit"));
        ui.menu_button(tl!("Pages"), |ui| crate::commands::registry_menu(app, ui, "Pages"));
        ui.menu_button(tl!("View"), |ui| {
            if let Some(i) = app.active {
                let v = &mut app.views[i];
                let ctx = ui.ctx().clone();
                let key = |s| crate::commands::shortcut_label(&ctx, s);
                use crate::commands::{ACTUAL_SIZE, FIT_WIDTH, NEXT_VIEW, PAGE_LEVEL, PREV_VIEW, ROTATE_CCW, ROTATE_CW, ZOOM_IN, ZOOM_OUT};
                ui.label(egui::RichText::new(tl!("Zoom")).color(t.text_faint).small());
                if widgets::menu_item(ui, tl!("Actual size"), &key(ACTUAL_SIZE)).clicked() {
                    v.set_zoom(1.0);
                }
                if widgets::menu_item(ui, tl!("Zoom to page level"), &key(PAGE_LEVEL)).clicked() {
                    v.fit = Fit::Page;
                }
                if widgets::menu_item(ui, tl!("Fit to width"), &key(FIT_WIDTH)).clicked() {
                    v.fit = Fit::Width;
                }
                if widgets::menu_item(ui, tl!("Fit to height"), "").clicked() {
                    v.fit = Fit::Height;
                    v.goto = Some((v.current, 0.0));
                }
                if widgets::menu_item(ui, tl!("Fit visible"), &crate::commands::command_shortcut_label(&ctx, "view.fit_visible")).clicked() {
                    ui.close();
                    app.execute("view.fit_visible");
                    return;
                }
                if widgets::menu_item(ui, tl!("Zoom in"), &key(ZOOM_IN)).clicked() {
                    v.zoom_step(true);
                }
                if widgets::menu_item(ui, tl!("Zoom out"), &key(ZOOM_OUT)).clicked() {
                    v.zoom_step(false);
                }
                if widgets::menu_item(ui, tl!("Rotate view clockwise"), &key(ROTATE_CW)).clicked() {
                    v.rotate_view(true);
                }
                if widgets::menu_item(ui, tl!("Rotate view counterclockwise"), &key(ROTATE_CCW)).clicked() {
                    v.rotate_view(false);
                }
                ui.separator();
                ui.label(egui::RichText::new(tl!("Page navigation")).color(t.text_faint).small());
                if ui.add_enabled(!v.back.is_empty(), egui::Button::new(tl!("Previous view")).shortcut_text(key(PREV_VIEW))).clicked() {
                    v.view_history(false);
                }
                if ui.add_enabled(!v.forward.is_empty(), egui::Button::new(tl!("Next view")).shortcut_text(key(NEXT_VIEW))).clicked() {
                    v.view_history(true);
                }
                ui.separator();
                if let Some(id) = page_display_menu(ui, &app.views[i]) {
                    app.execute(id);
                }
                ui.separator();
            }
            crate::commands::registry_menu(app, ui, "View");
            ui.menu_button(tl!("Display theme"), |ui| theme_menu(app, ui));
            ui.menu_button(tl!("Side panels"), |ui| {
                for (p, label) in [
                    (RightPanel::Comments, tl!("Comments")),
                    (RightPanel::Bookmarks, tl!("Bookmarks")),
                    (RightPanel::Pages, tl!("Pages")),
                    (RightPanel::Fields, tl!("Fields")),
                    (RightPanel::Layers, tl!("Layers")),
                    (RightPanel::Attachments, tl!("Attachments")),
                    (RightPanel::Signatures, tl!("Signatures")),
                    (RightPanel::Accessibility, tl!("Accessibility Checker")),
                    (RightPanel::Tags, tl!("Tags")),
                    (RightPanel::Search, tl!("Search")),
                    (RightPanel::Compare, tl!("Compare")),
                ] {
                    // The simplified interface has no accessibility or compare tools to fill these.
                    if app.lite && matches!(p, RightPanel::Accessibility | RightPanel::Tags | RightPanel::Compare) {
                        continue;
                    }
                    if ui.add(widgets::radio(app.right == Some(p), tl!(label))).clicked() {
                        app.choose_right_panel(Some(p));
                    }
                }
            });
        });
        ui.menu_button(tl!("Help"), |ui| crate::commands::registry_menu(app, ui, "Help"));
    });
}

pub fn right_rail(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((index, id)) = app.active_ids() else { return };
    let Some(doc) = app.session.get(id) else { return };
    let has_signatures = doc.is_signed();
    let has_check = app.a11y.report.as_ref().is_some_and(|(d, _)| *d == id);
    let (has_comments, has_outline, has_fields, has_layers, has_files) = (
        !doc.info.annotations.is_empty(),
        !doc.info.outline.is_empty(),
        !doc.info.fields.is_empty(),
        !doc.info.layers.is_empty(),
        !doc.info.attachments.is_empty(),
    );
    let page_count = doc.info.pages.len();
    let labels: Vec<String> = doc.info.pages.iter().map(|p| p.label.clone()).collect();
    egui::Panel::right("rail")
        .resizable(false)
        .exact_size(56.0)
        .frame(
            egui::Frame::NONE
                .fill(t.chrome)
                .corner_radius(14)
                .stroke(Stroke::new(1.0, t.border))
                .outer_margin(egui::Margin { left: 0, right: 8, top: 0, bottom: 8 })
                .inner_margin(egui::Margin::symmetric(6, 8)),
        )
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let mut rail_button = |ui: &mut egui::Ui, panel: RightPanel, icon: &str, tip: &str, has: bool| {
                let selected = app.right == Some(panel) && (app.mode == Mode::Read || app.docking.exposed(crate::docking::Panel::Inspector));
                let r = icons::button(ui, icon, 34.0, selected, tl!(tip));
                if has && !selected {
                    let c = r.rect.right_top() + vec2(-8.0, 8.0);
                    ui.painter().circle_filled(c, 3.0, t.accent);
                }
                if r.clicked() {
                    app.choose_right_panel(if selected { None } else { Some(panel) });
                }
            };
            rail_button(ui, RightPanel::Comments, "message-square-text", "Comments", has_comments);
            rail_button(ui, RightPanel::Bookmarks, "bookmark", "Bookmarks", has_outline);
            rail_button(ui, RightPanel::Pages, "files", "Page thumbnails", false);
            rail_button(ui, RightPanel::Fields, "text-cursor-input", "Form fields", has_fields);
            rail_button(ui, RightPanel::Layers, "layers", "Layers", has_layers);
            rail_button(ui, RightPanel::Attachments, "paperclip", "Attachments", has_files);
            rail_button(ui, RightPanel::Signatures, "signature", "Signatures", has_signatures);
            if has_check {
                rail_button(ui, RightPanel::Accessibility, "accessibility", "Accessibility Checker", false);
            }
            rail_button(ui, RightPanel::Tags, "tag", "Tags", false);

            // Page navigation cluster at the bottom (as in Acrobat's rail).
            let view = &mut app.views[index];
            let mut page_display = None;
            ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let tip = crate::commands::key_tip(ui.ctx(), tl!("Zoom out ({key})"), crate::commands::ZOOM_OUT);
                if icons::button(ui, "zoom-out", 32.0, false, &tip).clicked() {
                    view.zoom_step(false);
                }
                let tip = crate::commands::key_tip(ui.ctx(), tl!("Zoom in ({key})"), crate::commands::ZOOM_IN);
                if icons::button(ui, "zoom-in", 32.0, false, &tip).clicked() {
                    view.zoom_step(true);
                }
                // Between rotate and zoom, as in Acrobat. Its command runs once `view` is free.
                let tip = crate::i18n::fmt(tl!("Page display: {layout}"), &[("layout", tl!(view.layout.label()))]);
                let resp = icons::button(ui, view.layout.icon(), 32.0, false, &tip);
                page_display = egui::Popup::menu(&resp)
                    .show(|ui| {
                        ui.set_min_width(220.0);
                        rail_view_menu(ui, view)
                    })
                    .and_then(|r| r.inner);
                let tip = crate::commands::key_tip(ui.ctx(), tl!("Rotate view clockwise ({key})"), crate::commands::ROTATE_CW);
                if icons::button(ui, "rotate-cw", 32.0, false, &tip).clicked() {
                    view.rotate_view(true);
                }
                // Not `columns-2` while fitting the page: that is the two-page view's icon.
                let fit_icon = if view.fit == Fit::Width { "maximize" } else { "maximize-2" };
                if icons::button(ui, fit_icon, 32.0, false, tl!("Toggle fit page / fit width")).clicked() {
                    view.fit = if view.fit == Fit::Width { Fit::Page } else { Fit::Width };
                    view.goto = Some((view.current, 0.0));
                }
                let zoom = format!("{:.0}%", view.zoom * 100.0);
                ui.allocate_ui_with_layout(vec2(40.0, 16.0), Layout::top_down(Align::Center), |ui| {
                    ui.label(egui::RichText::new(zoom).font(theme::medium(11.0)).color(t.text_muted));
                });
                ui.add_space(6.0);
                if icons::button(ui, "chevron-down", 30.0, false, tl!("Next page")).clicked() {
                    view.step_page(true);
                }
                if icons::button(ui, "chevron-up", 30.0, false, tl!("Previous page")).clicked() {
                    view.step_page(false);
                }
                ui.label(egui::RichText::new(page_count.to_string()).font(theme::regular(11.0)).color(t.text_muted));
                let edit = egui::TextEdit::singleline(&mut view.page_input)
                    .id(egui::Id::new("page-input"))
                    .desired_width(34.0)
                    .horizontal_align(Align::Center)
                    .font(theme::medium(12.0))
                    .margin(vec2(2.0, 4.0));
                let r = egui::Frame::NONE
                    .fill(t.field)
                    .stroke(Stroke::new(1.0, t.border))
                    .corner_radius(CornerRadius::same(5))
                    .show(ui, |ui| ui.add(edit))
                    .inner
                    .on_hover_text(tl!("Current page — type a page number or label (such as iv) and press Enter"));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    // A page label first (logical page numbers, as Acrobat), then a number.
                    let typed = view.page_input.clone();
                    if !view.go_to_typed(&typed, &labels) {
                        view.page_input = (view.current + 1).to_string();
                    }
                }
            });
            if let Some(id) = page_display {
                app.execute(id);
            }
        });
}

/// View ▸ Page display and the rail's Page display menu: the layouts and the cover page.
/// Returns the command a click asks for, for the caller to run.
fn page_display_menu(ui: &mut egui::Ui, view: &DocView) -> Option<&'static str> {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(tl!("Page display")).color(t.text_faint).small());
    let mut picked = None;
    for l in PageLayout::ORDER {
        if ui.add(widgets::radio(view.layout == l, tl!(l.label()))).clicked() {
            picked = Some(l.command());
        }
    }
    ui.separator();
    // Continuous scrolling is an independent checkbox over the base layout, as in Acrobat (#752).
    let mut continuous = view.continuous();
    if ui.add(widgets::checkbox(&mut continuous, tl!("Continuous scrolling"))).changed() {
        picked = Some("view.continuous");
    }
    let mut cover = view.cover;
    if ui.add_enabled(view.cover_applies(), widgets::checkbox(&mut cover, tl!("Show cover page in two-page view"))).changed() {
        picked = Some("view.layout.cover");
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// The rail's Page display menu: Acrobat's view choices around the page display. Zoom changes
/// apply at once; the command a click asks for is returned for the caller to run.
fn rail_view_menu(ui: &mut egui::Ui, view: &mut DocView) -> Option<&'static str> {
    let mut picked = None;
    for (id, label) in [("view.fit_width_scrolling", "Fit to width scrolling"), ("view.fit_one_page", "Fit one full page")] {
        if ui.button(tl!(label)).clicked() {
            picked = Some(id);
        }
    }
    ui.separator();
    if ui.button(tl!("Actual size")).clicked() {
        view.set_zoom(1.0);
        ui.close();
    }
    if ui.button(tl!("Zoom to page level")).clicked() {
        view.set_fit(Fit::Page);
        ui.close();
    }
    if ui.button(tl!("Fit visible")).clicked() {
        picked = Some("view.fit_visible");
    }
    ui.separator();
    picked = page_display_menu(ui, view).or(picked);
    ui.separator();
    for (id, label) in [("view.read_mode", "Read mode"), ("view.full_screen", "Full screen mode")] {
        if ui.button(tl!(label)).clicked() {
            picked = Some(id);
        }
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

#[cfg(test)]
mod tab_widths {
    use super::{MAC_CONTENT_LEFT, MAC_LIGHT, MAC_LIGHT_PITCH, MAC_LIGHTS_LEFT, MAC_TITLE_BAR, tab_cap};

    /// Tabs that fit keep their own widths.
    #[test]
    fn tabs_that_fit_do_not_shrink() {
        assert_eq!(tab_cap(&[200.0, 150.0], 400.0, 4.0, 110.0), None);
        assert_eq!(tab_cap(&[], 0.0, 4.0, 110.0), None);
    }

    /// Too wide: narrow tabs keep their width, the wide ones share the rest equally, and
    /// together they fill the strip exactly.
    #[test]
    fn wide_tabs_share_what_narrow_ones_leave() {
        let natural = [120.0, 300.0, 300.0];
        let cap = tab_cap(&natural, 500.0, 4.0, 110.0).expect("must shrink");
        let used: f32 = natural.iter().map(|w| w.min(cap)).sum::<f32>() + 2.0 * 4.0;
        assert!((used - 500.0).abs() < 0.01, "{cap} uses {used}");
        assert!((cap - 186.0).abs() < 0.01, "{cap}");
    }

    /// Arrows appear only when the tabs can't fit even at their narrowest.
    #[test]
    fn arrows_only_when_the_narrowest_tabs_overflow() {
        use super::tabs_overflow;
        assert!(!tabs_overflow(&[250.0; 3], 600.0, 4.0, 180.0), "three shrunk tabs fit");
        assert!(tabs_overflow(&[250.0; 4], 600.0, 4.0, 180.0), "four don't, even at 180");
        assert!(!tabs_overflow(&[100.0; 5], 600.0, 4.0, 180.0), "narrow tabs fit as they are");
        assert!(!tabs_overflow(&[], 0.0, 4.0, 180.0));
    }

    /// Tabs never get narrower than the minimum: past it, the strip scrolls instead.
    #[test]
    fn tabs_stop_at_their_minimum() {
        assert_eq!(tab_cap(&[250.0; 12], 600.0, 4.0, 110.0), Some(110.0));
        assert_eq!(tab_cap(&[250.0; 3], 0.0, 4.0, 110.0), Some(110.0));
    }

    #[test]
    fn the_mac_title_bar_centres_and_clears_the_traffic_lights() {
        // The lights are centred in the bar, and content keeps the same gap after them (#789).
        let margin = (MAC_TITLE_BAR - MAC_LIGHT) / 2.0;
        let lights_end = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT;
        assert_eq!(MAC_CONTENT_LEFT - lights_end, margin);
        // `egui::Margin` is in `i8`.
        assert!(MAC_CONTENT_LEFT <= f32::from(i8::MAX));
    }
}
