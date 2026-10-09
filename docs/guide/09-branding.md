# 09. Product branding — giving the shell your machine's character

This page collects, in one place, what a manufacturer touches and what they have to prepare in
order to make a product look like their own. The default brand (the manta, the abyss) is **a
default, not a lock** — override one colour, or replace the background, the mark and the icons
outright, all without patching the crate.

Read first: [04 Customization](04-customization.md) (theme injection, painter hooks) and
[07 Config reference](07-config-reference.md) (the TOML keys).

---

## 0. The 30-second decision table

Where you reach depends on how far you want to go. **Cheap at the top, free at the bottom.**

| What you want | Where you go | Images | Effort |
|---|---|---|---|
| "Just our company colour" | One line of `[theme.palette] primary` | none | 1 min |
| "Our own flat or gradient background" | `[desktop] wallpaper = "#0B2237"`, or `Wallpaper::Gradient` | none | 5 min |
| "Pick one of the crate's presets" | `[theme] preset = "base"` \| `"abyss"` \| `"linen"` (**those three**) | none | 1 min |
| **"Just use a crate default set"** | `.preset(Preset::Abyss)` — palette + background in one line (§8.1) | none | 1 min |
| "Settings menus in our colours and layout" | `settings::add_all` + `SettingsConfig` (§8.3) | none | 10 min |
| "Our whole colour system" | The `[theme.palette]` roles, or `ShellBuilder::palettes(dark, light)` | none | 30 min |
| **"Our typeface"** | `ShellBuilder::fonts` (§3) — the crate carries no fonts | none | 1 hour |
| "Change the feel of the layout" | `set_dock_placement` — a left rail, a floating band across the screen (§6.3) | none | 10 min |
| **"Our own status bar and nav bar"** | `ShellBuilder::status_bar_painter` / `nav_bar_painter` (§6.2) | none | half a day |
| "Our photo or artwork as the background" | The `image_loader` hook + `[desktop] wallpaper = "file:…"` (§1) | **yes** (§2) | 30 min |
| "Our own procedural background art" | `Wallpaper::ThemedPainter` | none | days |
| "Our icon set" | `register_icon_painter`, or replacing the SVGs (§4) | **yes** | 1–3 days |
| "Our logo mark" | Register a painter, or author one with `brand::ribbon` (§5) | **yes** | 1 day |
| "No trace of the crate's brand at all" | **Do nothing** (§8) | none | 0 |

**Only three rows need images.** The other nine are config and code — most of "our character" is
colour, type and layout, not pictures.

> **Why the last row is "do nothing".** The default config has no brand in it. `[theme] preset`
> is `"base"` (neutral) and `[desktop] wallpaper` is `"background"` (a flat colour). The manta
> and the abyss are both **opt-in**, so writing nothing installs nothing. To drop them from the
> build as well, turn the `brand` feature off with `--no-default-features`.

---

## 1. The crate does not decode images — you plug in a loader

**Read this before §2; the rest of the page depends on it.**

`fairing` has no image decoder dependency and will not get one. It does not read
PNG, JPEG or WebP for you — which codec fits depends on the machine (software `image`,
`zune-image`, a hardware decoder …), and picking one in the crate makes every machine that does
not want it pay for it.

**Instead there is a socket.** Register one function with [`ShellBuilder::image_loader`] and the
config file takes it from there.

