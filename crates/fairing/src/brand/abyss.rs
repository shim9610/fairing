//! The Abyss procedural background. Seven layers plus the legibility veil, baked into **one
//! mesh**.
//!
//! `Mesh::colored_vertex` uses `WHITE_UV` and the default texture is the font atlas, so every
//! colour-only triangle goes into the same mesh — **one draw call**.
//!
//! **No fixed coordinates are used**. There are only two units:
//! - `s = min(w, h)` — the object unit. The mantas, fish, bubbles and coral are proportional to it,
//!   so however the screen stretches they are not squashed.
//! - `w` · `h` — the band and anchor units. Only things pinned to a screen edge are proportional to
//!   these.
//!
//! **Legibility beats composition.** A desktop label is stamped as an `on_surface` galley with
//! neither a shadow nor a plate behind it, so the background is a step darker than the original art
//! and the hero manta drops to the lower left at `t = 0.34` rather than sitting dead centre,
//! leaving the middle of the icon grid clear. What is left over is the legibility veil's job.

// This is procedural geometry. The names in this file use the formula notation as it stands —
// `s` (the object unit) · `w`/`h` (the screen) · `t` (a curve parameter) · `u`/`v` (screen fractions) ·
// `g` (geometry) · `c` (colours) · `p` (parameters). Spelled out at length, the formulas and the code
// could no longer be compared, so two lints are lifted for the whole file (the rest of `pedantic` stays).
#![allow(clippy::many_single_char_names, clippy::similar_names)]

use super::noise::{rand01, stratified};
use super::ribbon;
use crate::config::{AbyssConfig, AbyssTier};
use crate::desktop::Wallpaper;
use crate::theme::{Metrics, Palette, Theme};
use egui::{pos2, Color32, Mesh, Pos2, Rect};
use std::cell::{Cell, RefCell};
use std::sync::Arc;

/// The vertex cap for one mesh. Past it the tier is lowered and it is baked again — **in release
/// too**, since a `Mesh` with bad indices is skipped whole by `tessellate_mesh` and the background
/// **silently disappears**.
pub(super) const MAX_VERTS: usize = 4096;

/// How many stops the water column's gradient has. Raised from 10 to 24 to cut the
/// banding — the cost is 28 vertices, no draw call and no texture. There is no dithering (a
/// `Shape::Mesh` has no source of per-pixel noise).
const WATER_STOPS: usize = 24;

/// The nine colours Abyss uses. By default derived from the palette.
///
/// **The values are measured from the original art.** The median per band was taken from the five
/// backgrounds in `assets/brand/` and a piecewise-linear fit run over the water column's vertical
/// profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbyssColors {
    /// The very top, where the surface caustics lie (`v = 0`). **Without this fourth stop in the
    /// water column** the top 5 % alone is out by as much as 55 RGB — the original's surface shine
    /// cannot be drawn in two segments.
    pub water_glow: Color32,
    /// Just below the surface (`v ≈ 0.06`).
    pub water_surface: Color32,
    /// The middle of the water column (`v ≈ 0.48`).
    pub water_mid: Color32,
    /// The deep-sea floor (`v = 1`).
    pub water_deep: Color32,
    /// The reef and rock silhouettes.
    pub silhouette: Color32,
    /// The floor. **Not sand** — the original art has no warm ground (only 0.001–0.7 % of all pixels
    /// have `R > B`, and those are the highlights on a manta's belly) and what reads as the floor is
    /// **backscattered haze** of the same family as the water. Which is why it is called `floor` and
    /// not `sand`.
    pub floor: Color32,
    /// The god rays.
    pub light: Color32,
    /// A manta's back.
    pub manta_body: Color32,
    /// A manta's belly.
    pub manta_belly: Color32,
}

impl AbyssColors {
    /// Derived from the palette. Override `[theme.palette] primary` alone and the water colour
    /// follows — this is where following the palette is carried out rather than merely promised.
    ///
    /// **Not exactly the original art.** The original's mid-water is blue (`G/B = 0.54`) while the
    /// palette's `primary` is cyan (`G/B = 0.85`), so that colour is nowhere on the line joining
    /// `primary` and `background` (the per-channel `t` spreads over 0.33–0.96). Where the original
    /// exactly is wanted, use [`AbyssColors::art`] — one line of `[desktop.abyss] colors = "art"`.
    #[must_use]
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            water_glow: p.primary.lerp_to_gamma(Color32::WHITE, 0.72),
            // 0.55 → 0.20. Re-solved by least squares, and it cuts the surface error by 78 %.
            water_surface: p.primary.lerp_to_gamma(Color32::WHITE, 0.20),
            // 0.35 → 0.45.
            water_mid: p.primary.lerp_to_gamma(p.background, 0.45),
            // **`background` is not used as it stands.** The UI's ground colour is 64 blue units darker
            // than the deep sea — used as water, the bottom of the screen dies black. The legibility veil
            // still uses `background`, so the two roles part company.
            water_deep: p.background.lerp_to_gamma(p.primary, 0.22),
            silhouette: p.background.lerp_to_gamma(Color32::BLACK, 0.30),
            floor: p.background.lerp_to_gamma(p.primary, 0.30),
            light: p.focus,
            manta_body: p.background.lerp_to_gamma(p.primary, 0.14),
            manta_belly: p.on_surface.lerp_to_gamma(p.primary, 0.28),
        }
    }

    /// Fixed colours matched to the original art (they do not follow the palette). All measured values.
    #[must_use]
    pub fn art() -> Self {
        Self {
            water_glow: Color32::from_rgb(0xc4, 0xf3, 0xfb),
            water_surface: Color32::from_rgb(0x51, 0xcf, 0xff),
            water_mid: Color32::from_rgb(0x00, 0x5e, 0xaf),
            water_deep: Color32::from_rgb(0x04, 0x1e, 0x55),
            silhouette: Color32::from_rgb(0x01, 0x10, 0x2b),
            floor: Color32::from_rgb(0x03, 0x35, 0x6f),
            light: Color32::from_rgb(0x50, 0xcc, 0xff),
            manta_body: Color32::from_rgb(0x05, 0x1b, 0x4b),
            manta_belly: Color32::from_rgb(0xba, 0xd5, 0xf6),
        }
    }
}

