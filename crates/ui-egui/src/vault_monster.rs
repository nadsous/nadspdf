//! A vault's pixel monster: its identity, drawn from the vault's seed like a QR code is drawn from
//! its text. The same seed always gives the same monster and different seeds give different
//! ones: a body shape, eyes, a mouth, horns or antennae, arms, legs, and a texture of spots
//! unique to the seed, in the vault's colour. Mirrored left to right, as faces are.
//!
//! [`grid`] is the model (testable, no egui); [`paint`] draws it.

use egui::{Color32, CornerRadius, Rect, vec2};

/// The monster's grid: [`SIZE`] × [`SIZE`] cells.
pub const SIZE: usize = 12;
const HALF: usize = SIZE / 2;

/// What a cell holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Empty,
    /// The vault's colour.
    Body,
    /// A darker shade of it: the spots that make each monster unique, feet, horn tips.
    Shade,
    /// A lighter shade: highlights.
    Light,
    /// White: eyes and teeth.
    White,
    /// Near black: pupils and the mouth.
    Dark,
}

/// A small, fast, well-mixed generator (SplitMix64): the monster only needs to be stable and
/// varied, not secret.
struct Mix(u64);

impl Mix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`n` > 0).
    fn below(&mut self, n: u64) -> usize {
        usize::try_from(self.next() % n.max(1)).unwrap_or(0)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// The monster for `seed`.
pub fn grid(seed: u64) -> [[Cell; SIZE]; SIZE] {
    let mut g = [[Cell::Empty; SIZE]; SIZE];
    let mut m = Mix(seed);
    // The body: each row's half-width (from the centre line), from one of five silhouettes, so
    // monsters differ at a glance before their details do.
    let shape: [usize; SIZE] = match m.below(5) {
        // Round.
        0 => [0, 0, 3, 4, 5, 5, 5, 5, 5, 4, 3, 0],
        // Tall.
        1 => [0, 0, 3, 3, 4, 4, 4, 4, 4, 4, 3, 0],
        // Wide.
        2 => [0, 0, 0, 4, 5, 6, 6, 6, 6, 5, 4, 0],
        // Ghost: straight sides down to a wavy hem.
        3 => [0, 0, 3, 4, 4, 5, 5, 5, 5, 5, 5, 0],
        // Pear.
        _ => [0, 0, 2, 3, 3, 4, 5, 5, 6, 6, 5, 0],
    };
    let ghost = shape == [0, 0, 3, 4, 4, 5, 5, 5, 5, 5, 5, 0];
    for (row, half) in shape.iter().enumerate() {
        for col in HALF.saturating_sub(*half)..HALF {
            set(&mut g, row, col, Cell::Body);
        }
    }
    // A highlight on the brow.
    if let Some(col) = (0..HALF).find(|c| at(&g, 2, *c) == Cell::Body) {
        set(&mut g, 2, col.saturating_add(1).min(HALF - 1), Cell::Light);
    }
    // On top: horns, antennae with a ball, pointed ears, or a tuft.
    match m.below(5) {
        0 => {
            let col = HALF.saturating_sub(2 + m.below(2));
            set(&mut g, 1, col, Cell::Shade);
            set(&mut g, 0, col.saturating_sub(1), Cell::Shade);
        }
        1 => {
            let col = HALF.saturating_sub(1 + m.below(2));
            set(&mut g, 1, col, Cell::Body);
            set(&mut g, 0, col, Cell::Light);
        }
        2 => {
            let col = (0..HALF).find(|c| at(&g, 2, *c) != Cell::Empty).unwrap_or(2);
            set(&mut g, 1, col, Cell::Body);
            set(&mut g, 1, col.saturating_add(1), Cell::Body);
            set(&mut g, 0, col, Cell::Shade);
        }
        3 => {
            set(&mut g, 1, HALF - 1, Cell::Body);
            set(&mut g, 0, HALF - 1, Cell::Shade);
            set(&mut g, 1, HALF.saturating_sub(3), Cell::Body);
        }
        _ => {}
    }
    // Eyes: two (with pupils looking one way), one big cyclops eye, or three.
    let eye_row = 4 + m.below(2);
    let look = m.below(2);
    match m.below(6) {
        0 => {
            // Cyclops: a 2 × 2 eye on the centre line (mirrored, so 4 wide).
            for r in eye_row..eye_row + 2 {
                set(&mut g, r, HALF - 2, Cell::White);
                set(&mut g, r, HALF - 1, Cell::White);
            }
            set(&mut g, eye_row + look, HALF - 1, Cell::Dark);
        }
        1 => {
            // Three: a small middle one above two.
            set(&mut g, eye_row - 1, HALF - 1, Cell::Dark);
            let col = HALF.saturating_sub(3);
            set(&mut g, eye_row, col, Cell::White);
            set(&mut g, eye_row + 1, col, Cell::Dark);
        }
        _ => {
            let col = HALF.saturating_sub(2 + m.below(2));
            set(&mut g, eye_row, col, Cell::White);
            set(&mut g, eye_row, col.saturating_add(1), Cell::White);
            set(&mut g, eye_row + 1, col, Cell::White);
            set(&mut g, eye_row + 1, col.saturating_add(1), Cell::White);
            set(&mut g, eye_row + look, col.saturating_add(1 - look.min(1)), Cell::Dark);
        }
    }
    // The mouth: a smile, teeth, an open mouth or fangs.
    let mouth = eye_row + 3;
    match m.below(4) {
        0 => {
            for col in HALF.saturating_sub(2)..HALF {
                set(&mut g, mouth, col, Cell::Dark);
            }
            set(&mut g, mouth - 1, HALF.saturating_sub(3), Cell::Dark);
        }
        1 => {
            for (i, col) in (HALF.saturating_sub(3)..HALF).enumerate() {
                set(&mut g, mouth, col, if i % 2 == 0 { Cell::White } else { Cell::Dark });
            }
        }
        2 => {
            for col in HALF.saturating_sub(2)..HALF {
                set(&mut g, mouth, col, Cell::Dark);
                set(&mut g, mouth + 1, col, Cell::Dark);
            }
            set(&mut g, mouth + 1, HALF - 1, Cell::Light);
        }
        _ => {
            for col in HALF.saturating_sub(3)..HALF {
                set(&mut g, mouth, col, Cell::Dark);
            }
            set(&mut g, mouth + 1, HALF.saturating_sub(2), Cell::White);
        }
    }
    // Arms: a cell or two out from the side, raised or lowered.
    if m.chance(70) {
        let row = 6 + m.below(2);
        if let Some(edge) = (0..HALF).find(|c| at(&g, row, *c) != Cell::Empty) {
            set(&mut g, row, edge.saturating_sub(1), Cell::Body);
            if m.chance(50) && edge >= 2 {
                set(&mut g, row - 1, edge - 2, Cell::Shade);
            }
        }
    }
    // Feet: two or three legs, tentacles, or the ghost's wavy hem.
    let bottom = SIZE - 1;
    if ghost {
        for col in (0..HALF).filter(|c| c % 2 == 1) {
            if at(&g, bottom - 1, col) != Cell::Empty {
                set(&mut g, bottom, col, Cell::Body);
            }
        }
    } else {
        match m.below(3) {
            0 => {
                let col = HALF.saturating_sub(3 + m.below(2));
                set(&mut g, bottom, col, Cell::Shade);
                set(&mut g, bottom, col.saturating_add(1), Cell::Shade);
            }
            1 => {
                for col in [HALF.saturating_sub(4), HALF - 1] {
                    set(&mut g, bottom, col, Cell::Shade);
                }
            }
            _ => {
                for col in (HALF.saturating_sub(5)..HALF).filter(|c| c % 2 == 0) {
                    set(&mut g, bottom, col, Cell::Body);
                }
            }
        }
    }
    // The spots, last: the texture that makes the monster one of a kind, like a QR code's
    // modules. Any body cell may take a darker shade, from the seed's own bits.
    let spots = 18 + m.below(22) as u64;
    for row in 0..SIZE {
        for col in 0..HALF {
            // Around the eyes and mouth stays plain, so the face reads at a glance.
            let near_face = (row.saturating_sub(1)..=row + 1)
                .any(|r| (col.saturating_sub(1)..=col + 1).any(|c| matches!(at(&g, r, c), Cell::White | Cell::Dark)));
            if at(&g, row, col) == Cell::Body && !near_face && m.chance(spots) {
                set(&mut g, row, col, Cell::Shade);
            }
        }
    }
    g
}

/// Set a cell on the left half and its mirror on the right.
fn set(g: &mut [[Cell; SIZE]; SIZE], row: usize, col: usize, cell: Cell) {
    if col >= HALF {
        return;
    }
    if let Some(r) = g.get_mut(row) {
        if let Some(c) = r.get_mut(col) {
            *c = cell;
        }
        if let Some(c) = r.get_mut(SIZE - 1 - col) {
            *c = cell;
        }
    }
}

fn at(g: &[[Cell; SIZE]; SIZE], row: usize, col: usize) -> Cell {
    g.get(row).and_then(|r| r.get(col)).copied().unwrap_or(Cell::Empty)
}

/// A stable seed for a vault saved before vaults had one: from its name, so it keeps the same
/// monster from one start to the next.
pub fn seed_from_name(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// A fresh seed for a new monster: the clock's nanoseconds, mixed with `salt`.
pub fn new_seed(salt: u64) -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut m = Mix((nanos as u64) ^ salt.rotate_left(17));
    m.next()
}

/// Draw the monster for `seed` in `color` filling `rect`, on a soft tile of its colour.
pub fn paint(painter: &egui::Painter, rect: Rect, seed: u64, color: [u8; 3], radius: u8) {
    let base = Color32::from_rgb(color[0], color[1], color[2]);
    painter.rect_filled(rect, CornerRadius::same(radius), Color32::from_rgba_unmultiplied(color[0], color[1], color[2], 40));
    let shade = Color32::from_rgb(scale(color[0], 0.62), scale(color[1], 0.62), scale(color[2], 0.62));
    let light = Color32::from_rgb(lift(color[0]), lift(color[1]), lift(color[2]));
    let inner = rect.shrink(rect.width() * 0.1);
    let cell = (inner.width().min(inner.height()) / SIZE as f32).floor().max(1.0);
    let origin = inner.center() - vec2(cell * SIZE as f32, cell * SIZE as f32) * 0.5;
    for (row, cells) in grid(seed).iter().enumerate() {
        for (col, c) in cells.iter().enumerate() {
            let fill = match c {
                Cell::Empty => continue,
                Cell::Body => base,
                Cell::Shade => shade,
                Cell::Light => light,
                Cell::White => Color32::from_rgb(0xFA, 0xFA, 0xFA),
                Cell::Dark => Color32::from_rgb(0x1B, 0x1B, 0x1F),
            };
            let min = origin + vec2(cell * col as f32, cell * row as f32);
            painter.rect_filled(Rect::from_min_size(min, vec2(cell, cell)), CornerRadius::ZERO, fill);
        }
    }
}

fn scale(v: u8, by: f32) -> u8 {
    (f32::from(v) * by).round().clamp(0.0, 255.0) as u8
}

fn lift(v: u8) -> u8 {
    (f32::from(v) + (255.0 - f32::from(v)) * 0.45).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_monster_and_others_differ() {
        assert_eq!(grid(42), grid(42));
        let mut seen = std::collections::HashSet::new();
        for seed in 0..5000u64 {
            seen.insert(format!("{:?}", grid(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15))));
        }
        assert_eq!(seen.len(), 5000, "every seed draws its own monster");
    }

    #[test]
    fn every_monster_is_mirrored_and_has_eyes_and_a_mouth() {
        for seed in [0u64, 1, 7, u64::MAX, 0xDEAD_BEEF, seed_from_name("Impôts 2026")] {
            let g = grid(seed);
            for row in &g {
                for col in 0..SIZE {
                    assert_eq!(row[col], row[SIZE - 1 - col], "seed {seed}: mirrored");
                }
            }
            let count = |c: Cell| g.iter().flatten().filter(|x| **x == c).count();
            assert!(count(Cell::White) >= 1 && count(Cell::Dark) >= 2, "seed {seed}: eyes and a mouth");
            assert!(count(Cell::Body) + count(Cell::Shade) >= 30, "seed {seed}: a body");
        }
    }

    #[test]
    fn names_give_stable_seeds_and_new_seeds_vary() {
        assert_eq!(seed_from_name("Travaux"), seed_from_name("Travaux"));
        assert_ne!(seed_from_name("Travaux"), seed_from_name("Travail"));
        assert_ne!(new_seed(1), new_seed(2));
    }
}

