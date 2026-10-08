//! A file wallpaper (guide 09 §1) — **the crate does not decode images.**
//!
//! What it checks: whether `[desktop] wallpaper = "file:…"` really does come alive through a loader the integrator
//! wired in, whether **the device still starts** when there is no loader or the loader fails, and whether the
//! wallpaper holds on to the texture.
//!
//! Most of the tests run on a fake loader (it makes pixels up without a decoder) — the crate not leaning on a
//! decoder is itself what is being checked. Only the last one uses the dev-dependency `image` to see whether **the
//! code written down in the docs really runs**.
//!
//! The rules are the other integration tests': written as `fairing::Result<()>` with no `panic!` and no `unwrap`.

use fairing::desktop::{Fit, Wallpaper};
use fairing::testing::{single_level_access, Harness};
use fairing::{Error, Shell};
use std::path::{Path, PathBuf};

/// A loader that **only reads** the file and makes the pixels up. One 4 × 2 green.
///
/// It shows outright that decoding is work outside the crate — the shell gets only a `TextureHandle`.
fn stub_loader(ctx: &egui::Context, path: &Path) -> fairing::Result<egui::TextureHandle> {
    let _bytes = std::fs::read(path).map_err(|e| Error::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    let image = egui::ColorImage::new([4, 2], vec![egui::Color32::from_rgb(0x2E, 0x9E, 0x5B); 8]);
    Ok(ctx.load_texture("stub", image, egui::TextureOptions::LINEAR))
}

/// It writes any old bytes into the test's temporary directory. The fake loader does not look at the content.
fn touch(name: &str) -> fairing::Result<PathBuf> {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, b"pretend this is a picture").map_err(|e| Error::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(path)
}

/// It builds a shell with `wallpaper = "file:…"` and a loader.
fn shell_with(wallpaper: &str, fit: &str, loader: bool) -> fairing::Result<Harness> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    wallpaper.clone_into(&mut config.desktop.wallpaper);
    fit.clone_into(&mut config.desktop.wallpaper_fit);
    Harness::from_builder(move |ctx| {
        let mut builder = Shell::builder(config);
        if loader {
            builder = builder.image_loader(stub_loader);
        }
        builder.build(ctx)
    })
}

/// One config line plus one loader changes the wallpaper — a different one per shop **without touching the code**.
#[test]
fn a_file_wallpaper_loads_through_the_integrator_loader() -> fairing::Result<()> {
    let path = touch("wallpaper-cover.bin")?;
    let h = shell_with(&format!("file:{}", path.display()), "cover", true)?;
    let Wallpaper::Owned {
        source_size, fit, ..
    } = h.shell.desktop().wallpaper()
    else {
        return Err(Error::Config(format!(
            "the wallpaper is not Owned: {:?}",
            h.shell.desktop().wallpaper()
        )));
    };
    assert_eq!(
        *source_size,
        egui::vec2(4.0, 2.0),
        "it caches the source size"
    );
    assert_eq!(*fit, Fit::Cover);
    Ok(())
}

/// The fit comes from the config too.
#[test]
fn the_fit_comes_from_config() -> fairing::Result<()> {
    let path = touch("wallpaper-contain.bin")?;
    let h = shell_with(&format!("file:{}", path.display()), "contain", true)?;
    assert!(
        matches!(h.shell.desktop().wallpaper(), Wallpaper::Owned { fit, .. } if *fit == Fit::Contain),
        "{:?}",
        h.shell.desktop().wallpaper()
    );
    Ok(())
}

/// An unknown fit name falls back to cover — a typo does not stop the shell.
#[test]
fn an_unknown_fit_falls_back_to_cover() -> fairing::Result<()> {
    let path = touch("wallpaper-badfit.bin")?;
    let h = shell_with(&format!("file:{}", path.display()), "crop", true)?;
    assert!(
        matches!(h.shell.desktop().wallpaper(), Wallpaper::Owned { fit, .. } if *fit == Fit::Cover),
        "{:?}",
        h.shell.desktop().wallpaper()
    );
    Ok(())
}

/// **With no loader wired in** it warns and falls back. The crate does not decode it instead.
#[test]
fn without_a_loader_it_falls_back_to_background() -> fairing::Result<()> {
    let path = touch("wallpaper-noloader.bin")?;
    let mut h = shell_with(&format!("file:{}", path.display()), "cover", false)?;
    h.frames(2);
    assert_eq!(
        format!("{:?}", h.shell.desktop().wallpaper()),
        "Solid(Background)"
    );
    Ok(())
}

