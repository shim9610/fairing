//! Deterministic noise. No RNG crate — the constraint is zero new dependencies.
//!
//! **The seed changes the scatter and nothing else.** What `[desktop.abyss] seed` moves is where
//! the bubbles, the fish and the rocks go; the manta formation (how many, and the flight lines),
//! the number of god rays, the light position and the band heights are **authored constants**.
//! Leaving those to chance too would put a manta on top of the icons at some seed or other.

/// The lowbias32 integer hash. `wrapping_mul` is const-stable.
#[inline]
#[must_use]
pub(super) const fn hash32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// A `[0, 1)` value determined by `seed`, `index` and `channel`.
///
/// Each of the three is mixed in with a different odd constant — simply adding them would make
/// `(seed+1, index)` and `(seed, index+1)` come out the same.
#[inline]
#[must_use]
pub(super) fn rand01(seed: u32, index: u32, channel: u32) -> f32 {
    let mixed = hash32(
        seed ^ index
            .wrapping_mul(0x9e37_79b9)
            .wrapping_add(channel.wrapping_mul(0x85eb_ca6b)),
    );
    // Only the top 24 bits are used — an f32 has 24 significant bits, so the low 8 are eaten by the rounding anyway.
    #[expect(
        clippy::cast_precision_loss,
        reason = "24 bits fit exactly into an f32"
    )]
    let out = (mixed >> 8) as f32 / 16_777_216.0;
    out
}

/// Stratified sampling: one jittered point per cell of a `cols × rows` grid. No clumping and no
/// rejection-sampling loop.
///
/// It hands back `(index, u, v)` with `u` and `v` as `[0, 1)` screen fractions. Scattering 24
/// bubbles does not draw 24 random numbers; it divides the screen into a grid and jitters one
/// point inside each cell — which gives a blue-noise-like distribution at constant bake cost.
pub(super) fn stratified(seed: u32, cols: u32, rows: u32) -> impl Iterator<Item = (u32, f32, f32)> {
    let cols = cols.max(1);
    let rows = rows.max(1);
    #[expect(clippy::cast_precision_loss, reason = "the cell count is small")]
    let (fc, fr) = (cols as f32, rows as f32);
    (0..cols * rows).map(move |i| {
        let (cx, cy) = (i % cols, i / cols);
        #[expect(clippy::cast_precision_loss, reason = "the cell index is small")]
        let (x, y) = (cx as f32, cy as f32);
        let u = (x + rand01(seed, i, 0)) / fc;
        let v = (y + rand01(seed, i, 1)) / fr;
        (i, u, v)
    })
}

#[cfg(test)]
mod tests {
    use super::{hash32, rand01, stratified};

    /// The same input always gives the same value — the bake cache depends on it.
    /// This is a determinism check, so a bit-for-bit comparison is the right one (a tolerance
    /// would make the check meaningless).
    #[test]
    #[allow(clippy::float_cmp, reason = "it has to hold bit for bit")]
    fn noise_is_deterministic() {
        for i in 0..64 {
            assert_eq!(rand01(7, i, 0), rand01(7, i, 0));
            assert_eq!(hash32(i), hash32(i));
        }
    }

    /// The values stay inside `[0, 1)` — outside it, an object lands off-screen.
    #[test]
    fn values_stay_in_the_unit_range() {
        for seed in [0u32, 1, 0xFA12_1234, u32::MAX] {
            for i in 0..256 {
                for ch in 0..4 {
                    let v = rand01(seed, i, ch);
                    assert!((0.0..1.0).contains(&v), "seed {seed} i {i} ch {ch}: {v}");
                }
            }
        }
    }

    /// Without shaking each axis separately, the coordinates pile up along the diagonal.
    #[test]
    #[allow(clippy::float_cmp, reason = "the sameness is what is checked")]
    fn channels_are_independent() {
        let same = (0..64)
            .filter(|&i| rand01(9, i, 0) == rand01(9, i, 1))
            .count();
        assert_eq!(
            same, 0,
            "channels 0 and 1 giving the same value would make the jitter diagonal"
        );
    }

    /// A different seed moves the scatter (the composition does not change).
    #[test]
    fn seeds_move_the_scatter() {
        let a: Vec<_> = stratified(1, 4, 6).collect();
        let b: Vec<_> = stratified(2, 4, 6).collect();
        assert_eq!(a.len(), 24);
        assert_ne!(a, b);
    }

    /// The point of stratifying: every sample is **inside its own cell**. That is what stops the clumping.
    #[test]
    fn every_sample_stays_inside_its_own_cell() {
        let (cols, rows) = (4u32, 6u32);
        for (i, u, v) in stratified(0xFA12_1234, cols, rows) {
            let (cx, cy) = (i % cols, i / cols);
            #[expect(clippy::cast_precision_loss, reason = "the cell index is small")]
            let (lo_u, lo_v) = (cx as f32 / cols as f32, cy as f32 / rows as f32);
            #[expect(clippy::cast_precision_loss, reason = "the cell index is small")]
            let (hi_u, hi_v) = ((cx + 1) as f32 / cols as f32, (cy + 1) as f32 / rows as f32);
            assert!((lo_u..hi_u).contains(&u), "cell {i}'s u = {u}");
            assert!((lo_v..hi_v).contains(&v), "cell {i}'s v = {v}");
        }
    }

    /// Asking for zero cells does not panic (a config can hand over 0).
    #[test]
    fn zero_cells_do_not_panic() {
        assert_eq!(stratified(0, 0, 0).count(), 1);
    }
}