/// The Abyss background's composition parameters.
///
/// **All of them are authored values or caps on a count**, and there is no colour here — the
/// colours are derived from `&Theme` every frame. A plain `Copy` struct rather than a consuming
/// builder: struct update syntax is enough, and there is no rule to learn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbyssParams {
    /// The quality tier.
    pub tier: AbyssTier,
    /// The scatter seed.
    pub seed: u32,
    /// How many god rays (0..=16).
    pub rays: u8,
    /// How many bubbles (0..=64).
    pub bubbles: u8,
    /// How many fish (0..=64).
    pub fish: u8,
    /// How many mantas (0..=6).
    pub mantas: u8,
    /// The light's horizontal position (as a fraction of the screen's width).
    pub light_x: f32,
    /// The top veil's alpha.
    pub veil_top: f32,
    /// The veil's alpha over the icon grid.
    pub veil_field: f32,
    /// The bottom veil's alpha.
    pub veil_bottom: f32,
    /// Move the mantas every frame. **It breaks idling at 0 fps.**
    pub animate: bool,
    /// The bake budget (ms).
    pub bake_budget_ms: f32,
    /// `None` = derived from the palette (the default).
    pub colors: Option<AbyssColors>,
}

impl Default for AbyssParams {
    fn default() -> Self {
        Self::from_config(&AbyssConfig::default())
    }
}

impl AbyssParams {
    /// Build from `[desktop.abyss]`. The colours are not in the config — they follow the palette.
    #[must_use]
    pub fn from_config(cfg: &AbyssConfig) -> Self {
        Self {
            tier: cfg.tier,
            seed: cfg.seed,
            rays: cfg.rays.min(16),
            bubbles: cfg.bubbles.min(64),
            fish: cfg.fish.min(64),
            mantas: cfg.mantas.min(6),
            light_x: cfg.light_x.clamp(0.0, 1.0),
            veil_top: cfg.veil_top.clamp(0.0, 1.0),
            veil_field: cfg.veil_field.clamp(0.0, 1.0),
            veil_bottom: cfg.veil_bottom.clamp(0.0, 1.0),
            animate: cfg.animate,
            bake_budget_ms: cfg.bake_budget_ms.max(0.0),
            colors: match cfg.colors.as_str() {
                "art" => Some(AbyssColors::art()),
                "palette" => None,
                other => {
                    log::warn!(
                        "[desktop.abyss] colors = \"{other}\" is not a known mode (palette | art) - using palette"
                    );
                    None
                }
            },
        }
    }

    /// Fix the colours (so they do not follow the palette).
    #[must_use]
    pub fn with_colors(mut self, colors: AbyssColors) -> Self {
        self.colors = Some(colors);
        self
    }

    /// The composition (colours excluded) folded into one value. It goes into the bake cache's key.
    fn fold(self) -> u64 {
        let mut h = u64::from(self.seed);
        for byte in [
            self.tier as u8,
            self.rays,
            self.bubbles,
            self.fish,
            self.mantas,
            u8::from(self.animate),
        ] {
            h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(byte);
        }
        for f in [
            self.light_x,
            self.veil_top,
            self.veil_field,
            self.veil_bottom,
        ] {
            h = h.wrapping_mul(0x0100_0000_01b3) ^ u64::from(f.to_bits());
        }
        h
    }
}

/// The bake cache's key. **Every item in it changes the baked result.**
#[derive(Clone, Copy, PartialEq)]
struct BakeKey {
    /// The rect quantised to a 4 device px grid (the origin plus the size).
    rect_q: [i32; 4],
    /// `round(ppp × 64)` — the feather width and the LOD hang on it.
    ppp_q: u32,
    colors: AbyssColors,
    /// The veil's ground = `ColorRole::Background`. A palette change means the veil is baked again too.
    ground: Color32,
    status_bar_height_q: i32,
    bottom_band_q: i32,
    params_hash: u64,
}

impl BakeKey {
    fn new(
        rect: Rect,
        ppp: f32,
        colors: AbyssColors,
        ground: Color32,
        m: &Metrics,
        p: AbyssParams,
    ) -> Self {
        let q = |v: f32| quantize(v * ppp, 4.0);
        Self {
            rect_q: [
                q(rect.min.x),
                q(rect.min.y),
                q(rect.width()),
                q(rect.height()),
            ],
            ppp_q: quantize_u32(ppp * 64.0),
            colors,
            ground,
            status_bar_height_q: quantize(m.status_bar_height, 1.0),
            bottom_band_q: quantize(m.dock_height + m.page_indicator_height, 1.0),
            params_hash: p.fold(),
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the screen size is inside i32's range and the grid quantisation makes the truncation intentional"
)]
fn quantize(v: f32, grid: f32) -> i32 {
    (v / grid).round() as i32
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "ppp is a small positive number"
)]
fn quantize_u32(v: f32) -> u32 {
    v.max(0.0).round() as u32
}

/// The baked result.
struct Baked {
    key: BakeKey,
    mesh: Arc<Mesh>,
    /// The tier actually used (it may have been demoted for going over budget).
    tier: AbyssTier,
}

/// The Abyss background. It caches the baked mesh within itself.
///
/// `Wallpaper::ThemedPainter`'s callback is an `Fn`, so the cache is inside a **`RefCell`**.
/// `OnceLock` / `LazyLock` are banned by `xtask sync-check` and `Mutex` / `RwLock` by the no-locks rule — a
/// `RefCell` is a means the crate already uses.
pub struct Abyss {
    params: AbyssParams,
    cache: RefCell<Option<Baked>>,
    rebuilds: Cell<u32>,
    last_dark: Cell<Option<bool>>,
    freeze_until_stable: Cell<u8>,
}

impl std::fmt::Debug for Abyss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Abyss")
            .field("params", &self.params)
            .field("rebuilds", &self.rebuilds.get())
            .finish_non_exhaustive()
    }
}

impl Abyss {
    /// Build one. With `animate = true` it logs a warning once at startup — it is a choice that
    /// breaks idling at 0 fps, so it does not pass in silence.
    #[must_use]
    pub fn new(params: AbyssParams) -> Self {
        if params.animate {
            log::warn!(
                "[desktop.abyss] animate = true keeps the manta moving every frame, so the shell never goes idle"
            );
        }
        Self {
            params,
            cache: RefCell::new(None),
            rebuilds: Cell::new(0),
            last_dark: Cell::new(None),
            freeze_until_stable: Cell::new(0),
        }
    }

    /// How many times it has been baked again so far. The cache regression test looks at it.
    #[must_use]
    pub fn rebuilds(&self) -> u32 {
        self.rebuilds.get()
    }

    /// The tier actually in use (a demotion for going over budget included).
    #[must_use]
    pub fn effective_tier(&self) -> AbyssTier {
        self.cache
            .borrow()
            .as_ref()
            .map_or(self.params.tier, |b| b.tier)
    }

