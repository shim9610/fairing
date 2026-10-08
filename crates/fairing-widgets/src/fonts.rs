//! Installing fonts.
//!
//! egui's `default_fonts` (the Ubuntu family plus emoji) has no CJK glyphs, so Korean comes out
//! as □. The crate **does not embed** a font file — they are several MB, and how far to subset
//! is an integrator judgement that differs per device, not a value to bake into the code.
//!
//! What it gives you instead is the way to install one. An integrator picks one of three:
//!
//! 1. **Bake it into the binary** — make a `&'static [u8]` with `include_bytes!` and use
//!    [`FontSource::from_static`]. No deployment accident from a missing file.
//!    ```no_run
//!    # use fairing_widgets::fonts::FontSource;
//!    # let bytes: &'static [u8] = b"";  // in practice include_bytes!("NotoSansKR.ttf")
//!    let font = FontSource::from_static("ko", bytes);
//!    ```
//! 2. **Read it from a file** — [`FontSource::from_path`]. For shipping the font inside a snap
//!    or an image.
//! 3. **Find one already on the device** — [`find_system_font`], [`korean_font`]. Ubuntu Core
//!    images often carry `fonts-noto-cjk`.
//!
//! Either way the file IO happens **once, at startup**. Reading a font in the frame loop, or
//! calling `set_fonts` again, rebuilds the entire glyph atlas.
//!
//! ```no_run
//! # fn main() -> fairing_widgets::Result<()> {
//! use fairing_widgets::fonts::{FontSet, FontSource};
//!
//! let mut fonts = FontSet::new();
//! if let Some(path) = fairing_widgets::fonts::korean_font() {
//!     fonts.push(FontSource::from_path("ko", &path)?);
//! }
//! # Ok(())
//! # }
//! ```

use crate::{Error, Result};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// The recursive scan's depth limit. A font directory is usually two levels,
/// `<root>/<family>/<file>` — this is set generously and then cut, so a symlink loop cannot stop
/// startup.
const MAX_SCAN_DEPTH: usize = 4;

/// The font file extensions (compared lowercase).
const FONT_EXTENSIONS: [&str; 4] = ["ttf", "otf", "ttc", "otc"];

/// The egui font families to attach a font to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontFamilies {
    /// Body text (`FontFamily::Proportional`) only.
    Proportional,
    /// Monospace (`FontFamily::Monospace`) only.
    Monospace,
    /// Both — the default. A Korean fallback is usually needed in both.
    #[default]
    Both,
    /// The **bold** face, [`STRONG_FAMILY`].
    ///
    /// Screen titles and list-row titles are drawn in it (`Theme::strong`). The family always
    /// exists: with nothing registered here it holds the same faces as `Proportional`, so a shell
    /// with no bold renders exactly as it did and one with a bold gains a weight axis. Register
    /// with [`FontPriority::First`] — a bold added as a fallback would only be reached by
    /// characters the regular face is missing, which is the opposite of the point.
    Strong,
    /// The **display** face, [`DISPLAY_FAMILY`] — the one a brand is recognised by.
    ///
    /// [`Theme::display`](crate::theme::Theme::display) draws with it, and a screen title is the
    /// only thing in the crate that asks for it. Like [`Self::Strong`] it is **always bound** —
    /// but it is seeded from the *strong* faces rather than the regular ones, and only after every
    /// source has been applied. So a shell that registers no display face draws its titles in
    /// exactly the bold it drew them in before, and one that registers a serif gets the serif with
    /// nothing else moving.
    ///
    /// A display face is the one place a typeface may be chosen for its character rather than its
    /// coverage, which is why it is a slot of its own and not a second `Proportional`. It is also
    /// why **nothing but a title uses it**: a face picked for a headline may carry no Hangul, no
    /// tabular figures and no `℃`, and a body row cannot afford any of those to be missing.
    ///
    /// Register with [`FontPriority::First`], for the same reason [`Self::Strong`] does.
    Display,
}

/// The egui family name the bold face is bound to. It is always bound; see
/// [`FontFamilies::Strong`].
pub const STRONG_FAMILY: &str = "fairing-strong";

