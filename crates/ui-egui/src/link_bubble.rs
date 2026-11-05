//! NadsPDF: the bubble shown over a web or email link in a PDF. The site's own icon (or a
//! coloured badge with its initial), its name, and the full address under it.
//!
//! **The site's icon is opt-in** (Preferences ▸ Integration and extensions ▸ Show websites'
//! icons): asking a website for its icon tells it the link was looked at, so by default nothing
//! leaves the computer and the badge stands in. With it on, the icon is asked for once per site
//! (`https://<host>/favicon.ico`, on a worker thread, through the desktop app's
//! [`FaviconFetch`]) and kept for the session.
//!
//! **Vision Bubble** is a built-in extension, off by default (Home ▸ Extensions, or Preferences):
//! with it on, the bubble also shows a preview of the site, its title, description and picture,
//! read from the site's home page (never the link's own address, which can be a one-time action)
//! through the desktop app's [`PreviewFetch`], once per site and session.
//!
//! The bubble fades in over 140 ms with a strong ease-out the first time, and moves to the next
//! link without fading again, so sweeping across links feels immediate.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, vec2};

use crate::theme::{self, Tokens};

/// Asks `https://<host>/favicon.ico` (blocking; runs on its own thread). Set by the desktop app.
pub type FaviconFetch = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

/// What a website's home page says about it, for Vision Bubble.
#[derive(Clone, Debug, Default)]
pub struct SitePreview {
    pub title: String,
    pub description: String,
    /// Its preview picture (`og:image`), as downloaded.
    pub image: Option<Vec<u8>>,
    /// The icon it declares (often sharper than `favicon.ico`), as downloaded.
    pub icon: Option<Vec<u8>>,
}

/// Reads `https://<host>/` for a [`SitePreview`] (blocking; runs on its own thread). Set by the
/// desktop app.
pub type PreviewFetch = Arc<dyn Fn(&str) -> Result<SitePreview, String> + Send + Sync>;

/// The preview picture's largest size in the bubble, in points.
const PICTURE_W: f32 = 316.0;
const PICTURE_H: f32 = 166.0;

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

/// A site's preview, decoded on the worker thread.
struct Ready {
    title: String,
    description: String,
    picture: Option<egui::ColorImage>,
    icon: Option<Arc<Vec<u8>>>,
}

#[derive(Clone)]
enum Preview {
    Fetching,
    Ready(Arc<Ready>),
    None,
}

/// The session's previews, shared with the worker threads.
#[derive(Clone, Default)]
struct Previews(Arc<Mutex<HashMap<String, Preview>>>);

/// This frame's settings, published by the app ([`publish`]).
#[derive(Clone)]
struct Config {
    fetch: Option<FaviconFetch>,
    preview: Option<PreviewFetch>,
}

fn config_id() -> egui::Id {
    egui::Id::unique("nadspdf-link-bubble-config")
}

fn cache_id() -> egui::Id {
    egui::Id::unique("nadspdf-link-bubble-cache")
}

fn previews_id() -> egui::Id {
    egui::Id::unique("nadspdf-link-bubble-previews")
}

/// Make the fetchers available to the page view this frame: `fetch` when site icons are on,
/// `preview` when Vision Bubble is.
pub(crate) fn publish(ctx: &egui::Context, fetch: Option<FaviconFetch>, preview: Option<PreviewFetch>) {
    ctx.data_mut(|d| d.insert_temp(config_id(), Config { fetch, preview }));
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

/// A plain DNS name (letters, digits, `-`, `.`), at most 253 characters, whose last label has a
/// letter: never an IP address, so a document can't point NadsPDF at the local network.
fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.contains('.')
        && host.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        && !host.starts_with(['.', '-'])
        && host.rsplit('.').next().is_some_and(|tld| tld.bytes().any(|b| b.is_ascii_lowercase()))
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

/// The site's preview when known; starts reading it when Vision Bubble is on and it isn't asked
/// yet. `Some(None)` while it is on its way.
fn preview(ctx: &egui::Context, host: &str) -> Option<Option<Arc<Ready>>> {
    let fetch = ctx.data(|d| d.get_temp::<Config>(config_id())).and_then(|c| c.preview)?;
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Previews>(previews_id()).clone());
    let mut map = cache.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match map.get(host) {
        Some(Preview::Ready(r)) => return Some(Some(r.clone())),
        Some(Preview::Fetching) => return Some(None),
        Some(Preview::None) => return None,
        None => {}
    }
    let fetching = map.values().filter(|p| matches!(p, Preview::Fetching)).count();
    if fetching >= MAX_FETCHING || map.len() >= MAX_SITES {
        return None;
    }
    map.insert(host.to_string(), Preview::Fetching);
    drop(map);
    let (owned, shared, waker) = (host.to_string(), cache.0.clone(), ctx.clone());
    let spawned = std::thread::Builder::new().name("nadspdf-vision".into()).spawn(move || {
        let preview = match fetch(&owned) {
            Ok(p) => {
                let picture = p.image.as_deref().and_then(|b| crate::raster_icon::thumbnail(b, PICTURE_W as u32 * 2, PICTURE_H as u32 * 2));
                let icon = p.icon.filter(|b| crate::raster_icon::decode(b).is_some()).map(Arc::new);
                if p.title.is_empty() && p.description.is_empty() && picture.is_none() && icon.is_none() {
                    Preview::None
                } else {
                    Preview::Ready(Arc::new(Ready { title: p.title, description: p.description, picture, icon }))
                }
            }
            Err(_) => Preview::None,
        };
        shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(owned, preview);
        waker.request_repaint();
    });
    if spawned.is_err() {
        cache.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(host.to_string(), Preview::None);
        return None;
    }
    Some(None)
}