    /// Draw. On a cache hit, all the frame does is derive eight colours, compare the key and `Arc::clone`.
    pub fn paint(&self, painter: &egui::Painter, rect: Rect, theme: &Theme) {
        self.tick_freeze(theme);
        let ppp = painter.pixels_per_point();
        let colors = self
            .params
            .colors
            .unwrap_or_else(|| AbyssColors::from_palette(&theme.palette));
        // The legibility veil pushes towards **the UI's ground colour** rather than the water's —
        // that is the direction the labels are designed to contrast against, and under a light preset a
        // black veil is simply wrong.
        let ground = theme.color(crate::theme::ColorRole::Background);
        let key = BakeKey::new(rect, ppp, colors, ground, &theme.metrics, self.params);

        let mut cache = self.cache.borrow_mut();
        let hit = matches!(&*cache, Some(b) if b.key == key);
        // It freezes through a cross-fade — a different palette arrives every frame, so it would re-bake
        // 12 frames in a row.
        let frozen = self.freeze_until_stable.get() > 0 && cache.is_some();
        if !hit && !frozen {
            *cache = Some(self.bake(rect, ppp, colors, ground, key, &theme.metrics));
            self.rebuilds.set(self.rebuilds.get() + 1);
        }
        if let Some(b) = &*cache {
            painter.add(egui::Shape::mesh(Arc::clone(&b.mesh)));
        }
    }

    /// Detect a dark/light change by itself and freeze re-baking through the cross-fade. No new
    /// plumbing is needed in the shell and the `Fn` contract is not broken.
    fn tick_freeze(&self, theme: &Theme) {
        let was = self.last_dark.replace(Some(theme.dark));
        if was.is_some_and(|w| w != theme.dark) {
            let ms = theme.motion.theme_fade.duration.as_secs_f32() * 1000.0;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the frame count is small"
            )]
            let frames = (ms / 16.7).ceil().clamp(0.0, 250.0) as u8;
            self.freeze_until_stable.set(frames.saturating_add(2));
        } else {
            self.freeze_until_stable
                .set(self.freeze_until_stable.get().saturating_sub(1));
        }
    }

    /// Bake. Over the budget, or over the vertex cap, the tier is lowered and it is baked again — **in
    /// release too**.
    fn bake(
        &self,
        rect: Rect,
        ppp: f32,
        colors: AbyssColors,
        ground: Color32,
        key: BakeKey,
        metrics: &Metrics,
    ) -> Baked {
        let mut tier = self.params.tier;
        loop {
            let started = std::time::Instant::now();
            let mesh = bake_tier(rect, ppp, colors, ground, self.params, tier, metrics);
            let ms = started.elapsed().as_secs_f32() * 1000.0;
            let over_budget = self.params.bake_budget_ms > 0.0 && ms > self.params.bake_budget_ms;
            let over_verts = mesh.vertices.len() > MAX_VERTS;
            let Some(lower) = demote(tier) else {
                if over_verts {
                    log::warn!(
                        "Abyss: even the Flat tier needs {} vertices, over the {MAX_VERTS} cap - drawing it anyway",
                        mesh.vertices.len()
                    );
                }
                return Baked {
                    key,
                    mesh: Arc::new(mesh),
                    tier,
                };
            };
            if over_verts {
                log::warn!(
                    "Abyss: {tier:?} needs {} vertices, over the {MAX_VERTS} cap - dropping to {lower:?}",
                    mesh.vertices.len()
                );
            } else if over_budget {
                log::warn!(
                    "Abyss: baking {tier:?} took {ms:.2} ms, over the {:.2} ms budget - dropping to {lower:?}",
                    self.params.bake_budget_ms
                );
            } else {
                return Baked {
                    key,
                    mesh: Arc::new(mesh),
                    tier,
                };
            }
            tier = lower;
        }
    }
}

/// One tier down. `None` at `Flat`.
const fn demote(tier: AbyssTier) -> Option<AbyssTier> {
    match tier {
        AbyssTier::Full => Some(AbyssTier::Lite),
        AbyssTier::Lite => Some(AbyssTier::Flat),
        AbyssTier::Flat => None,
    }
}