### 1.1 The loader — ten lines

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
`background` role colour — **one missing image never stops the machine from booting.** With no
loader installed it logs a warning and takes the same fallback.

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
shell.desktop_mut().set_wallpaper(Wallpaper::owned(texture, Fit::Cover));
# }
```

### 1.4 The most common way to get a black background

`impl Drop for TextureHandle` frees the texture (epaint `texture_handle.rs`). Keep the handle in
a local and pass only its `id()`, and **the texture disappears when the function returns.**

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
breaks the crate's no-locks rule ([architecture §5](../architecture.md#5-threads)); the constructor does that one call for you.

`Wallpaper::TextureFit` is the branch left for integrators who **manage textures themselves**
(atlases, streaming). Otherwise use `Wallpaper::owned`.

### 1.5 Things to be careful about in a loader

> **Decoding happens synchronously on the UI thread.** A 4 MPx image is tens of milliseconds, so
> call it outside the frame budget: while building the shell, or before a screen transition.
> **Never per frame** — hold on to the `Wallpaper::Owned` you built once.

> **Guard against decompression bombs.** A 100 KB PNG that expands to 30000 × 30000 is 3.6 GB of
> RGBA and kills the machine. Read the size from the header first and return an error above your
> limit (`image` reads headers only with `ImageReader::into_dimensions()`). 8K is 33 MPx, so
> somewhere around there is a sane ceiling.

> **4096 px per side.** Embedded GPUs commonly cap textures at 4096 or 8192, and above that the
> upload fails **silently** and the background comes out black. A warning in the loader makes
> that much easier to diagnose later.

> **Match the egui version.** The loader is the **only** place where you touch egui types
> (`ColorImage`, `TextureHandle`) directly. If the egui in your `Cargo.toml` is a different minor
> from the one `fairing` uses, two copies of egui get linked, the types stop matching and the build
> breaks. Ask for the same minor with `egui = "0.36"` ([01 Getting started](01-getting-started.md)).

> **There are no mipmaps.** `TextureOptions::LINEAR` means `mipmap_mode: None`. Putting a
> 2560×1440 image on an 800×480 panel is a 3.2× downscale with no mipmaps, so it **shimmers even
> on a still screen** — most visibly on exactly the quiet low-contrast gradients §2.2 asks for.
> On the glow backend, upload with
> `TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..Default::default() }`, or
> prepare an image closer to the panel's size.

[`ShellBuilder::image_loader`]: ../../crates/fairing/src/shell/builder.rs

---

## 2. Background images — requirements

### 2.1 How many do you prepare?

**This crate assumes there is no fixed screen size.** Try to cover every panel with one image and
something will be squashed or cropped somewhere. Pick one of two strategies.

**Strategy A — one image with `Fit::Cover` (recommended; most people stop here)**

Make **one generous image** at the most common panel ratio (1.6–1.8 landscape) and accept the
cropping.

| Item | Spec |
|---|---|
| Size | **2560×1440** (16:9) or **2400×1600** (3:2) |
| Safe area | Everything that matters inside the middle **75 %** (see the note) |
| Format | PNG or WebP. No alpha needed |
| Colour space | **sRGB** (gamma 2.2). Deliver Display P3 and the saturation will be off |

> **Cropping is computed against the content ratio, not the panel ratio.** The background is
> drawn into the **content rect** — the screen minus the status bar and nav bar — not the whole
> screen. On a 1920×1080 panel the two bars take about a tenth of the height between them (their
> sizes follow the finger and the panel's density), so the content is wider in ratio than the
> panel — about 1.93 against 1.78 — and a 2560×1440 source (ratio 1.778) loses **another 4 %**
> off the top and bottom. That is why the safe area is 75 % and not 80 %.

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

A background cannot only be pretty. **Icon labels sit on top of it with no shadow.**

> **The shell does not lay a veil over a raster background — not by default.** The legibility
> veil is baked **into the mesh** by `brand::abyss` (`fn veil` in `brand/abyss.rs`) and exists
> only when `wallpaper = "abyss"`. A texture background is a single `painter.image(..)` —
> **the artwork itself is responsible for legibility.** §2.2a is the escape hatch for when you
> cannot change the artwork.

#### 2.2a When you cannot change the artwork — `label_legibility`

Photo and artwork backgrounds routinely put a bright area right where the icon grid is. The
manta reference art does exactly that — **the white belly crosses the middle of the grid** and
swallows that row's labels and white-stroke icons. Redrawing the background is the real answer,
but for integrators who cannot, there are two prescriptions.

```toml
[desktop]
label_legibility = "shadow"   # none (default) · shadow · veil
```

| Value | What it does | When |
|---|---|---|
| `none` | Nothing | **Default.** Flat, gradient, or deliberately dark artwork |
| `shadow` | One dark copy behind labels and icons | **Photo and artwork backgrounds.** Saves the text without flattening the picture |
| `veil` | A translucent `scrim` sheet over the whole content | When the background is bright everywhere. Certain, but it lays the picture down |

Change it along with the background at runtime — `Shadow` when switching to a photo, `None` when
going back to a flat colour:

```rust
# use fairing::desktop::Fit;
# fn f(shell: &mut fairing::Shell, ctx: &egui::Context) -> fairing::Result<()> {
# let path = "/opt/acme/brand/bg.webp";
use fairing::desktop::LabelLegibility;

