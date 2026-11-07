//! **The frosted backdrop behind a card** — the page as it was when the card began to
//! arrive, shrunk and blurred once, laid under the card's plate so the card reads as glass.
//!
//! egui's painter draws triangles and cannot read back what is already on the screen, so a live
//! blur would need the GPU — the framebuffer copied, a two-pass shader, drawn back — which is a
//! runner-only feature, invisible to the headless tests and a real cost on a software-rendered
//! kiosk. A card does not need a live one. What is behind it barely changes while it is open, and
//! blurred to a frost it would not show if it did; so the overlay asks the runner for **one
//! screenshot** as a card first shows (`ViewportCommand::Screenshot`), frosts it here on the CPU,
//! uploads it once, and draws it under the plate. Admitted readbacks and processing buffers are
//! bounded. A failed optional frost leaves a solid plate; no blur is recomputed on idle frames.
//!
//! The frost is a block-sum shrink by [`SHRINK`] and then [`PASSES`] separable box blurs of
//! radius [`RADIUS`] — three box passes are within a few per cent of a Gaussian, here one of
//! σ ≈ 28 screen pixels, and at an eighth of the size they cost a sixty-fourth. Linear filtering
//! scales it back up and adds its own softness. The arithmetic is on the premultiplied colours as
//! they come, which is what a frost wants: it averages what was drawn, not what was meant.
//!
//! **No pass divides.** Every texel carries the sum of the screen pixels that went into it and how
//! many there were, the passes only add and take away, and the one division is at the end. That is
//! exact — no light is rounded away pass by pass — and it is what made the first build slow: four
//! integer divisions per texel per pass were most of its time. A window shortened at an edge sums
//! fewer pixels and says so in its count, so the edge needs no padding rule.

use egui::{Color32, ColorImage};

/// How many screen pixels, on each axis, make one backdrop texel.
pub(super) const SHRINK: usize = 8;

/// The box blur's radius, in backdrop texels — 24 screen pixels.
pub(super) const RADIUS: usize = 3;

/// How many box passes: three are within a few per cent of a Gaussian.
const PASSES: usize = 3;

/// One texel: its channels summed over the screen pixels that went into it, and how many there
/// were. Adaptive shrinking uses u64 accumulators; the admitted source-pixel bound
/// times the blur multiplicity fits, including the largest input. The arithmetic
/// wraps, so it cannot trap: a running sum takes away only what it added, which is exact modulo
/// 2^64 for as long as the true sum fits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Sum {
    r: u64,
    g: u64,
    b: u64,
    a: u64,
    n: u64,
}

impl Sum {
    fn add(self, o: Self) -> Self {
        Self {
            r: self.r.wrapping_add(o.r),
            g: self.g.wrapping_add(o.g),
            b: self.b.wrapping_add(o.b),
            a: self.a.wrapping_add(o.a),
            n: self.n.wrapping_add(o.n),
        }
    }

    fn sub(self, o: Self) -> Self {
        Self {
            r: self.r.wrapping_sub(o.r),
            g: self.g.wrapping_sub(o.g),
            b: self.b.wrapping_sub(o.b),
            a: self.a.wrapping_sub(o.a),
            n: self.n.wrapping_sub(o.n),
        }
    }

    /// The mean colour, rounded — the one division.
    fn color(self) -> Color32 {
        let n = self.n.max(1);
        let c = |v: u64| u8::try_from(((v + n / 2) / n).min(255)).unwrap_or(u8::MAX);
        Color32::from_rgba_premultiplied(c(self.r), c(self.g), c(self.b), c(self.a))
    }
}

/// Maximum screenshot pixels admitted before requesting readback (64 MiB RGBA).
pub(super) const MAX_SOURCE_PIXELS: usize = 16 * 1024 * 1024;
/// At most 16,384 blurred texels; larger views use a coarser backdrop.
const MAX_FROST_PIXELS: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrostError {
    InvalidImage,
    TooLarge,
    AllocationFailed,
}

fn storage<T: Copy>(len: usize, value: T) -> Result<Vec<T>, FrostError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(len)
        .map_err(|_| FrostError::AllocationFailed)?;
    result.resize(len, value);
    Ok(result)
}