/// Wrap it in a [`Wallpaper::ThemedPainter`]. This is the one line an integrator writes.
///
/// ```no_run
/// # fn main() {
/// use fairing::brand::{abyss_wallpaper, AbyssParams};
/// let wallpaper = abyss_wallpaper(AbyssParams::default());
/// # let _ = wallpaper;
/// # }
/// ```
#[must_use]
pub fn abyss_wallpaper(params: AbyssParams) -> Wallpaper {
    let abyss = Abyss::new(params);
    Wallpaper::ThemedPainter(Box::new(move |painter, rect, theme| {
        abyss.paint(painter, rect, theme);
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// The bake — seven layers plus the veil
// ─────────────────────────────────────────────────────────────────────────────

/// The values drawn from the screen's geometry. Gathered so the formulas need look at this one struct.
#[derive(Clone, Copy)]
struct Geo {
    rect: Rect,
    w: f32,
    h: f32,
    /// The object unit, `min(w, h)`.
    s: f32,
    /// How strong the reef walls are — close to 1 in portrait.
    wall: f32,
    /// The flight-line lerp factor — 1 in landscape.
    a: f32,
}

impl Geo {
    fn new(rect: Rect) -> Self {
        let (w, h) = (rect.width().max(1.0), rect.height().max(1.0));
        Self {
            rect,
            w,
            h,
            s: w.min(h),
            // A square (a ratio of 1.0) has no walls — the original art is like that. They grow from a ratio of 0.80 and reach 1 at 0.56.
            wall: ((0.80 - w / h) / 0.24).clamp(0.0, 1.0),
            a: ((w / h - 0.55) / (1.80 - 0.55)).clamp(0.0, 1.0),
        }
    }

    /// Screen-fraction coordinates → pixel coordinates.
    fn at(self, u: f32, v: f32) -> Pos2 {
        pos2(
            self.w.mul_add(u, self.rect.min.x),
            self.h.mul_add(v, self.rect.min.y),
        )
    }
}

fn bake_tier(
    rect: Rect,
    ppp: f32,
    c: AbyssColors,
    ground: Color32,
    p: AbyssParams,
    tier: AbyssTier,
    metrics: &Metrics,
) -> Mesh {
    let g = Geo::new(rect);
    let mut mesh = Mesh::default();
    let feather_pt = ribbon::FEATHER_PX / ppp.max(0.1);

    water_column(&mut mesh, g, c);
    if tier != AbyssTier::Flat {
        let lenses = if tier == AbyssTier::Full { 84 } else { 32 };
        caustics(&mut mesh, g, c, p.seed, lenses);
    }
    if tier == AbyssTier::Full {
        god_rays(&mut mesh, g, c, p);
        bubbles(&mut mesh, g, c, p);
        fish(&mut mesh, g, c, p);
    }
    if tier != AbyssTier::Flat {
        let rocks = if tier == AbyssTier::Full { 12 } else { 8 };
        let coral = if tier == AbyssTier::Full { 6 } else { 0 };
        reef(&mut mesh, g, c, p, rocks, coral, feather_pt);
        let count = if tier == AbyssTier::Full {
            usize::from(p.mantas).max(1)
        } else {
            1
        };
        mantas(&mut mesh, g, c, count, ppp);
    }
    veil(&mut mesh, g, ground, p, metrics);
    mesh
}

/// 1. The water column — a 24-stop vertical gradient. The whole screen.
///
/// **There are three segments**. Two could not follow the surface shine in the top 5 %
/// and were out by as much as 55 RGB. Adding the single knee at `v = 0.06` brings it down to 12 —
/// at a cost of one `Color32` and no vertices (the mesh interpolates 24 stops either way).
fn water_column(mesh: &mut Mesh, g: Geo, c: AbyssColors) {
    /// Where the surface shine ends.
    const KNEE_GLOW: f32 = 0.06;
    /// Where the mid-water colour settles.
    const KNEE_MID: f32 = 0.48;
    let mut prev: Option<(Pos2, Pos2, Color32)> = None;
    for i in 0..WATER_STOPS {
        #[expect(clippy::cast_precision_loss, reason = "the stop count is small")]
        let t = i as f32 / (WATER_STOPS - 1) as f32;
        let color = if t < KNEE_GLOW {
            c.water_glow.lerp_to_gamma(c.water_surface, t / KNEE_GLOW)
        } else if t < KNEE_MID {
            c.water_surface
                .lerp_to_gamma(c.water_mid, (t - KNEE_GLOW) / (KNEE_MID - KNEE_GLOW))
        } else {
            c.water_mid
                .lerp_to_gamma(c.water_deep, (t - KNEE_MID) / (1.0 - KNEE_MID))
        };
        let y = g.h.mul_add(t, g.rect.min.y);
        let (l, r) = (pos2(g.rect.min.x, y), pos2(g.rect.max.x, y));
        if let Some((pl, pr, pc)) = prev {
            let base = u32::try_from(mesh.vertices.len()).unwrap_or(0);
            mesh.colored_vertex(pl, pc);
            mesh.colored_vertex(pr, pc);
            mesh.colored_vertex(l, color);
            mesh.colored_vertex(r, color);
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 1, base + 3, base + 2);
        }
        prev = Some((l, r, color));
    }
}

/// 2. The surface caustic band — the horizontally stretched bright lenses at the top.
///
/// From the measurements: the band comes down to `v ≈ 0.22`, with 13–25 lenses to a row
/// at a coverage of 0.38, and the alpha falls from 0.58 at the surface to 0.12 at the band's end.
/// The first draft was six times sparser, three times fainter and half as deep.
fn caustics(mesh: &mut Mesh, g: Geo, c: AbyssColors, seed: u32, lenses: u32) {
    let band = (0.22 * g.h).clamp(0.12 * g.s, 0.40 * g.s);
    let cols = lenses.clamp(1, 18);
    let rows = lenses.div_ceil(cols).max(1);
    for (i, u, v) in stratified(seed ^ 0x0caa_0001, cols, rows) {
        let cx = g.w.mul_add(u, g.rect.min.x);
        let cy = band.mul_add(v, g.rect.min.y);
        // The width is proportional to **the screen's width** — hung on `s` it would stretch with a wide panel.
        let half_w = g.w * 0.010f32.mul_add(rand01(seed, i, 2), 0.004);
        let half_h = band * 0.030f32.mul_add(rand01(seed, i, 3), 0.015);
        // It fades with depth. `v` is already 0..1 within the band.
        let alpha = 0.30f32.mul_add(rand01(seed, i, 4), 0.40) * (1.0 - v).powf(1.5);
        ribbon::convex(
            mesh,
            &[
                pos2(cx - half_w, cy),
                pos2(cx, cy - half_h),
                pos2(cx + half_w, cy),
                pos2(cx, cy + half_h),
            ],
            c.light.gamma_multiply(alpha),
        );
    }
}

/// 3. The god rays — bright wedges fanning out from a point above the surface. The alpha goes to 0 further down.
fn god_rays(mesh: &mut Mesh, g: Geo, c: AbyssColors, p: AbyssParams) {
    // The origin is **above** the screen. But at an extreme ratio `0.35·h` runs well past the object unit
    // (at a ratio of 0.2, h is five times s), so it is clamped by `s`: extreme ratios are
    // handled by clamping, not by the formula.
    let above = (0.35 * g.h).min(0.6 * g.s);
    let origin = pos2(g.w.mul_add(p.light_x, g.rect.min.x), g.rect.min.y - above);
    let length = (0.49 * g.h).clamp(0.4 * g.s, 1.0 * g.s);
    let rays = u32::from(p.rays);
    for i in 0..rays {
        #[expect(clippy::cast_precision_loss, reason = "the ray count is small")]
        let k = i as f32 / f32::from(p.rays.max(1));
        // The fan's angle is **derived from the target width**. Written in as a constant, the fan narrows
        // or overflows whenever the screen ratio changes — in the original art the outer rays are at u
        // 0.20/0.83 at v = 0.30.
        let spread = (0.30 * g.w / (above + 0.30 * g.h)).atan();
        let angle = (k - 0.5).mul_add(2.0 * spread, 0.06f32.mul_add(rand01(p.seed, i, 5), -0.03));
        let half = 0.012f32.mul_add(g.s, 0.03 * g.s * rand01(p.seed, i, 6));
        let (sin, cos) = angle.sin_cos();
        let dir = egui::vec2(sin, cos);
        let side = egui::vec2(cos, -sin);
        let far = origin + dir * (length * 1.9);
        let alpha = 0.10f32.mul_add(rand01(p.seed, i, 7), 0.12);
        let base = u32::try_from(mesh.vertices.len()).unwrap_or(0);
        mesh.colored_vertex(origin - side * half, c.light.gamma_multiply(alpha));
        mesh.colored_vertex(origin + side * half, c.light.gamma_multiply(alpha));
        mesh.colored_vertex(far - side * (half * 3.0), Color32::TRANSPARENT);
        mesh.colored_vertex(far + side * (half * 3.0), Color32::TRANSPARENT);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base + 1, base + 3, base + 2);
    }
}

/// 6. The bubbles — a few vertical chains.
fn bubbles(mesh: &mut Mesh, g: Geo, c: AbyssColors, p: AbyssParams) {
    /// Where the chains sit horizontally. The original art leaves the middle (`u ∈ [0.35, 0.60]`) clear.
    const CHAIN_U: [f32; 4] = [0.07, 0.20, 0.68, 0.90];
    let count = u32::from(p.bubbles);
    if count == 0 {
        return;
    }
    // The original's chains **cling to the edges** — of the 22 rows measured, 11 are at u < 0.25 and 8 at
    // u > 0.75, with not one in u ∈ [0.35, 0.60]. Leaving the middle clear is the composition.
    let chains = 4u32;
    for (i, _u, v) in stratified(p.seed ^ 0x0bbb_0002, chains, count.div_ceil(chains).max(1)) {
        let r = g.s * 0.008f32.mul_add(rand01(p.seed, i, 8), 0.0025);
        let lane = CHAIN_U.get((i % chains) as usize).copied().unwrap_or(0.5);
        // It is jittered by ±0.03 within the chain only — so a chain reads as a chain.
        let u = 0.06f32.mul_add(rand01(p.seed, i, 14), lane - 0.03);
        let center = g.at(u, 0.20 + v * 0.65);
        hexagon(mesh, center, r, c.light.gamma_multiply(0.22));
    }
}

/// 5. The shoal — a cluster of small triangles in the quadrant opposite the hero.
fn fish(mesh: &mut Mesh, g: Geo, c: AbyssColors, p: AbyssParams) {
    let count = u32::from(p.fish);
    if count == 0 {
        return;
    }
    // The original's shoal is not gathered into one quadrant — it is scattered across **the full width**
    // of the screen, at the same depth as the mantas.
    let (u0, v0, span_u, span_v) = (0.04f32, 0.26f32, 0.92f32, 0.50f32);
    let cols = 8u32;
    for (i, u, v) in stratified(p.seed ^ 0x0f15_0003, cols, count.div_ceil(cols).max(1)) {
        let center = g.at(span_u.mul_add(u, u0), span_v.mul_add(v, v0));
        let size = g.s * 0.007;
        let tilt = 0.5f32.mul_add(-1.0, rand01(p.seed, i, 9)) * 0.6;
        let (sin, cos) = tilt.sin_cos();
        let fwd = egui::vec2(cos, sin);
        let side = egui::vec2(-sin, cos);
        ribbon::convex(
            mesh,
            &[
                center + fwd * size,
                center - fwd * size + side * (size * 0.55),
                center - fwd * size - side * (size * 0.55),
            ],
            c.silhouette.gamma_multiply(0.20),
        );
    }
}

/// 7. The reef floor — dark silhouettes over the backscattered haze.
///
/// **There is no sand**. Only 0.001–0.7 % of the pixels across the original art have
/// `R > B`, and even those are highlights on a manta's belly. What reads as the floor is haze of the
/// same family as the water, so the first draft's bright sand band was painting a warm grey that is
/// not in the art — the largest error of the eight colours (198 RGB).
///
/// The ridge is **a low floor with high crests**. The first draft was a uniform slab (a crest
/// deviation of 0.055 h) and read as a wall rather than a horizon — the original swells over
/// 0.23–0.35 h.
///
/// **In portrait it climbs the left and right walls into a canyon.**
fn reef(
    mesh: &mut Mesh,
    g: Geo,
    c: AbyssColors,
    p: AbyssParams,
    rocks: u32,
    coral: u32,
    feather_pt: f32,
) {
    // The floor haze — it blends into the water at the top and brightens a little further down. It is haze, not ground.
    let haze_h = (0.16 * g.h).clamp(0.10 * g.s, 0.30 * g.s);
    let haze_top = g.rect.max.y - haze_h;
    let base = u32::try_from(mesh.vertices.len()).unwrap_or(0);
    let blend = c.floor.gamma_multiply(0.0);
    let lit = c.floor.gamma_multiply(0.85);
    mesh.colored_vertex(pos2(g.rect.min.x, haze_top), blend);
    mesh.colored_vertex(pos2(g.rect.max.x, haze_top), blend);
    mesh.colored_vertex(pos2(g.rect.min.x, g.rect.max.y), lit);
    mesh.colored_vertex(pos2(g.rect.max.x, g.rect.max.y), lit);
    mesh.add_triangle(base, base + 1, base + 2);
    mesh.add_triangle(base + 1, base + 3, base + 2);

    // The ridge is **one unbroken line**. Standing the peaks up separately gives a picket fence rather than
    // a reef — which is what happened. One noise curve crossing the screen's width is filled down to the
    // bottom, and domes are laid over it to give the silhouette its swell.
    let ridge = 0.30 * g.s;
    let floor_y = g.rect.max.y - 0.02 * g.h;
    let steps = (rocks * 3).clamp(12, 48);
    let mut top_edge: Vec<Pos2> = Vec::with_capacity(steps as usize + 1);
    let mut floor_edge: Vec<Pos2> = Vec::with_capacity(steps as usize + 1);
    for k in 0..=steps {
        #[expect(clippy::cast_precision_loss, reason = "the step count is small")]
        let u = k as f32 / steps as f32;
        // Two frequencies are overlaid to break the regularity. Squaring makes **the low places many and
        // the crests rare** — a uniform distribution puts everything at a middle height and gives a slab.
        let n1 = rand01(p.seed, k, 10);
        let n2 = rand01(p.seed, k / 3, 11);
        let hgt = ridge * (n1 * n1).mul_add(0.94, 0.06) * 0.5f32.mul_add(n2, 0.5);
        let x = g.w.mul_add(u, g.rect.min.x);
        top_edge.push(pos2(x, floor_y - hgt));
        floor_edge.push(pos2(x, g.rect.max.y));
    }
    ribbon::strip(mesh, &top_edge, &floor_edge, c.silhouette, c.silhouette);
    ribbon::feather(mesh, &top_edge, c.silhouette, feather_pt);

    // The rounded rocks — the original has 12–15 of them at intervals of 0.067–0.083 of the width.
    let boulders = rocks.max(12);
    for i in 0..boulders {
        #[expect(clippy::cast_precision_loss, reason = "the rock count is small")]
        let slot = (i as f32 + 0.5) / boulders as f32;
        #[expect(clippy::cast_precision_loss, reason = "the rock count is small")]
        let jitter = (rand01(p.seed, i, 12) - 0.5) / boulders as f32 * 0.8;
        let cx = g.w.mul_add(slot + jitter, g.rect.min.x);
        let half = g.w * 0.026f32.mul_add(rand01(p.seed, i, 13), 0.022);
        let height = 0.04 * g.h * 1.0f32.mul_add(rand01(p.seed, i, 14), 0.5);
        let base_y = g.rect.max.y;
        let skew = 0.5f32.mul_add(rand01(p.seed, i, 15), -0.25);
        let poly = [
            pos2(cx - half, base_y),
            pos2(cx - half * 0.72, base_y - height * 0.52),
            pos2(skew.mul_add(half, cx), base_y - height),
            pos2(cx + half * 0.66, base_y - height * 0.40),
            pos2(cx + half, base_y),
        ];
        ribbon::convex(mesh, &poly, c.silhouette);
        ribbon::feather(mesh, &poly, c.silhouette, feather_pt);
    }

    // The coral — it rises **above** the ridge. In the first draft it was buried inside it and not one stem showed.
    for i in 0..coral {
        #[expect(clippy::cast_precision_loss, reason = "the coral count is small")]
        let u = (i as f32 + 0.25) / coral.max(1) as f32;
        let cx = g.w.mul_add(u, g.rect.min.x);
        // It finds the ridge's crest at its own place and rises from there.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the index is a small integer"
        )]
        let k = ((u * f32::from(u16::try_from(steps).unwrap_or(u16::MAX))).round() as usize)
            .min(top_edge.len().saturating_sub(1));
        let anchor = top_edge.get(k).map_or(g.rect.max.y, |q| q.y);
        let hgt = g.s * 0.04f32.mul_add(rand01(p.seed, i, 20), 0.03);
        let wid = g.w * 0.004;
        ribbon::convex(
            mesh,
            &[
                pos2(cx - wid, anchor + hgt * 0.3),
                pos2(cx - wid * 0.4, anchor - hgt),
                pos2(cx + wid * 0.4, anchor - hgt * 0.8),
                pos2(cx + wid, anchor + hgt * 0.3),
            ],
            c.silhouette.gamma_multiply(0.85),
        );
    }

    // The left and right walls — the portrait canyon. The same noise as the bottom ridge, turned 90° and joined on.
    if g.wall <= f32::EPSILON {
        return;
    }
    let wall_h = g.wall * 0.92 * g.h;
    let wall_w = g.s * 0.30;
    for (side, sign) in [(0u32, 1.0f32), (1, -1.0)] {
        let x0 = if sign > 0.0 {
            g.rect.min.x
        } else {
            g.rect.max.x
        };
        let mut poly = vec![pos2(x0, g.rect.max.y)];
        let steps = 10u32;
        for k in 0..=steps {
            #[expect(clippy::cast_precision_loss, reason = "the step count is small")]
            let t = k as f32 / steps as f32;
            let y = g.rect.max.y - wall_h * t;
            // It narrows towards the top but keeps **a nearly parallel stretch** — a pure power gives a
            // triangle rather than a wall. In the original the width is nearly constant at 0.10–0.13 w
            // over v 0.15..0.62 and only spreads lower down.
            let taper = 0.70f32.mul_add((1.0 - t).powi(3), 0.30);
            let bulge = wall_w * 0.40f32.mul_add(rand01(p.seed, side * 16 + k, 21), 0.80) * taper;
            poly.push(pos2(sign.mul_add(bulge, x0), y));
        }
        poly.push(pos2(x0, g.rect.max.y - wall_h));
        // A wall can be concave, so it is filled with **a strip** rather than a fan — the wall's curve and
        // the screen's edge are its two boundaries.
        let outer: Vec<Pos2> = poly.iter().map(|q| pos2(x0, q.y)).collect();
        ribbon::strip(mesh, &outer, &poly, c.silhouette, c.silhouette);
    }
}

