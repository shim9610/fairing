//! Allocation-free retention of recent display samples and an allocation-free warning raster.
//! These APIs own display copies only; they never discard application records or commands.

/// What was omitted from a display view. Counters saturate; reporting never allocates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DisplayStatus {
    /// Display items/samples omitted by retention, view limits or rejection.
    /// Merged observations may overlap; this is not a count of lost acquisition records.
    pub discarded: usize,
    /// An optional display allocation failed. Reclamation may subsequently have succeeded.
    pub allocation_failed: bool,
}
impl DisplayStatus {
    /// Whether a reduced-display warning is needed.
    #[must_use]
    pub const fn is_reduced(self) -> bool {
        self.discarded != 0 || self.allocation_failed
    }
    /// Static notice, without constructing or formatting a String.
    #[must_use]
    pub const fn message(self) -> Option<&'static str> {
        if self.is_reduced() {
            Some("DISPLAY LIMITED")
        } else {
            None
        }
    }
    /// Combine observations without allocation or counter wrap.
    pub fn merge(&mut self, other: Self) {
        self.discarded = self.discarded.saturating_add(other.discarded);
        self.allocation_failed |= other.allocation_failed;
    }
}

#[allow(
    clippy::implicit_saturating_sub,
    reason = "this explicit branch is verified verbatim over every usize input by Verus"
)]
pub(crate) const fn display_skip(retained: usize, limit: usize) -> usize {
    if retained > limit {
        retained - limit
    } else {
        0
    }
}

/// N inline display samples. Full storage overwrites the oldest sample.
/// There is no heap allocation, overflow buffer or hidden retry queue.
/// T is Copy so retained samples cannot own a destructor/heap allocation.
/// Borrowed data inside T remains owned by its caller.
///
/// Large N requires suitably sized caller stack/static storage.
/// ```
/// use fairing_widgets::memory::DisplayWindow;
/// let mut view = DisplayWindow::<u32, 3>::new();
/// let status = view.extend(&[1, 2, 3, 4, 5]);
/// assert_eq!(status.discarded, 2);
/// assert!(view.recent(2).copied().eq([4, 5]));
/// ```
pub struct DisplayWindow<T: Copy, const N: usize> {
    samples: [Option<T>; N],
    head: usize,
    len: usize,
    status: DisplayStatus,
}
impl<T: Copy, const N: usize> std::fmt::Debug for DisplayWindow<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisplayWindow")
            .field("capacity", &N)
            .field("len", &self.len)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}
