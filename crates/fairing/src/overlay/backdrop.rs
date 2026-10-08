//! **The frosted backdrop behind a card** — the page as it was when the card began to
//! arrive, shrunk and blurred once, laid under the card's plate so the card reads as glass.
//!
//! egui's painter draws triangles and cannot read back what is already on the screen, so a live
//! blur would need the GPU — the framebuffer copied, a two-pass shader, drawn back — which is a
//! runner-only feature, invisible to the headless tests and a real cost on a software-rendered
//! kiosk. A card does not need a live one. What is behind it barely changes while it is open, and
//! blurred to a frost it would not show if it did; so the overlay asks the runner for **one
//! screenshot** as a card first shows (`ViewportCommand::Screenshot`), frosts it here on the CPU,
//! uploads it once, and draws it under the plate. The cost is one framebuffer read and about two
//! milliseconds of arithmetic per open (1280 × 800; four at 1920 × 1080), and none per frame.
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
/// were. The largest a sum gets is `255 × SHRINK² × (2 × RADIUS + 1)^(2 × PASSES)` — under 2^31
/// with these constants, and a test holds it under `u32::MAX` if they change. The arithmetic
/// wraps, so it cannot trap: a running sum takes away only what it added, which is exact modulo
/// 2^32 for as long as the true sum fits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Sum {
    r: u32,
    g: u32,
    b: u32,
    a: u32,
    n: u32,
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
        let n = u64::from(self.n.max(1));
        let c = |v: u32| u8::try_from(((u64::from(v) + n / 2) / n).min(255)).unwrap_or(u8::MAX);
        Color32::from_rgba_premultiplied(c(self.r), c(self.g), c(self.b), c(self.a))
    }
}

/// **Frost a screenshot**: shrink it by [`SHRINK`] and blur it [`PASSES`] times at [`RADIUS`].
#[must_use]
pub(super) fn frost(image: &ColorImage) -> ColorImage {
    let (mut px, w, h) = shrink(image, SHRINK);
    let mut spare = vec![Sum::default(); px.len()];
    for _ in 0..PASSES {
        box_rows(&px, &mut spare, w, RADIUS);
        box_columns(&spare, &mut px, w, RADIUS);
    }
    ColorImage::new([w, h], px.into_iter().map(Sum::color).collect())
}

/// The sum of every `f × f` block (the last row and column of blocks may be short, and count
/// fewer pixels).
fn shrink(image: &ColorImage, f: usize) -> (Vec<Sum>, usize, usize) {
    let [w, h] = image.size;
    let f = f.max(1);
    let (ow, oh) = (w.div_ceil(f).max(1), h.div_ceil(f).max(1));
    let mut sums = vec![Sum::default(); ow * oh];
    for (y, row) in image.pixels.chunks(w.max(1)).enumerate() {
        let start = (y / f) * ow;
        let Some(out) = sums.get_mut(start..start + ow) else {
            continue;
        };
        for (acc, block) in out.iter_mut().zip(row.chunks(f)) {
            for c in block {
                acc.r = acc.r.wrapping_add(u32::from(c.r()));
                acc.g = acc.g.wrapping_add(u32::from(c.g()));
                acc.b = acc.b.wrapping_add(u32::from(c.b()));
                acc.a = acc.a.wrapping_add(u32::from(c.a()));
            }
            acc.n = acc
                .n
                .wrapping_add(u32::try_from(block.len()).unwrap_or(u32::MAX));
        }
    }
    (sums, ow, oh)
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
fn box_columns(src: &[Sum], dst: &mut [Sum], w: usize, r: usize) {
    let w = w.max(1);
    let row = |y: usize| src.get(y * w..(y + 1) * w).unwrap_or_default();
    let mut acc = vec![Sum::default(); w];
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
        out.copy_from_slice(&acc);
    }
}

#[cfg(test)]
mod tests {
    use super::{frost, PASSES, RADIUS, SHRINK};
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
    fn the_sums_fit_their_integers() {
        let window = 2 * RADIUS as u64 + 1;
        let passes = (0..2 * PASSES).fold(1_u64, |grown, _| grown * window);
        let most = 255 * (SHRINK * SHRINK) as u64 * passes;
        assert!(u32::try_from(most).is_ok(), "the largest sum is {most}");
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