/// 4. The manta formation — one hero plus 2 to 4 followers (smaller and fainter, aerial perspective).
///
/// It uses **the same authored coordinates** as the mark ([`super::manta::append_manta`]) — if the
/// manta in the background were a different creature from the manta in the logo, it would not be a
/// brand.
///
/// The flight line is a quadratic Bézier `P(t)` with two sets of control points lerped by `a`.
/// **Extreme ratios are handled by clamping, not by the formula** — the centre is pulled inside
/// `rect.expand(-0.05 · s)`.
fn mantas(mesh: &mut Mesh, g: Geo, c: AbyssColors, count: usize, ppp: f32) {
    use super::manta::{append_manta, MantaLod, MantaTones, GRID_H, GRID_W};
    /// How far the back sinks into the water. **Independent of distance** — the back is already
    /// almost the colour of the deep sea, so there is nothing for aerial perspective to do.
    /// `manta_body` is already a measured colour, so only the remainder is left.
    const BODY_VEIL: f32 = 0.10;
    /// The positions along the flight line. Representative values from the original art's
    /// measurements (sq 0.257/0.457/0.551/0.661/0.756 · uw 0.280/0.393/0.461/0.648/0.769).
    const TS: [f32; 5] = [0.28, 0.45, 0.55, 0.66, 0.77];
    /// The widths (as multiples of the object unit `s`). The original is **not monotonically
    /// decreasing** — two of the near ones lie one behind the other, so the ratios swell over
    /// [1.00, 0.74, 0.36, 0.89, 0.41]. That rhythm is what makes the formation read as a formation.
    const WIDTHS: [f32; 5] = [0.40, 0.24, 0.14, 0.34, 0.16];
    let safe = g.rect.expand(-0.05 * g.s);
    for (&t, &wf) in TS.iter().zip(&WIDTHS).take(count.min(5)) {
        let (u, v) = flight_path(g.a, t);
        let raw = g.at(u, v);
        let center = pos2(
            raw.x.clamp(safe.min.x, safe.max.x),
            raw.y.clamp(safe.min.y, safe.max.y),
        );
        let width = wf * g.s;
        let rect = Rect::from_center_size(center, egui::vec2(width, width * GRID_H / GRID_W));
        // **The back and the belly sink differently.** Measured: the belly goes from 0.27 near to 0.87 far,
        // more than seven times fainter, while the back is already almost the colour of the deep sea and
        // stays at 0.15 whatever the distance. Using the same value for both floats a near manta's back up
        // to `#053A74` (the original's `#051B4B`).
        //
        // The belly's sinking is hung on **the width drawn rather than the formation index** — the
        // original's formation is not lined up in order of size (the rhythm of WIDTHS). It is the straight
        // line through the two measured points (0.27 at a width of 0.40, 0.87 at a width of 0.14).
        let veil_belly = 2.31f32.mul_add(-wf, 1.193).clamp(0.20, 0.90);
        let tones = MantaTones {
            body: c.manta_body.lerp_to_gamma(c.water_mid, BODY_VEIL),
            belly: c.manta_belly.lerp_to_gamma(c.water_mid, veil_belly),
        };
        let lod = MantaLod::for_size_px(width * ppp);
        append_manta(mesh, rect, lod, tones, false, ppp, 2.0);
    }
}