/// **Whether this crate's named font families are live on `ctx` yet.**
///
/// `Context::set_fonts` does not apply in the pass it is called from — egui swaps the new
/// definitions in at the start of the next one. A [`FontId`](egui::FontId) naming a family that is
/// not yet live is not a fallback, it is a panic inside epaint, so a shell built from *inside* a
/// frame must not draw in that frame. `Shell` asks this for you; ask it yourself if you drive the
/// widgets without one.
#[must_use]
pub fn families_are_live(ctx: &egui::Context) -> bool {
    ctx.fonts(|f| {
        let bound = &f.definitions().families;
        bound.contains_key(&strong_family()) && bound.contains_key(&display_family())
    })
}

/// The [`egui::FontFamily`] for bold text.
#[must_use]
pub fn strong_family() -> egui::FontFamily {
    egui::FontFamily::Name(STRONG_FAMILY.into())
}

/// The egui family name the display face is bound to. It is always bound; see
/// [`FontFamilies::Display`].
pub const DISPLAY_FAMILY: &str = "fairing-display";

/// The [`egui::FontFamily`] for display text.
#[must_use]
pub fn display_family() -> egui::FontFamily {
    egui::FontFamily::Name(DISPLAY_FAMILY.into())
}

/// The position within a family's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontPriority {
    /// At the front — this font becomes **the default**. Latin text is drawn with it too.
    First,
    /// At the back — only characters the fonts before it **do not have** are drawn with it. The
    /// default, and where a Korean fallback usually goes (leaving Latin in egui's own font
    /// spaces better).
    #[default]
    Fallback,
}

/// One font to install.
///
/// `bytes` is the contents of a `.ttf`, `.otf`, `.ttc` or `.otc` file as-is. For a collection
/// (`.ttc` / `.otc`), pick the face with [`FontSource::index`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSource {
    /// The name this font goes by inside egui. Registering the same name twice, the later one wins.
    pub name: Cow<'static, str>,
    /// The font file's contents.
    pub bytes: Cow<'static, [u8]>,
    /// The face index to use from a collection file. `0` for a single font.
    pub index: u32,
    /// Which families to attach it to.
    pub families: FontFamilies,
    /// Its position within the family.
    pub priority: FontPriority,
}

impl FontSource {
    /// A font baked into the binary (`include_bytes!`).
    #[must_use]
    pub fn from_static(name: impl Into<Cow<'static, str>>, bytes: &'static [u8]) -> Self {
        Self {
            name: name.into(),
            bytes: Cow::Borrowed(bytes),
            index: 0,
            families: FontFamilies::default(),
            priority: FontPriority::default(),
        }
    }

    /// From bytes you already read.
    #[must_use]
    pub fn from_bytes(name: impl Into<Cow<'static, str>>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            bytes: Cow::Owned(bytes),
            index: 0,
            families: FontFamilies::default(),
            priority: FontPriority::default(),
        }
    }

    /// Read from a file. Call it **once, at startup** — there is no file IO in the frame loop.
    ///
    /// # Errors
    /// [`Error::Io`] if the file cannot be read, [`Error::Config`] if the contents do not look
    /// like a font.
    pub fn from_path(name: impl Into<Cow<'static, str>>, path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|err| Error::Io {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;
        if !looks_like_font(&bytes) {
            return Err(Error::Config(format!(
                "{} does not look like a font file (no sfnt, ttcf or OTTO signature)",
                path.display()
            )));
        }
        Ok(Self::from_bytes(name, bytes))
    }

    /// Set the collection's face index (`.ttc` / `.otc`).
    #[must_use]
    pub fn index(mut self, index: u32) -> Self {
        self.index = index;
        self
    }

    /// **Why this source cannot be handed to epaint**, or `None` when nothing is provably wrong.
    ///
    /// epaint does not report a bad font, it panics:
    /// `Error parsing "name" TTF/OTF font file: ...`, from an
    /// `unwrap_or_else(|| panic!(..))` around a `Result` it throws away. A library user who passed
    /// the bytes cannot catch that, so what can be checked here is checked here.
    ///
    /// Two things are decidable without a parser: whether the file begins with a font signature at
    /// all, and whether [`index`](Self::index) names a face the file actually holds. Both are the
    /// ordinary mistakes — the wrong file, an empty `Vec`, a `.ttc` index copied from another
    /// machine's font.
    #[must_use]
    pub fn reject_reason(&self) -> Option<String> {
        let name = &self.name;
        let Some(faces) = face_count(&self.bytes) else {
            return Some(format!(
                "font {name:?} does not begin with a font signature (sfnt, ttcf, OTTO or true) —                  {} byte(s)",
                self.bytes.len()
            ));
        };
        (self.index >= faces).then(|| {
            format!(
                "font {name:?} asks for face index {} but the file holds {faces}",
                self.index
            )
        })
    }

    /// Set which families to attach it to.
    #[must_use]
    pub fn families(mut self, families: FontFamilies) -> Self {
        self.families = families;
        self
    }

    /// Set its position within the family.
    #[must_use]
    pub fn priority(mut self, priority: FontPriority) -> Self {
        self.priority = priority;
        self
    }
}