/// Validate before allocation. Reduce optional backdrop resolution before growing buffers.
pub(super) fn frost(image: &ColorImage) -> Result<ColorImage, FrostError> {
    let [width, height] = image.size;
    let source = width
        .checked_mul(height)
        .filter(|&n| n != 0)
        .ok_or(FrostError::InvalidImage)?;
    if image.pixels.len() != source {
        return Err(FrostError::InvalidImage);
    }
    if source > MAX_SOURCE_PIXELS {
        return Err(FrostError::TooLarge);
    }
    let mut factor = SHRINK;
    let (w, h) = loop {
        let w = width.div_ceil(factor);
        let h = height.div_ceil(factor);
        if w.checked_mul(h).is_some_and(|n| n <= MAX_FROST_PIXELS) && w <= 512 {
            break (w, h);
        }
        factor = factor.checked_mul(2).ok_or(FrostError::TooLarge)?;
    };
    let count = w.checked_mul(h).ok_or(FrostError::TooLarge)?;
    // Every buffer is fallible and bounded, including the column scratch formerly allocated per pass.
    let mut px = storage(count, Sum::default())?;
    let mut spare = storage(count, Sum::default())?;
    let mut columns = storage(w, Sum::default())?;
    let mut colors = storage(count, Color32::TRANSPARENT)?;
    for (y, row) in image.pixels.chunks(width).enumerate() {
        let start = (y / factor) * w;
        let out = px
            .get_mut(start..start + w)
            .ok_or(FrostError::InvalidImage)?;
        for (acc, block) in out.iter_mut().zip(row.chunks(factor)) {
            for color in block {
                acc.r += u64::from(color.r());
                acc.g += u64::from(color.g());
                acc.b += u64::from(color.b());
                acc.a += u64::from(color.a());
            }
            acc.n += u64::try_from(block.len()).map_err(|_| FrostError::TooLarge)?;
        }
    }
    for _ in 0..PASSES {
        box_rows(&px, &mut spare, w, RADIUS);
        box_columns(&spare, &mut px, w, RADIUS, &mut columns);
    }
    for (out, sum) in colors.iter_mut().zip(px) {
        *out = sum.color();
    }
    let w16 = u16::try_from(w).map_err(|_| FrostError::TooLarge)?;
    let h16 = u16::try_from(h).map_err(|_| FrostError::TooLarge)?;
    Ok(ColorImage {
        size: [w, h],
        source_size: egui::vec2(f32::from(w16), f32::from(h16)),
        pixels: colors,
    })
}

/// One box pass along every row of a `w`-wide image: each texel becomes the sum of the up to
/// `2r + 1` texels around it in its row, kept as a running sum.
fn box_rows(src: &[Sum], dst: &mut [Sum], w: usize, r: usize) {
    let w = w.max(1);
    for (row, out) in src.chunks(w).zip(dst.chunks_mut(w)) {
        let mut acc = row
            .iter()
            .take(r)
            .fold(Sum::default(), |acc, p| acc.add(*p));
        for (x, o) in out.iter_mut().enumerate() {
            if let Some(p) = row.get(x + r) {
                acc = acc.add(*p);
            }
            if let Some(p) = x.checked_sub(r + 1).and_then(|gone| row.get(gone)) {
                acc = acc.sub(*p);
            }
            *o = acc;
        }
    }
}

/// The same down every column: a running sum per column, a row added below and a row taken away
/// above at each step, so the image is walked in the order it lies in memory.
fn box_columns(src: &[Sum], dst: &mut [Sum], w: usize, r: usize, acc: &mut [Sum]) {
    let w = w.max(1);
    let row = |y: usize| src.get(y * w..(y + 1) * w).unwrap_or_default();
    acc.fill(Sum::default());
    for y in 0..r {
        for (a, p) in acc.iter_mut().zip(row(y)) {
            *a = a.add(*p);
        }
    }
    for (y, out) in dst.chunks_mut(w).enumerate() {
        for (a, p) in acc.iter_mut().zip(row(y + r)) {
            *a = a.add(*p);
        }
        if let Some(gone) = y.checked_sub(r + 1) {
            for (a, p) in acc.iter_mut().zip(row(gone)) {
                *a = a.sub(*p);
            }
        }
        out.copy_from_slice(acc);
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "image-quality fixtures must fit their explicit budget"
)]
mod tests {
    use super::{frost as try_frost, FrostError, MAX_FROST_PIXELS, PASSES, RADIUS, SHRINK};
    fn frost(image: &egui::ColorImage) -> egui::ColorImage {
        try_frost(image).expect("bounded fixture")
    }
    use egui::{Color32, ColorImage};

