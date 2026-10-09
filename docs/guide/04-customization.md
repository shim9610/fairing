# 04. Compile-time customization

## 0. Principle

The look of a fairing shell is **decided in code, at build time.** There is no runtime theme
editor and no drag-a-widget-here mode, and there will not be one. A machine's screen has to look
the way it looks when it ships, and the person who decides that is the integrator, not the end
user.

So customization splits into two layers. The config file (`fairing.toml`) only handles
**placement and assignment** — which item in which slot, which gate at which level. Changing the
drawing itself is always code. You do not have to fork or patch the crate: you hook into
`Shell::builder(..)`.

| Hook | What it changes | Section |
|---|---|---|
| `.theme(Theme)` | The whole palette, metrics and motion tokens | §1 |
| `.metrics_spec(..)` · `.component_spec(..)` · `.control_spec(..)` · `.elevation_spec(..)` | Sizes in physical units: the shell's metrics, what each widget draws with, the lengths controls share, how containers show height | §1.2 · §1.6 · §1.7 |
| `[theme.palette]` | Colours only (on top of dark/light) | §2 |
| `.status_bar_painter(..)` · `.nav_bar_painter(..)` | One whole bar | §3 |
| `.slot_painter(..)` | Each desktop cell | §4 |
| `Wallpaper::Painter` | The desktop background | §5 |
| Cargo features · config | Turning whole parts off | §6 |
| `[motion]` · `ShellHandle::set_motion` | How animation feels | §8 |
| `.translations(..)` · `[shell] locale` · `ui.locale` | The words on screen and their language | §9 |
| `.nav_bar_layout(..)` · `.status_bar_layout(..)` · `.toast_layout(..)` · `.toast_painter(..)` · `.heads_up_layout(..)` · `.heads_up_painter(..)` · `.osk_key_layout(..)` · `.osk_key_painter(..)` · `.shade_tile_layout(..)` · `.shade_tile_painter(..)` · `.shade_panel_painter(..)` · `.lock_screen_painter(..)` · `.unlock_prompt_painter(..)` · `.recents_card_painter(..)` · `.recents_ground_painter(..)` · `.widget_painters(..)` · `.gesture_handle_painter(..)` | Where the nav bar's items, the status bar's items, the toasts, the banner, the keys and the shade's tiles go; how toasts, the banner, the keys, the shade's tiles and its panel, the lock screen, the unlock prompt, the recent screens, the widgets and the gesture handles are drawn | §10 |

Narrower extensions — adding status bar or nav bar items, adding tiles, registering custom icons
— are already open through the declaration API. See [03 Chrome](03-chrome.md).

---

## 1. Injecting a theme

A `Theme` is four things: palette, metrics, motion tokens, and whether it is dark.

```rust
use fairing::{Shell, ShellConfig, Theme};

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .theme(Theme::light())
        .build(ctx)
}
```

Injecting a theme makes `[shell] theme` (dark|light) and `[theme.palette]` **ignored**, and logs
a warning at startup. Ignoring them silently would only ever show up as "why is the colour not
changing".

The single source of truth for bar heights is `theme.metrics.status_bar_height` ·
`nav_bar_height`; `build()` feeds those into the status bar and the nav bar. Without an injected
theme, the shell resolves both — and `status_icon_size` — from its metrics spec every frame:
7 mm, a finger plus 8 du and 3.6 mm by default, or what `[status_bar] height` ·
`[status_bar] icon_size` · `[nav_bar] height` pin ([07 §2](07-config-reference.md)). An injected
theme is used as it stands: its metrics win, and those three keys are ignored with a warning.

### 1.1 The palette roles

Colours are used by role, not by name. `Theme::color(ColorRole::Surface)` is the lookup, and the
config file uses the snake_case name.

| Role | Config name | dark | light | Where it is used |
|---|---|---|---|---|
| `Background` | `background` | `#0b0b0d` | `#e1e1e8` | Desktop base background, egui `extreme_bg_color` |
| `Surface` | `surface` | `#141416` | `#ececf1` | **The page.** Status bar, nav bar, shade panel, screen background, toast and heads-up cards, OSK key bed, dock band (×0.6) |
| `SurfaceVariant` | `surface_variant` | `#242428` | `#ffffff` | **What sits on the page, one step lighter than it in both modes** (iOS and Windows 11 both lift the card off the page rather than recessing it). Card ground (`layout::group`), tile ground, OSK keys, `BigButton::Normal`, egui `faint_bg_color` · `widgets.inactive` |
| `OnSurface` | `on_surface` | `#f0f0f3` | `#1a1a1c` | Default text and icon colour on a surface, default status bar icon, egui `override_text_color` |
| `Muted` | `muted` | `#93939d` | `#63636b` | Dimmed text, disabled icons (alpha 0.6), the empty arc and empty remainder of parametric icons, list subtitles |
| `Primary` | `primary` | `#2878a9` | `#106aa2` | Tiles and switches that are on, `BigButton::Primary`, pressed widget outline, the emergency gesture ring, the default user-level dot |
| `OnPrimary` | `on_primary` | white | white | Text on a Primary or Danger ground, switch knobs, notification badge numbers |
| `Danger` | `danger` | `#ff6b5e` | `#c0322a` | Battery at 20 % or below, error notifications, `BigButton::Danger` |
| `Warning` | `warning` | `#e9a83b` | `#8f5e00` | Warning notifications, warning badges |
| `Success` | `success` | `#4fd07a` | `#1a7f37` | Success notifications |
| `Focus` | `focus` | `#549ac5` | `#2784be` | Focus outline — egui `selection.stroke`, so a focused `TextEdit` wears it. Deliberately not the same value as `Primary`: a focus ring drawn over a pressed control has to be visible against the press |
| `Scrim` | `scrim` | black α128 | black α100 | The dim behind the shade; its alpha is how dark it gets at the full pull |
| `Outline` | `outline` | `#303036` | `#d2d2da` | 1 px borders, list separators, slider tracks |
| `ControlEdge` | `control_edge` | `#70707b` | `#85858e` | The boundary that makes a control a control — a switch's track, a checkbox, a radio, a segmented strip. Gated at 3:1 against the surface, where `Outline` is decoration |
| `Pressed` | `pressed` | white α28 | black α24 | Press tint (nav items, icons, keys, buttons, list rows) |
| `Shadow` | `shadow` | white α14 | black α14 | The cast shadow of a floating container in light mode, its rim in dark mode. Figure-ground help, not a boundary |
| `ShadeSurface` | `shade_surface` | = `surface` | = `surface` | The pull-down shade's face: the curtain, the floating card and the tile pucks on it. Unset, it follows `surface`; set it to colour the shade alone. Text on it stays `on_surface`, so keep the two readable together |

Switching dark ↔ light interpolates every role per channel over 200 ms. So a new palette
that fills in one or two roles and leaves the rest looks wrong mid-transition. Fill the rest with
`..Palette::dark()`.

### 1.2 Metrics tokens