/// A set of fonts to install. They are installed in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontSet {
    sources: Vec<FontSource>,
}

impl FontSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one.
    pub fn push(&mut self, source: FontSource) {
        self.sources.push(source);
    }

    /// Itself with one added (for chaining).
    #[must_use]
    pub fn with(mut self, source: FontSource) -> Self {
        self.push(source);
        self
    }

    /// **Every source that will be left out, and why** — see [`FontSource::reject_reason`].
    ///
    /// [`Self::install`] drops these rather than handing them to epaint, because epaint's answer to
    /// a font it cannot read is `panic!` and a shell must not die over one optional fallback face.
    /// Dropping is the right shape for that — the remaining faces, and egui's own defaults beneath
    /// them, still draw everything — but it must not be silent, so `install` also logs each one and
    /// this is here to be checked programmatically at startup.
    ///
    /// # What this does **not** promise
    ///
    /// It is a header check, not a parse: a file that carries a font signature and a plausible face
    /// count but is truncated or corrupt inside still reaches epaint, and epaint still panics on it.
    /// Closing that would mean parsing the font ourselves, which needs a font-parser dependency —
    /// a decision for the dependency-approval process, not something to take quietly.
    #[must_use]
    pub fn rejected(&self) -> Vec<String> {
        self.sources
            .iter()
            .filter_map(FontSource::reject_reason)
            .collect()
    }

    /// Whether it is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// How many.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// The fonts to install.
    #[must_use]
    pub fn sources(&self) -> &[FontSource] {
        &self.sources
    }

    /// Whether a **bold** face is registered — a readable source attached to
    /// [`FontFamilies::Strong`]. Without one the strong family holds the regular faces, so
    /// `Theme::strong` (screen titles, list-row titles) draws in the regular weight.
    #[must_use]
    pub fn has_bold(&self) -> bool {
        self.sources.iter().any(|source| {
            source.families == FontFamilies::Strong && source.reject_reason().is_none()
        })
    }

    /// Install into egui.
    ///
    /// It runs even for an empty set, because binding [`FontFamilies::Strong`] and
    /// [`FontFamilies::Display`] is part of the job and epaint panics on an unbound family. An
    /// empty set therefore installs egui's own fonts plus those two families pointing at the same
    /// faces — the same render as before, with the weight and display axes in place for anyone who
    /// fills them.
    ///
    /// `set_fonts` rebuilds the glyph atlas and is expensive, so call it **once, at startup**.
    /// Give it through `fairing_widgets::ShellBuilder::fonts` and the shell calls it once for you.
    ///
    /// **It does not take effect in the pass it is called from.** `Context::set_fonts` stores the
    /// definitions and egui swaps them in "at the start of the next pass", so until then the live
    /// set is still the old one — which has no [`STRONG_FAMILY`] and no [`DISPLAY_FAMILY`] in it.
    /// Ask for one of those in that window and epaint does not fall back, it panics:
    /// `FontFamily::Name("fairing-strong") is not bound to any fonts`. [`families_are_live`] is the
    /// question to ask before drawing.
    pub fn install(&self, ctx: &egui::Context) {
        ctx.set_fonts(self.definitions());
    }

    /// What [`Self::install`] hands egui.
    ///
    /// Separated from the install so the seeding rules above can be tested for what they are —
    /// which family holds which faces — without a `Context`, a glyph atlas or a real font file.
    fn definitions(&self) -> egui::FontDefinitions {
        let mut defs = egui::FontDefinitions::default();
        // **The strong family is seeded with the regular faces and always bound.** epaint panics
        // outright on a `FontFamily::Name` with no fonts, so "bold if there is one" cannot be
        // decided at the call site — it is decided here, once. With no bold registered the family
        // holds exactly what `Proportional` holds and every `Theme::strong` draw is unchanged.
        let regular = defs
            .families
            .get(&egui::FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();
        defs.families.insert(strong_family(), regular);
        for source in &self.sources {
            // **A source that cannot be read is left out, not handed over.** epaint panics on a
            // font it cannot parse, and a panic is not something the integrator who passed the
            // bytes can handle — see `FontSet::rejected`.
            if let Some(reason) = source.reject_reason() {
                log::warn!("[fonts] {reason}; leaving it out");
                continue;
            }
            let data = egui::FontData {
                font: source.bytes.clone(),
                index: source.index,
                tweak: egui::FontTweak::default(),
            };
            defs.font_data
                .insert(source.name.as_ref().to_owned(), std::sync::Arc::new(data));
            for family in source.families.list() {
                let entry = defs.families.entry(family).or_default();
                match source.priority {
                    FontPriority::First => entry.insert(0, source.name.as_ref().to_owned()),
                    FontPriority::Fallback => entry.push(source.name.as_ref().to_owned()),
                }
            }
        }
        // **The display family is seeded last, from the strong faces.** Unlike the strong family
        // above it cannot be seeded before the loop: seeded from `Proportional` it would hold the
        // *regular* face, and a shell that registers a bold but no display face would find its
        // titles losing their weight. Seeded here, from whatever `Strong` ended up holding, a
        // registered display face wins and no display face at all leaves a title drawing exactly
        // the bold it drew before. As with `Strong`, the family must exist either way — epaint
        // panics outright on a `FontFamily::Name` bound to nothing.
        if !defs.families.contains_key(&display_family()) {
            let strong = defs
                .families
                .get(&strong_family())
                .cloned()
                .unwrap_or_default();
            defs.families.insert(display_family(), strong);
        }
        defs
    }
}

