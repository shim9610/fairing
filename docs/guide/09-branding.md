# 09. Product branding — giving the shell your machine's character

Fonts and images use egui's [font configuration](https://docs.rs/egui/0.37.0/egui/struct.FontDefinitions.html)
and [texture API](https://docs.rs/egui/0.37.0/egui/struct.Context.html#method.load_texture).
Fairing adds installation helpers and theme/branding hooks; image decoding and
asset selection belong to the application.

This page collects, in one place, what a manufacturer touches and what they have to prepare in
order to make a product look like their own. The optional Abyss styling (the manta and
the abyss) can be changed — override one colour, or replace the background, the mark and the icons
outright, all without patching the crate.

The global painter, procedural wallpaper and image-loader hooks below describe
ordinary `Shell` customization. Protected construction rejects untyped hooks;
use themes/fonts and permission-bound UI elements for protected integration.
See [security architecture](../security-architecture.md#2-construction-and-registration)
for that boundary.

Read first: [04 Customization](04-customization.md) (theme injection, painter hooks) and
[07 Config reference](07-config-reference.md) (the TOML keys).

---

## 0. Branding options

Choose the hook that matches the part of the product you want to change.

| What you want | Where you go | Images |
|---|---|---|
| "Just our company colour" | One line of `[theme.palette] primary` | none |
| "Our own flat or gradient background" | `[desktop] wallpaper = "#0B2237"`, or `Wallpaper::Gradient` | none |
| "Pick one of the crate's presets" | `[theme] preset = "base"` \| `"abyss"` \| `"linen"` | none |
| "Use the Abyss preset" | `.preset(Preset::Abyss)` — palette + background (§8.1) | none |
| "Settings menus in our colours and layout" | `settings::add_all` + `SettingsConfig` (§8.3) | none |
| "Our whole colour system" | The `[theme.palette]` roles, or `ShellBuilder::palettes(dark, light)` | none |
| "Our typeface" | `ShellBuilder::fonts` (§3) — the crate carries no fonts | none |
| "Change the feel of the layout" | `set_dock_placement` — a left rail, a floating band across the screen (§6.3) | none |
| "Our own status bar and nav bar" | `ShellBuilder::status_bar_painter` / `nav_bar_painter` (§6.2) | none |
| "Our photo or artwork as the background" | The `image_loader` hook + `[desktop] wallpaper = "file:…"` (§1) | yes (§2) |
| "Our own procedural background art" | `Wallpaper::ThemedPainter` | none |
| "Our icon set" | `register_icon_painter`, or replacing the SVGs (§4) | SVGs or reference art for the chosen route |
| "Our logo mark" | Register a custom mesh painter (§5) | Reference art for authoring |
| "No trace of the crate's brand at all" | Keep the `Base` preset (§8) | none |

Colour, typography and layout changes use configuration and code. Prepare image
assets or reference art for the routes identified in the last column.

> **Why the last row is "do nothing".** The default config has no brand in it. `[theme] preset`
> is `"base"` (neutral) and `[desktop] wallpaper` is `"background"` (a flat colour). The manta
> and the abyss are both **opt-in**, so writing nothing installs nothing. To drop them from the
> build as well, turn the `brand` feature off with `--no-default-features`.

---

## 1. The crate does not decode images — you plug in a loader

**Read this before §2; the rest of the page depends on it.**

`fairing` delegates wallpaper decoding to the application's loader. It does not read
PNG, JPEG or WebP for you — which codec fits depends on the machine (software `image`,
`zune-image`, a hardware decoder …). The application chooses the codec and its
dependency, memory and decoding policy.

**Instead there is a socket.** Register one function with [`ShellBuilder::image_loader`] and the
config file takes it from there.

### 1.1 Registering the loader

```rust,ignore
// Your Cargo.toml. image, zune-image, stb — your choice.
//   image = { version = "0.25", features = ["png", "jpeg", "webp"] }
// (Only this block is `ignore` — `image` is not a fairing dependency, so it cannot compile here.)

let shell = fairing::Shell::builder(config)
    .image_loader(|ctx: &egui::Context, path: &std::path::Path| {
        let bytes = std::fs::read(path).map_err(|e| fairing::Error::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        let rgba = image::load_from_memory(&bytes)
            .map_err(|e| fairing::Error::Image(e.to_string()))?
            .to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        Ok(ctx.load_texture(path.to_string_lossy(), img, egui::TextureOptions::LINEAR))
    })
    .build(&ctx)?;
```

This is a dependency of **your binary**, not of `fairing` — it does not go through the crate's
audit gate. The demo carries the same function (`image_loader` in `examples/common/mod.rs`), so
copying it is fine.

### 1.2 After that, one config line

With a loader installed you can give each site a different background **without rebuilding** the
device. On kiosks and signage this is usually the path:

```toml
# fairing.toml — the file that ships on the machine.
[desktop]
wallpaper = "file:/opt/acme/brand/store-bg.webp"
wallpaper_fit = "cover"     # cover (default) · contain · stretch
```

It is read **once**, when the shell is built. On failure it logs the error and falls back to the
`background` role colour. With no loader installed it logs a warning and uses the
same fallback. This handles a reported wallpaper-load error; the application
loader and the rest of device startup remain application responsibilities.

Swapping at runtime (a shop owner picking a photo in a settings screen) is one line too:

```rust
# use fairing::desktop::Fit;
# fn f(shell: &mut fairing::Shell, ctx: &egui::Context) -> fairing::Result<()> {
shell.load_wallpaper(ctx, "/opt/acme/brand/new-bg.webp", Fit::Cover)?;
# Ok(()) }
```

### 1.3 Installing one without a loader

If the image is baked into the binary rather than read from a path, or you already hold a
texture, you do not need the loader:

```rust
# fn decode(_b: &[u8]) -> egui::ColorImage { egui::ColorImage::filled([1, 1], egui::Color32::BLACK) }
# fn f(shell: &mut fairing::Shell, ctx: &egui::Context) {
# let opts = egui::TextureOptions::LINEAR;
# const BG: &[u8] = b"";
use fairing::desktop::{Fit, Wallpaper};

let texture = ctx.load_texture("bg", decode(BG), opts);   // BG = include_bytes!("../art/bg.webp")
shell.set_wallpaper(Wallpaper::owned(texture, Fit::Cover));
# }
```

### 1.4 Texture-handle lifetime

egui/epaint releases a managed texture when its last `TextureHandle` is dropped.
Passing an ID does not retain its handle; use `Wallpaper::owned` or retain a handle
in application state.

```rust
# use fairing::desktop::{Fit, Wallpaper};
# fn f(ctx: &egui::Context) -> (Wallpaper, Wallpaper) {
# let (name, opts) = ("bg", egui::TextureOptions::LINEAR);
# let color = egui::ColorImage::filled([1, 1], egui::Color32::BLACK);
# let source_size = egui::vec2(1.0, 1.0);
// ✗ Wrong. `handle` dies here and the background comes out black.
let handle = ctx.load_texture(name, color, opts);
let bad = Wallpaper::TextureFit { id: handle.id(), source_size, fit: Fit::Cover };

// ✓ The wallpaper holds the handle.
let good = Wallpaper::owned(handle, Fit::Cover);
# (bad, good) }
```

`Wallpaper::owned` also reads the source size **once, at construction**, and caches it.
`TextureHandle::size()` takes the texture manager's lock, so calling it in the paint loop
adds synchronization to that path. The constructor reads it once rather than
repeating the lookup during painting ([architecture §5](../architecture.md#5-threads)).

`Wallpaper::TextureFit` is the branch left for integrators who **manage textures themselves**
(atlases, streaming). Otherwise use `Wallpaper::owned`.

### 1.5 Things to be careful about in a loader

> **The loader is synchronous.** Its work runs on the calling UI thread. Decode
> during setup or another deliberate loading step and retain the resulting
> `Wallpaper::Owned`; measure the chosen decoder on the target hardware.

> **Bound decoded image storage.** A 30000 × 30000 RGBA image requires about
> 3.6 GB before decoder overhead. Read dimensions first, check width, height and
> decoded-byte limits, and reject images above the product's budget. For `image`,
> `ImageReader::into_dimensions()` reads the dimensions without full decoding.

> **Check texture limits.** Constrain dimensions to the limits of the selected
> egui host and graphics backend. Report an unsupported image size from the loader
> rather than relying on the upload failure's visual result.

> **Match the egui version.** The loader uses egui types such as `ColorImage` and
> `TextureHandle`, as do other UI extension points. Use `fairing::egui`, or select
> the matching egui version for a direct dependency ([01 Getting started](01-getting-started.md)).

> **Choose texture filtering for the displayed size.** `TextureOptions::LINEAR`
> leaves `mipmap_mode` unset. Strong minification can alias detailed source images.
> On the glow backend, mipmaps can be requested with
> `TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..Default::default() }`, or
> prepare an image closer to the panel's size.

[`ShellBuilder::image_loader`]: ../../crates/fairing/src/shell/builder.rs

---

## 2. Background images — requirements

### 2.1 How many do you prepare?

Prepare assets for the supported content rectangles. A shared image can be
cropped with `Cover` or letterboxed with `Contain`; separate aspect-ratio variants
can preserve different compositions.

**Strategy A — one image with `Fit::Cover`**

For a landscape product, the following sizes are possible starting assets. Check
the crop against each supported content rectangle.

| Item | Spec |
|---|---|
| Size | **2560×1440** (16:9) or **2400×1600** (3:2) |
| Composition margin | Keep important content within the middle 75 % as a starting layout, then verify the actual crop |
| Format | PNG or WebP. No alpha needed |
| Colour space | sRGB; convert other source colour spaces to the colour space expected by the texture upload |

> **Cropping is computed against the content ratio, not the panel ratio.** The background is
> drawn into the **content rect** — the screen minus the status bar and nav bar — not the whole
> screen. Bar dimensions, chrome policy and panel metadata change the resulting
> content ratio. A fixed percentage margin alone does not establish that artwork
> will remain inside every crop.

**Strategy B — one per ratio (when you ship instrument panels and portrait panels together)**

Count the panel ratios you actually ship and make that many. For reference, the set in this
repository (`assets/brand/`) is:

| Ratio | Size | For |
|---|---|---|
| 4.00 | 2560×640 | Bar-shaped instrument panels |
| 1.78 | 1671×941 | Ordinary landscape |
| 1.33 | 1600×1200 | Older industrial panels |
| 1.00 | 1440×1440 | Square HMIs |
| 0.56 | 940×1672 | Portrait panels |

Pick **whichever is closest to the real panel ratio**; it does not have to match exactly, since
`TextureFit` preserves the aspect.

### 2.2 What a background has to do — legibility

Desktop icon labels are drawn over the background without a shadow by default.

> **The shell does not lay a veil over a raster background — not by default.** The legibility
> veil is baked **into the mesh** by `brand::abyss` (`fn veil` in `brand/abyss.rs`) and exists
> only when `wallpaper = "abyss"`. A texture background is a single `painter.image(..)` —
> choose the image and overlaid colours together. §2.2a describes optional label
> treatments.

#### 2.2a When you cannot change the artwork — `label_legibility`

Bright image regions can reduce the contrast of overlaid labels and icons.
Change the artwork or select a label-legibility treatment, then inspect the
composited result. The reference manta's pale belly is one such region.

```toml
[desktop]
label_legibility = "shadow"   # none (default) · shadow · veil
```

| Value | What it does | When |
|---|---|---|
| `none` | Nothing | **Default.** Flat, gradient, or deliberately dark artwork |
| `shadow` | One dark copy behind labels and icons | Photo and artwork backgrounds where a local dark outline improves contrast |
| `veil` | A translucent `scrim` sheet over the whole content | Bright backgrounds where a full-content tint is preferable; check resulting contrast |

Change it along with the background at runtime — `Shadow` when switching to a photo, `None` when
going back to a flat colour:

```rust
# use fairing::desktop::Fit;
# fn f(shell: &mut fairing::Shell, ctx: &egui::Context) -> fairing::Result<()> {
# let path = "/opt/acme/brand/bg.webp";
use fairing::desktop::LabelLegibility;

shell.load_wallpaper(ctx, path, Fit::Cover)?;
shell.set_desktop_legibility(LabelLegibility::Shadow);
# Ok(()) }
```

> Fairing's `shadow` treatment draws an offset dark copy rather than a blurred
> shadow. It does not ensure contrast for every image/label pair. Inspect the
> composited result as described in §9.2.

The main desktop elements over the wallpaper are listed below. With ordinary
shown bars, the background is drawn into `content`, and the bars draw their own
surfaces. `BarMode::Overlay` changes that relationship for the status bar.

| What sits on the background | Where | What the background owes it |
|---|---|---|
| **The icon grid and its labels** | Most of the content's middle | Dark and low contrast (luminance 60 or below, for dark themes). Labels get no shadow and no plate by default (§2.2a) |
| **Dock labels** | The dock band, at its configured edge or floating position | Check the composited label/background contrast in that band |
| **The page indicator** | `page_indicator_height` (24 du) above the dock. Only with two or more pages | Check selected and unselected dot contrast |

| Rule | Why |
|---|---|
| Push bright areas into the **top 25 % of the content** | There is slack above the first icon row. The bottom is taken by the dock |
| Check label/background contrast against the product's text requirements | 4.5:1 is the WCAG normal-text criterion; size, content and the rendered background also matter (§9) |
| Never put text, logos or watermarks **inside the image** | They get cropped, rotated, and never translated |

For a light theme, consider a separate light image or an adjusted veil. Check
contrast with the dark label colours actually rendered.

### 2.3 The code

With a loader installed (§1.1), one config line does it:

```toml
[desktop]
wallpaper = "file:/opt/acme/brand/bg.webp"
wallpaper_fit = "cover"
```

At runtime:

```rust
# fn f(shell: &mut fairing::Shell, ctx: &egui::Context) -> fairing::Result<()> {
use fairing::desktop::Fit;

shell.load_wallpaper(ctx, "/opt/acme/brand/bg.webp", Fit::Cover)?;
# Ok(()) }
```

Directly, without a loader:

```rust
# const BYTES: &[u8] = b"";
# fn my_decode(_b: &[u8]) -> egui::ColorImage { egui::ColorImage::filled([1, 1], egui::Color32::BLACK) }
# fn f(shell: &mut fairing::Shell, ctx: &egui::Context) {
use fairing::desktop::{Fit, Wallpaper};

let texture = ctx.load_texture("bg", my_decode(BYTES), egui::TextureOptions::LINEAR);
shell.set_wallpaper(Wallpaper::owned(texture, Fit::Cover));
# }
```

| `Fit` | Behaviour | When |
|---|---|---|
| `Cover` | Fills the short side and **crops** the long one | Default. Photo and artwork backgrounds |
| `Contain` | Fits it all in; the remainder is the `background` role colour | Drawings and logos that must not be cropped |
| `Stretch` | Stretches (the same as `Wallpaper::Texture`) | Only when the source already has that ratio |

The config string is read by `Fit::parse` — `"cover"` · `"contain"` · `"stretch"`,
case-insensitive. An unknown name warns and falls back to `cover`.

> `Wallpaper::Texture { id }` and `Wallpaper::TextureFit { .. }` still exist. The first pins UV
> to `0..1` and **stretches**; the second keeps the aspect but **does not hold the texture**
> (§1.4). Unless you manage an atlas yourself, use `Wallpaper::owned`.

### 2.4 Theme-responsive backgrounds

Use `Wallpaper::themed(dark, light)` to switch between paired image backgrounds.
Use `ThemedPainter` when drawing should respond to the current palette or other
theme values, including `[theme.palette]` overrides.

```rust
# fn my_art(_painter: &egui::Painter, _rect: egui::Rect, _palette: &fairing::theme::Palette) {}
# fn set(shell: &mut fairing::Shell) {
use fairing::{ColorRole, Theme};
use fairing::desktop::Wallpaper;

shell.set_wallpaper(Wallpaper::ThemedPainter(Box::new(
    |painter: &egui::Painter, rect: egui::Rect, theme: &Theme| {
        // Do not capture colours — **read them from `theme` every frame** so they follow.
        painter.rect_filled(rect, 0.0, theme.color(ColorRole::Background));
        my_art(painter, rect, &theme.palette);
    },
)));
# }
```

The callback runs on each UI pass. Cache costly geometry where appropriate and
rebuild it when its inputs change. `brand::Abyss` is an example of a cached mesh;
the graphics host determines the resulting GPU draw calls.

> During the 200 ms theme crossfade, `theme.palette` can change on each UI pass.
> A cache keyed on the current colours can therefore rebuild repeatedly. `Abyss`
> uses the following policies to limit those rebuilds; adapt them to the painter's
> actual inputs and cost.
>
> | Device | What it does |
> |---|---|
> | `Abyss::tick_freeze` | When `theme.dark` flips, it **freezes** rebaking for the crossfade's duration and bakes exactly once on the frame it ends |
> | `BakeKey` quantisation | Rounds the rect to **4 device px** and `pixels_per_point` to **1/64**, so a resize does not bake once per pixel |

---

## 3. Type — font installation

**There are no fonts in the crate.** egui's `default_fonts` has no CJK glyphs, so Korean, Chinese
and Japanese come out as **□**. If you use those locales, injecting a typeface is not optional.

```rust
# const ACME_SANS: &[u8] = b"";   // include_bytes!("../art/AcmeSans.ttf")
# fn f(config: fairing::config::ShellConfig, ctx: &egui::Context) -> fairing::Result<()> {
use fairing::fonts::{FontPriority, FontSet, FontSource};

let mut fonts = FontSet::new();
// 1) Embed the product typeface.
fonts.push(FontSource::from_bytes("acme", ACME_SANS.to_vec()));
// 2) Find a system font already on the machine (keeps the binary small).
if let Some(path) = fairing::fonts::korean_font() {
    fonts.push(FontSource::from_path("ko", &path)?);
}
let shell = fairing::Shell::builder(config).fonts(fonts).build(ctx)?;
# let _ = (shell, FontPriority::default());
# Ok(()) }
```

| Item | Spec |
|---|---|
| Format | TrueType `.ttf` · OpenType `.otf` · collections `.ttc` (a face index can be given) |
| Coverage | Include the glyphs used by the UI. For arbitrary modern Korean input, the 11,172 precomposed Hangul syllables provide broader coverage than the 2,350-character KS X 1001 subset |
| Size | Subset before embedding. A full CJK font is 5–20 MB each |
| Priority | `FontPriority` orders the families. Product typeface for Latin, system font for CJK is a common combination |
| Licence | Check that the font licence permits the chosen binary/device distribution; web-use permission alone may not cover it |

> Changing egui font definitions can rebuild the glyph atlas and layout caches.
> `ShellBuilder::fonts` installs them during setup; avoid reinstalling the same
> definitions every frame.

> **Changing the typeface moves the metrics.** Row height is the maximum across the **whole**
> font family list, so adding a CJK fallback thickens label rows. Re-run the capture review in §9
> after installing a typeface.

---

## 4. Icon sets — requirements

### 4.1 Replacing the SVGs (if you fork the repository)

Swap the files in `assets/icons/*.svg` and run `cargo xtask icons`; it regenerates
`crates/fairing-widgets/src/icons/generated.rs`.

| Item | Spec |
|---|---|
| Grid | `viewBox="0 0 24 24"` **recommended**. Other viewBoxes are accepted and scaled uniformly into the centre of the 24 grid (`fit_to_grid` in `xtask/src/svg.rs`) |
| Stroke | `stroke="currentColor"` · `stroke-width="2"` · round caps and joins. **The compiler does not read these** — the real width is `IconStyle::stroke_px()` at runtime (2.0 by default) and the painter fixes caps and joins. You write them so the editor preview matches |
| Fill | `fill="none"` on the root. **To fill**, every subpath must be **convex** (below) |
| Path count | Six or fewer per icon recommended |
| Coordinates | All within `[0, 24]` **recommended**. The check only tests for finiteness |
| Forbidden (hard error) | `<text>` · `<image>` · gradients · filters · `transform` |
| File name | `[a-z0-9][a-z0-9_-]*.svg` — a **hard constraint**. Uppercase, spaces or dots are rejected |

> **The convexity constraint on filled icons.** The painter fills with a triangle fan from point
> 0, so a concave filled path breaks. `check_convex_fill` in `cargo xtask icons` catches it
> before it ships. If you need a concave shape, draw it with a **painter** instead of an icon
> (§5).

> **Do not touch the edge of the grid.** Strokes are drawn on the path's **centre line**, so a
> point at `x = 0` or `x = 24` bleeds half the stroke width outside the cell. The effective safe
> area is `[1, 23]`.

> When replacing built-in assets, keep these repository relationships consistent:
> 1. **Names and constants** — `builtin_names_resolve`,
>    `the_generated_table_covers_the_v1_set` and `the_generated_table_has_no_strays`
>    check the generated set against `icons/builtin.rs`. Update the constants and
>    expected set when names are added or removed.
> 2. **The attribution obligations change.** Today `assets/icons/` is a Lucide subset (ISC/MIT),
>    declared by `assets/icons/LICENSE-lucide` · `MAPPING.md` · `THIRD_PARTY.md`. Replacing the
>    set requires corresponding provenance and licence notices.
> 3. This route changes the repository assets. §4.2 provides application-local
>    registration when a fork is not needed.

### 4.2 Replacing them with painters (no fork; recommended)

Register one closure under one name. The crate is untouched.

```rust
# fn f(shell: &mut fairing::Shell, ctx: &egui::Context) -> fairing::Result<()> {
let id = shell.register_icon_painter(Box::new(
    |painter: &egui::Painter, rect: egui::Rect, style: &fairing::icons::IconStyle, color: egui::Color32| {
        // `color` is **already resolved** — the role tint, the theme crossfade and the
        // disabled alpha are all in it. Using it as-is keeps the icon contract.
        painter.circle_filled(rect.center(), rect.width() * 0.4, color);
        let _ = style;
    },
));
// Use it wherever an IconRef::Custom(id) goes.
# let _ = id;
# Ok(()) }
```

Two constraints:

- **It is `Fn`, not `FnMut`.** `IconPainter = Box<dyn Fn(..)>`, so it cannot hold `&mut` state.
  If you need a cache, capture a `RefCell` — that is what `brand::manta_painter` does.
  (`Mutex` / `RwLock` / `OnceLock` are banned in this crate.)
- **It is called once per icon drawn per frame.** The "no heap allocation" rule from §2.4 applies
  here too. Twelve icons on a desktop page means twelve calls a frame.

> To draw a **whole desktop cell** (icon + label + badge + press state), the hook is
> `ShellBuilder::slot_painter`, not an icon painter — that one is `FnMut` and gets the cell rect,
> gate result and press scale through `SlotCx` ([04 Customization](04-customization.md)).

### 4.3 Raster icons

Use `IconRef::Texture { id, tint }` for raster icons. Choose source resolution and
filtering for the supported display sizes; 72 px for a 24-point icon is one
starting asset size. With `tint = true`, the source is multiplied by the role
colour, so a white silhouette preserves that tint.

---

## 5. Logo marks — requirements

The reference manta has a concave silhouette and two colour regions, so it uses
a custom mesh painter rather than a single-colour filled icon path.

### 5.1 Authoring one the way we did (a vector mesh)

The repository's manta was made this way, and **the procedure is written down in
`assets/brand/README.md`.** It needs two images.

| File | Spec | Used for |
|---|---|---|
| Silhouette | **1600×1000 PNG**, pure white ground, pure black solid silhouette, **no** shadow, gradient, texture or outline, 5 % margin | The outline coordinates |
| Two-tone | Same size, same drawing, exactly two colours (e.g. navy + white) | The interior split coordinates |

The procedure that extracts coordinates from those two (threshold → opening → sub-pixel contour
tracing → PCA axis split → least-squares cubic Bézier fit) is in `assets/brand/README.md`, under
the section on where the mark's coordinates came from. Check the resulting mesh
against the reference images at the intended display sizes.

**How the crate's manta is drawn**, for a mark of your own with concave edges and two tones —
epaint's convex-polygon fill expects convex geometry; a triangulated `egui::Mesh`
can represent other shapes. The crate's own helpers live
in a private module (`brand::ribbon`; only `Cubic` is public), so a painter of yours writes these
four steps on top of `egui::Mesh`:

| Step | What it does |
|---|---|
| Sample each curve to a fixed count | A cubic Bézier chain becomes `n` points **regardless of its knot count**. Two curves used as a pair must use the same `n` |
| A strip between two boundaries | Fill between them with quads. Paired boundaries can describe concave strips; check that their sampling and triangulation do not cross |
| A feather | An alpha-0 band on edges that meet the background. It fakes the antialiasing a raw `Mesh` does not get from epaint |
| Fan fill | Only for pieces that are convex |

The reference mark draws a body ribbon and overlaps a lighter region. Check mesh
coverage and antialiasing at the shared edge when authoring another mark.

### 5.2 Drawing it with a painter

Register a drawing closure as in §4.2. The closure receives a rectangle and
resolved colour and can submit suitable egui shapes or meshes. Its geometry,
colour handling and fit are the application's implementation.

### 5.3 Keeping the default mark but changing its tone

```rust
# fn f(shell: &mut fairing::Shell, ctx: &egui::Context) -> fairing::Result<()> {
use fairing::brand::{manta_painter, MantaStyle};

let id = shell.register_icon_painter(manta_painter(MantaStyle {
    belly_mix: 0.30,   // a less bright belly
    flip_x: true,      // head to the left
    ..MantaStyle::default()
}));
# let _ = id;
# Ok(()) }
```

> `MantaCache` keys its mesh on absolute position and size. Repeated calls with
> the same key reuse it; rectangle changes during press animation or page swipes
> can require regeneration. Measure that work when using the painter in moving cells.

---

## 6. Chrome — status bar, nav bar, dock placement

Bar visibility follows configuration and per-screen chrome policy. Status/nav
bar styling and dock placement can be changed without raster image assets.

### 6.1 What config alone can do

```toml
[status_bar]
height = 40                 # du, pinned; left out, it is 7 mm
icon_size = 20              # du, pinned; left out, 3.6 mm
icon_color = "primary"      # a role name, or "#RRGGBB"
left   = ["status.clock"]
right  = ["status.wifi", "status.battery"]

[nav_bar]
height = 64                 # du, pinned; left out, a finger plus 8 du
style  = "buttons"          # or "gesture": the home indicator in place of the buttons
items  = ["back", "home", "recents"]
```

For per-item captions, text sizes and padding, edit an owned `status_item_spec(id)` and commit it with `set_status_item_spec(id, spec)` —
that is how the instrument-panel look (a caption under each icon) comes out.

### 6.2 Painting a bar entirely yourself

```rust
# const MY_BRAND_BAR: egui::Color32 = egui::Color32::BLACK;
# fn f(config: fairing::config::ShellConfig, ctx: &egui::Context) -> fairing::Result<()> {
let shell = fairing::Shell::builder(config)
    .status_bar_painter(|ui, cx| {
        // `cx` brings the item list, the gate results and the theme. The built-in render is
        // not called.
        ui.painter().rect_filled(cx.rect, 0.0, MY_BRAND_BAR);
        // … draw it our way
    })
    .build(ctx)?;
# let _ = shell;
# Ok(()) }
```

> **Parametric icons do not change when you replace icons.** The status bar's Wi-Fi, battery,
> Bluetooth and signal glyphs are **parametric** drawings that take state as an argument, so an
> `IconRef` cannot point at them (nor can the painter registration in §4.2). The only way to make
> those four yours is `status_bar_painter`.

### 6.3 Dock placement

```rust
# fn f(shell: &mut fairing::Shell) {
use fairing::desktop::{Axis, DockPlacement};
use fairing::gesture::Edge;

shell.set_dock_placement(DockPlacement::Edge(Edge::Left));      // a left rail
shell.set_dock_placement(DockPlacement::Band {                  // a floating band
    axis: Axis::Horizontal,
    at: 0.66,
});
# }
```

In TOML that is `[desktop] dock_edge = "left"` and `dock_band = 0.66`.

| Placement | Impression |
|---|---|
| `Edge(Bottom)` (default) | The phone convention |
| `Edge(Left)` / `Edge(Right)` | Portrait panels, machines with side grips |
| `Edge(Top)` | An instrument cluster |
| `Band { .. }` | A **floating panel** over the desktop. The old PMP launcher look |

### 6.4 Different chrome per screen

`.background(role)` on a declaration gives that screen a different ground colour;
`ChromePolicy` with `status_bar = BarMode::Overlay` puts **the status bar over the content** so
the background shows through underneath (that is the one case where §2.2's "keep the top clear"
means anything). `ChromePolicy::fullscreen()` hides both bars.

---

## 7. Splash / boot screens (**no API yet**)

> There is no `Splash` and no `ShellBuilder::splash` in the crate. It is planned, and the
> implementation is still on the to-do list. The host can draw its own startup
> screen before the shell's first frame. The table below gives example assets
> for that host-owned UI.

| Item | Spec |
|---|---|
| Size | Landscape **1920×1080**, portrait **1080×1920** (whichever orientation you use) |
| Composition | **Leave the middle third empty** — the wordmark goes there, drawn in code |
| Format | PNG/WebP, sRGB |
| Text | Not inside the image |

The recommended route is an image for the background only, with the mark and wordmark drawn in
code. Then a change of locale, resolution or theme does not mean remaking the asset.

---

## 8. Branding customization scope

The tables below describe configuration, registration and painting hooks for
built-in surfaces. Protected-shell restrictions still apply to these hooks.

### 8.1 One line on, one line off

To take a crate default set, one line:

```rust
# fn f(config: fairing::config::ShellConfig, ctx: &egui::Context) -> fairing::Result<()> {
use fairing::theme::Preset;

let shell = fairing::Shell::builder(config)
    .preset(Preset::Abyss)   // the abyss palette + the procedural background
    .build(ctx)?;
# let _ = shell;
# Ok(()) }
```

| `preset` | Palette | Background |
|---|---|---|
| `Preset::Base` (default) | Neutral dark/light | The `background` role, flat |
| `Preset::Abyss` | A deep-sea palette drawn from the manta art | Procedural deep sea (feature `brand`) |

**Call nothing and nothing is installed.** Without `preset` and with nothing in the TOML, you get
the neutral palette and a flat background — no manta, no abyss. That is why there is no
`[shell] brand = false` kill switch: there is nothing to switch off.

### 8.2 The layer-by-layer override table

Whatever is called **after** `preset` wins. The rows are independent, so you can change only the
colours and keep our background.

| # | What the crate produces | How to override it | Where |
|---|---|---|---|
| 1 | The palette (its roles) | `[theme.palette]`, or `.palettes(dark, light)` | §0 |
| 2 | Metrics | `.metrics_spec(..)` · `.scale_policy(..)` | §0 |
| 3 | Motion tokens | `[motion]`, or `Shell::set_motion` | 04 §8 |
| 4 | Type | `.fonts(FontSet)` — **the crate carries no fonts** | §3 |
| 5 | The background | `.wallpaper(..)` · `[desktop] wallpaper` · `image_loader` + `file:` | §1 · §2 |
| 6 | 80 vector icons | `register_icon_painter` (recommended), or replacing the SVGs | §4 |
| 7 | 4 parametric icons (wifi/battery/bluetooth/signal) | **An `IconRef` cannot point at them** — take the whole bar with `status_bar_painter` | §6.2 |
| 8 | Status bar · nav bar | `.status_bar_painter(..)` / `.nav_bar_painter(..)` | §6.2 |
| 9 | Desktop cells | `.slot_painter(..)` (`SlotCx` brings press state and gates) | 04 |
| 9b | **Colour, corners and spacing of the layout inside a screen** | `layout::Deco` on `grid` · `group_with` · `action_bar_with` | — |
| 10 | Dock placement | `set_dock_placement` · `[desktop] dock_edge` · `dock_band` | §6.3 |
| 11 | **The built-in settings screens** | §8.3 below | 06 |
| 12 | The manta mark | Opt-in to begin with — do not call `manta_painter` and it is not there | §5 |
| 13 | Strings | `.translations(Translations)` — an entry of yours wins over the built-in one, and a language of yours joins English and Korean. Replacing a screen replaces its labels too | 04 §9 · §8.3 |

Row 7 is the only place you cannot change shallowly. Those four change shape with their state, so
they cannot be expressed as a static icon slot.

### 8.3 The built-in settings screens — three depths

```rust
# fn f(shell: &mut fairing::Shell) {
use fairing::settings::{add_all, SettingsConfig};

add_all(shell, &SettingsConfig::default());   // ← every built-in screen in one line
# }
```

| Depth | How | What changes |
|---|---|---|
| Shallow | `[theme.palette]` / `.palettes(..)` | Colours resolved through palette roles |
| Shallow | `.metrics_spec(..)` / `.scale_policy(..)` | Row heights, padding, corners, touch targets |
| Middle | `SettingsConfig::without("settings.wifi")` | That screen **is not created** |
| Middle | `SettingsConfig::only([..])` | Only the ones listed are created |
| Middle | `.without_home_icon()` | Keeps the screen, drops the desktop icon |
| Middle | `add_all_with(shell, cfg, list)` | **Which rows the settings list offers** — yours in, built-ins out, in your order |
| Deep | `shell.add(screen("settings.wifi", ..))` | **That whole screen** (labels, layout, behaviour) |
| All | Never call `add_all` | None of them exist |
| Compile | Exclude `settings` under `default-features = false` | They leave the binary as well |

**Registering the same id again replaces it** — the point being that there is no special API for
it:

```rust
# use fairing::settings::{add_all, SettingsConfig};
# use fairing::screen;
# fn my_wifi_screen(_ui: &mut egui::Ui, _cx: &mut fairing::Cx<'_>) {}
# fn f(shell: &mut fairing::Shell) {
add_all(shell, &SettingsConfig::default());          // the built-ins
shell.add(screen("settings.wifi", my_wifi_screen));  // one of them replaced with ours
# }
```

**Wide screens become two columns on their own.** `settings.home` splits into a list on the left
and a screen on the right when the pane is at least 680 du wide and at least 1.35× as wide as it
is tall (a 1024×600 panel and an unfolded foldable both qualify). Narrow again and it goes back to
one column. There is nothing to configure.

The right column draws **whatever is registered under the selected id** — the replacement above
included, so the deep row holds on a wide screen as well as a narrow one. It uses
`Cx::draw_screen` ([02 §3.1](02-screens.md#31-drawing-one-screen-inside-another)), so a screen
declared with `screen_with` goes up as a pushed screen instead of into the column: it owns no
screen between opens, and there is nothing to draw there. A screen of yours that is a `struct` —
because it wants `on_back`, say, so that back cancels an edit rather than leaving — registers with
`screen_of(id, value)` and is resident, so it goes in the column like the built-ins do.

**The list of rows is a `Vec` you own.** `screens::entries()` hands back the built-in one; take
rows out of it, put rows of your own in, reorder it, and `add_all_with` registers `settings.home`
over it. Everything else works exactly as in `add_all`, which is this call with the default list.

```rust
# fn f(shell: &mut fairing::Shell) {
use fairing::icon;
use fairing::screen;
use fairing::settings::screens::{entries, SettingsEntry};
use fairing::settings::{add_all_with, SettingsConfig};

let mut list = entries();
list.retain(|entry| entry.id != "settings.locale");                  // one taken out
list.push(SettingsEntry::new("app.heater", icon::GAUGE, "Heater"));  // one of ours put in

shell.add(screen("app.heater", |ui: &mut egui::Ui, _cx: &mut fairing::Cx<'_>| {
    ui.label("heater");
}));
add_all_with(shell, &SettingsConfig::default(), list);
# }
```

A row is drawn **only where a screen really is registered under its id and that screen's own gate
passes**, so add a screen for each application row. `settings.credentials` is
registered when the authenticator exposes administration support. A repeated row
id keeps its first occurrence and logs a warning.

Capability-dependent screens are omitted when the backend does not report the
needed capability. To register them during development regardless, use
`SettingsConfig::ignoring_capabilities()`.

> **Do not forget the gates.** A screen id is a gate name (05 §3). A high `default_gate` locks
> the settings screens as a group — list the ones that should open in `[access.gates]`.
> Conversely, **power and accounts are right to leave locked**: mis-pressing shutdown, and
> editing accounts, should not be open to everyone on a machine.

> **Power does not turn itself off.** Even when `settings.power` gets its confirmation, the shell
> does not act; it emits `ShellEvent::PowerRequest`. The action happens when the integrator has
> finished cleaning up and calls `shell.commit_power(request)` — **not calling it is the
> refusal.** The host performs application cleanup and decides whether to commit
> the request.

### 8.4 Removing it from the build too

```toml
# The default features are ["mock", "overlay", "osk", "brand", "settings"].
# Drop what you want gone and **re-list the rest**, or it disappears quietly.
[dependencies.fairing]
git = "https://github.com/shim9610/fairing"
default-features = false
features = ["mock", "overlay", "osk", "runner"]
#            ↑ without brand (manta, abyss) and settings (the built-in screens)
```

| Feature | What goes when it is off | What stays |
|---|---|---|
| `brand` | The **drawing code** for the manta mark and the procedural abyss background | The palette presets, `AbyssTier` and the `Wallpaper` variants are outside the gate and stay |
| `settings` | The built-in settings **screens** | The value model (`SettingKey` · `SettingsView` · `keys`) is outside the gate — `cx.settings` is always there |
| `overlay` | The shade and quick-settings tiles | — |
| `osk` | The on-screen keyboard | — |

> An unknown `[theme] preset` returns `Error::Config` during startup. An
> unrecognized `[desktop] wallpaper` value logs a warning and falls back to the
> `background` colour.

Turning `brand` off removes `brand::*` entirely. The `[desktop.abyss]` section is parsed and
ignored, and `wallpaper = "abyss"` warns and falls back to the `background` role colour — the
build does not break just because the config still mentions them.

### 8.5 Checking that nothing of ours is left

```sh
# 1. No brand keys in the config
grep -nE '^\s*(preset|wallpaper)\s*=' fairing.toml

# 2. No brand calls in the code
grep -rn 'manta_painter\|abyss\|Preset::Abyss' src/

# 3. No brand in the compiled tree
cargo tree -e features -i fairing | grep -c brand
```

These searches locate explicit brand references and the enabled feature. Inspect the
rendered result as well: the palette, metrics, 80 icons and built-in settings screens
remain independently configurable through §8.2.

---

## 9. Review — run these before you hand it over

When the branding is done, check these four. **Looking at it is not enough.**

### 9.1 The contrast floor

Check opaque palette colour pairs using the
[WCAG relative-luminance formula](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html).
This example includes the helper rather than calling a public Fairing API:

```rust
fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
    fn luminance(c: egui::Color32) -> f32 {
        fn channel(v: u8) -> f32 {
            let v = f32::from(v) / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126f32.mul_add(
            channel(c.r()),
            0.7152f32.mul_add(channel(c.g()), 0.0722 * channel(c.b())),
        )
    }
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

// Replace this with the palette used by your product.
let p = fairing::theme::Palette::dark();
assert!(contrast(p.on_surface, p.surface) >= 7.0);
assert!(contrast(p.on_primary, p.primary) >= 4.5);
```

The crate uses the same calculation in its test-only `theme::contrast` helper.
These pair checks do not assess font size, composited transparency, background
images or overall accessibility conformance.

The crate's `every_preset_meets_the_contrast_floor` (`theme/mod.rs`) is the worked example.
Check the actual `on_primary`/`primary` pair against the stated contrast floor.
The Base dark palette uses white `on_primary`; the Abyss dark palette uses dark
foreground ink on its brighter accent. Check the selected preset's actual pair.

### 9.2 Label legibility over the background

`testing::Harness::frame_shapes()` returns shapes rather than rendered pixels.
To assess composited label contrast over a raster background, capture a rendered
window or obtain pixel output from the integrating host.

- In this repository, `examples/demo.rs --tour <dir>` does exactly that — it opens a window on
  Xvfb with a software renderer and dumps frames as PNGs (the encoder uses std only).
- On your side, bring your own `eframe` app up the same way once, capture it, and measure the
  luminance of the pixels under the label rects (`Desktop::icon_rect(id)` gives you those).
- Include the supported display profiles, palettes, label lengths and image
  backgrounds in the product review; two resolutions in dark/light mode are
  a starting sample, not an exhaustive set.

### 9.3 Idle repaint behavior

Animations and application callbacks can keep requesting updates. In an idle
scenario, inspect repaint requests as well as backend wakes and host input.
Common sources include:

| Switch | What it does |
|---|---|
| `[desktop.abyss] animate = true` | Moves the mantas every frame. Turning it on logs a warning at startup |
| `[shell] repaint = "continuous"` | Requests continuous updates; the host controls the delivered frame cadence |
| A `ctx.request_repaint()` inside your own background callback | Requests an update whenever that callback executes |

### 9.4 Draw calls and allocations

If you wrote a procedural background, count the shapes per frame. One baked mesh is the target —
`brand::abyss`'s `abyss_emits_one_shape` test pins that contract.

---

## 10. Summary — the image checklist

Example asset sizes for the image-based customization routes:

| # | File | Size | Format | Required? |
|---|---|---|---|---|
| 1 | Background (dark) | 2560×1440 | PNG/WebP sRGB | To change the background |
| 2 | Background (light) | 2560×1440 | ″ | If you ship a light theme too |
| 3 | Mark silhouette | 1600×1000 | PNG, two-tone black/white | To change the mark |
| 4 | Mark two-tone | 1600×1000 | PNG, two colours | ″ (if it has an interior split) |
| 5 | Splash | 1920×1080 | PNG/WebP | If you use a boot screen (**no API yet**, §7) |
| 6 | Icon set | 24×24 SVG | Stroke | To change the icons |

If all you want is a colour change, **none of these are needed.** One line of `[theme.palette]`
carries through to the water.

For wallpaper images 1 and 2, register `ShellBuilder::image_loader` (§1.1).
The host supplies decoding for its startup screen in row 5. Rows 3 and 4 are
authoring references, not automatically bundled runtime assets. Built-in SVGs
are converted to vector geometry at build time.

> **A note on colour space.** Texture backgrounds pass the source through unchanged, using
> `Color32::WHITE` as the multiplicative identity. Palette interpolation uses
> `lerp_to_gamma`. Inspect rendered boundaries when combining texture colours
> with palette-based fills; matching input RGB values alone does not assess the
> full composited result.

---

## Appendix A. Every `[desktop.abyss]` key

These tune the optional Abyss background. They are only read
when `wallpaper = "abyss"`.

| Key | Default | Meaning |
|---|---|---|
| `colors` | `"palette"` | `"palette"` follows the theme · `"art"` uses fixed colours measured from the reference art |
| `tier` | `"lite"` | `flat` (~100 vertices, for software renderers) · `lite` · `full` (~2100) |
| `seed` | `0xFA12_1234` | Changes **the scatter only**. Formations, ray count and light position are authored constants |
| `rays` | `10` | God rays (0..=16) |
| `bubbles` | `32` | Bubbles (0..=64) |
| `fish` | `40` | Fish (0..=64) |
| `mantas` | `5` | Mantas (0..=6) |
| `light_x` | `0.50` | Light source position across the screen (as a fraction of width) |
| `veil_top` | `0.22` | **Legibility veil**, top alpha |
| `veil_field` | `0.18` | Veil opacity over the icon-grid band; check label contrast when changing it |
| `veil_bottom` | `0.16` | The bottom band (dock, indicator) |
| `animate` | `false` | Requests updates for animation. Turning it on warns at startup |
| `bake_budget_ms` | `1.5` | Over budget drops the tier one step and warns |

## Appendix B. Every public hook that touches branding

| Hook | What it changes |
|---|---|
| `ShellBuilder::theme(Theme)` | Palette **plus metrics and motion tokens**. `[theme]` alone only changes colours |
| `ShellBuilder::palettes(dark, light)` | The two palettes the theme toggle moves between |
| `ShellBuilder::fonts(FontSet)` / `font(..)` | Type (§3) |
| `ShellBuilder::scale_policy(..)` | **Defaults to a bare 9 mm finger** and text read at hand-held distance. The finger moves touch targets, rows, bar thickness and icon cells together; `ScalePolicy::gloved()` is 13 mm |
| `ShellBuilder::metrics_spec(..)` | The shell's metrics, mixing mm, finger units and du |
| `ShellBuilder::physical_mm(w, h)` | Explicit visible panel dimensions, overriding backend-reported physical size |
| `ShellBuilder::status_bar_painter` / `nav_bar_painter` | A whole bar (§6.2) |
| `ShellBuilder::slot_painter` | A whole **desktop cell** (`FnMut`; `SlotCx` brings press state and gates) |
| `ShellBuilder::image_loader` | The file → texture decoder (§1). With it installed, `wallpaper = "file:…"` works |
| `ShellBuilder::wallpaper(..)` | The background from code; takes precedence over `[desktop] wallpaper` |
| `ShellBuilder::preset(Preset)` | **A crate default set** (palette + background) in one line (§8.1) |
| `Shell::set_palettes` / `apply_palette` / `set_theme_dark` | Palettes at runtime |
| `Shell::set_motion` | Motion tokens |
| `Shell::register_icon` / `register_icon_painter` | A vector icon / a painter |
| `Shell::set_status_bar_height` · `set_nav_height` · `set_status_item_spec` | Persistent metrics and owned item configuration |
| `Shell::set_wallpaper` · `set_dock_placement` | Background · dock position |
| `Shell::load_wallpaper(ctx, path, fit)` | Swapping the background at runtime (needs a loader) |
| `settings::add_all(shell, &SettingsConfig)` | The built-in settings screens (§8.3) |
| `settings::add_all_with(shell, &SettingsConfig, list)` | The same, over a settings list of your own (§8.3) |
| `Shell::commit_power(request)` | Carrying out a power action. **Not calling it is the refusal** (§8.3) |
| `ChromePolicy` (per declaration) | Per-screen ground colour, bar modes, fullscreen |

`Wallpaper` has nine variants: `Solid(role)` · `Fixed(color)` · `Gradient{top,bottom}` (its
mesh is cached until the rectangle or colours change) · `Texture` · `TextureFit` · **`Owned`** (holds the
texture handle, §1.4) · `Painter` · `ThemedPainter` · `Themed{dark,light}` (a pair the shell
picks from as the palette switches; `Wallpaper::themed(dark, light)`). It is `#[non_exhaustive]`, so a `match`
**outside** the crate needs a `_` arm.