shell.load_wallpaper(ctx, path, Fit::Cover)?;
shell.desktop_mut().set_legibility(LabelLegibility::Shadow);
# Ok(()) }
```

> **`shadow` is not a cure-all either.** egui has no blur, so it approximates with a dark copy
> offset by one step; with extreme background contrast (a white icon on a white ground) it is
> still not enough. Then either fix the artwork or use `veil`. Either way, confirm it with the
> capture review in §9.2.

**Only three things actually sit on the background.** The status bar and the nav bar do **not** —
the shell draws the background into the `content` rect with those two removed, and the
status bar lays down its own opaque `Surface`. Effort spent keeping the top of the image clear is
wasted unless the screen uses `BarMode::Overlay`.

| What sits on the background | Where | What the background owes it |
|---|---|---|
| **The icon grid and its labels** | Most of the content's middle | Dark and low contrast (luminance 60 or below, for dark themes). Labels get no shadow and no plate by default (§2.2a) |
| **Dock labels** | The bottom `dock_height` of the content (96 du by default) | A bright band here kills the dock labels outright |
| **The page indicator** | `page_indicator_height` (24 du) above the dock. **Only with two or more pages** | Tiny dots; without contrast they vanish |

| Rule | Why |
|---|---|
| Push bright areas into the **top 25 % of the content** | There is slack above the first icon row. The bottom is taken by the dock |
| Contrast between the label colour (`on_surface`) and the background **at least 4.5:1** | The accessibility floor. Confirm it with the review in §9 |
| Never put text, logos or watermarks **inside the image** | They get cropped, rotated, and never translated |

If you are shipping a light theme too, make **a separate bright image**. Dark labels on a dark
background cannot be rescued by any veil.

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
shell.desktop_mut().set_wallpaper(Wallpaper::owned(texture, Fit::Cover));
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

### 2.4 When the background has to follow the theme — a painter, not a raster

If the machine switches dark and light, or the background has to follow `[theme.palette]`
overrides, a raster cannot do it. Use `ThemedPainter`.

```rust
# fn my_art(_painter: &egui::Painter, _rect: egui::Rect, _palette: &fairing::theme::Palette) {}
# fn set(shell: &mut fairing::Shell) {
use fairing::{ColorRole, Theme};
use fairing::desktop::Wallpaper;