impl FontFamilies {
    /// The egui families to attach to.
    fn list(self) -> Vec<egui::FontFamily> {
        match self {
            Self::Proportional => vec![egui::FontFamily::Proportional],
            Self::Monospace => vec![egui::FontFamily::Monospace],
            Self::Both => vec![egui::FontFamily::Proportional, egui::FontFamily::Monospace],
            Self::Strong => vec![strong_family()],
            Self::Display => vec![display_family()],
        }
    }
}

/// Check the sfnt signature. Installing a file that is not a font panics egui, so it is filtered
/// out first. `0x00010000` (TrueType), `OTTO` (CFF), `ttcf` (a collection) and `true` (old
/// macOS) are accepted.
#[must_use]
fn looks_like_font(bytes: &[u8]) -> bool {
    matches!(
        bytes.first_chunk::<4>(),
        Some(b"\x00\x01\x00\x00" | b"OTTO" | b"ttcf" | b"true")
    )
}

/// **How many faces the file holds**, or `None` when it is not a font file at all.
///
/// A collection (`ttcf`) states the count as a big-endian `u32` at offset 8, right after the tag and
/// the version. Every other signature is a single face. This is header arithmetic, not parsing —
/// see [`FontSet::rejected`] on what that can and cannot promise.
fn face_count(bytes: &[u8]) -> Option<u32> {
    match bytes.first_chunk::<4>()? {
        b"ttcf" => bytes
            .get(8..12)
            .and_then(|n| n.first_chunk::<4>().copied())
            .map(u32::from_be_bytes),
        b"\x00\x01\x00\x00" | b"OTTO" | b"true" => Some(1),
        _ => None,
    }
}

/// The directories to look for system fonts in (earlier ones win).
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/share/fonts"));
        dirs.push(home.join(".fonts"));
    }
    dirs.push(PathBuf::from("/usr/local/share/fonts"));
    dirs.push(PathBuf::from("/usr/share/fonts"));
    dirs
}

