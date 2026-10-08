//! Vector icon definitions, references, styles and painters.
//!
//! An icon is not a raster image but a small vector path expressed as a sequence of [`Seg`]s.
//! `cargo xtask icons` regenerates [`generated::ICONS`] from the sources in
//! `assets/icons/*.svg`. This module defines the representation types ([`Seg`], [`IconDef`] —
//! **an xtask generation contract; do not change them**), the lookup function, and the runtime
//! side: the reference [`IconRef`], the style [`IconStyle`], the painter [`paint`], the polyline
//! cache [`IconCache`] and the parametric icons in [`parametric`].

pub mod builtin;
mod crossfade;
mod generated;
mod painter;
pub mod parametric;

pub use crossfade::ParamFade;
pub use generated::ICONS;
pub use painter::{flatten, paint, Flattened, IconCache, Polyline};

use crate::theme::{ColorRole, Theme};
use egui::{Color32, Painter, Rect};

/// A disabled icon's alpha ("`enabled = false` means muted").
const DISABLED_ALPHA: f32 = 0.6;

/// A [`CustomIconId`] with its top bit set is a painter callback; otherwise it is a vector icon.
const PAINTER_BIT: u32 = 0x8000_0000;

/// The UV rect covering a whole texture.
const UV_FULL: Rect = Rect {
    min: egui::Pos2 { x: 0.0, y: 0.0 },
    max: egui::Pos2 { x: 1.0, y: 1.0 },
};

/// One piece (segment) of an icon's path.
///
/// The coordinates are on the **24×24 design grid** (a grid drawn assuming a
/// stroke of 2.0). Converting to real pixels is the painter's job ([`paint`]), which fits it
/// uniformly into the target `Rect`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    /// Move-to: move the current position and start a new subpath.
    M(f32, f32),
    /// Line-to.
    L(f32, f32),
    /// A quadratic Bézier: control point `(cx, cy)`, end point `(x, y)`.
    Q(f32, f32, f32, f32),
    /// A cubic Bézier: control points `(c1x, c1y)` and `(c2x, c2y)`, end point `(x, y)`.
    C(f32, f32, f32, f32, f32, f32),
    /// Close the current subpath.
    Z,
}

/// A named icon definition: its segments and whether it is filled.
#[derive(Debug, Clone, Copy)]
pub struct IconDef {
    /// The name identifying it (`"wifi"`, `"battery"`, …).
    pub name: &'static str,
    /// The path segments it is made of.
    pub segs: &'static [Seg],
    /// `true` draws it filled; `false` draws the stroke only.
    pub fill: bool,
}

/// Find a built-in icon by name. `None` if there is none.
///
/// [`ICONS`] is emitted by the generator (`cargo xtask icons`) **in ascending name order**, so
/// this is a binary search (it is called once per icon on the render path). The `icons_table_is_sorted`
/// test pins that ordering guarantee.
#[must_use]
pub fn find(name: &str) -> Option<&'static IconDef> {
    let index = ICONS.binary_search_by(|icon| icon.name.cmp(name)).ok()?;
    ICONS.get(index)
}

/// The id of an icon the integrator registered ([`IconSet::register`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CustomIconId(pub u32);

/// An icon reference.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum IconRef {
    /// A built-in icon name (`"wifi"`, `"shield"`, …). Use the constants in [`builtin`].
    Builtin(&'static str),
    /// An icon registered with [`IconSet::register`] or [`IconSet::register_painter`].
    Custom(CustomIconId),
    /// A texture the integrator uploaded. With `tint`, dyed the icon colour.
    Texture {
        /// The texture id.
        id: egui::TextureId,
        /// Whether to tint it.
        tint: bool,
    },
    /// A font glyph or emoji (within the installed fonts).
    Glyph(String),
}

/// An icon's colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IconColor {
    /// A palette role.
    Role(ColorRole),
    /// A fixed colour.
    Fixed(Color32),
}