impl<T: Copy, const N: usize> Default for DisplayWindow<T, N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: Copy, const N: usize> DisplayWindow<T, N> {
    /// Initialize inline storage without allocating, including when N is zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            samples: [None; N],
            head: 0,
            len: 0,
            status: DisplayStatus {
                discarded: 0,
                allocation_failed: false,
            },
        }
    }
    fn index(&self, offset: usize) -> usize {
        let remaining = N.saturating_sub(self.head);
        if offset < remaining {
            self.head + offset
        } else {
            offset - remaining
        }
    }
    /// Append one sample; return the number discarded (zero or one).
    pub fn push(&mut self, value: T) -> DisplayStatus {
        let discarded = usize::from(N == 0 || self.len == N);
        if N != 0 {
            let index = if self.len == N {
                let index = self.head;
                self.head = if self.head == N - 1 { 0 } else { self.head + 1 };
                index
            } else {
                let index = self.index(self.len);
                self.len += 1;
                index
            };
            if let Some(slot) = self.samples.get_mut(index) {
                *slot = Some(value);
            }
        }
        let status = DisplayStatus {
            discarded,
            allocation_failed: false,
        };
        self.status.merge(status);
        status
    }
    /// Retain the newest N samples. At most N input elements are read/copied,
    /// even when the input contains millions of older samples.
    pub fn extend(&mut self, samples: &[T]) -> DisplayStatus {
        let mut status = DisplayStatus::default();
        if samples.len() >= N {
            status.discarded = self.len.saturating_add(samples.len().saturating_sub(N));
            self.clear();
            let start = samples.len().saturating_sub(N);
            for value in samples.get(start..).unwrap_or_default() {
                let _ = self.push(*value);
            }
            // push() retained all N values and did not omit anything itself.
            self.status.merge(status);
        } else {
            for value in samples {
                status.merge(self.push(*value));
            }
        }
        status
    }
    /// View only the newest `limit` samples, in chronological order, without copying.
    pub fn recent(&self, limit: usize) -> impl Iterator<Item = &T> {
        let skip = display_skip(self.len, limit);
        (skip..self.len).filter_map(move |offset| self.samples.get(self.index(offset))?.as_ref())
    }
    /// Status for a clipped view, without changing retained data or allocating.
    #[must_use]
    pub fn view_status(&self, limit: usize) -> DisplayStatus {
        DisplayStatus {
            discarded: display_skip(self.len, limit),
            allocation_failed: false,
        }
    }
    /// Clear display samples only. Retain the omission notice until explicitly acknowledged.
    pub fn clear(&mut self) {
        self.samples.fill(None);
        self.head = 0;
        self.len = 0;
    }
    /// Number of retained display samples.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }
    /// Whether no sample is retained.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Compile-time display capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }
    /// Read and acknowledge accumulated omissions without allocating.
    pub fn take_status(&mut self) -> DisplayStatus {
        std::mem::take(&mut self.status)
    }
}

/// Invalid caller-provided warning surface. No error message allocation is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarningSurfaceError;
impl std::fmt::Display for WarningSurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid display warning surface")
    }
}
impl std::error::Error for WarningSurfaceError {}

// Five-bit rows of the fixed warning alphabet. No font loader, glyph atlas or text layout.
const fn glyph(letter: u8) -> [u8; 7] {
    match letter {
        b'D' => [30, 17, 17, 17, 17, 17, 30],
        b'I' => [31, 4, 4, 4, 4, 4, 31],
        b'S' => [15, 16, 16, 14, 1, 1, 30],
        b'P' => [30, 17, 17, 30, 16, 16, 16],
        b'L' => [16, 16, 16, 16, 16, 16, 31],
        b'A' => [14, 17, 17, 31, 17, 17, 17],
        b'Y' => [17, 17, 10, 4, 4, 4, 4],
        b'M' => [17, 27, 21, 21, 17, 17, 17],
        b'T' => [31, 4, 4, 4, 4, 4, 4],
        b'E' => [31, 16, 16, 30, 16, 16, 31],
        _ => [0; 7],
    }
}

// All indexing is evaluated by rustc over this fixed bitmap, never over runtime input.
#[allow(
    clippy::indexing_slicing,
    reason = "fixed loop bounds are checked during constant evaluation"
)]
const fn prepared_warning() -> [[u8; 4]; 96 * 12] {
    let mut pixels = [[24, 24, 24, 255]; 96 * 12];
    let text = b"DISPLAY LIMITED";
    let mut letter = 0;
    while letter < text.len() {
        let rows = glyph(text[letter]);
        let mut y = 0;
        while y < 7 {
            let mut x = 0;
            while x < 5 {
                if rows[y] & (1 << (4 - x)) != 0 {
                    pixels[(2 + y) * 96 + 2 + letter * 6 + x] = [255, 208, 64, 255];
                }
                x += 1;
            }
            y += 1;
        }
        letter += 1;
    }
    pixels
}
static WARNING_TILE: [[u8; 4]; 96 * 12] = prepared_warning();

/// Borrow the compiled warning tile. No runtime preparation, allocation or font resource.
#[must_use]
pub fn display_warning_tile() -> ([usize; 2], &'static [[u8; 4]]) {
    ([96, 12], &WARNING_TILE)
}

