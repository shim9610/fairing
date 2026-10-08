# fairing brand art

The default art concept is a **manta ray**. The background, mark and palette the shell draws by
default all come out of it — but **all of it is a default, not a fixture**: the palette, the
background, the icons and the mark can each be changed, from overriding a single role colour to
replacing the drawing entirely with your own painter (`docs/guide/04-customization.md`).

## Files

### The mark

| File | Size | Format | Use |
|---|---|---|---|
| `manta-mark.webp` | 1254×1254 | WebP | The logo mark (the transparent-background original). For splashes, documents and READMEs |
| `manta-app-icon.webp` | 1254×1254 | WebP | The same mark as an app icon, on a rounded-square tile |
| `manta-mark-black.png` | 1600×1000 | PNG, flat black on white | **An authoring reference.** The mark coordinates in the code came from this |
| `manta-mark-two-tone.png` | 1600×1000 | PNG, navy `#041F3A` on white | **An authoring reference.** The back/belly split coordinates came from this |

### Backgrounds

| File | Size | Ratio | Use |
|---|---|---|---|
| `abyss-landscape.webp` | 1671×941 | 1.78 | Landscape, **dark palette** |
| `abyss-light-landscape.webp` | 1672×941 | 1.78 | Landscape, **light palette** — pair it with the line above through `Wallpaper::themed` |
| `abyss-portrait.webp` | 940×1672 | 0.56 | Portrait |
| `abyss-ultrawide.webp` | 2560×640 | 4.00 | Bar-shaped instrument panels |
| `abyss-square.webp` | 1440×1440 | 1.00 | Square HMIs |
| `abyss-4x3.webp` | 1600×1200 | 1.33 | Older industrial panels |
| `abyss-splash-1920x1080.webp` | 1920×1080 | 1.78 | Splash / boot (one hero manta, the middle left empty) |
| `abyss-splash-1080x1920.webp` | 1080×1920 | 0.56 | Splash / boot (portrait, a canyon) |

`abyss-light-landscape.webp` is the same scene in a pale key, for the palette's light form. A
texture wallpaper follows nothing on its own, so a shell that switches at dusk would otherwise keep
the dark painting under a pale UI — `Wallpaper::themed(dark, light)` is what carries both. It is
quality-92 WebP at 67 KB, half the dark one: the picture is mostly flat pale tone, and 92 rather
than 85 because a smooth pale gradient is exactly where WebP bands (measured: max per-pixel error
10 at q92 against 20 at q85).

The last five are lossless WebP, around 1 MB each. If repository size becomes a problem, they
re-encode at quality 95 to about a seventh of that (PSNR 47–51 dB, indistinguishable by eye).

## None of this is compiled into the crate

`fairing` takes no image decoder dependency. So the raster
originals are **integrator assets that the crate never reads**. The background the shell draws by
default is a **procedural** one that reproduces the same art direction with egui shapes alone
(`brand::abyss`), and the mark is a ribbon mesh rather than a raster (`brand::manta`).

To actually use the raster originals, decoding is yours: decode with whichever image crate you
use, make an `egui::TextureId`, and hand it over. **Use `TextureFit` so it does not squash on a
panel with a different ratio** — `Texture` pins UV to `0..1` and stretches.

```rust
use fairing::desktop::{Fit, Wallpaper};

// Decoding and texture upload go through your own dependency.
let (id, size) = upload_my_texture(ctx, include_bytes!("../assets/brand/abyss-4x3.webp"));
shell.desktop_mut().set_wallpaper(Wallpaper::TextureFit {
    id,
    source_size: size, // the source size your decoder reported (egui::vec2)
    fit: Fit::Cover,   // fill the short side and crop the long one
});
```

## Where the mark's coordinates came from

The authored coordinates in `crates/fairing/src/brand/manta.rs` were not placed by hand; they
were traced from `manta-mark-black.png`. To reproduce them:

1. Threshold at 128 to binarise, then an opening of radius 14 to take the thin tail off and leave
   the body.
2. Sub-pixel contour tracing (marching squares), then split into an upper and a lower boundary at
   the two ends of the PCA principal axis.
3. Scale the bounding box of the **whole** mark (tail included) uniformly into a 32×20 grid with
   a 2 % margin, centred.
4. Least-squares fit a cubic Bézier chain to each boundary (four reparameterisation passes). The
   back/belly boundary uses the same method on the two-colour boundary in
   `manta-mark-two-tone.png`.

The fit error is **at most 0.36** grid units (1.1 px on a 96 px mark), and rasterising the result
at 32 samples gives an IoU of **0.976** against the reference art. The silhouettes of the two
reference PNGs agree with each other at IoU 0.974, so they are the same drawing.

## Provenance and licence

Every file in this directory was **made by this project with a generative image model** and
contains no third-party material. Unlike the Lucide subset in `assets/icons/` (ISC/MIT), there
are no third-party licence obligations, so they are not listed in the assets table of
`THIRD_PARTY.md`.

They are distributed under the MIT licence, the same as the repository's code (`LICENSE`). Protecting the name and the logo as trademarks would mean excluding brand marks
from the code licence, as is common; the repository owner has chosen to keep them under the code
licence.