impl IconColor {
    /// Parse a config string (`"on_surface"` or `"#RRGGBB"`).
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if let Some(hex) = text.strip_prefix('#') {
            // Both `#RRGGBB` and **`#RRGGBBAA`** are taken. Without an alpha there is no way to change
            // [`ColorRole::Scrim`] or [`ColorRole::Pressed`] from the config — and being translucent is
            // those two roles' reason to exist. Caught while putting the brand palette onto the kiosk.
            if hex.len() != 6 && hex.len() != 8 {
                return None;
            }
            let value = u32::from_str_radix(hex, 16).ok()?;
            let (rgb, a) = if hex.len() == 8 {
                (value >> 8, u8::try_from(value & 0xff).ok()?)
            } else {
                (value, 0xff)
            };
            let r = u8::try_from((rgb >> 16) & 0xff).ok()?;
            let g = u8::try_from((rgb >> 8) & 0xff).ok()?;
            let b = u8::try_from(rgb & 0xff).ok()?;
            // **It is read unmultiplied.** A `#RRGGBBAA` a person writes states the colour and the
            // transparency separately; it is not a premultiplied value.
            return Some(Self::Fixed(Color32::from_rgba_unmultiplied(r, g, b, a)));
        }
        ColorRole::parse(text).map(Self::Role)
    }

    /// The actual colour.
    #[must_use]
    pub fn resolve(&self, theme: &Theme) -> Color32 {
        match self {
            Self::Role(role) => theme.color(*role),
            Self::Fixed(color) => *color,
        }
    }
}

/// An icon's style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconStyle {
    /// The pixel size (the side of the square).
    pub size: f32,
    /// The colour.
    pub color: IconColor,
    /// A stroke override in design-grid units (2.0 by default).
    pub stroke: Option<f32>,
    /// `false` → the muted colour plus alpha.
    pub enabled: bool,
}

impl IconStyle {
    /// The given size, in the `OnSurface` role.
    #[must_use]
    pub fn sized(size: f32) -> Self {
        Self {
            size,
            color: IconColor::Role(ColorRole::OnSurface),
            stroke: None,
            enabled: true,
        }
    }

    /// Set the colour.
    #[must_use]
    pub fn color(mut self, color: IconColor) -> Self {
        self.color = color;
        self
    }

    /// Enabled or disabled.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The real colour with the theme applied (muted and translucent when disabled).
    #[must_use]
    pub fn resolve_color(&self, theme: &Theme) -> Color32 {
        if self.enabled {
            self.color.resolve(theme)
        } else {
            theme.color(ColorRole::Muted).gamma_multiply(DISABLED_ALPHA)
        }
    }

    /// The stroke width in pixels: the grid stroke × (size / 24).
    #[must_use]
    pub fn stroke_px(&self) -> f32 {
        self.stroke.unwrap_or(2.0) * self.size / 24.0
    }

    /// Convert to the argument the parametric icons ([`parametric`]) take.
    ///
    /// Derived from one [`IconStyle`] so that it uses the same colour and stroke rules as the
    /// static icons. The dim colour is [`ColorRole::Muted`] and the danger colour is
    /// [`ColorRole::Danger`]; disabled, all three lie down to the same muted translucency as
    /// [`Self::resolve_color`].
    #[must_use]
    pub fn param_style(&self, theme: &Theme) -> parametric::ParamStyle {
        let color = self.resolve_color(theme);
        if self.enabled {
            parametric::ParamStyle {
                color,
                muted: theme.color(ColorRole::Muted),
                danger: theme.color(ColorRole::Danger),
                stroke_px: self.stroke_px(),
            }
        } else {
            parametric::ParamStyle {
                color,
                muted: color,
                danger: color,
                stroke_px: self.stroke_px(),
            }
        }
    }
}

impl Default for IconStyle {
    fn default() -> Self {
        Self::sized(24.0)
    }
}

/// A custom painter callback (`register_icon_painter`).
pub type IconPainter = Box<dyn Fn(&Painter, Rect, &IconStyle, Color32)>;

/// The icon set the shell owns: the built-in table, the integrator's registrations, and the polyline cache.
#[derive(Default)]
pub struct IconSet {
    /// The per-size polyline cache.
    pub cache: IconCache,
    custom: Vec<IconDef>,
    painters: Vec<IconPainter>,
    /// Built-in icon names already warned about. The same typo is not reported again every frame.
    warned: std::collections::BTreeSet<String>,
}

