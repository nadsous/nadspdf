//! NadsPDF: the bubble shown over a web or email link in a PDF. The site's own icon (or a
//! coloured badge with its initial), its name, and the full address under it.
//!
//! **The site's icon is opt-in** (Preferences ▸ Integration and extensions ▸ Show websites'
//! icons): asking a website for its icon tells it the link was looked at, so by default nothing
//! leaves the computer and the badge stands in. With it on, the icon is asked for once per site
//! (`https://<host>/favicon.ico`, on a worker thread, through the desktop app's
//! [`FaviconFetch`]) and kept for the session.
//!
//! The bubble fades in over 140 ms with a strong ease-out the first time, and moves to the next
//! link without fading again, so sweeping across links feels immediate.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, vec2};

use crate::theme::{self, Tokens};

/// Asks `https://<host>/favicon.ico` (blocking; runs on its own thread). Set by the desktop app.
pub type FaviconFetch = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

/// Sites whose icon is kept per session, and requests under way at once.
const MAX_SITES: usize = 256;
const MAX_FETCHING: usize = 2;

/// What is known of each site's icon.
#[derive(Clone)]
enum Icon {
    Fetching,
    Bytes(Arc<Vec<u8>>),
    None,
}

/// The session's icons and what is being fetched, shared with the worker threads.
#[derive(Clone, Default)]
struct Cache(Arc<Mutex<HashMap<String, Icon>>>);

/// This frame's settings, published by the app ([`publish`]).
#[derive(Clone)]
struct Config {
    fetch: Option<FaviconFetch>,
}

fn config_id() -> egui::Id {
    egui::Id::unique("nadspdf-link-bubble-config")
}

fn cache_id() -> egui::Id {
    egui::Id::unique("nadspdf-link-bubble-cache")
}

/// Make the fetcher available to the page view this frame, or none when the preference is off.
pub(crate) fn publish(ctx: &egui::Context, fetch: Option<FaviconFetch>) {
    ctx.data_mut(|d| d.insert_temp(config_id(), Config { fetch }));
}

/// A link's site: its host (lowercase, without `www.`) for web addresses, its domain for email.
/// `None` for anything else.
pub(crate) fn site(url: &str) -> Option<(String, bool)> {
    let lower = url.trim().to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("mailto:") {
        let domain = rest.split(['?', '#']).next()?.rsplit_once('@')?.1.to_string();
        return valid_host(&domain).then_some((domain, true));
    }
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.split(':').next()?;
    let host = host.strip_prefix("www.").unwrap_or(host).to_string();
    valid_host(&host).then_some((host, false))
}

/// A plain DNS name (letters, digits, `-`, `.`), at most 253 characters.
fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.contains('.')
        && host.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        && !host.starts_with(['.', '-'])
}

/// The badge colour for `host`: one of a few nads-friendly hues, the same for the same site.
fn badge_color(host: &str) -> Color32 {
    const HUES: [Color32; 6] = [
        Color32::from_rgb(0xD9, 0x2D, 0x26),
        Color32::from_rgb(0x2F, 0x7D, 0x4F),
        Color32::from_rgb(0x28, 0x48, 0x8C),
        Color32::from_rgb(0xC9, 0x8A, 0x1E),
        Color32::from_rgb(0x7A, 0x3E, 0x9D),
        Color32::from_rgb(0x2A, 0x84, 0x8E),
    ];
    let sum = host.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(usize::from(b)));
    HUES.get(sum % HUES.len()).copied().unwrap_or(HUES[0])
}

/// The site's icon bytes when known; starts fetching them when allowed and not yet asked.
fn icon_bytes(ctx: &egui::Context, host: &str) -> Option<Arc<Vec<u8>>> {
    let fetch = ctx.data(|d| d.get_temp::<Config>(config_id())).and_then(|c| c.fetch)?;
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(cache_id()).clone());
    let mut map = cache.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match map.get(host) {
        Some(Icon::Bytes(b)) => return Some(b.clone()),
        Some(Icon::Fetching | Icon::None) => return None,
        None => {}
    }
    let fetching = map.values().filter(|i| matches!(i, Icon::Fetching)).count();
    if fetching >= MAX_FETCHING || map.len() >= MAX_SITES {
        return None;
    }
    map.insert(host.to_string(), Icon::Fetching);
    drop(map);
    let (owned, shared, waker) = (host.to_string(), cache.0.clone(), ctx.clone());
    let spawned = std::thread::Builder::new().name("nadspdf-favicon".into()).spawn(move || {
        let icon = match fetch(&owned) {
            Ok(bytes) if crate::raster_icon::decode(&bytes).is_some() => Icon::Bytes(Arc::new(bytes)),
            _ => Icon::None,
        };
        shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(owned, icon);
        waker.request_repaint();
    });
    if spawned.is_err() {
        cache.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(host.to_string(), Icon::None);
    }
    None
}