/// Clear a preallocated RGBA8 surface and rasterize "DISPLAY LIMITED" into it.
/// This function does not call egui, allocate, load fonts or upload a texture.
/// The host must present/reuse this surface through its already prepared backend.
/// It cannot make an allocating presentation API recoverable.
/// # Errors
/// Dimensions overflow, are zero, exceed the caller's pixel budget, or mismatch the slice.
pub fn paint_display_warning(
    rgba: &mut [u8],
    size: [usize; 2],
    max_pixels: usize,
) -> Result<(), WarningSurfaceError> {
    let [w, h] = size;
    let pixels = w
        .checked_mul(h)
        .filter(|&p| p != 0 && p <= max_pixels)
        .ok_or(WarningSurfaceError)?;
    if pixels.checked_mul(4) != Some(rgba.len()) {
        return Err(WarningSurfaceError);
    }
    for pixel in rgba.as_chunks_mut::<4>().0.iter_mut() {
        pixel.copy_from_slice(&[24, 24, 24, 255]);
    }
    draw_notice(rgba, size);
    Ok(())
}

// Caller validated the surface once. On tiny targets, use one warning-colour pixel.
fn draw_notice(rgba: &mut [u8], [w, h]: [usize; 2]) {
    if w < 92 || h < 11 {
        let start = rgba.len().saturating_sub(4);
        if let Some(pixel) = rgba.get_mut(start..) {
            pixel.copy_from_slice(&[255, 208, 64, 255]);
        }
        return;
    }
    for row in rgba.as_chunks_mut::<4>().0.chunks_mut(w).take(11) {
        for pixel in row.iter_mut().take(92) {
            pixel.copy_from_slice(&[24, 24, 24, 255]);
        }
    }
    for (letter, byte) in b"DISPLAY LIMITED".iter().enumerate() {
        for (y, row) in glyph(*byte).iter().enumerate() {
            for x in 0..5 {
                let px = 2 + letter * 6 + x;
                let py = 2 + y;
                if row & (1 << (4 - x)) == 0 || px >= w || py >= h {
                    continue;
                }
                let at = (py * w + px) * 4;
                if let Some(pixel) = rgba.get_mut(at..at + 4) {
                    pixel.copy_from_slice(&[255, 208, 64, 255]);
                }
            }
        }
    }
}