/// The flight line `P(t)` — a quadratic Bézier whose landscape and portrait control points are lerped by `a`. In screen-fraction coordinates.
fn flight_path(a: f32, t: f32) -> (f32, f32) {
    // landscape (a = 1) / portrait (a = 0)
    // Re-seated from the original's measurements — the formation passes through **the lower quadrant**.
    // The first draft was at v ≈ 0.535, right in the middle of the icon grid; the original is at v ≈ 0.705
    // at the same u.
    let p0 = (lerp(-0.08, -0.06, a), lerp(0.50, 0.46, a));
    let cp = (lerp(0.46, 0.50, a), lerp(0.80, 0.96, a));
    let p1 = (lerp(1.06, 1.06, a), lerp(0.70, 0.65, a));
    let u = 1.0 - t;
    (
        (u * u).mul_add(p0.0, (2.0 * u * t).mul_add(cp.0, t * t * p1.0)),
        (u * u).mul_add(p0.1, (2.0 * u * t).mul_add(cp.1, t * t * p1.1)),
    )
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    (b - a).mul_add(t, a)
}

/// One bubble — a hexagon (6 vertices, 4 triangles) reads well enough as a circle.
fn hexagon(mesh: &mut Mesh, center: Pos2, r: f32, color: Color32) {
    let mut poly = [Pos2::ZERO; 6];
    for (i, q) in poly.iter_mut().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "6 is a small integer")]
        let a = i as f32 / 6.0 * std::f32::consts::TAU;
        let (sin, cos) = a.sin_cos();
        *q = pos2(r.mul_add(cos, center.x), r.mul_add(sin, center.y));
    }
    ribbon::convex(mesh, &poly, color);
}