shell.desktop_mut().set_wallpaper(Wallpaper::ThemedPainter(Box::new(
    |painter: &egui::Painter, rect: egui::Rect, theme: &Theme| {
        // Do not capture colours — **read them from `theme` every frame** so they follow.
        painter.rect_filled(rect, 0.0, theme.color(ColorRole::Background));
        my_art(painter, rect, &theme.palette);
    },
)));
# }
```

**Do not allocate inside the callback.** It runs every frame. Put heavy work in a `RefCell`
cache and re-bake only when the size or the colours change — `brand::Abyss` is the worked
example (one mesh, one draw call, zero rebakes at rest).

> **A naive cache key blows up on a theme switch.** `Shell::set_theme_dark` runs a 200 ms
> crossfade and writes **a different palette every frame** into `theme.palette` — a cache keyed
> on colour misses twelve frames in a row and re-bakes on each one. `Abyss` blocks this with two
> devices, and your painter needs both.
>
> | Device | What it does |
> |---|---|
> | `Abyss::tick_freeze` | When `theme.dark` flips, it **freezes** rebaking for the crossfade's duration and bakes exactly once on the frame it ends |
> | `BakeKey` quantisation | Rounds the rect to **4 device px** and `pixels_per_point` to **1/64**, so a resize does not bake once per pixel |

---

## 3. Type — half of branding, and the crate carries no fonts

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
| Coverage | Check the glyphs for your locales are actually in there. For Korean, **the 2,350 characters of KS X 1001 are not enough** — the full 11,172 precomposed syllables are recommended |
| Size | Subset before embedding. A full CJK font is 5–20 MB each |
| Priority | `FontPriority` orders the families. Product typeface for Latin, system font for CJK is a common combination |
| Licence | **A webfont licence does not cover embedded distribution.** Check that redistribution in a binary is allowed (OFL usually is) |

> **`set_fonts` is expensive.** It throws away the glyph atlas and the galley cache.
> `ShellBuilder::fonts` installs it **once at startup**; never call it in the frame loop.

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

> **Replacing the whole set is a bigger job than it looks.** Expect three things.
> 1. **The tests pin the names** — `builtin_names_resolve` ·
>    `the_generated_table_covers_the_v1_set` (which asserts on `NAMES.len()`) ·
>    `the_generated_table_has_no_strays`. All three break, and the constants in
>    `icons/builtin.rs` have to change with them.
> 2. **The attribution obligations change.** Today `assets/icons/` is a Lucide subset (ISC/MIT),
>    declared by `assets/icons/LICENSE-lucide` · `MAPPING.md` · `THIRD_PARTY.md`. Replacing the
>    set means replacing those three with your own provenance — **the first thing legal asks
>    about.**
> 3. It requires a fork. Which is why looking at the painter route in §4.2 first is usually
>    better.

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

Possible, not recommended — on a machine with no fixed size, a raster icon will be mushy at some
resolution. If you do it anyway, prepare them at **3× the display size** (72 px or more for a
24 pt icon) and pass `IconRef::Texture { id, tint }`. With `tint = true` they are multiplied by
the role colour, so author them as **white silhouettes**.

---

## 5. Logo marks — requirements

A mark does not fit the icon pipeline (two tones, and the silhouette is concave). There are two
routes.

### 5.1 Authoring one the way we did (a vector mesh)

The repository's manta was made this way, and **the procedure is written down in
`assets/brand/README.md`.** It needs two images.

| File | Spec | Used for |
|---|---|---|
| Silhouette | **1600×1000 PNG**, pure white ground, pure black solid silhouette, **no** shadow, gradient, texture or outline, 5 % margin | The outline coordinates |
| Two-tone | Same size, same drawing, exactly two colours (e.g. navy + white) | The interior split coordinates |

The procedure that extracts coordinates from those two (threshold → opening → sub-pixel contour
tracing → PCA axis split → least-squares cubic Bézier fit) is in `assets/brand/README.md`, under
the section on where the mark's coordinates came from. The repository's manta reaches an IoU of
0.976 against the reference art, with 1.1 px of error at 96 px.

**How the crate's manta is drawn**, for a mark of your own with concave edges and two tones —
epaint fills only convex shapes, so a raw `egui::Mesh` is the way. The crate's own helpers live
in a private module (`brand::ribbon`; only `Cubic` is public), so a painter of yours writes these
four steps on top of `egui::Mesh`:

| Step | What it does |
|---|---|
| Sample each curve to a fixed count | A cubic Bézier chain becomes `n` points **regardless of its knot count**. Two curves used as a pair must use the same `n` |
| A strip between two boundaries | Fill between them with quads. **The boundaries may be arbitrarily concave** — this is how you get around the convexity constraint |
| A feather | An alpha-0 band on edges that meet the background. It fakes the antialiasing a raw `Mesh` does not get from epaint |
| Fan fill | Only for pieces that are convex |

The way to get two tones is **not** to split along a centre line but to lay down one body ribbon
and **overlap** a lighter piece on top of it — abutting leaves a seam, overlapping does not.

### 5.2 Drawing it with a painter (the most freedom)

Register a closure that draws your mark, the same way as §4.2. Concave, multi-colour, gradients —
all fine. Keep the icon contract (colour comes in as an argument, size comes from `rect`) and it
fits anywhere an icon fits.

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

> **It is only free in cells that hold still.** `MantaCache`'s key includes the **absolute screen
> position and size** (because `Shape::mesh` takes an `Arc<Mesh>`, which cannot be moved). Press
> animations and page swipes change the rect every frame, so it re-bakes every frame while they
> run. A desktop cell at rest hits 100 %; while it moves it misses — and it is repainting anyway.

---

## 6. Chrome — status bar, nav bar, dock placement

These are **the two surfaces a manufacturer sees first and longest**. The status bar is on screen
100 % of the time. And none of the three needs an image.

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

For per-item captions, text sizes and padding, take `status_bar_mut().spec_mut(id)` in code —
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

### 6.3 Dock placement — the biggest change of impression for zero images

```rust
# fn f(shell: &mut fairing::Shell) {
use fairing::desktop::{Axis, DockPlacement};
use fairing::gesture::Edge;