impl IconSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a vector icon.
    ///
    /// A filled icon's (`def.fill`) subpaths have to be convex — `epaint`'s fan fill draws a
    /// concave polygon wrong (see the [`paint`] docs). A name colliding with a
    /// built-in icon mixes up the [`IconCache`] keys, so it warns.
    pub fn register(&mut self, def: IconDef) -> CustomIconId {
        if find(def.name).is_some() {
            log::warn!(
                "register_icon: `{}` collides with a built-in icon name. Pick another one so the caches do not mix",
                def.name
            );
        }
        self.custom.push(def);
        CustomIconId(u32::try_from(self.custom.len() - 1).unwrap_or(u32::MAX))
    }

    /// Register an arbitrary drawing callback. The id space continues from the vector icons (with the top bit set).
    pub fn register_painter(&mut self, painter: IconPainter) -> CustomIconId {
        self.painters.push(painter);
        CustomIconId(PAINTER_BIT | u32::try_from(self.painters.len() - 1).unwrap_or(0))
    }

    /// Resolve a reference and draw it. An unknown name draws nothing and returns `false`.
    ///
    /// Parametric icons take their state as an argument, so an [`IconRef`] cannot point at them.
    /// Whoever knows the state — a status item, say — builds the arguments with
    /// [`IconStyle::param_style`] and calls the [`parametric`] function directly.
    pub fn paint(
        &mut self,
        painter: &Painter,
        rect: Rect,
        icon: &IconRef,
        style: &IconStyle,
        theme: &Theme,
    ) -> bool {
        let color = style.resolve_color(theme);
        match icon {
            IconRef::Builtin(name) => {
                let Some(def) = find(name) else {
                    // It warns **once**. This is called every frame, so left as it is the log pours out,
                    // and passing quietly leaves an integrator unable to find the typo — an icon not
                    // showing with nothing said about it is the worst of the two.
                    if self.warned.insert((*name).to_owned()) {
                        log::warn!("no built-in icon named \"{name}\" - check the spelling");
                    }
                    return false;
                };
                paint(
                    painter,
                    rect,
                    def,
                    color,
                    style.stroke_px(),
                    &mut self.cache,
                );
                true
            }
            IconRef::Custom(id) => {
                if id.0 & PAINTER_BIT != 0 {
                    let index = (id.0 & !PAINTER_BIT) as usize;
                    match self.painters.get(index) {
                        Some(callback) => {
                            callback(painter, rect, style, color);
                            true
                        }
                        None => false,
                    }
                } else {
                    match self.custom.get(id.0 as usize) {
                        Some(def) => {
                            paint(
                                painter,
                                rect,
                                def,
                                color,
                                style.stroke_px(),
                                &mut self.cache,
                            );
                            true
                        }
                        None => false,
                    }
                }
            }
            IconRef::Texture { id, tint } => {
                // A tint takes the icon's colour, otherwise the original's. Disabled flattens both cases
                // by the same alpha. The `WHITE` here is not a role colour but **the
                // multiplicative identity** — it passes the texture's original colours straight
                // through.
                let tint = if *tint {
                    color
                } else if style.enabled {
                    Color32::WHITE
                } else {
                    Color32::WHITE.gamma_multiply(DISABLED_ALPHA)
                };
                painter.image(*id, rect, UV_FULL, tint);
                true
            }
            IconRef::Glyph(text) => {
                if text.is_empty() {
                    return false;
                }
                // A glyph is fitted to the smaller of `rect` and the style's size. Building the galley is
                // egui's font cache's job.
                let size = rect.width().min(rect.height()).min(style.size);
                if size <= 0.0 {
                    return false;
                }
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(size * 0.85),
                    color,
                );
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{find, IconColor, IconDef, IconSet, IconStyle, Seg, ICONS};
    use crate::theme::{ColorRole, Theme};

    const BOX: IconDef = IconDef {
        name: "test-box",
        segs: &[
            Seg::M(4.0, 4.0),
            Seg::L(20.0, 4.0),
            Seg::L(20.0, 20.0),
            Seg::L(4.0, 20.0),
            Seg::Z,
        ],
        fill: false,
    };

    #[test]
    fn icon_names_are_unique() {
        for (index, icon) in ICONS.iter().enumerate() {
            let duplicate = ICONS
                .iter()
                .skip(index + 1)
                .any(|other| other.name == icon.name);
            assert!(!duplicate, "the name is taken twice: {}", icon.name);
        }
    }

    /// [`find`] is a binary search, so the generated table's ordering is a contract — if the
    /// generator changes the order, it is caught here (rather than quietly returning `None`).
    #[test]
    fn icons_table_is_sorted() {
        for pair in ICONS.windows(2) {
            let [a, b] = pair else { continue };
            assert!(
                a.name < b.name,
                "the generated file is not in ascending name order: {} then {}",
                a.name,
                b.name
            );
        }
    }

    #[test]
    fn find_matches_the_generated_table() {
        for icon in ICONS {
            let found = find(icon.name).map(|def| def.name);
            assert_eq!(found, Some(icon.name));
        }
        assert!(find("no-such-icon").is_none());
    }

    /// The generated coordinates stay inside the 24-grid. Outside it means the source SVG's
    /// `viewBox` or the `fit_to_grid` scaling is wrong, and the painter draws outside its `rect`.
    #[test]
    fn generated_coordinates_stay_in_the_design_grid() {
        for icon in ICONS {
            for seg in icon.segs {
                let coords: &[f32] = match seg {
                    Seg::M(x, y) | Seg::L(x, y) => &[*x, *y],
                    Seg::Q(cx, cy, x, y) => &[*cx, *cy, *x, *y],
                    Seg::C(ax, ay, bx, by, x, y) => &[*ax, *ay, *bx, *by, *x, *y],
                    Seg::Z => &[],
                };
                for value in coords {
                    assert!(
                        (0.0..=24.0).contains(value),
                        "{}'s coordinate {value} is outside the 24-grid",
                        icon.name
                    );
                }
            }
        }
    }

    #[test]
    fn icon_color_parses_roles_and_hex() {
        assert_eq!(
            IconColor::parse("on_surface"),
            Some(IconColor::Role(ColorRole::OnSurface))
        );
        assert!(matches!(
            IconColor::parse("#112233"),
            Some(IconColor::Fixed(_))
        ));
        assert!(IconColor::parse("#123").is_none());
        assert!(IconColor::parse("nope").is_none());
    }

    #[test]
    fn stroke_scales_with_size() {
        assert!((IconStyle::sized(24.0).stroke_px() - 2.0).abs() < 1e-6);
        assert!((IconStyle::sized(48.0).stroke_px() - 4.0).abs() < 1e-6);
        let thin = IconStyle {
            stroke: Some(1.5),
            ..IconStyle::sized(24.0)
        };
        assert!((thin.stroke_px() - 1.5).abs() < 1e-6);
    }

    #[test]
    fn disabled_style_mutes_every_parametric_role() {
        let theme = Theme::dark();
        let style = IconStyle::sized(24.0).enabled(false);
        let param = style.param_style(&theme);
        assert_eq!(param.color, param.muted);
        assert_eq!(param.color, param.danger);
        let enabled = IconStyle::sized(24.0).param_style(&theme);
        assert_eq!(enabled.danger, theme.color(ColorRole::Danger));
        assert!((enabled.stroke_px - 2.0).abs() < 1e-6);
    }

    #[test]
    fn custom_ids_do_not_collide_with_painter_ids() {
        let mut set = IconSet::new();
        let vector = set.register(BOX);
        let painter = set.register_painter(Box::new(|_, _, _, _| {}));
        assert_ne!(vector, painter);
        assert_eq!(vector.0 & super::PAINTER_BIT, 0);
        assert_ne!(painter.0 & super::PAINTER_BIT, 0);
    }
}

#[cfg(test)]
mod color_parse_tests {
    use super::{ColorRole, IconColor};

    /// **`#RRGGBBAA` is accepted.** While only six digits were, `Scrim` and `Pressed` could not
    /// be touched from config at all — being translucent is the reason those two exist.
    #[test]
    fn hex_parses_with_and_without_alpha() {
        let Some(IconColor::Fixed(opaque)) = IconColor::parse("#4C8DFF") else {
            unreachable!("the 6-digit form was not read")
        };
        assert_eq!(opaque.to_srgba_unmultiplied(), [0x4C, 0x8D, 0xFF, 0xFF]);

        let Some(IconColor::Fixed(faded)) = IconColor::parse("#0A070480") else {
            unreachable!("the 8-digit form was not read")
        };
        // A value a person wrote is **not premultiplied** — unpacked, it should come back almost exactly.
        // `Color32` stores premultiplied, so a low alpha loses ±1 per channel.
        let [r, g, b, a] = faded.to_srgba_unmultiplied();
        assert_eq!(a, 0x80, "the alpha has to be exact");
        for (got, want) in [(r, 0x0A_u8), (g, 0x07), (b, 0x04)] {
            let diff = i32::from(got) - i32::from(want);
            assert!(diff.abs() <= 2, "{got} vs {want}");
        }
    }

    /// A length that is neither 6 nor 8 is read as a role name rather than a colour, and failing that, `None`.
    #[test]
    fn other_lengths_are_not_colors() {
        assert!(IconColor::parse("#ABC").is_none());
        assert!(IconColor::parse("#ABCDEFA").is_none());
        assert!(matches!(
            IconColor::parse("primary"),
            Some(IconColor::Role(ColorRole::Primary))
        ));
        assert!(IconColor::parse("nope").is_none());
    }
}