/// The legibility veil — one vertical 4-stop scrim. 8 vertices, 6 triangles.
///
/// **The top band is not the status bar's place.** The background is drawn into `content`, with the
/// status bar and nav bar taken out, and the status bar lays down its own opaque `Surface`. What the
/// top band covers is **the first row of icons**, and it uses `status_bar_height` only because that
/// is the one token that follows the device's finger metrics (which is why it is read only inside
/// the two clamps).
///
/// **The veil's colour is `background`** — not `scrim`. `scrim` is a separate token carrying an
/// alpha of its own, and multiplying two alphas blurs the meaning; above all, **a black veil is
/// wrong under a light preset**. Pushing the background towards `background` is exactly the
/// direction `on_surface` is designed to contrast against.
fn veil(mesh: &mut Mesh, g: Geo, ground: Color32, p: AbyssParams, m: &Metrics) {
    let top_band = (1.6 * m.status_bar_height).clamp(0.10 * g.h, 0.28 * g.h);
    let bottom_band = (m.dock_height + m.page_indicator_height).clamp(0.10 * g.h, 0.30 * g.h);
    let base_color = ground;
    let stops = [
        (g.rect.min.y, p.veil_top),
        (g.rect.min.y + top_band, p.veil_field),
        (g.rect.max.y - bottom_band, p.veil_field),
        (g.rect.max.y, p.veil_bottom),
    ];
    let mut prev: Option<(f32, f32)> = None;
    for &(y, alpha) in &stops {
        if let Some((py, pa)) = prev {
            if y > py {
                let base = u32::try_from(mesh.vertices.len()).unwrap_or(0);
                let (top, bot) = (
                    base_color.gamma_multiply(pa),
                    base_color.gamma_multiply(alpha),
                );
                mesh.colored_vertex(pos2(g.rect.min.x, py), top);
                mesh.colored_vertex(pos2(g.rect.max.x, py), top);
                mesh.colored_vertex(pos2(g.rect.min.x, y), bot);
                mesh.colored_vertex(pos2(g.rect.max.x, y), bot);
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base + 1, base + 3, base + 2);
            }
        }
        prev = Some((y, alpha));
    }
}

#[cfg(test)]
mod tests {
    use super::{bake_tier, demote, AbyssColors, AbyssParams, Geo, MAX_VERTS};
    use crate::config::AbyssTier;
    use crate::theme::{Metrics, Palette, Theme};
    use egui::Rect;

    fn colors() -> AbyssColors {
        AbyssColors::from_palette(&Palette::dark())
    }

    /// The veil's ground = the dark palette's `background`.
    const GROUND: egui::Color32 = egui::Color32::from_rgb(0x12, 0x14, 0x18);