shell.desktop_mut().set_dock_placement(DockPlacement::Edge(Edge::Left));      // a left rail
shell.desktop_mut().set_dock_placement(DockPlacement::Band {                  // a floating band
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
> implementation is still on the to-do list. For now the integrator
> draws it before the first frame. The image spec below is what it will want, and it will fit
> the API when that arrives.

| Item | Spec |
|---|---|
| Size | Landscape **1920×1080**, portrait **1080×1920** (whichever orientation you use) |
| Composition | **Leave the middle third empty** — the wordmark goes there, drawn in code |
| Format | PNG/WebP, sRGB |
| Text | Not inside the image |

The recommended route is an image for the background only, with the mark and wordmark drawn in
code. Then a change of locale, resolution or theme does not mean remaking the asset.

---

## 8. Overriding everything — what is ours, and how deep

This section is the page's **audit table**. It counts everything the crate leaves on screen and
says what replaces each one. It gets deeper as it goes down.

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
| 6 | 78 vector icons | `register_icon_painter` (recommended), or replacing the SVGs | §4 |
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

| Depth | How | What changes | Effort |
|---|---|---|---|
| Shallow | `[theme.palette]` / `.palettes(..)` | **The colours of all of them.** There is not one colour literal in the screen code | 1 min |
| Shallow | `.metrics_spec(..)` / `.scale_policy(..)` | Row heights, padding, corners, touch targets | 5 min |
| Middle | `SettingsConfig::without("settings.wifi")` | That screen **is not created** | 1 min |
| Middle | `SettingsConfig::only([..])` | Only the ones listed are created | 1 min |
| Middle | `.without_home_icon()` | Keeps the screen, drops the desktop icon | 1 min |
| Middle | `add_all_with(shell, cfg, list)` | **Which rows the settings list offers** — yours in, built-ins out, in your order | 5 min |
| Deep | `shell.add(screen("settings.wifi", ..))` | **That whole screen** (labels, layout, behaviour) | per screen |
| All | Never call `add_all` | None of them exist | 0 |
| Compile | Exclude `settings` under `default-features = false` | They leave the binary as well | — |

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
passes**, so a row of your own comes with a screen of your own — and the row the crate has not
written yet (`settings.credentials`) simply does not appear until someone registers it. A repeated id keeps its first row and warns; the selection is an id, so two rows for
one screen cannot both be right.

Some screens the crate does not create at all. **No backend, no registration** — the worst
outcome is a Wi-Fi entry on a machine with no Wi-Fi backend that says "not supported" once you
press it. To see the screens during development, use `SettingsConfig::ignoring_capabilities()`.

> **Do not forget the gates.** A screen id is a gate name (05 §3). A high `default_gate` locks
> the settings screens as a group — list the ones that should open in `[access.gates]`.
> Conversely, **power and accounts are right to leave locked**: mis-pressing shutdown, and
> editing accounts, should not be open to everyone on a machine.

> **Power does not turn itself off.** Even when `settings.power` gets its confirmation, the shell
> does not act; it emits `ShellEvent::PowerRequest`. The action happens when the integrator has
> finished cleaning up and calls `shell.commit_power(request)` — **not calling it is the
> refusal.** Machines routinely have to close a valve and flush logs before power goes, and the
> crate cannot do that cleanup for you.

### 8.4 Removing it from the build too

```toml
# The default features are ["mock", "overlay", "osk", "brand", "settings"].
# Drop what you want gone and **re-list the rest**, or it disappears quietly.
fairing = { git = "https://github.com/shim9610/fairing", default-features = false,
            features = ["mock", "overlay", "osk", "runner"] }
#            ↑ without brand (manta, abyss) and settings (the built-in screens)
```

| Feature | What goes when it is off | What stays |
|---|---|---|
| `brand` | The **drawing code** for the manta mark and the procedural abyss background | The palette presets, `AbyssTier` and the `Wallpaper` variants are outside the gate and stay |
| `settings` | The built-in settings **screens** | The value model (`SettingKey` · `SettingsView` · `keys`) is outside the gate — `cx.settings` is always there |
| `overlay` | The shade and quick-settings tiles | — |
| `osk` | The on-screen keyboard | — |

> **One side fails to boot, the other falls back quietly.** When you sweep the brand keys out of
> your TOML, a typo in `[theme] preset` is an **`Error::Config` at startup** (fail-closed —
> better than hunting for why the brand did not come on), while a typo in `[desktop] wallpaper`
> **warns and falls back to `background`** (fail-open — one image should not stop the shell).

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

Zero on all three and what is left on screen is **the palette, the metrics, the 78 icons and the
built-in settings screens** — and all of those are overridable per the table in §8.2.

---

## 9. Review — run these before you hand it over

When the branding is done, check these four. **Looking at it is not enough.**

### 9.1 The contrast floor

```rust
# fn main() {}
# fn my_dark_palette() -> fairing::theme::Palette { fairing::theme::Palette::dark() }
# fn contrast(_a: egui::Color32, _b: egui::Color32) -> f32 { 21.0 }
#[test]
fn my_palette_is_legible() {
    let p = my_dark_palette();
    // WCAG relative luminance. on_surface/surface >= 7.0, muted/surface >= 4.5,
    // on_primary/primary >= 4.5, primary/background >= 3.0
    assert!(contrast(p.on_surface, p.surface) >= 7.0);
    assert!(contrast(p.on_primary, p.primary) >= 4.5);
}
```

> `contrast` and `luminance` are **not public API** — they live in `#[cfg(test)] mod tests` in
> `theme/mod.rs`. To use the code above, **copy** those two functions into your own tests (they
> are the WCAG 2.x relative luminance formula, under ten lines).

The crate's `every_preset_meets_the_contrast_floor` (`theme/mod.rs`) is the worked example.
**White text almost always fails on a bright `primary`** — which is why the default dark
palette's `on_primary` is not white.

### 9.2 Label legibility over the background

**The headless harness cannot measure this.** The strongest thing `testing::Harness` gives you is
`frame_shapes()` (a shape list); there is no pixel readback. And for a raster background the
shell does not even hold the pixels (it has no decoder). So this check goes through **an actual
window, captured**.

- In this repository, `examples/demo.rs --tour <dir>` does exactly that — it opens a window on
  Xvfb with a software renderer and dumps frames as PNGs (the encoder uses std only).
- On your side, bring your own `eframe` app up the same way once, capture it, and measure the
  luminance of the pixels under the label rects (`Desktop::icon_rect(id)` gives you those).
- Two resolutions × dark/light, four combinations, is enough.

### 9.3 Zero fps at rest

If branding drags animation in with it, the machine never sleeps. Run N frames with no input and
check that no repaint was requested.

There are three switches that wake it, and **all three have names.**

| Switch | What it does |
|---|---|
| `[desktop.abyss] animate = true` | Moves the mantas every frame. Turning it on logs a warning at startup |
| `[shell] repaint = "continuous"` | For debugging. Draws every frame |
| A `ctx.request_repaint()` inside your own background callback | No warning at all. This one is the hardest to find |

### 9.4 Draw calls and allocations

If you wrote a procedural background, count the shapes per frame. One baked mesh is the target —
`brand::abyss`'s `abyss_emits_one_shape` test pins that contract.

---

## 10. Summary — the image checklist

The **minimum** a manufacturer prepares to get "our character":

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

For 1, 2 and 5 (rasters) you plug a decoder into `ShellBuilder::image_loader` (§1.1) — ten lines.
3 and 4 are **reference images** for authoring a mark and never ship on the machine, and the
SVGs in 6 are converted to vectors at build time, so neither needs a decoder.

> **A note on colour space.** Texture backgrounds pass the source through unchanged, using
> `Color32::WHITE` as the multiplicative identity. `Gradient`, and every colour blend in the
> crate, interpolates in **gamma (sRGB) space** (`lerp_to_gamma`). Pick a photo's colours to
> match `[theme.palette]` values by eye and they will drift where the two meet — if they are
> going to meet, render the boundary and check it.

---

## Appendix A. Every `[desktop.abyss]` key

These are for **using the default brand background but tuning it to taste**. They are only read
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
| `veil_field` | `0.18` | The icon grid band. **Dropping it to 0 and losing the labels is the integrator's choice** |
| `veil_bottom` | `0.16` | The bottom band (dock, indicator) |
| `animate` | `false` | **Breaks zero fps at rest.** Turning it on warns at startup |
| `bake_budget_ms` | `1.5` | Over budget drops the tier one step and warns |

## Appendix B. Every public hook that touches branding

| Hook | What it changes |
|---|---|
| `ShellBuilder::theme(Theme)` | Palette **plus metrics and motion tokens**. `[theme]` alone only changes colours |
| `ShellBuilder::palettes(dark, light)` | The two palettes the theme toggle moves between |
| `ShellBuilder::fonts(FontSet)` / `font(..)` | Type (§3) |
| `ShellBuilder::scale_policy(..)` | **Defaults to a bare 9 mm finger** and text read at hand-held distance. The finger moves touch targets, rows, bar thickness and icon cells together; `ScalePolicy::gloved()` is 13 mm |
| `ShellBuilder::metrics_spec(..)` | The shell's metrics, mixing mm, finger units and du |
| `ShellBuilder::physical_mm(w, h)` | The escape hatch for a panel whose EDID lies |
| `ShellBuilder::status_bar_painter` / `nav_bar_painter` | A whole bar (§6.2) |
| `ShellBuilder::slot_painter` | A whole **desktop cell** (`FnMut`; `SlotCx` brings press state and gates) |
| `ShellBuilder::image_loader` | The file → texture decoder (§1). With it installed, `wallpaper = "file:…"` works |
| `ShellBuilder::wallpaper(..)` | The background, from code. Beats `[desktop] wallpaper` |
| `ShellBuilder::preset(Preset)` | **A crate default set** (palette + background) in one line (§8.1) |
| `Shell::set_palettes` / `apply_palette` / `set_theme_dark` | Palettes at runtime |
| `Shell::set_motion` | Motion tokens |
| `Shell::register_icon` / `register_icon_painter` | A vector icon / a painter |
| `Shell::status_bar_mut()` · `nav_bar_mut()` | Height, background role colour, per-item spec |
| `Desktop::set_wallpaper` · `set_dock_placement` | Background · dock position |
| `Shell::load_wallpaper(ctx, path, fit)` | Swapping the background at runtime (needs a loader) |
| `settings::add_all(shell, &SettingsConfig)` | The built-in settings screens (§8.3) |
| `settings::add_all_with(shell, &SettingsConfig, list)` | The same, over a settings list of your own (§8.3) |
| `Shell::commit_power(request)` | Carrying out a power action. **Not calling it is the refusal** (§8.3) |
| `ChromePolicy` (per declaration) | Per-screen ground colour, bar modes, fullscreen |

`Wallpaper` has nine variants: `Solid(role)` · `Fixed(color)` · `Gradient{top,bottom}` (the mesh
is cached, zero allocations per frame) · `Texture` · `TextureFit` · **`Owned`** (holds the
texture handle, §1.4) · `Painter` · `ThemedPainter` · `Themed{dark,light}` (a pair the shell
picks from as the palette switches; `Wallpaper::themed(dark, light)`). It is `#[non_exhaustive]`, so a `match`
**outside** the crate needs a `_` arm.