`Metrics` is `Copy`, so change a few fields with `..Metrics::default()`. Units are du
(density-independent units, egui's points) unless stated otherwise. There are 50 fields today;
the table covers the ones you are likely to change, and the API docs describe every one.
Twenty-two of them, plus the type scale and the slider thumb, are resolved
from `MetricsSpec` in physical units each frame rather than being fixed

| Field | Default | What it changes |
|---|---|---|
| `status_bar_height` | `32.0` | Status bar height (the single source of truth for bar height) |
| `nav_bar_height` | one finger + 8 du, `56.0` floor | Nav bar height |
| `touch_target` | one finger, `48.0` floor | Minimum touch target — one finger (9 mm by default, about 57 du on a 160 dpi panel). Nav item minimum width, the basis for slider and switch heights, egui `interact_size` |
| `icon_cell` | two fingers, `96.0` floor | The side of a desktop cell. What the automatic grid counts columns and rows from |
| `icon_size` | one finger, `48.0` floor | Desktop icon size. Toast, heads-up and tile icons are half of it |
| `status_icon_size` | `18.0` | The status bar's icons and the icons in the shade's notification rows. `[status_bar] icon_size` pins it from config |
| `row_height` | one finger, `48.0` floor | `ListRow` height (the larger of this and `widget_height`). **One touch target** by default: the padding is inside the row, and a row grows only when its own text needs more |
| `corner_radius` | `12.0` | **Container** corner radius — cards, tiles, toasts, heads-up, OSK keys, press tint, egui `window_corner_radius` and `menu_corner_radius` |
| `control_radius` | `6.0` | **Control** corner radius — every egui widget (buttons, fields, combos). Half the container's, so a button inside a card does not read as another card. Set it equal to `corner_radius` for the old uniform look |
| `edge_px` | `24.0` | Edge zone width. When a bar is visible, the whole bar becomes the zone |
| `dock_height` | `96.0` | Dock band height |
| `screen_inset` | `12.0` | Padding inside the rect a screen receives. `ChromePolicy::inset` overrides it per screen |
| `desktop_label_size` | `13.0` | Desktop label font size |
| `desktop_label_line` | `1.3` | Label line-height multiplier (an approximation, for space calculations) |
| `desktop_label_gap` | `6.0` | Gap between icon and label |
| `desktop_badge_size` | `10.0` | Desktop badge font size |
| `desktop_lock_size` | `16.0` | Padlock badge side (tile padlocks use the same value) |
| `desktop_icon_stroke` | `2.0` | Desktop icon stroke (in 24-grid units — 4 px on a 48 px icon) |
| `page_indicator_height` | `24.0` | Page indicator strip height (it only takes space with two or more pages) |
| `page_indicator_step` | `16.0` | Spacing between indicator dots |
| `page_indicator_dot_off` | `3.0` | Radius of an unselected dot |
| `page_indicator_dot_on` | `4.0` | Radius of the selected dot |
| `page_indicator_hit_ratio` | `0.5` | The dots' tap strip height = `touch_target × this`. The dots sit right under the grid, so a full-size target would steal taps from the last icon row — raise it to 1.0 when the grid has room |
| `desktop_icon_ratio` | `0.5` | Upper bound on how much of a cell the icon takes |
| `desktop_press_inset` | `4.0` | How far the press tint sits inside the cell |
| `desktop_label_pad` | `8.0` | How far the label width pulls back from the cell (also its floor) |
| `desktop_lock_ring_pad` | `4.0` | How much larger the padlock badge's **outer circle** is than the padlock. Raising `desktop_lock_size` grows the glyph too; this only grows the ring |
| `desktop_badge_dot_r` | `5.0` | Radius of the numberless notification dot |
| `desktop_content_min_pad` | `2.0` | Minimum gap between the icon+label block and the top/bottom of the cell |
| `nav_icon_size` | `24.0` | Nav bar icon side |
| `nav_item_max_span` | `3.0` | Nav item maximum width multiplier (`touch_target` × 3.0) |
| `nav_indicator_length` | `108.0` | The gesture style's home indicator length — never more than half the band ([03 §2.6](03-chrome.md#26-the-gesture-style)) |
| `nav_indicator_thickness` | `5.0` | The home indicator's thickness (its ends are round) |
| `status_edge_pad` | `12.0` | Status bar side padding |
| `status_overlay_alpha` | `1.0` | Background alpha of a `BarMode::Overlay` status bar — opaque, since 0.72 put caption text under the contrast floor |
| `tile_size` | `72.0` | Quick-settings tile side (an upper bound) |
| `shade_handle_width` | `40.0` | Width of the shade's bottom handle |
| `notification_row_height` | `72.0` | Notification row height |
| `toast_width` | `420.0` | Maximum toast width |
| `heads_up_height` | `88.0` | Heads-up banner height |
| `osk_key_gap` | `6.0` | Gap between OSK keys |
| `osk_max_key` | `72.0` du floor, `finger × 1.5` | The tallest a row of OSK keys gets, gap included — a cap on `[osk] height_ratio`, so a tall panel does not get rows the size of a palm. `[osk] min_key_px` wins where the two cross. `f32::INFINITY` lifts the cap — `None` in a `MetricsSpec` ([03 §6.2](03-chrome.md#62-layouts)) |
| `split_divider` | `8.0` | The band between two panes ([03 §2.3](03-chrome.md#23-recent-screens-and-the-split)). Its handle is widened to `touch_target` for the finger; this is what is drawn and what the panes give up |
| `widget_height` | one finger, `48.0` floor | Default height of `BigButton` · `ListRow` · the shade footer. One touch target, like a row |
| `slider_thumb` | `28.0` du floor, `finger × 0.585` | Slider thumb diameter. It is dragged with a finger, so it is a **touch** dimension and tracks `finger_mm` — 28 du (4.4 mm) is right for a Material-sized hand and visibly under-scale beside a gloved 13 mm target |
| `type_scale` | `13/16/17/22/16` du floors, `text × 0.811/1/1.063/1.374/1` | **The five text sizes** (`small` · `body` · `button` · `heading` · `monospace`). `Theme::egui_style()` builds egui's `TextStyle` from them. **They track the viewing distance, not `finger_mm`**: one body em (`Dim::text(1.0)`) is `0.008684 ×` `ScalePolicy::viewing_distance_mm` — 3.13 mm at the default 360 mm, about 20 du, a phone's body text. The hand and the eye are separate knobs: a gloved policy grows the touch targets and leaves the text alone, and a standing kiosk raises `viewing_distance_mm` and the text grows with everything sized in `text` beside it. The `du` floors are the old fixed sizes, so a panel of unknown density still reads |

### 1.3 MotionTokens

`MotionTokens` is `[motion]` turned into motion-system types. The value table and how to tune
them are in §8.

| Token | Type | What it moves |
|---|---|---|
| `reduce` | `bool` | When true every tween is 0 ms and springs settle instantly |
| `spring` | `Spring` | The default spring (`k`, `c`) |
| `snap_ratio` · `fling_px_s` · `slop_px` | `f32` | Release distance and velocity thresholds, drag slop |
| `tap` · `long_press` | `Duration` | Tap and long-press timing |
| `push` · `pop` · `parallax` · `dim` | `Tween` · `f32` | A3 stack push/pop |
| `home_open` · `home_close` · `desktop_scale` | `Tween` · `f32` | A2 home ↔ task |
| `press` · `press_release` · `press_scale` | `Tween` · `f32` | A7 press feedback |
| `shade` | `ShadeTokens` | A1 shade (`spring` · `snap_ratio` · `rubber`) |
| `page` | `PageTokens` | A4 page swipe (`spring` · `fling_px_s` · `rubber`) |
| `osk_show` · `osk_hide` · `osk_hide_debounce` | `Tween` · `Duration` | A5 keyboard |
| `toast_in` · `toast_out` · `toast_shift` | `Tween` | A6 toasts |
| `heads_up_in` · `heads_up_out` · `heads_up_hold` | `Tween` · `Duration` | A6 heads-up banner |
| `switch` | `Tween` | A7 switch knob and track |
| `crossfade` | `Tween` | Parametric status icon crossfade (120 ms) |
| `theme_fade` | `Tween` | Dark/light palette interpolation (200 ms) |
| `clear_top` | `Tween` | Clear-top crossfade when a Single screen is reused (160 ms) |

### 1.4 Starting from dark/light and overriding a few things

Do not fill a theme in from nothing. Copy a default and override what you need.

```rust
use fairing::theme::{Metrics, Palette};
use fairing::{Shell, ShellConfig, Theme};

/// A machine pressed with gloved hands: thicker chrome, green accent.
fn device_theme() -> Theme {
    let mut theme = Theme::dark();
    theme.palette = Palette {
        background: egui::Color32::from_rgb(0x06, 0x11, 0x0d),
        surface: egui::Color32::from_rgb(0x0d, 0x1f, 0x18),
        surface_variant: egui::Color32::from_rgb(0x14, 0x2e, 0x24),
        on_surface: egui::Color32::from_rgb(0xd6, 0xf5, 0xe4),
        muted: egui::Color32::from_rgb(0x5f, 0x8a, 0x76),
        primary: egui::Color32::from_rgb(0x2f, 0xe0, 0x9b),
        ..Palette::dark()
    };
    theme.metrics = Metrics {
        status_bar_height: 44.0,
        touch_target: 64.0,
        icon_cell: 132.0,
        icon_size: 56.0,
        screen_inset: 20.0,
        corner_radius: 18.0,
        ..Metrics::default()
    };
    theme
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .theme(device_theme())
        .build(ctx)
}
```

To change only the motion and leave the colours alone, swap `theme.motion`.

```rust
use fairing::config::MotionConfig;
use fairing::theme::MotionTokens;
use fairing::Theme;

fn snappy() -> Theme {
    let mut theme = Theme::dark();
    theme.motion = MotionTokens::from_config(&MotionConfig {
        snap_ratio: 0.25,
        ..MotionConfig::default()
    });
    theme
}
```

### 1.5 Two things to watch

- **Status bar icon size** is `Metrics::status_icon_size`: the status bar and the shade's
  notification rows both draw at it. From config, `[status_bar] icon_size` pins it; with an
  injected theme, the theme's value is used and the key is ignored with a warning.
- **The egui `Style`** comes from the theme too. `Theme::egui_style()` stops separating hover
  from press (touch has no hover), removes shadows, and redefines the five text sizes from
  `Metrics::type_scale` — 13/16/17/22/16 du in a bare `Theme::dark()`, resolved from the viewing
  distance on a running shell (§1.2). egui's own widgets inside a screen get this style.

### 1.6 Widget metrics — `ComponentSpec`

If §1.2's `Metrics` is what the **shell** divides the screen with, `Theme::components`
(`ComponentMetrics`) is what a **single widget draws itself** with. Twenty-seven tokens in nine
groups.

| Group | Tokens | Default (du) |
|---|---|---|
| `toast` | `enter_offset` · `lift_step` · `stack_gap` · `pad_x` · `icon_gap` · `accent_w` | 24 · 8 · 16 · 16 · 12 · 3 |
| `heads_up` | `pad` · `icon_gap` · `progress_h` | 16 · 12 · 4 |
| `button` | `pad` · `icon_gap` · `ring` · `ring_stroke` | 12 · 8 · 24 · 3 |
| `slider` | `label_gap` · `track_ratio` | 8 · 0.64 |
| `switch` | `knob_inset` · `height` | 3 · `finger(0.6)` (floor 28.8) |
| `list_row` | `pad` · `two_line_offset` · `chevron_w` | 16 · 11 · 20 |
| `overview` | `card_gap` · `card_min_width` | 24 · `finger(3.0)` (floor 144) — the recent screens' cards are `0.6 ×` the content, never narrower than this ([03 §2.3](03-chrome.md#23-recent-screens-and-the-split)) |
| `popover` | `max_width` · `caret` · `icon_gap` | 420 · 8 · 12 — a desktop icon's info popover: the widest it is drawn, how far it stands off the icon, the gap beside its icons ([03 §3.8](03-chrome.md#38-holding-an-icon-the-info-popover)) |
| `status_bar` | `item_gap` · `user_dot` | 10 · 8 — the gap between two status items, and the diameter of the user level's colour dot. The bar's height, icon size and edge pad are `Metrics` (§1.2); these two are the bar's own |
| `shade` | **nine multipliers** — separate table below | |

The shade alone uses different units. It does not carry lengths but **multipliers on two axes**,
so that a density change does not grow the icons while leaving the padding behind.

| Axis | Multipliers | Defaults | Multiplied by |
|---|---|---|---|
| Visual | `pad` · `gap` · `card_pad` · `card_gap` · `radius` | 1.34 · 1.0 · 1.17 · 0.67 · 1.34 | `metrics.corner_radius` |
| Finger | `tile_puck` · `note_icon` · `close` · `footer_button` | 0.8 · 0.62 · 0.72 · 0.82 | `metrics.touch_target` |

Of the four finger-axis values only `tile_puck` is purely a drawing size — a tile's hit area is
the whole cell, which is larger than the puck. `note_icon` is not pressable. `close` and
`footer_button`, though, **have hit areas equal to their drawing**, so shrinking these shrinks
what can be pressed.

**Why these are separate.** These twenty used to be `const`s inside their files. That meant
changing one button's padding forced you up to rung 5 of the override ladder (the painter) and
made you take over the drawing entirely — except **widgets have no painter hook at all**, and
notifications had none until §10.2 arrived. There was no path.

**The entrance is the builder.** There is no `[components]` TOML section yet — `MetricsSpec` is
in the same state, so the two have the same shape.

```rust
# fn build(ctx: &egui::Context) -> fairing::Result<()> {
use fairing::theme::ComponentSpec;
use fairing::unit::{Dim, Span};
use fairing::{Shell, ShellConfig};

let base = ComponentSpec::default();
let spec = ComponentSpec {
    // Button side padding as a physical 4 mm. On a panel whose density is known,
    // that is the same size whatever the diagonal.
    button: [Span::fixed(Dim::mm(4.0)), base.button[1], base.button[2], base.button[3]],
    // A thicker track — a ratio against the thumb (`metrics.slider_thumb`), not a length.
    slider_track_ratio: 0.85,
    ..base
};
let shell = Shell::builder(ShellConfig::default()).component_spec(spec).build(ctx)?;
# let _ = shell;
# Ok(()) }
```

**Three rules.**

- **Values are `Span`s.** Mix `Dim::du` · `Dim::mm` · `Dim::finger` freely and put a floor on
  them with `.min(..)`. They resolve against that screen's `Scale` every frame.
- **Only the track thickness is a ratio.** `slider_track_ratio` is a multiple of
  `metrics.slider_thumb`, not a length — exposed as a length it invites a big thumb on a
  thread-thin track.
- **Switch height is a drawing size.** The hit rect keeps the touch target regardless. Shrinking
  this does not shrink what your finger can land on.

`ComponentSpec::validate()` checks every one of them at the reference scale — negative or
non-finite is an `Error::Config`. It does not quietly draw a zero-thickness track.

### 1.7 Control and elevation specs

Two more specs resolve every frame beside `metrics_spec` and `component_spec`, so they follow the
panel's density like the rest:

- **`.control_spec(ControlSpec)`** — the lengths every drawn control shares: the mark size a
  switch, checkbox and radio are built from, the stroke ladder, the focus gap and the gaps.
- **`.elevation_spec(ElevationSpec)`** — how high a container sits and how that is shown: a
  shadow in light mode, a rim in dark mode, both in millimetres with a device-pixel floor.

```rust
use fairing::theme::ElevationSpec;
use fairing::{Shell, ShellConfig};

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        // An e-ink or 16-level panel dithers a soft shadow into rings: draw no height at all.
        .elevation_spec(ElevationSpec::flat())
        .build(ctx)
}
```

Like the other two, they are left alone when a whole theme is injected (§1).

---

## 2. Changing only the colours with `[theme.palette]`

Leaving metrics and motion alone and changing only colours is the most common request. Do not
inject a theme for it; use config. It overlays whichever dark/light palette `[shell] theme`
picked.

```toml
[shell]
theme = "dark"

[theme.palette]
primary = "#2fe09b"
on_primary = "#04110b"
background = "#061109"
surface = "#0d1f18"
```

| Rule | Detail |
|---|---|
| Keys | The config names from §1.1. Anything else is a **config error at startup** |
| Values | `"#RRGGBB"` or `"#RRGGBBAA"`. `#RGB` shorthand and colour names are errors |
| Alpha | The `AA` of `#RRGGBBAA`. `scrim`, `pressed` and `shadow` are translucent by default; the rest are opaque |
| Precedence | Injecting a theme ignores this whole section (with a warning) |

Not swallowing typos is deliberate. Write `primary_color = "#..."` and the shell does not start;
it tells you which key is wrong.

---

## 3. Painting a whole bar yourself

If the built-in status bar or nav bar does not lay out the way you want, you can take the whole
drawing. With a painter installed, the built-in render (`StatusBar::ui` · `NavBar::ui`) is
**never called** — the background is yours too.

### 3.1 What the shell keeps doing and what the painter takes on

| Still the shell's job | The painter's job |
|---|---|
| Bar rect (`Layout.status` · `Layout.nav`) | All drawing, background included |
| Visibility per chrome policy (`Hide` means the painter is not called either) | Item layout |
| Edge zone calculation | Item widgets and their hit testing |
| The item list and its order (straight from config) | Press feedback |
| Gate decisions → `BarItem::allowed` | |
| Item on/off → `BarItem::enabled` | |
| Whether back is available → `BarCx::back_enabled` | |
| Taps on the status bar itself (`tap_opens_shade`) | |
| Re-checking the gate on launch (`shell.launch`) | |

With a painter installed, `StatusBar::item_rect` · `NavBar::item_rect` always return `None`. The
shell did not draw the items, so it has no rects for them.

### 3.2 The `BarCx` API

```rust,ignore
pub struct BarCx<'a> {
    pub kind: BarKind,        // Status | Nav — both bars use the same painter type
    pub rect: Rect,           // the bar rect the shell decided. Drawing outside it is clipped
    pub back_enabled: bool,   // nav bar only; always false for a status bar painter
    pub cx: Cx<'a>,           // the same handle a screen closure gets
}

impl BarCx<'_> {
    pub fn items(&self) -> impl Iterator<Item = BarItem<'_>>;      // config order (status: left → center → right)
    pub fn slot(&self, slot: Slot) -> impl Iterator<Item = BarItem<'_>>;
    pub fn item(&self, id: &str) -> Option<BarItem<'_>>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

pub struct BarItem<'a> {
    pub id: &'a str,          // "status.clock", "back", your own id …
    pub slot: Option<Slot>,   // None for nav bar items
    pub enabled: bool,
    pub allowed: bool,        // passed its gate
}
impl BarItem<'_> { pub fn live(&self) -> bool; }  // enabled && allowed
```

`bar.cx` is the same `Cx` a screen gets: backend snapshots (`cx.services`), the theme
(`cx.theme`), the icon set (`cx.icons`) and the shell handle (`cx.shell`) are all in there.

### 3.3 A status bar painter

```rust
use fairing::time::ClockFormat;
use fairing::{BarCx, ColorRole, Shell, ShellConfig};

const DEVICE: &str = "LAB-7 CONTROL";

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    // Keeping the clock string buffer in the closure means no per-frame heap allocation.
    let mut clock = String::new();
    Shell::builder(ShellConfig::default())
        .status_bar_painter(move |ui: &mut egui::Ui, bar: &mut BarCx<'_>| {
            let theme = bar.cx.theme;
            let painter = ui.painter().clone();
            painter.rect_filled(bar.rect, 0.0, theme.color(ColorRole::Surface));
            // Our machine's accent line, which the built-in status bar does not have.
            painter.hline(
                bar.rect.x_range(),
                bar.rect.max.y - 1.0,
                egui::Stroke::new(2.0, theme.color(ColorRole::Primary)),
            );
            let pad = theme.metrics.status_edge_pad;
            let font = egui::FontId::proportional(16.0);
            painter.text(
                egui::pos2(bar.rect.min.x + pad, bar.rect.center().y),
                egui::Align2::LEFT_CENTER,
                DEVICE,
                font.clone(),
                theme.color(ColorRole::OnSurface),
            );
            // If an item fails its gate the shell says so through live() == false.
            if bar.item("status.clock").is_some_and(|item| item.live()) {
                bar.cx
                    .services
                    .clock
                    .now()
                    .format_into(&mut clock, ClockFormat::Hm);
                painter.text(
                    egui::pos2(bar.rect.max.x - pad, bar.rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    &clock,
                    font,
                    theme.color(ColorRole::Primary),
                );
            }
        })
        .build(ctx)
}
```

### 3.4 A nav bar painter

The painter takes item taps with its own widgets and asks the shell through `bar.cx.shell`. The
shell re-checks the gate at that point.

```rust
use fairing::{BarCx, ColorRole, Shell, ShellConfig};

fn nav_bar_painter(ui: &mut egui::Ui, bar: &mut BarCx<'_>) {
    let theme = bar.cx.theme;
    let shell = bar.cx.shell;
    let rect = bar.rect;
    let count = bar.len().max(1);
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0.0, theme.color(ColorRole::Surface));

    let width = rect.width() / count as f32;
    for (index, item) in bar.items().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + width * index as f32, rect.min.y),
            egui::vec2(width, rect.height()),
        );
        let response = ui.interact(
            cell,
            egui::Id::new(("device.nav", index)),
            egui::Sense::click(),
        );
        let color = if item.live() {
            theme.color(ColorRole::OnSurface)
        } else {
            theme.color(ColorRole::Muted)
        };
        if response.is_pointer_button_down_on() {
            painter.rect_filled(
                cell.shrink(6.0),
                theme.metrics.corner_radius,
                theme.color(ColorRole::Pressed),
            );
        }
        painter.text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            item.id,
            egui::FontId::proportional(15.0),
            color,
        );
        if item.live() && response.clicked() {
            match item.id {
                "back" => shell.back(),
                "home" => shell.home(),
                _ => {}
            }
        }
    }
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .nav_bar_painter(nav_bar_painter)
        .build(ctx)
}
```

`painter.text` allocates a string on every call. In real device code, cache the galley and only
re-lay it out when the value changes — that is what the built-in bars do.

---

## 4. Painting desktop slots yourself

`slot_painter` draws one cell. Grid placement, the dock, pages, hit testing, press detection,
gate filtering and recording the A2 origin rect all stay with the shell.

```rust,ignore
pub struct SlotCx<'a> {
    pub cell: Rect,           // cell rect (the whole touch target)
    pub icon: Rect,           // icon rect before the press scale (the A2 origin)
    pub pressed_icon: Rect,   // icon rect with the press scale applied
    pub slot: &'a IconSlot,   // id · label · icon · badge · gate · kind
    pub scale: f32,           // press scale (1.0 → motion.press.scale)
    pub pressed: bool,
    pub allowed: bool,        // passed its gate — false needs a padlock or a disabled look
    pub in_dock: bool,
    pub theme: &'a Theme,
    pub icons: &'a mut IconSet,
    pub now: Instant,
    pub strings: &'a Strings,  // the label is a key: strings.get(&slot.label)
}
```

How an `allowed == false` cell looks is the painter's call. A cell that is `Visibility::Hidden`
never reaches the painter at all.

```rust
use fairing::icons::{IconColor, IconStyle};
use fairing::{ColorRole, Shell, ShellConfig, SlotCx};

/// Draw icons as round badges. A locked cell gets a muted ring.
fn slot_painter(ui: &mut egui::Ui, slot: SlotCx<'_>) {
    let theme = slot.theme;
    let painter = ui.painter().clone();
    let center = slot.pressed_icon.center();
    let radius = slot.pressed_icon.width() * 0.62;
    let fill = if slot.pressed {
        theme.color(ColorRole::SurfaceVariant)
    } else {
        theme.color(ColorRole::Surface)
    };
    let ring = if slot.allowed {
        theme.color(ColorRole::Primary)
    } else {
        theme.color(ColorRole::Muted)
    };
    painter.circle_filled(center, radius, fill);
    painter.circle_stroke(center, radius, egui::Stroke::new(2.0, ring));

    let inner = egui::Rect::from_center_size(center, egui::Vec2::splat(radius));
    let style = IconStyle::sized(inner.width())
        .color(IconColor::Role(ColorRole::OnSurface))
        .enabled(slot.allowed);
    let _ = slot
        .icons
        .paint(&painter, inner, &slot.slot.icon, &style, theme);
    painter.text(
        egui::pos2(center.x, slot.cell.max.y - 14.0),
        egui::Align2::CENTER_CENTER,
        slot.strings.get(&slot.slot.label),
        egui::FontId::proportional(14.0),
        if slot.allowed {
            theme.color(ColorRole::OnSurface)
        } else {
            theme.color(ColorRole::Muted)
        },
    );
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .slot_painter(slot_painter)
        .build(ctx)
}
```

Taking `SlotCx` by value is the `SlotPainter` contract: it carries an `&mut IconSet`, so it
cannot be taken by reference.

To swap it at runtime use `shell.desktop_mut().set_slot_painter(..)`, and `has_slot_painter()`
to ask whether one is installed.

---

## 5. Painting the wallpaper yourself

`Wallpaper::Painter` is one callback. It is not handed the theme, so pull the colours you need
out first and capture them.

```rust
use fairing::{ColorRole, Shell, Theme, Wallpaper};

fn grid_wallpaper(theme: &Theme) -> Wallpaper {
    let background = theme.color(ColorRole::Background);
    let line = theme.color(ColorRole::SurfaceVariant);
    Wallpaper::Painter(Box::new(move |painter: &egui::Painter, rect: egui::Rect| {
        painter.rect_filled(rect, 0.0, background);
        let step = 44.0;
        let stroke = egui::Stroke::new(1.0, line);
        let mut x = rect.min.x;
        while x <= rect.max.x {
            painter.vline(x, rect.y_range(), stroke);
            x += step;
        }
        let mut y = rect.min.y;
        while y <= rect.max.y {
            painter.hline(rect.x_range(), y, stroke);
            y += step;
        }
    }))
}

fn install(shell: &mut Shell) {
    let wallpaper = grid_wallpaper(shell.theme());
    shell.desktop_mut().set_wallpaper(wallpaper);
}
```

`rect` is the whole content rect. It is called every frame, so do not allocate in it — drawing
lines directly, as above, is enough. On a machine that switches dark and light, the captured
colours are frozen, so rebuild the wallpaper on the switch.

The other four backgrounds (`Solid` · `Fixed` · `Gradient` · `Texture`) are in
[03 Chrome §3.7](03-chrome.md#37-wallpaper).

---

## 6. Turning parts off

### 6.1 What can be turned off, and how

| What | How | Result |
|---|---|---|
| Status bar | `[status_bar] enabled = false` | No bar, and the content grows by that much. The top edge zone stays |
| Status bar (per screen) | `ChromePolicy::status_bar = BarMode::Hide` | Hidden only while that screen is focused. With `allow_peek`, a pull still shows it |
| Nav bar | `[nav_bar] enabled = false` | Going back is up to in-screen buttons (`cx.finish()`) or the edge gesture |
| One nav item | Remove it from `[nav_bar] items` | The slot itself disappears |
| Gesture back | `[nav_bar] back_edges = []` | Edge swipes no longer go back |
| Dock | `[desktop] dock = []` and no `.dock()` declaration | No band is drawn and the grid grows |
| Page indicator | Fit the icons on one page | With one page it gives its space back automatically |
| Shade (at runtime) | Put the `overlay.open` gate at a higher level + `[status_bar] tap_opens_shade = false` | It does not open; only `UnlockRequested` fires |
| Shade (at compile time) | Turn the `overlay` feature off | The `overlay` module, tiles and scrim all go |
| Some shade tiles | Remove them from `[overlay] tiles` | |
| OSK (per screen) | `ChromePolicy::osk = OskMode::Off` | For a screen with a hardware keyboard |
| OSK (at compile time) | Turn the `osk` feature off | The `osk` module goes entirely |
| All gestures | `[gesture] enabled = false` | No edge swipes, long presses or flings. **The emergency gesture stays** |
| Heads-up banners | `[notify] heads_up = false` | Notifications only stack in the shade |
| Animation | `[motion] reduce = true` | Every tween 0 ms, springs settle instantly |
| Notifications themselves | Send none | There is no setting for it |

Config sections for a subsystem turned off by feature are **parsed and ignored.** With `overlay`
off, a config file containing `[overlay]` still parses. That is so one config file can drive
builds with different feature combinations.

### 6.2 Feature combinations

| Feature | Default | On | Off |
|---|---|---|---|
| `mock` | on | `services::mock` (`MockClock` · `MockWifi` · `MockPower` …) | Examples and tests drop out of the build |
| `overlay` | on | The `overlay` module, shade, tiles and scrim, `tile()` · `TileKind` · handling of `LaunchAction::OpenOverlay` | The shell does not know a shade exists |
| `osk` | on | The `osk` module, the on-screen keyboard, `Shell::osk()` | `Layout.osk` is always `None`, `inset_bottom` always 0 |
| `brand` | on | `fairing::brand` — the manta mark and the Abyss background | `[theme] preset` and the palettes stay; `wallpaper = "abyss"` falls back to the background colour, with a warning |
| `settings` | on | The built-in settings screens and `settings::add_all` | No built-in settings screens; the setting keys and values are there either way |
| `runner` | off | `fairing::runner` (an eframe kiosk window) | The integrator owns the event loop |
| `runner-x11` | off | `runner` plus eframe's x11 backend | Wayland only |
| `chrono` | off | `services::clock::ChronoClock` — a clock that asks the OS time-zone database, so daylight saving corrects itself | `SystemClock`: a fixed offset that does not know about DST |

```toml
# Integrator Cargo.toml: a minimal shell with no shade and no keyboard
[dependencies]
fairing = { git = "https://github.com/shim9610/fairing", default-features = false }
```

```toml
# For development: including the runner
[dependencies]
fairing = { git = "https://github.com/shim9610/fairing", features = ["runner-x11", "mock"] }
```

Until the crates are on crates.io, depend on the repository as above.

`default-features = false` drops `mock` too. Ask for it explicitly if your tests need the mock
backends.

---

## 7. Reading `examples/custom_chrome.rs`

A working example that changes the whole shell without touching the crate. It puts §1 · §3 · §4 ·
§5 of this page, and the two custom tile kinds from [03 §4.4c](03-chrome.md#44c-tiles-the-crate-has-no-kind-for), into one file.

```sh
cargo run -p fairing --features runner-x11 --example custom_chrome -- --size=1024x600
```

| Order in the file | Function | What it does | This page |
|---|---|---|---|
| 1 | `theme()` | Starts from `Theme::dark()` and overrides six colours and five metrics | §1.4 |
| 2 | `config()` | A config literal. Status bar slots are `["status.clock", "status.wifi"]`, nav bar off, no dock, and `[overlay] tiles` mixes built-in ids with our own | §6.1 |
| 3 | `services()` | The mock backends (clock, power, Wi-Fi) | [06 Services](06-services.md) |
| 4 | `build(ctx)` | Hangs the theme, services, status bar painter and slot painter on the builder, installs the wallpaper, then adds screens and tiles | §1 · §3 · §4 · §5 |
| 5 | `status_bar_painter()` | Device name on the left, Wi-Fi strength and clock on the right. The closure owns the string buffer | §3.3 |
| 6 | `slot_painter()` | Icons as round badges; `allowed` picks the ring colour | §4 |
| 7 | `grid_wallpaper()` | A procedural grid background, colours captured up front | §5 |
| 8 | `add_hopper_tile()` | A tile that opens three gauges — declared with `TileKind::Gauges` (per-row setting key, name, colour, unit; one read-only row), drawn by the crate | [03 §4.4c](03-chrome.md#44c-tiles-the-crate-has-no-kind-for) |
| 9 | `add_jog_tile()` | A tile that opens a two-axis jog pad — `tile_panel` hands over the whole expanded row | [03 §4.4c](03-chrome.md#44c-tiles-the-crate-has-no-kind-for) |
| 10 | `back_row()` | With no nav bar, a back row at the top of the screen | §6.1 |
| 11 | `add_screens()` | A four-icon desktop | [02 Screens](02-screens.md) |

The thing to watch while reading is **what the shell keeps doing.** Three painters and two
foreign tiles later, the layout rects, gate decisions, hit testing and press detection, the
A2/A3 transitions and the repaint policy are all still the shell's. The painters only paint.

---

## 8. Tuning motion

"Feel" cannot be settled in a document. It gets settled on the device.

### 8.1 motion_lab

```sh
cargo run -p fairing --features runner-x11 --example motion_lab -- --size=1024x600
```

It puts every `[motion]` token on a slider and replays transitions on a loop while you pick
values.

| On screen | What it does |
|---|---|
| Collapsible slider groups | Common · A2/A3 · A1 shade · A4 pages · A5 OSK · A6 toast/heads-up · A7 widgets |
| `play loop` | Home → page round trip → icon zoom → shade open/close → push/pop, forever |
| `export [motion]` | Prints the current values as `[motion]` TOML to the log (stderr). Paste it straight into a config file |
| `shade` · `push child` · `toast` · `notify` · `home` · `hold 2 s` | Each transition once |
| Frame graph, top right | The last 180 frame intervals (ms) with p50 and p95. The gridline is the 33 ms budget |

Moving a slider calls `ShellHandle::set_motion(tokens)` and takes effect from the next frame; an
animation already in flight is left alone. A bar over the 33 ms line means that motion is eating
the frame budget.

The same thing from code:

```rust
use fairing::config::MotionConfig;
use fairing::theme::MotionTokens;
use fairing::ShellHandle;

fn apply_snappy(handle: &ShellHandle) {
    let tokens = MotionTokens::from_config(&MotionConfig {
        snap_ratio: 0.25,
        fling_px_s: 600.0,
        ..MotionConfig::default()
    });
    handle.set_motion(tokens);
}
```

### 8.2 `[motion]` tokens

| Key | Default | Meaning |
|---|---|---|
| `reduce` | `false` | True makes every tween 0 ms (§8.3) |
| `spring` | `{ k = 400, c = 40 }` | The default spring. `c = 2√k` is critical damping |
| `snap_ratio` | `0.33` | Release distance threshold (0..=1) |
| `fling_px_s` | `800.0` | Release velocity threshold (px/s) |
| `slop_px` | `12.0` | Drag detection slop |
| `tap_ms` | `300` | Tap window |
| `long_press_ms` | `500` | Long press duration |
| `[motion.push]` `ms` · `parallax` · `dim` | `220` · `0.25` · `0.15` | A3 push tween, and the outgoing layer's parallax and dim |
| `[motion.pop]` `ms` | `200` | A3 pop tween |
| `[motion.home]` `open_ms` · `close_ms` · `desktop_scale` | `240` · `200` · `0.92` | A2 home ↔ task |
| `[motion.press]` `ms` · `release_ms` · `scale` | `80` · `120` · `0.97` | A7 press |
| `[motion.shade]` `spring` · `snap_ratio` · `rubber` · `rubber_max_px` | `{k=400,c=40}` · `0.33` · `0.25` · `40.0` | A1 shade |
| `[motion.page]` `spring` · `fling_px_s` · `rubber` · `rubber_max` | `{k=300,c=35}` · `600.0` · `0.3` · `0.15` | A4 page swipe |
| `[motion.osk]` `show_ms` · `hide_ms` · `hide_debounce_ms` | `180` · `160` · `100` | A5 keyboard |
| `[motion.toast]` `in_ms` · `out_ms` · `shift_ms` | `160` · `200` · `120` | A6 toasts |
| `[motion.toast]` `heads_up_in_ms` · `heads_up_out_ms` · `heads_up_hold_ms` | `220` · `200` · `4000` | A6 heads-up banner |
| `[motion.switch]` `ms` | `140` | A7 switch |
| `crossfade_ms` | `120` | Parametric status icon crossfade |
| `theme_fade_ms` | `200` | Dark/light palette interpolation |
| `clear_top_ms` | `160` | Clear-top crossfade when a Single screen is reused |

Out-of-range values are a config error at startup: `spring.k <= 0`, `snap_ratio` outside 0..=1,
`press.scale` above 1, `home.desktop_scale` at or below 0, and so on.

```toml
[motion]
snap_ratio = 0.25
fling_px_s = 600

[motion.push]
ms = 180
parallax = 0.2

[motion.shade]
snap_ratio = 0.3
spring = { k = 500, c = 45 }
```

### 8.3 reduce mode

```toml
[motion]
reduce = true
```

`reduce` makes every tween 0 ms and settles springs instantly. The state transitions themselves
are unchanged — the shade still opens and closes, there are simply no frames in between.

| Where it is used | Why |
|---|---|
| Machines with an accessibility requirement | Users sensitive to motion |
| Slow devices | Reclaiming frame budget |
| Headless tests | No waiting for transition frames |
| Screenshot tours | No blurred intermediate frames |

Whether `reduce` is on is readable from the `MotionTokens::reduce` field. If integrator code has
to draw differently with and without animation, look at `cx.theme.motion.reduce`.

---

## 9. Text and translations

Every word the shell draws goes through `fairing::i18n::Strings`, and the words of your screens
can too. **The English text is the key**: `cx.strings.get("Pump pressure")` returns the active
language's entry for it, or the text as written where the table has none. A word nobody
translated reads as English, never as a blank.

| Language | Where it comes from |
|---|---|
| English | No table — the keys themselves |
| Korean (`ko`) | Built in: the shell, the shade, the lock screen, the unlock prompt and the `settings` screens |
| Anything else | A `Translations` table you hand to `ShellBuilder::translations` |

### 9.1 Choosing the language

`[shell] locale` is the language to start in. `"ko-KR"` finds the `ko` table; a tag with no table
warns at start-up and the text stays English.

While the shell runs, the language is **the `ui.locale` setting**. The built-in `settings.locale`
screen lists every language there is a table for — the built-in two and yours — and writes it.
Anything else can write it too, and the next frame is in the new language; nothing restarts:

```rust
use fairing::settings::{keys, SettingValue};

fn to_korean(shell: &mut fairing::Shell) {
    shell.set_setting(keys::UI_LOCALE.into(), SettingValue::Text("ko".into()));
}
```

A tag with no table changes nothing and logs a warning.

### 9.2 Your own words, and other languages

```rust
use fairing::i18n::Translations;
use fairing::{screen, Cx, Shell, ShellConfig};

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    let mut shell = Shell::builder(ShellConfig::default())
        // A language the crate has no table for.
        .translations(
            Translations::new("de", "Deutsch")
                .entry("Pump pressure", "Pumpendruck")
                .entry("Settings", "Einstellungen"),
        )
        // Your words in Korean, and one built-in word changed.
        .translations(
            Translations::new("ko", "한국어")
                .entry("Pump pressure", "펌프 압력")
                .entry("Settings", "환경설정"),
        )
        .build(ctx)?;
    shell.add(
        screen("pump", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            ui.label(cx.strings.get("Pump pressure"));
        })
        .title("Pump pressure"),
    );
    Ok(shell)
}
```

- A table for a language that is already there joins it, and its entries win. The Korean table
  above keeps every built-in entry except `Settings`.
- **A screen's `.title(..)` is a key.** The desktop label and the recent-screens card look it up
  where they draw it. A screen's body looks its own words up through `cx.strings`.
- **The labels of `[access] levels` are keys too.** The shade and the status bar name the session
  by them.
- A key with placeholders keeps them: `"{n} min ago"` → `"{n}분 전"`. Two keys are templates for
  the language's own order. The lock screen's date is `"{y}-{mm}-{dd}"`: `{mm}` and `{dd}` are two
  digits, `{m}` and `{d}` are the plain numbers, and Korean writes `"{y}년 {m}월 {d}일"`. The
  12-hour clock is `"{t} AM"` and `"{t} PM"`, and Korean puts the half of the day first:
  `"오전 {t}"`.
- Every built-in key, with its Korean, is in `crates/fairing/src/i18n/ko.rs`. A test keeps that
  table whole: a word added to the shell with no Korean entry fails it.

### 9.3 Fonts

egui's own fonts have no Hangul, so a Korean panel needs a font that has it, handed over with
`ShellBuilder::fonts` ([09 §3](09-branding.md#3-type--half-of-branding-and-the-crate-carries-no-fonts)).
The shell checks once per language: where no loaded font has the language's letters, it says so
in the log rather than drawing boxes with no explanation.

```rust
use fairing::fonts::{FontSet, FontSource};

fn fonts() -> fairing::Result<FontSet> {
    let mut fonts = FontSet::new();
    if let Some(path) = fairing::fonts::korean_font() {
        fonts.push(FontSource::from_path("ko", &path)?);
    }
    Ok(fonts)
}
```

### 9.4 A widget's own words

A widget from `fairing::widgets` has no string table, so where it draws a word of its own it
takes it from you. One does: a `Dropdown` search that finds nothing. Its key is in
`fairing::i18n::labels`, and the Korean table has it:

```rust
use fairing::i18n::labels;
use fairing::widgets::{Dropdown, Opener};

fn body(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>, picked: &mut usize) {
    let no_match = cx.strings.get(labels::NO_MATCH);
    Dropdown::new("pump", &["Inlet", "Outlet"], picked)
        .opener(Opener::Search)
        .no_match(no_match)
        .show(ui, &mut cx.widgets());
}
```

---

## 10. Moving and redrawing the other pieces

§3–§5 take over a whole bar, a desktop cell or the background. The rest of the chrome has
narrower hooks, on two rungs of the override ladder:

- **A layout (rung 4)** says where each piece goes. The shell still draws it, presses it and
  times it — use one when the built-in placement is wrong for your device.
- **A painter (rung 5)** draws a piece. The shell still decides when it shows and what a press
  on it does — use one when the shell's look is wrong for your device.

| Hook | Rung | Feature | You change | The shell keeps |
|---|---|---|---|---|
| `.nav_bar_layout(..)` | 4 | — | Where each nav bar item goes | Drawing the items, their gates, presses and taps, `NavBar::item_rect` |
| `.status_bar_layout(..)` | 4 | — | Where each status bar item goes — a collapsed one too | Measuring, the collapse, drawing the items, their gates and taps, `StatusBar::item_rect` |
| `.toast_layout(..)` · `.toast_painter(..)` | 4 · 5 | — | Where the toast stack goes · each toast's card | The queue, the hold and its timer, the fade and the rise, the tap that dismisses |
| `.heads_up_layout(..)` · `.heads_up_painter(..)` | 4 · 5 | — | Where the banner goes and how wide · the banner | Its height, its slide in and out, the hold, the tap that opens, the swipe that puts it away |
| `.osk_key_layout(..)` · `.osk_key_painter(..)` | 4 · 5 | `osk` | Where each key goes · each key | When the keyboard shows and slides, each key's hit area, the press, typing, composition, ⇧ and its lock, the faces |
| `.shade_tile_layout(..)` · `.shade_tile_painter(..)` · `.shade_panel_painter(..)` | 4 · 5 · 5 | `overlay` | Where each quick-settings tile goes · each tile · the panel's ground | The pull and the reveal, the two-step stop, taps, long presses, gates, the row a tile opens, the list, the footer |
| `.lock_screen_painter(..)` · `.unlock_prompt_painter(..)` | 5 · 5 | — | The lock screen's ground, clock and date · the unlock prompt's backdrop and card | The way in, drawn over them; what is typed, the answer, the lockout, the shake, the motion, every press |
| `.recents_card_painter(..)` · `.recents_ground_painter(..)` | 5 · 5 | — | Each recent screen's card · the ground under the cards | Where the cards go, the carousel's drag, taps, the throw that closes a task, the screen shrinking into its card, the split buttons, "Close all" |
| `.widget_painters(..)` | 5 | — | Each kind of widget, wherever it is drawn — your screens, the shell's settings screens, its prompt and its recent screens | Each widget's rects, its press and drag, the value and when it changes, its motion, focus and keys |
| `.gesture_handle_painter(..)` | 5 | — | Each gesture handle's strip, and the arrow while it is swiped ([03 §9.2](03-chrome.md#92-gesture-handles)) | The strips and where they go, which press is a handle's, the way, the reach and the rest, the gestures and what they run |
| `GestureRegion::paint` | 5 | — | A gesture region of your own, drawn by its own trait ([03 §9.3](03-chrome.md#93-gesture-regions-of-your-own)) — with its rect and its touch | Where it may go (over a screen or the desktop), which press is its, its touch from the press to the release, the guard that keeps the press from what is under it |

Three rules hold for every one of them.

- **A layout starts from the built-in placement.** It is handed the rects the shell would use and
  changes only the ones it wants, so moving one piece is one line rather than a re-implementation.
  The rects are in a fixed order — the config's for items and tiles (the status bar's left,
  centre, right), reading order for keys, oldest first for toasts — and each context names them
  (`index_of`, `id`).
- **A rect the layout empties (`egui::Rect::NOTHING`) leaves that piece out** — not drawn, not
  pressed. A toast or a banner left out is still timed and goes when it would have.
- **A painter wins over a layout** where the painter takes the whole surface. Both bars have a
  whole-bar painter (§3) and a layout: with `nav_bar_painter` set, `nav_bar_layout` is never
  called, and with `status_bar_painter` set, `status_bar_layout` is not either — `build` says so
  once in the log. A painter that draws one piece — a toast, a
  key, a shade tile — works with its layout: the layout says where, the painter how.

**The widgets have painters, kind by kind** (§10.8), and no layouts: where a widget goes is the
egui layout of the screen that draws it.

### 10.1 The nav bar's items

```rust
use fairing::{NavLayoutCx, Shell, ShellConfig};

/// Back hard against the left edge; home and recents where the shell put them.
fn back_left(bar: &NavLayoutCx<'_>, cells: &mut [egui::Rect]) {
    if let Some(cell) = bar.index_of("back").and_then(|i| cells.get_mut(i)) {
        *cell = cell.translate(egui::vec2(bar.rect.left() - cell.left(), 0.0));
    }
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .nav_bar_layout(back_left)
        .build(ctx)
}
```

`NavLayoutCx` carries the bar's `rect` and the `theme`, and per item `id(i)`, `index_of(id)` and
`is_live(i)` — back is not live on the desktop, a `nav_item` past its gate is not either. A cell
that reaches past the bar is cut by it. The gesture-style bar (03 §2.6) has no items, so its
layout is never called.

### 10.2 Toasts and the heads-up banner

```rust
use fairing::notify::{HeadsUpLayoutCx, ToastCx};
use fairing::{ColorRole, Shell, ShellConfig};

/// A flat card in the accent colour, taller than the built-in one.
fn accent_toast(ui: &mut egui::Ui, toast: &mut ToastCx<'_>) {
    toast.set_height(64.0);
    let theme = toast.theme;
    ui.painter()
        .rect_filled(toast.rect, 4.0, theme.color(ColorRole::Primary));
    ui.painter().text(
        toast.rect.center(),
        egui::Align2::CENTER_CENTER,
        &toast.toast.text,
        egui::FontId::proportional(theme.metrics.type_scale.body),
        theme.color(ColorRole::OnPrimary),
    );
}

/// The banner in the top left corner, 360 wide.
fn banner_top_left(cx: &HeadsUpLayoutCx<'_>, rect: &mut egui::Rect) {
    *rect = egui::Rect::from_min_size(
        egui::pos2(cx.screen.left() + 16.0, cx.screen.top() + 8.0),
        egui::vec2(360.0, cx.height),
    );
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .toast_painter(accent_toast)
        .heads_up_layout(banner_top_left)
        .build(ctx)
}
```

- **The rects a layout gets are at rest.** A toast layout gets one rect per toast on screen, the
  oldest first, stacked up from the bottom centre of `area` — the content between the bars, and
  above the keyboard while it is up, though never shorter than one toast needs, so on a short
  panel it reaches over the keyboard's top rows. The shell adds the rise of one arriving, and the
  lift a newer toast gives the older ones, on top of where the layout puts them. The heads-up layout gets the banner centred at the top of `screen`;
  it may move it and change its width, but **the height stays the banner's own** — it still comes
  down into its rect from above and leaves upwards.
- **A painter draws the whole card**, background included. The shell has already faded a toast
  (the `Ui`'s opacity) and moved it; it takes the tap on the rect after the painter has drawn.
  `ToastCx` carries `rect`, `toast` (its text, level and icon), `theme`, `icons` and `strings`;
  `HeadsUpCx` carries `rect`, `id`, `title`, `body`, `icon`, `level`, `progress`, `pressed`,
  `theme`, `icons` and `strings`.
- **A card that needs more room asks for it** with `set_height(..)`. It is laid out at that
  height from the next frame, never below the built-in card's floor — `metrics.widget_height`
  for a toast, `metrics.heads_up_height` for the banner. Lower the token to go smaller.

### 10.3 The keyboard's keys

What the keys *are* — their labels, what they type, the faces — stays the `OskLayout`'s
(03 §6.2). These two hooks say where they go and how they look.

```rust
use fairing::osk::{OskKeyCx, OskKeyLayoutCx};
use fairing::{ColorRole, Shell, ShellConfig};

/// One field to a screen, so no focus keys: ⏮ and ⏭ go and the space bar takes their room.
fn no_focus_keys(keys: &OskKeyLayoutCx<'_>, rects: &mut [egui::Rect]) {
    let (Some(prev), Some(space), Some(next)) =
        (keys.index_of("⏮"), keys.index_of(" "), keys.index_of("⏭"))
    else {
        return;
    };
    let (Some(&left), Some(&right)) = (rects.get(prev), rects.get(next)) else {
        return;
    };
    if let Some(rect) = rects.get_mut(space) {
        *rect = egui::Rect::from_min_max(left.min, right.max);
    }
    for gone in [prev, next] {
        if let Some(rect) = rects.get_mut(gone) {
            *rect = egui::Rect::NOTHING;
        }
    }
}

/// Flat keys with a hairline round them; the locked ⇧ in the accent colour.
fn hairline_key(painter: &egui::Painter, key: &mut OskKeyCx<'_>) {
    let theme = key.theme;
    let ink = theme.color(if key.locked {
        ColorRole::Primary
    } else {
        ColorRole::OnSurface
    });
    let width = if key.pressed { 2.0 } else { 1.0 };
    painter.rect_stroke(
        key.rect,
        2.0,
        egui::Stroke::new(width, ink),
        egui::StrokeKind::Inside,
    );
    painter.text(
        key.rect.center(),
        egui::Align2::CENTER_CENTER,
        key.label,
        egui::FontId::proportional(theme.metrics.type_scale.button),
        ink,
    );
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .osk_key_layout(no_focus_keys)
        .osk_key_painter(hairline_key)
        .build(ctx)
}
```

- The rects come one per key of the face on show, in reading order (the first row left to right,
  then the next), placed the built-in way. `OskKeyLayoutCx` names them — `key(i)` gives a key's
  row, place and `KeyDef`, `index_of(label)` finds one — and carries `panel` (the keyboard at its
  full height, where it is this frame: it slides up from below, so place keys relative to it) and
  `face_index`. The layout runs for every face, so a key moved on the lowercase face is not moved
  on the symbols face unless the layout says so there too.
- A key's hit area is its rect and half the gap round it, cut by the panel, wherever the layout
  puts it. Keep the rects apart: where two overlap, the one later in reading order takes the
  press.
- A painter gets one key at a time: `rect` (already shrunk while it is pressed), `label`,
  `action`, `pressed`, `locked` (the ⇧ held as caps lock), `theme` and `icons`. The panel behind
  the keys stays the shell's. The built-in keys draw ⇧ ⌫ ↵ ✓ ▾ and the language key as icons; a
  painter decides for itself.

### 10.4 The shade's tiles and panel

```rust
use fairing::overlay::ShadeTileLayoutCx;
use fairing::{Shell, ShellConfig};

/// Every tile the session may not use is left out, and the rest close up, one row.
fn allowed_only(tiles: &ShadeTileLayoutCx<'_>, rects: &mut [egui::Rect]) {
    let mut x = tiles.area.left() + 12.0;
    for (i, rect) in rects.iter_mut().enumerate() {
        if tiles.is_allowed(i) {
            *rect = egui::Rect::from_min_size(egui::pos2(x, rect.top()), rect.size());
            x += rect.width() + 12.0;
        } else {
            *rect = egui::Rect::NOTHING;
        }
    }
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .shade_tile_layout(allowed_only)
        .build(ctx)
}
```

- The rects come one per tile, in `[overlay] tiles` order (the declared tiles after them),
  `tile_columns` to a row and each row centred — the tiles at rest. `ShadeTileLayoutCx` carries
  `panel` (the panel at its full height), `area` (where the built-in rows go), `columns`,
  `expanded`, and per tile `id(i)`, `index_of(id)` and `is_allowed(i)`.
- **What follows the tiles follows the lowest one.** The row a slider, gauge or panel tile opens,
  the notification list under it, and the stop of a two-step shade all go below the lowest tile
  the layout placed, so a layout that adds a row gets the room for it.
- **An expanded tile leaves its rect and comes back to it**, but the tiles round it stay where
  the layout put them — the built-in rows close up the gap, a layout's do not. The layout is told
  which tile is out (`expanded`) and may close up itself.
- A tile's label is laid out for the built-in tile width and cut to the tile's rect, so a much
  narrower tile shows less of it.

The two painters draw the tiles and the ground under the panel's content:

```rust
use fairing::overlay::{ShadePanelCx, ShadeTileCx, TileState};
use fairing::{ColorRole, Shell, ShellConfig};

/// Square tiles: the state is the fill, the label goes inside, a slider's value is a bar.
fn square_tile(painter: &egui::Painter, tile: &mut ShadeTileCx<'_>) {
    let theme = tile.theme;
    let (fill, ink) = if tile.lit {
        (ColorRole::Primary, ColorRole::OnPrimary)
    } else {
        (ColorRole::SurfaceVariant, ColorRole::OnSurface)
    };
    let mut fill = theme.color(fill);
    if tile.pressed {
        fill = fill.gamma_multiply(0.8);
    }
    if !tile.live {
        fill = fill.gamma_multiply(0.45);
    }
    painter.rect_filled(tile.rect, theme.metrics.corner_radius, fill);
    painter.text(
        tile.rect.center(),
        egui::Align2::CENTER_CENTER,
        tile.label,
        egui::FontId::proportional(theme.metrics.type_scale.small),
        theme.color(ink).gamma_multiply(tile.fade),
    );
    if let TileState::Value(v) = tile.state {
        let bar = egui::Rect::from_min_size(
            tile.rect.left_bottom() - egui::vec2(0.0, 4.0),
            egui::vec2(tile.rect.width() * v, 4.0),
        );
        painter.rect_filled(bar, 0.0, theme.color(ColorRole::Primary));
    }
}

/// A flat panel in the background colour, square at the bottom.
fn flat_panel(painter: &egui::Painter, panel: &mut ShadePanelCx<'_>) {
    let ground = panel.theme.color(ColorRole::Background).gamma_multiply(panel.alpha);
    painter.rect_filled(panel.rect, 0.0, ground);
    panel.ground = ground;
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .shade_tile_painter(square_tile)
        .shade_panel_painter(flat_panel)
        .build(ctx)
}
```

- **A tile painter draws one tile, all of it** — what the built-in tile draws as its puck, icon,
  label, value ring and padlock. It is called for every tile drawn, in `[overlay] tiles` order,
  with `ShadeTileCx`: `rect`, `id`, `label` (in the language on screen), `icon`, `kind`, `state`,
  `lit` (whether the built-in tile would be lit), `live`, `allowed`, `pressed`, `fade`, `theme` and
  `icons`. The shell keeps where the tile goes, its tap and long press, what a tap does, the
  unlock a locked tile asks for and the row a tile opens.
- **A tile going out to its row** — a slider, gauge or panel tile, pressed — shrinks toward the
  row's icon while `fade` drops to 0, and the painter is called for it all the way.
- **A panel painter draws the ground** under the content: what the built-in panel draws as the
  plate and its corners, a card's shadow, frosted backdrop and relief, and a curtain's closing
  line. `ShadePanelCx` carries `rect` (what shows of the panel), `content` (the panel at its full
  height), `corner` (the built-in corners), `panel` (the one shade, or which of the split
  panels), `reveal` (a curtain or a card), `alpha` (how far it has arrived — multiply your
  colours by it), `leaving` (the panel a split shade crosses away from) and `ground`.
- **Set `ground` to your ground's colour.** The notification list fades into it at its ends where
  it overflows, and the built-in tiles sit a locked tile's padlock on a disc of it. It comes in as
  the theme's surface colour.
- With a panel painter a card takes no screenshot of the page for its frosted backdrop
  (`[overlay] card_glass`): the painter is not handed one, so the shell does not make it.

### 10.5 The status bar's items

```rust
use fairing::{Shell, ShellConfig, StatusLayoutCx};

/// The clock in the middle of the bar, whatever slot it is in; the battery left out.
fn clock_in_the_middle(bar: &StatusLayoutCx<'_>, rects: &mut [egui::Rect]) {
    if let Some(clock) = bar.index_of("status.clock").and_then(|i| rects.get_mut(i)) {
        *clock = clock.translate(egui::vec2(bar.rect.center().x - clock.center().x, 0.0));
    }
    if let Some(battery) = bar.index_of("status.battery").and_then(|i| rects.get_mut(i)) {
        *battery = egui::Rect::NOTHING;
    }
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .status_bar_layout(clock_in_the_middle)
        .build(ctx)
}
```

- The rects come one per item the bar would draw — enabled, past its gate, reported by its
  backend — in left, centre, right order, with a slot's `status_item` declarations that its list
  does not name at the end of that slot. They are placed the built-in way: measured, the left
  items from the left edge, the right ones from the right, the centre ones in the middle of what
  is left. `StatusLayoutCx` carries `rect` (the bar), `inner` (the bar less its side padding) and,
  per item, `id(i)`, `index_of(id)`, `slot(i)` and `is_collapsed(i)`.
- **A collapsed item comes empty.** Where the width runs short the bar drops the lowest
  priority first ([03 §1.2](03-chrome.md#12-slot-placement)); those come as `Rect::NOTHING`, and
  `is_collapsed` says which. Give one a rect and it is drawn there — a second row, say.
- A built-in item's width is measured each frame; a `status_item`'s is what it drew last frame.
  The bar keeps the taps — the built-in items' and the bar's own, which opens the shade.

### 10.6 The lock screen and the unlock prompt

```rust
use fairing::access::{LockScreenCx, PromptPiece, UnlockPromptCx};
use fairing::{ColorRole, Shell, ShellConfig};

/// A plain ground, and the time large in the accent colour.
fn lock_screen(painter: &egui::Painter, lock: &mut LockScreenCx<'_>) {
    let theme = lock.theme;
    let ground = theme.color(ColorRole::Background).gamma_multiply(lock.alpha);
    painter.rect_filled(lock.screen, 0.0, ground);
    painter.text(
        lock.clock.center(),
        egui::Align2::CENTER_CENTER,
        lock.time,
        egui::FontId::proportional(theme.metrics.type_scale.heading * 4.0),
        theme.color(ColorRole::Primary).gamma_multiply(lock.alpha),
    );
}

/// A dark veil, and a square card with a hairline round it.
fn unlock_prompt(painter: &egui::Painter, prompt: &mut UnlockPromptCx<'_>) {
    let theme = prompt.theme;
    match prompt.piece {
        PromptPiece::Backdrop => {
            let veil = egui::Color32::from_black_alpha(200).gamma_multiply(prompt.alpha);
            painter.rect_filled(prompt.rect, 0.0, veil);
        }
        PromptPiece::Card => {
            let fill = theme.color(ColorRole::Surface).gamma_multiply(prompt.alpha);
            painter.rect_filled(prompt.rect, 0.0, fill);
            let line = theme.color(ColorRole::Outline).gamma_multiply(prompt.alpha);
            let hairline = egui::Stroke::new(1.0, line);
            painter.rect_stroke(prompt.rect, 0.0, hairline, egui::StrokeKind::Inside);
        }
        _ => {}
    }
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .lock_screen_painter(lock_screen)
        .unlock_prompt_painter(unlock_prompt)
        .build(ctx)
}
```

- **The shell keeps the way in** — the gate's hint, the title and what replaces it, the method
  tabs, the keypad, the dots or the fields, the way out — and draws it over what the painters
  drew. It keeps what is typed, the authenticator and its answer, the lockout, the shake and when
  the modal comes and goes ([05 §5](05-access-control.md#5-the-shells-prompt-and-the-authenticator)).
  The keys and the buttons are widgets and follow the theme (§1).
- **The lock screen painter** draws what the built-in lock screen draws under the way in: the
  ground over the whole screen, the time and the date. `LockScreenCx` carries `screen`, `clock`
  (where the built-in clock goes — beside the way in on a wide panel, above it on a tall one),
  `room` (where the way in is centred), `time` and `date` (written as the built-in ones are: the
  12- or 24-hour switch, the language's order for the date), `alpha`, `leaving`, `theme` and
  `icons`. Leaving, the lock screen grows to 1.04 and fades: the growing is done to what the
  painter drew, the fading is `alpha`.
- **The prompt painter** is called twice a frame, once for each `PromptPiece`: the `Backdrop` over
  the screen — the built-in one is the theme's scrim — and the `Card` under the way in, a floating
  plate in the surface colour. `UnlockPromptCx` carries `piece`, `rect`, `corner` (the built-in
  card's), `alpha` (each piece's own fade: the backdrop's and the card's run on different
  curves), `leaving` and `theme`.
- **The card is on a layer the shell scales and shakes** — in from 0.98, down whole on a screen too
  short for the keypad, side to side at a wrong PIN. `rect` is the card in that layer, so what the
  painter draws there moves with the way in.
- **Multiply your colours by `alpha`.** Both painters get the layer unfaded and are told how far
  the fade has gone, as the shade's are.

### 10.7 The recent screens

```rust
use fairing::workspace::{RecentCardCx, RecentsGroundCx};
use fairing::{ColorRole, Shell, ShellConfig};

/// Flat cards with a coloured top edge, the title and when under it.
fn flat_card(painter: &egui::Painter, card: &mut RecentCardCx<'_>) {
    let theme = card.theme;
    let fade = |role| theme.color(role).gamma_multiply(card.alpha);
    painter.rect_filled(card.rect, 0.0, fade(ColorRole::SurfaceVariant));
    let edge = egui::Rect::from_min_size(card.rect.min, egui::vec2(card.rect.width(), 6.0));
    painter.rect_filled(edge, 0.0, fade(ColorRole::Primary));
    let font = egui::FontId::proportional(theme.metrics.type_scale.body);
    let title = card.rect.left_top() + egui::vec2(12.0, 18.0);
    let when = title + egui::vec2(0.0, font.size * 1.4);
    let ink = fade(ColorRole::OnSurface);
    painter.text(title, egui::Align2::LEFT_TOP, card.title, font.clone(), ink);
    painter.text(when, egui::Align2::LEFT_TOP, card.when, font, fade(ColorRole::Muted));
}

/// A dark ground.
fn dark_ground(painter: &egui::Painter, ground: &mut RecentsGroundCx<'_>) {
    let dark = egui::Color32::from_gray(12).gamma_multiply(ground.alpha);
    painter.rect_filled(ground.rect, 0.0, dark);
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    Shell::builder(ShellConfig::default())
        .recents_card_painter(flat_card)
        .recents_ground_painter(dark_ground)
        .build(ctx)
}
```

- **The shell keeps the recent screens** — where the cards go and the carousel's drag, a tap that
  brings a task forward, the throw that ends one, the screen on show shrinking into its card — and
  draws a card's split button, "Close all" and the words over and under the cards over what the
  painters drew ([03 §2.3](03-chrome.md#23-recent-screens-and-the-split)). The buttons are
  widgets and follow the theme.
- **A card painter draws one card, all of it** — what the built-in card draws as its plate, the
  task's icon and title in the corner, when it was used and the level it needs, and the icon large
  in the middle. It is called for every card drawn, the task that was on show first, with
  `RecentCardCx`: `rect` (moved up while a finger throws it), `corner`, `title` (in the language
  on screen), `when` ("Just now", "3 min ago"), `level` (where the screen needs more than everyone
  has — the built-in card writes it after `when`), `icon`, `current` (the card the screen on show
  shrinks into), `split_button` (where the shell draws it — leave it clear), `alpha`, `theme` and
  `icons`.
- **A ground painter draws what the cards stand on** — the built-in one is a plain ground in the
  background colour over the content area. `RecentsGroundCx` carries `rect`, `over` and `alpha`.
  Over the desktop (`RecentsOver::Desktop`) it fades in over it with the cards; over a task
  (`RecentsOver::Task`) it is whole from the start, behind the screen shrinking into its card. A
  screen lifted from the bottom edge with the gesture bar stands on it too — the recent screens it
  may become.
- **Multiply your colours by `alpha`.** The cards fade in, the task on show's card takes over from
  its shrinking screen, and a card thrown away fades as it goes up; the painters are told, not
  faded.

### 10.8 The widgets

```rust
use fairing::widgets::{ButtonKind, ButtonLook, SwitchLook, WidgetPainters};
use fairing::{ColorRole, Shell, ShellConfig};

/// Square buttons: the kind is the fill, the press darkens it, the focus is a bar under it.
fn square_button(painter: &egui::Painter, button: &mut ButtonLook<'_>) {
    let theme = button.theme;
    let role = match button.kind {
        ButtonKind::Primary => ColorRole::Primary,
        ButtonKind::Danger => ColorRole::Danger,
        _ => ColorRole::SurfaceVariant,
    };
    let mut fill = theme.color(role).gamma_multiply(1.0 - 0.2 * button.press);
    if !button.enabled {
        fill = fill.gamma_multiply(theme.control.disabled_alpha);
    }
    painter.rect_filled(button.drawn, 0.0, fill);
    let ink = theme.color(ColorRole::OnSurface);
    let at = button.drawn.center();
    painter.text(at, egui::Align2::CENTER_CENTER, button.label, button.font.clone(), ink);
    if button.focused {
        let bar = egui::Rect::from_min_max(button.drawn.left_bottom(), button.drawn.right_bottom())
            .expand2(egui::vec2(0.0, 2.0));
        painter.rect_filled(bar, 0.0, theme.color(ColorRole::Focus));
    }
}

/// A switch that is a lamp: lit on, dark off, lighting up as the knob travels.
fn lamp_switch(painter: &egui::Painter, switch: &mut SwitchLook<'_>) {
    let theme = switch.theme;
    let off = theme.color(ColorRole::Outline);
    let lit = off.lerp_to_gamma(theme.color(ColorRole::Primary), switch.travel);
    painter.circle_filled(switch.track.center(), switch.track.height() / 2.0, lit);
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    let painters = WidgetPainters::new()
        .button(square_button)
        .switch(lamp_switch);
    Shell::builder(ShellConfig::default())
        .widget_painters(painters)
        .build(ctx)
}
```

- **One painter per kind, each optional.** A kind without one draws itself, so painting the
  buttons leaves the switches as they were. Every widget of a kind is drawn by its painter: on
  your screens, through `cx.widgets()`, and on the shell's own — the settings screens, the unlock
  prompt's keys and buttons, the recent screens' buttons.
- **The widget keeps everything that is not paint**: the rect it takes in the layout and the rect
  that is pressed, the press and the drag, the value and when it changes, its animations, focus
  and keys. The painter is told the widget's look for the frame and draws all of it — the focus
  ring too, where `focused` says so.
- **A look's rects are in the coordinates the widget is drawn in.** Where its layer is scaled or
  moved — the unlock prompt's card, a screen sliding in — the same transform carries what the
  painter drew.
- **A widget of several pieces is told piece by piece.** The PIN pad calls its painter for the
  dot row and once per key; the dropdown for the control, and while it is open for the panel and
  each option on it. `part` says which piece.
- **What is typed stays egui's.** A text field's text, hint, caret and selection, and an editable
  number field's figure, are egui's `TextEdit`, drawn over what the painter drew. The painter draws
  the field around them.
- **A control on a widget stays its own kind.** A row's switch or radio and a card's action disc
  are drawn over the painter's drawing by their own kinds — through the switch, radio or icon
  button painter where one is given — in the slot the look names (`switch`, `radio`, `action`).
- **Without a `WidgetCx` there are no painters.** `paint_bar` and `CountBadge::paint_over` draw
  the built-in look. The shell's own uses of them follow their own painters: the notification
  banner's bar is the banner's (`heads_up_painter`), and the desktop's icon badge is the slot's
  (`slot_painter`).
- Outside the shell, a `fairing_widgets::WidgetCx` takes the painters in its `painters` field.

| Kind | Look | Told |
|---|---|---|
| `BigButton` | `ButtonLook` | `rect`, `drawn` (grown while pressed), `label`, `font`, `icon`, `kind`, `pressed`, `press`, `enabled`, `focused`, `hold` (a long press: where the ring goes, `progress`, `done`) |
| `IconButton` | `IconButtonLook` | `rect`, `disc`, `icon`, `name`, `kind`, `pressed`, `press`, `enabled`, `focused`, `hold` |
| `Switch` | `SwitchLook` | `rect`, `track`, `on`, `travel` (where the knob is, 0 to 1 — under the finger while dragged), `pressed`, `press`, `enabled`, `focused` |
| `Checkbox` | `CheckboxLook` | `rect`, `drawn`, `on`, `indeterminate`, `mark` (0 to 1), `pressed`, `press`, `enabled`, `focused` |
| `Radio` · a `RadioGroup`'s marks | `RadioLook` | `rect`, `drawn`, `selected`, `mark`, `pressed`, `press`, `enabled`, `focused` — a group draws its rows' tint, labels and focus itself |
| `SegmentedControl` (its strip) | `SegmentedLook` | `rect`, `strip`, `band`, `labels`, `selected`, `travel` (where the face is, in segments), `held`, `press`, `enabled`, `focused`, and `cell(at)` — stacked, the options are rows |
| `Chip` · a `ChipRow`'s chips | `ChipLook` | `rect`, `drawn`, `label`, `icon`, `selected`, `mark`, `pressed`, `press`, `enabled`, `focused` |
| `TouchSlider` | `SliderLook` | `rect`, `track` (thicker while pressed), `axis` (where 0 % and 100 % are), `value`, `range`, `fraction`, `handle`, `value_text`, `style`, `colors`, `pressed`, `press`, `enabled`, `focused` — the shade's brightness row draws its own track |
| `Stepper` | `StepperLook` | `rect`, `minus` and `plus` — each a `StepEnd`: `rect`, `live` (enabled and not at that end), `pressed`, `press` — `value`, `range`, `enabled` |
| `NumberField` | `NumberFieldLook` | `rect`, `track`, `minus`, `plus`, `figure` (the cell between the ends), `figure_text`, `value`, `unit` (its text and where it goes), `enabled` — disabled, nothing is typed and the painter draws the figure too |
| `TextField` | `TextFieldLook` | `rect`, `focused`, `empty` (the hint shows), `password`, `multiline`, `enabled` |
| `WheelPicker` | `WheelLook` | `rect`, `window` (where the chosen row stands), `options`, `rows` — each a `WheelRow`: `index`, `rect` where it is this frame, `distance` from the window in rows — `selected`, `wrap`, `enabled` |
| `Dropdown` | `DropdownLook` | `part`: the control (`Trigger` — `open`, `pressed`, `head` for the open panel's first row), the panel (`Panel` — `shown`, `unroll`, a sheet's `close`) or an option (`Option` — `index`, `selected`, `pressed`, `cell`). Then `rect`, `trigger`, `opener`, `label`, `text`, `hint`, `enabled` and `ground`: the panel's colour, which a scrolling list fades into — set it to yours. The dim behind a sheet stays the widget's |
| `PinPad` | `PinPadLook` | `part`: the dot row (`Dots` — `entered`, `length`) or a key (`Key` — `key`, `live`, `pressed`, `press`; erase and OK are not live on an empty buffer). Then `rect`, `drawn` (a key grown while pressed), `enabled` |
| `PatternPad` | `PatternPadLook` | `rect`, `dots`, `dot`, `reach`, `taken` (how far each dot has turned, 0 to 1), `path`, `finger`, `mark`, `enabled` — with `show_path(false)` the path is kept from the painter too: `path` is empty, `finger` is `None`, no dot is taken |
| `ListRow` | `RowLook` | `rect`, `title`, `subtitle`, `icon`, `icon_color`, `badge`, `title_role`, `strong`, `value`, `trailing_icon`, `switch` and `radio` (where they go), `chevron`, `disclosure`, `separator`, `pressed`, `enabled` |
| `ProgressBar` | `ProgressBarLook` | `rect`, `track`, `fill` — a `ProgressFill`: `To` a share, a moving `Segment`, or a `Pulse` under reduced motion — `readout` (its text and where it goes), `tone`, `stop_indicator`, `steps`, `stale` (the breath of a bar that has heard nothing), `enabled` |
| `ProgressRing` | `ProgressRingLook` | `rect`, `center`, `radius`, `thickness`, `start` and `sweep` (radians), `fill`, `value_text`, `label`, `style`, `tone`, `stale`, `enabled`, and `point(t)`: the point at share `t` of the arc |
| `Meter` | `MeterLook` | `rect`, `track`, `scale`, `value`, `pointer` (the value eased), `normal`, `setpoint`, `limits` (read with `at()`, `is_high()`, `caption()`, `verdict()`), `readout`, `verdict`, `stale`, `enabled`, and `x(value)`: where a value falls on the track |
| `StatusLamp` | `LampLook` | `rect`, `lens` (the disc's square), `word` (where the word goes), `label`, `state`, `enabled` |
| `CountBadge` | `BadgeLook` | `rect`, `value`, `text` (as the built-in badge writes it — `99+`), `tone`, `enabled` |
| `MediaCard` | `MediaCardLook` | `rect`, `picture`, `text`, `image` (crop it with `fairing_widgets::fit::cover_uv`), `shape`, `title`, `subtitle`, `value`, `veil`, `action`, `selected`, `pressed`, `enabled` |
| `FeatureCard` | `FeatureCardLook` | `rect`, `text` (the words' column), `title`, `body`, `art` (and its square), `action`, `lit`, `outlined`, `pressed` |

Every look also carries `theme` and `icons`.

---

## Related

| Topic | Page |
|---|---|
| Adding items, tiles and icons (the narrow extensions) | [03 Chrome](03-chrome.md) |
| Declaring screens · `ChromePolicy` | [02 Screens](02-screens.md) |
| Gate names and level assignment | [05 Access control](05-access-control.md) |
| Injecting backends (`.services(..)`) | [06 Services](06-services.md) |
| The full config key tables | [07 Config reference](07-config-reference.md) |
| How the crate is put together | [Architecture](../architecture.md) |