/// **One wallpaper must not stop a device from starting.** The shell comes up even where the loader fails.
#[test]
fn a_failing_loader_still_lets_the_shell_start() -> fairing::Result<()> {
    let mut h = shell_with("file:/nonexistent/fairing/nope.png", "cover", true)?;
    h.frames(2);
    assert_eq!(
        format!("{:?}", h.shell.desktop().wallpaper()),
        "Solid(Background)"
    );
    Ok(())
}

/// The wallpaper **holds on to** the texture — it is not released even where the integrator holds no handle.
/// (Dropping the handle gave a black screen, which was `TextureFit`'s trap.)
#[test]
fn an_owned_wallpaper_keeps_the_texture_alive() -> fairing::Result<()> {
    let path = touch("wallpaper-owned.bin")?;
    let mut h = shell_with(&format!("file:{}", path.display()), "cover", true)?;
    h.frames(3);
    let Wallpaper::Owned { texture, .. } = h.shell.desktop().wallpaper() else {
        return Err(Error::Config("it is not Owned".to_owned()));
    };
    assert_eq!(
        texture.size(),
        [4, 2],
        "it is alive after running frames too"
    );
    Ok(())
}

/// Swapping it out at run time — the path a shopkeeper choosing a photo on a settings screen takes.
#[test]
fn the_wallpaper_can_be_swapped_at_runtime() -> fairing::Result<()> {
    let path = touch("wallpaper-runtime.bin")?;
    let mut h = shell_with("background", "cover", true)?;
    h.frames(2);
    assert_eq!(
        format!("{:?}", h.shell.desktop().wallpaper()),
        "Solid(Background)"
    );
    let ctx = h.ctx.clone();
    h.shell.load_wallpaper(&ctx, &path, Fit::Contain)?;
    assert!(
        matches!(h.shell.desktop().wallpaper(), Wallpaper::Owned { fit, .. } if *fit == Fit::Contain),
        "{:?}",
        h.shell.desktop().wallpaper()
    );
    Ok(())
}

/// Calling the run-time swap with no loader is an error — it is not quietly ignored.
#[test]
fn swapping_without_a_loader_is_an_error() -> fairing::Result<()> {
    let mut h = shell_with("background", "cover", false)?;
    let ctx = h.ctx.clone();
    let err = h.shell.load_wallpaper(&ctx, "/tmp/x.png", Fit::Cover).err();
    assert!(matches!(err, Some(Error::Config(_))), "{err:?}");
    Ok(())
}

/// **The loader written down in the docs really runs.** The dev-dependency `image` unpacks a real PNG — it is code
/// an integrator will copy out, so merely compiling is not enough.
#[test]
fn the_documented_loader_decodes_a_real_png() -> fairing::Result<()> {
    // A 2 × 1 PNG (red, half-transparent blue). The smallest file, written by hand with no encoder dependency.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0xF4,
        0x22, 0x7F, 0x8A, 0x00, 0x00, 0x00, 0x0E, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x42, 0x0D, 0x00, 0x0F, 0x7A, 0x03, 0x7E, 0x6A, 0x81, 0x31, 0xE1, 0x00,
        0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("wallpaper-real.png");
    std::fs::write(&path, PNG).map_err(|e| Error::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;

    // ── The loader as it stands in guide 09 §1 ──
    let loader = |ctx: &egui::Context, path: &Path| -> fairing::Result<egui::TextureHandle> {
        let bytes = std::fs::read(path).map_err(|e| Error::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        let rgba = image::load_from_memory(&bytes)
            .map_err(|e| Error::Image(e.to_string()))?
            .to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        Ok(ctx.load_texture(path.to_string_lossy(), image, egui::TextureOptions::LINEAR))
    };

    let mut config = single_level_access();
    config.motion.reduce = true;
    config.desktop.wallpaper = format!("file:{}", path.display());
    let h =
        Harness::from_builder(move |ctx| Shell::builder(config).image_loader(loader).build(ctx))?;
    assert!(
        matches!(
            h.shell.desktop().wallpaper(),
            Wallpaper::Owned { source_size, .. } if *source_size == egui::vec2(2.0, 1.0)
        ),
        "{:?}",
        h.shell.desktop().wallpaper()
    );
    Ok(())
}