/// Find a font file whose name contains one of `needles` (case-insensitively).
///
/// `needles` is **in priority order** — if a file is found for the first name it is returned,
/// otherwise it moves to the next. Call it once at startup (it walks directories).
///
/// ```no_run
/// let path = fairing_widgets::fonts::find_system_font(&["NotoSansCJK", "NanumGothic"]);
/// ```
#[must_use]
pub fn find_system_font(needles: &[&str]) -> Option<PathBuf> {
    let dirs = font_dirs();
    let mut found = Vec::new();
    for dir in &dirs {
        collect_fonts(dir, 0, &mut found);
    }
    for needle in needles {
        let needle = needle.to_lowercase();
        if let Some(path) = found.iter().find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_lowercase().contains(&needle))
        }) {
            return Some(path.clone());
        }
    }
    None
}

/// Find a common font with Hangul glyphs. `None` if there is none — then the integrator has to
/// ship a font themselves.
///
/// The order is Korean-specific and CJK-unified first, then the Chinese-market unified fonts.
/// The last two (the `WenQuanYi` family) carry Hangul as well and are kept only as a last resort.
#[must_use]
pub fn korean_font() -> Option<PathBuf> {
    find_system_font(&[
        "NotoSansKR",
        "NotoSansCJKkr",
        "NotoSansCJK",
        "NotoSerifCJK",
        "NanumGothic",
        "NanumBarunGothic",
        "Pretendard",
        "SpoqaHanSans",
        "malgun",
        "UnDotum",
        "baekmuk",
        "wqy-zenhei",
        "wqy-microhei",
    ])
}

/// Find a common **bold** sans face for [`FontFamilies::Strong`]. `None` if there is none.
///
/// A device UI leans on weight for hierarchy — a screen title against a row title, a row title
/// against its subtitle — and egui's own fonts ship one weight only (`Ubuntu-Light`), which is why
/// a shell built on the defaults comes out flat however well its sizes and colours are chosen.
/// This looks for a bold in the same places [`korean_font`] looks, so an image that carries a font
/// stack gets the axis for free and one that carries nothing degrades to the regular face.
///
/// Korean first, because a bold Latin face with no Hangul would leave Korean titles falling back
/// to the regular weight and the two would not match.
#[must_use]
pub fn strong_font() -> Option<PathBuf> {
    find_system_font(&[
        "NotoSansKR-Bold",
        "NotoSansCJKkr-Bold",
        "NotoSansCJK-Bold",
        "NanumGothicBold",
        "NanumBarunGothicBold",
        "Pretendard-Bold",
        "malgunbd",
        "NotoSans-Bold",
        "DejaVuSans-Bold",
        "LiberationSans-Bold",
        "FreeSansBold",
        "Ubuntu-Bold",
        "Arimo-Bold",
    ])
}

/// Find a common **serif** face for [`FontFamilies::Display`]. `None` if there is none.
///
/// A display face is the half of a brand a palette cannot carry: the two European-restaurant
/// kiosks this crate was measured against are recognisable by their serif headline before any
/// colour is read. This looks in the same places [`korean_font`] and [`strong_font`] look, so an
/// image that already carries a font stack gets one for free.
///
/// Korean first, for the reason [`strong_font`] gives: a serif Latin face with no Hangul would
/// leave a Korean title falling back to the sans and the two would not match. A serif is the only
/// thing looked for — a display face is a choice, and guessing anything more opinionated than
/// "the system's serif" on the integrator's behalf would be picking their brand for them.
#[must_use]
pub fn display_font() -> Option<PathBuf> {
    find_system_font(&[
        "NotoSerifKR-SemiBold",
        "NotoSerifKR-Bold",
        "NotoSerifCJKkr-Bold",
        "NanumMyeongjo-Bold",
        "NanumMyeongjoBold",
        "batang",
        "NotoSerif-SemiBold",
        "NotoSerif-Bold",
        "DejaVuSerif-Bold",
        "LiberationSerif-Bold",
        "FreeSerifBold",
        "Tinos-Bold",
    ])
}

/// Walk a directory collecting font file paths (down to [`MAX_SCAN_DEPTH`]).
fn collect_fonts(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // `file_type` does not follow a symlink — it does not fall into a link loop.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_fonts(&path, depth + 1, out);
        } else if file_type.is_file() && has_font_extension(&path) {
            out.push(path);
        }
    }
}

