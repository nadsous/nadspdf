//! NadsPDF: small raster pictures from untrusted bytes (an extension's PNG, a website's icon),
//! decoded once with bounded size and cached as textures by key.

use egui::{ColorImage, TextureHandle, TextureOptions};

/// The largest picture decoded, per side, and the memory one decode may take.
const MAX_SIDE: u32 = 512;
const MAX_ALLOC: u64 = 8 * 1024 * 1024;

/// The picture in `bytes` (PNG, or an ICO's best entry), or `None` when it isn't one, is too
/// large, or doesn't decode.
pub(crate) fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Ico) {
        return None;
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let rgba = reader.decode().ok()?.into_rgba8();
    let size = [usize::try_from(rgba.width()).ok()?, usize::try_from(rgba.height()).ok()?];
    if size[0] == 0 || size[1] == 0 {
        return None;
    }
    Some(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

/// The texture for `key`, decoding `bytes` the first time; a picture that doesn't decode is
/// remembered too, so it isn't tried every frame.
pub(crate) fn texture(ctx: &egui::Context, key: &str, bytes: impl FnOnce() -> Option<Vec<u8>>) -> Option<TextureHandle> {
    let id = egui::Id::unique(("nadspdf-raster", key));
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<TextureHandle>>(id)) {
        return cached;
    }
    let made = bytes().and_then(|b| decode(&b)).map(|img| ctx.load_texture(key, img, TextureOptions::LINEAR));
    ctx.data_mut(|d| d.insert_temp(id, made.clone()));
    made
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_small_pngs_and_icons_decode() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 3, image::Rgba([217, 45, 38, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let img = decode(&png).expect("a small PNG");
        assert_eq!(img.size, [4, 3]);
        assert!(decode(b"GIF89a....").is_none(), "other formats are left out");
        assert!(decode(b"\x89PNG\r\n\x1a\nbroken").is_none(), "a damaged PNG is refused");
        let mut big = Vec::new();
        image::RgbaImage::new(MAX_SIDE + 1, 2).write_to(&mut std::io::Cursor::new(&mut big), image::ImageFormat::Png).unwrap();
        assert!(decode(&big).is_none(), "too wide");
    }
}
