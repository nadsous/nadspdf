//! NadsPDF: small raster pictures from untrusted bytes (an extension's PNG, a website's icon),
//! decoded once with bounded size and cached as textures by key.

use egui::{ColorImage, TextureHandle, TextureOptions};

/// The largest picture decoded, per side, and the memory one decode may take.
const MAX_SIDE: u32 = 512;
const MAX_ALLOC: u64 = 8 * 1024 * 1024;

/// The picture in `bytes` (PNG, or an ICO's best entry), or `None` when it isn't one, is too
/// large, or doesn't decode.
pub(crate) fn decode(bytes: &[u8]) -> Option<ColorImage> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => decode_as(bytes, image::ImageFormat::Png),
        // The `image` ICO reader refuses a whole icon when its best entry is a PNG that isn't
        // RGBA (a palette PNG, as some websites serve); the entries are then read one by one.
        image::ImageFormat::Ico => decode_as(bytes, image::ImageFormat::Ico).or_else(|| ico_entries(bytes)),
        _ => None,
    }
}

/// The most entries of an ICO directory looked at.
const MAX_ICO_ENTRIES: usize = 32;

/// The largest entry of the ICO in `bytes` that decodes on its own: a PNG entry as a PNG, any
/// other as a one-entry ICO. Entries that point outside the file are skipped.
fn ico_entries(bytes: &[u8]) -> Option<ColorImage> {
    let count = usize::from(u16::from_le_bytes([*bytes.get(4)?, *bytes.get(5)?])).min(MAX_ICO_ENTRIES);
    let mut entries: Vec<(u32, &[u8], &[u8])> = (0..count)
        .filter_map(|i| {
            let start = 6usize.checked_add(i.checked_mul(16)?)?;
            let entry = bytes.get(start..start.checked_add(16)?)?;
            // A side of 0 in the directory means 256.
            let side = |b: u8| if b == 0 { 256u32 } else { u32::from(b) };
            let area = side(*entry.first()?).saturating_mul(side(*entry.get(1)?));
            let size = usize::try_from(u32::from_le_bytes(entry.get(8..12)?.try_into().ok()?)).ok()?;
            let offset = usize::try_from(u32::from_le_bytes(entry.get(12..16)?.try_into().ok()?)).ok()?;
            let data = bytes.get(offset..offset.checked_add(size)?)?;
            Some((area, entry, data))
        })
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.0));
    entries.into_iter().find_map(|(_, entry, data)| {
        if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            return decode_as(data, image::ImageFormat::Png);
        }
        // One entry, its data right after the 6-byte header and 16-byte directory.
        let mut single = Vec::with_capacity(data.len().saturating_add(22));
        single.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
        single.extend_from_slice(entry.get(..12)?);
        single.extend_from_slice(&22u32.to_le_bytes());
        single.extend_from_slice(data);
        decode_as(&single, image::ImageFormat::Ico)
    })
}

/// The largest picture [`thumbnail`] decodes, per side, and the memory one decode may take.
const MAX_PHOTO_SIDE: u32 = 4096;
const MAX_PHOTO_ALLOC: u64 = 80 * 1024 * 1024;

/// A website's preview picture (PNG, JPEG or WebP) scaled down to fit `max_w` × `max_h`, or
/// `None` when it isn't one of those, is too large, or doesn't decode.
pub(crate) fn thumbnail(bytes: &[u8], max_w: u32, max_h: u32) -> Option<ColorImage> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP) {
        return None;
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_PHOTO_SIDE);
    limits.max_image_height = Some(MAX_PHOTO_SIDE);
    limits.max_alloc = Some(MAX_PHOTO_ALLOC);
    reader.limits(limits);
    let picture = reader.decode().ok()?;
    if picture.width() == 0 || picture.height() == 0 || max_w == 0 || max_h == 0 {
        return None;
    }
    let small = if picture.width() > max_w || picture.height() > max_h { picture.thumbnail(max_w, max_h) } else { picture };
    let rgba = small.into_rgba8();
    let size = [usize::try_from(rgba.width()).ok()?, usize::try_from(rgba.height()).ok()?];
    Some(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

fn decode_as(bytes: &[u8], format: image::ImageFormat) -> Option<ColorImage> {
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

#[cfg(test)]
mod ico_tests {
    use super::*;

    /// A `w`×`h` PNG with a 16-colour palette (4 bits a pixel), as some websites put in their icons.
    fn indexed_png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Indexed);
            enc.set_depth(png::BitDepth::Four);
            enc.set_palette((0..16u8).flat_map(|i| [i * 16, 45, 38]).collect::<Vec<u8>>());
            let mut writer = enc.write_header().unwrap();
            let row = usize::try_from(w.div_ceil(2)).unwrap();
            writer.write_image_data(&vec![0x12; row * usize::try_from(h).unwrap()]).unwrap();
        }
        out
    }

    fn rgba_png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    /// An ICO file holding `entries` (side, PNG bytes) in that order.
    fn ico(entries: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let count = u16::try_from(entries.len()).unwrap();
        let mut out = vec![0, 0, 1, 0];
        out.extend_from_slice(&count.to_le_bytes());
        let mut offset = 6 + 16 * u32::from(count);
        for (side, data) in entries {
            let size = u32::try_from(data.len()).unwrap();
            out.extend_from_slice(&[*side, *side, 0, 0, 1, 0, 32, 0]);
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            offset += size;
        }
        for (_, data) in entries {
            out.extend_from_slice(data);
        }
        out
    }

    #[test]
    fn an_icon_whose_largest_entry_is_a_palette_png_still_decodes() {
        let icon = ico(&[(16, rgba_png(16, 16)), (64, indexed_png(64, 64))]);
        let img = decode(&icon).expect("the icon decodes");
        assert_eq!(img.size, [64, 64], "the largest entry is used");
        // An icon with only a damaged entry is still refused.
        assert!(decode(&ico(&[(16, b"\x89PNG\r\n\x1a\nbroken".to_vec())])).is_none());
        // Entries pointing past the end of the file are skipped, not read.
        let mut cut = ico(&[(16, rgba_png(16, 16)), (64, indexed_png(64, 64))]);
        cut.truncate(cut.len() - 10);
        assert_eq!(decode(&cut).map(|i| i.size), Some([16, 16]), "the intact entry is used");
    }
}

#[cfg(test)]
mod thumbnail_tests {
    use super::*;

    #[test]
    fn a_large_preview_picture_is_scaled_down_and_keeps_its_shape() {
        let mut jpeg = Vec::new();
        image::RgbImage::from_pixel(1200, 630, image::Rgb([20, 30, 200]))
            .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .unwrap();
        let small = thumbnail(&jpeg, 320, 170).expect("a JPEG decodes");
        assert!(small.size[0] <= 320 && small.size[1] <= 170, "{:?}", small.size);
        assert!((small.size[0] as f32 / small.size[1] as f32 - 1200.0 / 630.0).abs() < 0.05, "{:?}", small.size);
        assert!(thumbnail(b"GIF89a....", 320, 170).is_none());
        assert!(thumbnail(&jpeg, 0, 170).is_none());
        assert!(thumbnail(&jpeg[..40], 320, 170).is_none(), "a cut picture is refused");
    }
}