/// Rasterize a borrowed RGBA8 image directly into a preallocated, bounded surface.
/// Input is validated before writing. Large images are sampled to the output size;
/// no decoded copy, work buffer or texture is allocated here. A reduced preview
/// includes a static warning; targets too small for text receive a warning-colour corner.
/// # Errors
/// Invalid dimensions/lengths or an output larger than `max_pixels`.
pub fn paint_image_preview(
    source: &[u8],
    source_size: [usize; 2],
    target: &mut [u8],
    target_size: [usize; 2],
    max_pixels: usize,
) -> Result<DisplayStatus, WarningSurfaceError> {
    let [sw, sh] = source_size;
    let [tw, th] = target_size;
    let source_pixels = sw
        .checked_mul(sh)
        .filter(|&n| n != 0)
        .ok_or(WarningSurfaceError)?;
    let target_pixels = tw
        .checked_mul(th)
        .filter(|&n| n != 0 && n <= max_pixels)
        .ok_or(WarningSurfaceError)?;
    if source_pixels.checked_mul(4) != Some(source.len())
        || target_pixels.checked_mul(4) != Some(target.len())
        || sw.checked_mul(tw).is_none()
        || sh.checked_mul(th).is_none()
    {
        return Err(WarningSurfaceError);
    }
    for (y, row) in target.chunks_exact_mut(tw * 4).enumerate() {
        let sy = y * sh / th;
        for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let sx = x * sw / tw;
            let at = (sy * sw + sx) * 4;
            let color = source.get(at..at + 4).ok_or(WarningSurfaceError)?;
            pixel.copy_from_slice(color);
        }
    }
    // Resampling can discard information on one axis while enlarging the other.
    let sampled = sw.min(tw) * sh.min(th);
    let status = DisplayStatus {
        discarded: source_pixels.saturating_sub(sampled),
        allocation_failed: false,
    };
    if status.is_reduced() {
        draw_notice(target, target_size);
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
    use super::*;
    fn sequences<const N: usize>() {
        let mut view = DisplayWindow::<u32, N>::new();
        let mut expected = std::collections::VecDeque::new();
        for step in 0..150u32 {
            let batch: Vec<_> = (step..step + step % 19).collect();
            let mut discarded = 0;
            for &v in &batch {
                expected.push_back(v);
                if expected.len() > N {
                    expected.pop_front();
                    discarded += 1;
                }
            }
            assert_eq!(view.extend(&batch).discarded, discarded);
            for limit in 0..=N + 1 {
                let skip = expected.len().saturating_sub(limit);
                assert!(view
                    .recent(limit)
                    .copied()
                    .eq(expected.iter().skip(skip).copied()));
            }
            assert_eq!(view.len(), expected.len());
        }
    }
    #[test]
    fn only_the_newest_values_survive_wrapping_batching_and_view_clipping() {
        sequences::<0>();
        sequences::<1>();
        sequences::<3>();
        sequences::<17>();
    }
    #[test]
    fn warning_counters_saturate_and_zero_capacity_omits_without_allocating_storage() {
        let mut zero = DisplayWindow::<u32, 0>::new();
        assert_eq!(zero.extend(&[1, 2]).discarded, 2);
        assert!(zero.is_empty());
        assert_eq!(zero.take_status().message(), Some("DISPLAY LIMITED"));
        assert!(!zero.take_status().is_reduced());
        let mut secret = DisplayWindow::<&str, 2>::new();
        secret.push("patient-secret");
        assert!(!format!("{secret:?}").contains("patient-secret"));
        let mut status = DisplayStatus {
            discarded: usize::MAX,
            allocation_failed: false,
        };
        status.merge(DisplayStatus {
            discarded: 1,
            allocation_failed: true,
        });
        assert_eq!(status.discarded, usize::MAX);
        assert!(status.allocation_failed);
    }
    #[test]
    fn warning_uses_only_the_provided_pixels_and_rejects_bad_geometry_before_writing() {
        let mut pixels = [0u8; 96 * 12 * 4];
        paint_display_warning(&mut pixels, [96, 12], 96 * 12).unwrap();
        assert!(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p == &[255, 208, 64, 255]));
        let (size, tile) = display_warning_tile();
        assert_eq!(size, [96, 12]);
        assert_eq!(pixels.as_chunks::<4>().0, tile);
        let before = pixels;
        for size in [[usize::MAX, 2], [0, 1], [96, 13]] {
            assert_eq!(
                paint_display_warning(&mut pixels, size, usize::MAX),
                Err(WarningSurfaceError)
            );
            assert_eq!(pixels, before);
        }
        assert!(paint_display_warning(&mut pixels, [96, 12], 1).is_err());
        assert_eq!(pixels, before);
    }
    #[test]
    fn large_preview_samples_the_source_without_constructing_another_image() {
        let source: Vec<_> = (0..32u8).flat_map(|v| [v, v, v, 255]).collect();
        let mut target = [0u8; 4 * 4];
        let status = paint_image_preview(&source, [8, 4], &mut target, [2, 2], 4).unwrap();
        assert_eq!(status.discarded, 28);
        assert_eq!(
            target,
            [0, 0, 0, 255, 4, 4, 4, 255, 16, 16, 16, 255, 255, 208, 64, 255]
        );
        let transpose = paint_image_preview(&source[..16], [4, 1], &mut target, [1, 4], 4).unwrap();
        assert_eq!(transpose.discarded, 3);
        let before = target;
        assert!(paint_image_preview(&source, [usize::MAX, 2], &mut target, [2, 2], 4).is_err());
        assert_eq!(target, before);
    }
}