/// Draw the bubble for `url` near `pos`. Returns `false` (nothing drawn) when `url` isn't a web
/// or email address, so the caller shows its plain tip instead.
pub(crate) fn show(ctx: &egui::Context, pos: Pos2, url: &str) -> bool {
    let Some((host, email)) = site(url) else { return false };
    let t = Tokens::get(ctx);
    // Fades in the first time; moving to another link keeps it shown (no second fade).
    let shown = ctx.animate_bool_with_time(egui::Id::unique("nadspdf-link-bubble-shown"), true, 0.14);
    let alpha = 1.0 - (1.0 - shown).powi(4);
    // The page view's own tip area: it lets the pointer through and stays under dialogs.
    let area = egui::Area::new(egui::Id::unique("canvas-hover")).order(egui::Order::Tooltip).interactable(false);
    area.fixed_pos(pos + vec2(14.0, 18.0 + 4.0 * (1.0 - alpha))).show(ctx, |ui| {
        ui.multiply_opacity(alpha);
        egui::Frame::NONE
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(CornerRadius::same(12))
            .shadow(egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(90) })
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_max_width(340.0);
                ui.horizontal(|ui| {
                    let (badge, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                    let icon = (!email).then(|| icon_bytes(ctx, &host)).flatten();
                    let texture = icon.and_then(|b| crate::raster_icon::texture(ctx, &format!("favicon/{host}"), || Some(b.to_vec())));
                    match texture {
                        Some(tex) => {
                            ui.painter().rect_filled(badge, CornerRadius::same(8), Color32::WHITE);
                            egui::Image::new(&tex).paint_at(ui, Rect::from_center_size(badge.center(), vec2(20.0, 20.0)));
                        }
                        None => {
                            ui.painter().rect_filled(badge, CornerRadius::same(8), badge_color(&host));
                            let initial =
                                if email { "@".to_string() } else { host.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default() };
                            ui.painter().text(badge.center(), egui::Align2::CENTER_CENTER, initial, theme::semibold(15.0), Color32::WHITE);
                        }
                    }
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(egui::RichText::new(&host).font(theme::semibold(13.5)).color(t.text));
                        let shown_url =
                            if url.chars().count() > 120 { format!("{}…", url.chars().take(119).collect::<String>()) } else { url.to_string() };
                        ui.label(egui::RichText::new(shown_url).font(theme::regular(11.5)).color(t.text_muted));
                        let hint = if email { tl!("Click to write an email") } else { tl!("Click to open in your browser") };
                        ui.label(egui::RichText::new(hint).font(theme::regular(11.0)).color(t.text_faint));
                    });
                });
            });
    });
    true
}

/// No link under the pointer this frame: the next bubble fades in again.
pub(crate) fn hide(ctx: &egui::Context) {
    ctx.animate_bool_with_time(egui::Id::unique("nadspdf-link-bubble-shown"), false, 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_are_read_from_web_and_email_addresses_only() {
        assert_eq!(site("https://www.Nads.site/a?b#c"), Some(("nads.site".into(), false)));
        assert_eq!(site("http://user@example.com:8080/x"), Some(("example.com".into(), false)));
        assert_eq!(site("mailto:Contact@Example.org?subject=hi"), Some(("example.org".into(), true)));
        for bad in ["javascript:alert(1)", "file:///c:/x.pdf", "https://", "https://localhost/", "https://exa mple.com", "ftp://a.b"] {
            assert_eq!(site(bad), None, "{bad}");
        }
        assert_eq!(badge_color("nads.site"), badge_color("nads.site"));
    }
}