/// The preview picture's texture, made once per site.
fn picture_texture(ctx: &egui::Context, host: &str, ready: &Ready) -> Option<egui::TextureHandle> {
    let picture = ready.picture.as_ref()?;
    let id = egui::Id::unique(("nadspdf-vision-picture", host));
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(tex);
    }
    let tex = ctx.load_texture(format!("vision/{host}"), picture.clone(), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
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
    // Under the pointer in the top half of the window, over it in the bottom half, so a tall
    // preview stays on screen.
    let below = pos.y < ctx.content_rect().center().y;
    let rise = 4.0 * (1.0 - alpha);
    let area = if below {
        area.pivot(egui::Align2::LEFT_TOP).fixed_pos(pos + vec2(14.0, 18.0 + rise))
    } else {
        area.pivot(egui::Align2::LEFT_BOTTOM).fixed_pos(pos + vec2(14.0, -12.0 + rise))
    };
    area.constrain(true).show(ctx, |ui| {
        ui.multiply_opacity(alpha);
        egui::Frame::NONE
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(CornerRadius::same(12))
            .shadow(egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(90) })
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_max_width(340.0);
                let vision = if email { None } else { preview(ctx, &host) };
                let ready = vision.clone().flatten();
                if let Some(ready) = &ready {
                    ui.set_width(PICTURE_W);
                    if let Some(tex) = picture_texture(ctx, &host, ready) {
                        let size = tex.size_vec2();
                        let scale = (PICTURE_W / size.x.max(1.0)).min(PICTURE_H / size.y.max(1.0));
                        let shown = vec2(size.x * scale, size.y * scale);
                        let (rect, _) = ui.allocate_exact_size(vec2(PICTURE_W, shown.y), Sense::hover());
                        let rect = Rect::from_center_size(rect.center(), shown);
                        egui::Image::new(&tex).corner_radius(CornerRadius::same(8)).paint_at(ui, rect);
                        ui.add_space(8.0);
                    }
                }
                ui.horizontal(|ui| {
                    let (badge, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                    // The icon the site declares, else its favicon.ico.
                    let declared = ready.as_ref().and_then(|r| r.icon.clone()).map(|b| (b, "vision-icon"));
                    let icon = declared.or_else(|| (!email).then(|| icon_bytes(ctx, &host)).flatten().map(|b| (b, "favicon")));
                    let texture = icon.and_then(|(b, kind)| crate::raster_icon::texture(ctx, &format!("{kind}/{host}"), || Some(b.to_vec())));
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
                        let title = ready.as_ref().map(|r| r.title.as_str()).filter(|s| !s.is_empty());
                        ui.add(egui::Label::new(egui::RichText::new(title.unwrap_or(&host)).font(theme::semibold(13.5)).color(t.text)).wrap());
                        if title.is_some() {
                            ui.label(egui::RichText::new(&host).font(theme::regular(11.5)).color(t.accent_text));
                        }
                        let shown_url =
                            if url.chars().count() > 120 { format!("{}…", url.chars().take(119).collect::<String>()) } else { url.to_string() };
                        ui.label(egui::RichText::new(shown_url).font(theme::regular(11.5)).color(t.text_muted));
                        let hint = if email { tl!("Click to write an email") } else { tl!("Click to open in your browser") };
                        ui.label(egui::RichText::new(hint).font(theme::regular(11.0)).color(t.text_faint));
                    });
                });
                if let Some(ready) = &ready
                    && !ready.description.is_empty()
                {
                    ui.add_space(6.0);
                    let mut job = egui::text::LayoutJob::simple(ready.description.clone(), theme::regular(12.0), t.text_muted, PICTURE_W);
                    job.wrap.max_rows = 3;
                    let galley = ui.fonts_mut(|f| f.layout_job(job));
                    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
                    ui.painter().galley(rect.min, galley, t.text_muted);
                }
                if matches!(vision, Some(None)) {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(11.0).color(t.text_faint));
                        ui.label(egui::RichText::new(tl!("Vision Bubble is reading the site…")).font(theme::regular(11.0)).color(t.text_faint));
                    });
                }
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
        for bad in [
            "javascript:alert(1)",
            "file:///c:/x.pdf",
            "https://",
            "https://localhost/",
            "https://exa mple.com",
            "ftp://a.b",
            "http://192.168.1.1/admin",
            "https://10.0.0.1:8080/",
        ] {
            assert_eq!(site(bad), None, "{bad}");
        }
        assert_eq!(badge_color("nads.site"), badge_color("nads.site"));
    }
}