#[cfg(test)]
mod sheet {
    /// A sheet of monsters for looking at them: `NADSPDF_MONSTER_SHEET=out.png cargo test -p
    /// pdfcraft-ui-egui --lib monster_sheet -- --ignored`.
    #[test]
    #[ignore = "writes a picture to look at"]
    fn monster_sheet() {
        use super::*;
        let Ok(out) = std::env::var("NADSPDF_MONSTER_SHEET") else { return };
        let colors = crate::vaults::COLORS;
        let (cols, rows, px) = (8u32, 4u32, 10u32);
        let side = SIZE as u32 * px + 2 * px;
        let mut img = image::RgbaImage::from_pixel(cols * side, rows * side, image::Rgba([250, 250, 250, 255]));
        for i in 0..cols * rows {
            let seed = new_seed(u64::from(i) * 7919);
            let c = colors[i as usize % colors.len()];
            let shade = [scale(c[0], 0.62), scale(c[1], 0.62), scale(c[2], 0.62)];
            let (ox, oy) = ((i % cols) * side + px, (i / cols) * side + px);
            for (r, line) in grid(seed).iter().enumerate() {
                for (k, cell) in line.iter().enumerate() {
                    let rgb = match cell {
                        Cell::Empty => continue,
                        Cell::Body => c,
                        Cell::Shade => shade,
                        Cell::Light => [lift(c[0]), lift(c[1]), lift(c[2])],
                        Cell::White => [250, 250, 250],
                        Cell::Dark => [27, 27, 31],
                    };
                    for y in 0..px {
                        for x in 0..px {
                            img.put_pixel(ox + k as u32 * px + x, oy + r as u32 * px + y, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
                        }
                    }
                }
            }
        }
        img.save(out).unwrap();
    }
}