    fn rect(w: f32, h: f32) -> Rect {
        Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, h))
    }

    /// Over a sweep of ratios from 0.2 to 5.0, every
    /// vertex is inside `rect.expand(s)` and every coordinate is finite. **Extreme ratios are handled
    /// by clamping, not by the formula.**
    #[test]
    fn abyss_survives_extreme_aspect_ratios() {
        let m = Metrics::default();
        for i in 0..25 {
            #[expect(clippy::cast_precision_loss, reason = "it is a small integer")]
            let ar = 0.2 + (5.0 - 0.2) * (i as f32 / 24.0);
            let r = rect(600.0 * ar, 600.0);
            for tier in [AbyssTier::Flat, AbyssTier::Lite, AbyssTier::Full] {
                let mesh = bake_tier(r, 1.0, colors(), GROUND, AbyssParams::default(), tier, &m);
                let slack = r.expand(r.width().min(r.height()));
                for v in &mesh.vertices {
                    assert!(
                        v.pos.x.is_finite() && v.pos.y.is_finite(),
                        "ar {ar} {tier:?}"
                    );
                    assert!(slack.contains(v.pos), "ar {ar} {tier:?}: {:?}", v.pos);
                }
                let n = u32::try_from(mesh.vertices.len()).unwrap_or(0);
                assert!(
                    mesh.indices.iter().all(|&i| i < n),
                    "ar {ar} {tier:?} index"
                );
                assert_eq!(mesh.indices.len() % 3, 0);
            }
        }
    }

    /// Each tier's vertices are within budget and all under the cap.
    #[test]
    fn abyss_vertex_budget_per_tier() {
        let m = Metrics::default();
        let r = rect(1280.0, 800.0);
        let p = AbyssParams::default();
        for (tier, budget) in [
            (AbyssTier::Flat, 128),
            (AbyssTier::Lite, 1200),
            (AbyssTier::Full, 3000),
        ] {
            let mesh = bake_tier(r, 1.0, colors(), GROUND, p, tier, &m);
            assert!(
                mesh.vertices.len() <= budget,
                "{tier:?}: {} > {budget}",
                mesh.vertices.len()
            );
            assert!(mesh.vertices.len() < MAX_VERTS, "{tier:?} ceiling");
        }
    }

    /// The vertices grow with the tier — otherwise the tiers mean nothing.
    #[test]
    fn tiers_are_ordered_by_cost() {
        let m = Metrics::default();
        let r = rect(800.0, 480.0);
        let p = AbyssParams::default();
        let n = |t| bake_tier(r, 1.0, colors(), GROUND, p, t, &m).vertices.len();
        assert!(n(AbyssTier::Flat) < n(AbyssTier::Lite));
        assert!(n(AbyssTier::Lite) < n(AbyssTier::Full));
        assert_eq!(demote(AbyssTier::Full), Some(AbyssTier::Lite));
        assert_eq!(demote(AbyssTier::Lite), Some(AbyssTier::Flat));
        assert_eq!(demote(AbyssTier::Flat), None);
    }

    /// **One** draw call. The seven layers and the veil are one mesh.
    #[test]
    fn abyss_emits_one_shape() {
        let ctx = egui::Context::default();
        let abyss = super::Abyss::new(AbyssParams::default());
        let theme = Theme::dark();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            abyss.paint(ui.painter(), rect(800.0, 480.0), &theme);
        });
        out.textures_delta.clear();
        let meshes = out
            .shapes
            .iter()
            .filter(|c| matches!(c.shape, egui::Shape::Mesh(_)))
            .count();
        assert_eq!(meshes, 1, "the background is a single mesh");
    }

    /// A still desktop never bakes again. With the colours and the size unchanged, the cache hits.
    #[test]
    fn a_still_desktop_never_rebakes() {
        let ctx = egui::Context::default();
        let abyss = super::Abyss::new(AbyssParams::default());
        let theme = Theme::dark();
        for _ in 0..8 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                abyss.paint(ui.painter(), rect(800.0, 480.0), &theme);
            });
            out.textures_delta.clear();
        }
        assert_eq!(abyss.rebuilds(), 1, "only the one at the start");
    }

    /// Frozen through the 12-frame cross-fade, then baked
    /// again **exactly once**.
    #[test]
    fn abyss_bakes_once_per_theme_toggle() {
        let ctx = egui::Context::default();
        let abyss = super::Abyss::new(AbyssParams::default());
        let dark = Theme::dark();
        let light = Theme::light();
        let draw = |theme: &Theme| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                abyss.paint(ui.painter(), rect(800.0, 480.0), theme);
            });
            out.textures_delta.clear();
        };
        draw(&dark);
        let before = abyss.rebuilds();
        // The frozen stretch after a toggle — it does not bake again however the palette differs each frame.
        for _ in 0..12 {
            draw(&light);
        }
        assert_eq!(abyss.rebuilds(), before, "it does not bake while frozen");
        // Run until the freeze lifts.
        for _ in 0..20 {
            draw(&light);
        }
        assert_eq!(abyss.rebuilds(), before + 1, "exactly once");
    }

    /// The taller the panel, the higher the canyon walls grow. In landscape they are 0.
    #[test]
    fn the_canyon_wall_grows_as_the_panel_gets_taller() {
        assert!(
            Geo::new(rect(1671.0, 941.0)).wall.abs() < f32::EPSILON,
            "a landscape one has no walls"
        );
        let portrait = Geo::new(rect(940.0, 1672.0)).wall;
        assert!(portrait > 0.7, "a portrait one is a canyon: {portrait}");
        assert!(
            (0.0..=1.0).contains(&Geo::new(rect(1.0, 10_000.0)).wall),
            "the extremes are clamped too"
        );
    }

    /// The colours follow the palette — override `primary` alone and **all nine** change.
    ///
    /// `water_deep` is no exception. It used to take `background` as it stood, but the UI's ground
    /// colour is 64 blue units darker than the original's deep sea, and the bottom of the screen died
    /// black.
    #[test]
    fn colors_follow_the_palette() {
        let mut p = Palette::dark();
        let base = AbyssColors::from_palette(&p);
        p.primary = egui::Color32::from_rgb(0xff, 0x7a, 0x00);
        let orange = AbyssColors::from_palette(&p);
        for (name, a, b) in [
            ("water_glow", base.water_glow, orange.water_glow),
            ("water_surface", base.water_surface, orange.water_surface),
            ("water_mid", base.water_mid, orange.water_mid),
            ("water_deep", base.water_deep, orange.water_deep),
            ("floor", base.floor, orange.floor),
            ("manta_body", base.manta_body, orange.manta_body),
            ("manta_belly", base.manta_belly, orange.manta_belly),
        ] {
            assert_ne!(a, b, "{name} does not follow primary");
        }
        // The silhouette comes from `background` alone, so it is independent of primary — which is the intent.
        assert_eq!(base.silhouette, orange.silhouette);
        // Fixed colours do not look at the palette at all.
        assert_eq!(AbyssColors::art(), AbyssColors::art());
        assert_ne!(AbyssColors::art().water_mid, base.water_mid);
    }

    /// The water column **darkens monotonically from top to bottom.** Reverse the order of
    /// even one of the four stops and the water brightens in the middle and looks like sky.
    #[test]
    fn the_water_column_gets_darker_all_the_way_down() {
        let lum = |c: egui::Color32| {
            0.2126f32.mul_add(
                f32::from(c.r()),
                0.7152f32.mul_add(f32::from(c.g()), 0.0722 * f32::from(c.b())),
            )
        };
        for (name, c) in [
            ("art", AbyssColors::art()),
            ("base", AbyssColors::from_palette(&Palette::dark())),
            (
                "abyss",
                AbyssColors::from_palette(&crate::theme::Palette::preset(
                    crate::theme::Preset::Abyss,
                    true,
                )),
            ),
        ] {
            let stops = [c.water_glow, c.water_surface, c.water_mid, c.water_deep];
            for pair in stops.windows(2) {
                let [a, b] = pair else { continue };
                assert!(lum(*a) > lum(*b), "{name}: {a:?} → {b:?} gets brighter");
            }
        }
    }
}