/// Whether the extension is a font's.
fn has_font_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            let ext = ext.to_lowercase();
            FONT_EXTENSIONS.contains(&ext.as_str())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every named family is bound, even when nothing is registered.**
    ///
    /// epaint does not fall back on a `FontFamily::Name` it has no fonts for — it panics — so a
    /// slot that is only *sometimes* bound is a crash waiting for the first shell that fills
    /// neither. `Strong` and `Display` are both seeded for that reason, and this is the test a
    /// third slot added later cannot forget.
    #[test]
    fn every_named_family_is_bound_even_with_nothing_registered() {
        let defs = FontSet::default().definitions();
        for family in [strong_family(), display_family()] {
            let bound = defs.families.get(&family).is_some_and(|l| !l.is_empty());
            assert!(bound, "{family:?} is bound to no fonts");
        }
    }

    /// **With no display face registered, a title draws exactly the bold it drew before.**
    ///
    /// `Display` is seeded from the *strong* faces, and after the sources rather than before them.
    /// Seeded the way `Strong` itself is — from `Proportional`, before the loop — a shell that
    /// registered a bold and no display face would silently lose the weight on every screen title,
    /// because the display slot would be holding the regular face.
    #[test]
    fn the_display_family_falls_back_to_the_strong_faces() {
        let bold = FontSource::from_bytes("a-bold", b"\x00\x01\x00\x00".to_vec())
            .families(FontFamilies::Strong)
            .priority(FontPriority::First);
        let defs = FontSet::default().with(bold).definitions();
        let strong = defs.families.get(&strong_family());
        assert_eq!(strong, defs.families.get(&display_family()));
        assert_eq!(
            strong.and_then(|l| l.first()).map(String::as_str),
            Some("a-bold"),
            "the registered bold did not reach the strong slot"
        );
    }

    /// **A registered display face wins over the strong seeding**, which is the whole point of the
    /// slot: the seeding is a floor, not a ceiling.
    #[test]
    fn a_registered_display_face_is_not_overwritten_by_the_seeding() {
        let serif = FontSource::from_bytes("a-serif", b"\x00\x01\x00\x00".to_vec())
            .families(FontFamilies::Display)
            .priority(FontPriority::First);
        let defs = FontSet::default().with(serif).definitions();
        assert_eq!(
            defs.families
                .get(&display_family())
                .and_then(|l| l.first())
                .map(String::as_str),
            Some("a-serif")
        );
        assert_ne!(
            defs.families.get(&display_family()),
            defs.families.get(&strong_family()),
            "the seeding overwrote a display face the integrator registered"
        );
    }

    #[test]
    fn sfnt_signatures_are_recognised() {
        assert!(looks_like_font(b"\x00\x01\x00\x00rest"));
        assert!(looks_like_font(b"OTTOrest"));
        assert!(looks_like_font(b"ttcfrest"));
        assert!(!looks_like_font(b"<svg"));
        assert!(!looks_like_font(b"\x00\x01"), "too short to be a font");
    }

    #[test]
    fn extensions_are_case_insensitive() {
        assert!(has_font_extension(Path::new("/a/B.TTF")));
        assert!(has_font_extension(Path::new("/a/b.ttc")));
        assert!(!has_font_extension(Path::new("/a/b.png")));
        assert!(!has_font_extension(Path::new("/a/b")));
    }

    #[test]
    fn a_non_font_file_is_rejected_with_a_config_error() -> Result<()> {
        let dir = std::env::temp_dir().join("fairing-fonts-test");
        std::fs::create_dir_all(&dir).map_err(|err| Error::Io {
            path: dir.display().to_string(),
            message: err.to_string(),
        })?;
        // **The name carries the process id.** With no dev-dependency the directory is a
        // plain `temp_dir()` one, so two test binaries on the same machine — `cargo test` twice
        // over, or two CI jobs on one runner — met on the same file, and the first to finish
        // removed it out from under the other. Measured: 7 failures in 80 concurrent runs.
        let path = dir.join(format!("not-a-font-{}.ttf", std::process::id()));
        std::fs::write(&path, b"hello").map_err(|err| Error::Io {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;
        let err = FontSource::from_path("x", &path);
        assert!(
            matches!(err, Err(Error::Config(_))),
            "a file that is not a font is a Config error: {err:?}"
        );
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let err = FontSource::from_path("x", "/definitely/not/here.ttf");
        assert!(matches!(err, Err(Error::Io { .. })), "{err:?}");
    }

    #[test]
    fn builders_set_every_field() {
        let source = FontSource::from_static("ko", b"\x00\x01\x00\x00")
            .index(2)
            .families(FontFamilies::Monospace)
            .priority(FontPriority::First);
        assert_eq!(source.index, 2);
        assert_eq!(source.families, FontFamilies::Monospace);
        assert_eq!(source.priority, FontPriority::First);
        assert_eq!(source.name, "ko");
    }

    #[test]
    fn a_font_set_keeps_insertion_order() {
        let set = FontSet::new()
            .with(FontSource::from_static("a", b""))
            .with(FontSource::from_static("b", b""));
        assert_eq!(set.len(), 2);
        let names: Vec<&str> = set.sources().iter().map(|s| s.name.as_ref()).collect();
        assert_eq!(names, ["a", "b"]);
        assert!(!set.is_empty());
    }

    #[test]
    fn families_expand_to_egui_families() {
        assert_eq!(FontFamilies::Both.list().len(), 2);
        assert_eq!(
            FontFamilies::Proportional.list(),
            vec![egui::FontFamily::Proportional]
        );
    }
}

#[cfg(test)]
mod reject_tests {
    use super::{face_count, FontSet, FontSource};

    /// A real font's header, enough for the checks: `ttcf` with two faces.
    fn ttc(faces: u32) -> Vec<u8> {
        let mut v = b"ttcf".to_vec();
        v.extend_from_slice(&[0, 1, 0, 0]); // version
        v.extend_from_slice(&faces.to_be_bytes());
        v
    }

    #[test]
    fn a_collection_states_its_face_count_and_a_single_face_file_holds_one() {
        assert_eq!(face_count(&ttc(7)), Some(7));
        assert_eq!(face_count(b"\x00\x01\x00\x00rest"), Some(1));
        assert_eq!(face_count(b"OTTO....."), Some(1));
        assert_eq!(face_count(b"not a font"), None, "no signature, no count");
        assert_eq!(face_count(b""), None, "and an empty file is not one either");
    }

    /// **The panic this exists to prevent**: epaint answers a font it cannot parse with
    /// `panic!`, so a source that cannot be read must never reach it.
    #[test]
    fn a_source_that_cannot_be_read_is_rejected_rather_than_handed_over() {
        let junk = FontSource::from_bytes("junk", b"this is not a font".to_vec());
        assert!(
            junk.reject_reason().is_some(),
            "bytes with no font signature have to be caught here"
        );

        let past_the_end = FontSource::from_bytes("coll", ttc(2)).index(2);
        assert!(
            past_the_end.reject_reason().is_some(),
            "face index 2 of a 2-face collection is out of range — epaint's parse would panic"
        );
        assert!(
            FontSource::from_bytes("coll", ttc(2))
                .index(1)
                .reject_reason()
                .is_none(),
            "index 1 of two faces is the last valid one and must be let through"
        );
    }

    /// Rejected sources are dropped from what egui is given, and stay reportable.
    #[test]
    fn the_definitions_leave_out_what_was_rejected() {
        let set = FontSet::new()
            .with(FontSource::from_bytes("junk", b"nope".to_vec()))
            .with(FontSource::from_bytes("coll", ttc(1)).index(9));
        assert_eq!(set.rejected().len(), 2, "both, with a reason each");

        let defs = set.definitions();
        for name in ["junk", "coll"] {
            assert!(
                !defs.font_data.contains_key(name),
                "{name} reached egui's definitions, where epaint would panic on it"
            );
        }
        // And the families this crate names are still bound: an unbound family is the other way
        // epaint panics on fonts.
        assert!(
            defs.families.contains_key(&super::strong_family())
                && defs.families.contains_key(&super::display_family()),
            "rejecting every source must not leave a named family unbound"
        );
    }
}