    fn image(w: usize, h: usize, at: impl Fn(usize, usize) -> Color32) -> ColorImage {
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                pixels.push(at(x, y));
            }
        }
        ColorImage::new([w, h], pixels)
    }

    fn texel(img: &ColorImage, x: usize, y: usize) -> Color32 {
        img.pixels
            .get(y * img.size[0] + x)
            .copied()
            .unwrap_or(Color32::TRANSPARENT)
    }

    /// No sum can outgrow its `u32`: a block of white, summed through every pass, still fits.
    #[test]
    fn malformed_images_are_rejected_and_larger_views_use_bounded_outputs() {
        for size in [[0, 1], [usize::MAX, 2], [2, 2]] {
            let bad = ColorImage {
                size,
                source_size: egui::Vec2::ZERO,
                pixels: Vec::new(),
            };
            assert_eq!(try_frost(&bad), Err(FrostError::InvalidImage));
        }
        let big = image(2048, 2048, |_, _| Color32::WHITE);
        let output = frost(&big);
        assert!(output.pixels.len() <= MAX_FROST_PIXELS);
        assert!(output.pixels.iter().all(|&p| p == Color32::WHITE));
    }

    #[test]
    fn the_sums_fit_their_integers() {
        let window = 2 * RADIUS as u64 + 1;
        let passes = (0..2 * PASSES).fold(1_u64, |grown, _| grown * window);
        let most = 255u64
            .checked_mul(super::MAX_SOURCE_PIXELS as u64)
            .and_then(|n| n.checked_mul(passes));
        assert!(most.is_some(), "all admitted source sums fit u64");
    }

    /// The frost is an eighth of the size on each axis, rounded up.
    #[test]
    fn the_frost_is_an_eighth_of_the_size() {
        let out = frost(&image(1282, 801, |_, _| Color32::WHITE));
        assert_eq!(
            out.size,
            [1282_usize.div_ceil(SHRINK), 801_usize.div_ceil(SHRINK)]
        );
    }

    /// A flat colour stays that colour — a blur moves colour about, it does not make or lose it,
    /// not even where a short block or a window shortened at an edge sums fewer pixels.
    #[test]
    fn a_flat_colour_stays_flat() {
        let teal = Color32::from_rgb(20, 140, 150);
        let out = frost(&image(323, 205, |_, _| teal));
        for c in &out.pixels {
            assert_eq!(*c, teal);
        }
    }

    /// A pattern finer than a texel frosts to its mean — every pixel is read, none skipped, so a
    /// one-pixel checkerboard or stripes cannot come out as one of their colours.
    #[test]
    fn a_fine_pattern_frosts_to_its_mean() {
        let patterns: [fn(usize, usize) -> bool; 3] = [
            |x, y| (x + y) % 2 == 0,
            |x, _| x % 2 == 0,
            |_, y| y % 2 == 0,
        ];
        for pattern in patterns {
            let out = frost(&image(160, 96, |x, y| {
                if pattern(x, y) {
                    Color32::WHITE
                } else {
                    Color32::BLACK
                }
            }));
            for c in &out.pixels {
                assert!((126..=129).contains(&c.r()), "a mid grey, not {c:?}");
            }
        }
    }

    /// A hard edge becomes a ramp: far from it the two sides keep their colours, at it the colour
    /// is in between, and it climbs monotonically from one side to the other.
    #[test]
    fn a_hard_edge_becomes_a_ramp() {
        let out = frost(&image(800, 40, |x, _| {
            if x < 400 {
                Color32::BLACK
            } else {
                Color32::WHITE
            }
        }));
        let y = out.size[1] / 2;
        let far = RADIUS * PASSES + 2;
        let mid = 400 / SHRINK;
        assert_eq!(
            texel(&out, mid - far, y).r(),
            0,
            "well to the left it is still black"
        );
        assert_eq!(
            texel(&out, mid + far, y).r(),
            255,
            "well to the right still white"
        );
        let at = texel(&out, mid, y).r();
        assert!(
            (60..=195).contains(&at),
            "at the edge it is in between: {at}"
        );
        let mut last = 0;
        for x in mid - far..mid + far {
            let v = texel(&out, x, y).r();
            assert!(v >= last, "the ramp only climbs: {last} then {v} at {x}");
            last = v;
        }
    }

    /// A small bright patch spreads into a soft blob and keeps its light.
    #[test]
    fn a_bright_patch_spreads_and_keeps_its_light() {
        let (w, h) = (512, 512);
        let lit = |x: usize, y: usize| (240..272).contains(&x) && (240..272).contains(&y);
        let input = image(w, h, |x, y| {
            if lit(x, y) {
                Color32::WHITE
            } else {
                Color32::BLACK
            }
        });
        let out = frost(&input);
        let total_in: u64 = input.pixels.iter().map(|c| u64::from(c.r())).sum();
        let total_out: u64 = out.pixels.iter().map(|c| u64::from(c.r())).sum();
        // The frost has a sixty-fourth of the texels, so the same light is a sixty-fourth of the
        // sum, give or take the last rounding to eight bits.
        let expected = total_in / (SHRINK * SHRINK) as u64;
        assert!(
            total_out * 100 >= expected * 97 && total_out * 100 <= expected * 103,
            "the light is kept: {total_out} of {expected}"
        );
        let centre = texel(&out, 256 / SHRINK, 256 / SHRINK).r();
        let edge = texel(&out, 256 / SHRINK + RADIUS * 2, 256 / SHRINK).r();
        assert!(
            centre < 255 && centre > edge && edge > 0,
            "a soft blob: {centre} then {edge}"
        );
    }
}
