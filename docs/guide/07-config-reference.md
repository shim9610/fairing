# 07. Config reference

Every section and key of `fairing.toml`, read off the source
(`crates/fairing/src/config.rs`). This page does not argue for the shape — follow
the links beside each table for that.

## 0. Load rules

| Rule | What happens |
|---|---|
| The entry point | `ShellConfig::load(path)` — called once, at start-up |
| No file | Not an error: `ShellConfig::default()` |
| A file that cannot be read | `Error::Io` |
| Bad syntax or a wrong type | `Error::Config` (`ShellConfig::from_toml` → `toml::from_str` fails) |
| A value out of range | `Error::Config` from `ShellConfig::validate` — the "Validation" column below |
| A float that is not a number | `Error::Config`. `nan` and `inf` are valid TOML floats, but every float key must be finite, those whose "Validation" column says "None" included |
| An unknown section or key | Silently ignored (`#[serde(default)]` plus serde's default behaviour). A typo still boots; only range and cross-reference errors stop start-up |
| A section behind a feature you turned off (`overlay`, `osk`) | **Still parsed.** A config file with `[overlay]` reads fine with the feature off, so two builds with different feature sets can share one file |

Whether an id in a `Vec<String>` that was never declared warns or silently becomes
an empty slot differs per section. See
[08 §2](08-troubleshooting.md#2-referring-to-an-id-that-was-never-declared).

## 1. The sections

| Section | Required | Feature | Covers |
|---|---|---|---|
| `[shell]` | No | — | Locale, theme choice, repaint policy, cursor |
| `[status_bar]` | No | — | Top bar slots |
| `[nav_bar]` | No | — | Bottom bar items |
| `[desktop]` / `[[desktop.pages]]` / `[desktop.abyss]` | No | — | Grid, dock, rail, wallpaper, per-page icon overrides |
| `[overlay]` | No | `overlay` | Shade and tiles |
| `[notify]` | No | — | Toasts and the notification centre |
| `[osk]` | No | `osk` | On-screen keyboard |
| `[gesture]` | No | — | Edge swipes, long press, the emergency gesture |
| `[motion]` plus twelve sub-tables | No | — | Animation tokens |
| `[theme]` / `[theme.palette]` | No | — | Palette preset and colour overrides |
| `[access]` / `[access.gates]` / `[access.pin_table]` / `[access.pattern_table]` / `[access.lock_screen]` | No | — | Level table, gate assignment and the reference authenticator |
| `[workspace]` | No | — | Two panes and the recent screens ([§16](#16-workspace)) |

Every section is `#[serde(default)]`, so an empty `fairing.toml` is valid.

---

## 2. `[shell]`

| Key | Type | Default | Meaning | Validation | Code |
|---|---|---|---|---|---|
| `locale` | `String` | `"en"` | The language to start in: a tag such as `"en"`, `"ko"` or `"ko-KR"` (a region finds its language's table) | None (any string). One with no table **warns at start-up** and the text stays English — not an error | `crate::i18n::Strings::new`. English and Korean are built in; add a language, or change a built-in entry, with `ShellBuilder::translations` ([04 §9](04-customization.md#9-text-and-translations)). The `ui.locale` setting (the `settings.locale` screen) switches it while the shell runs, from the next frame |
| `theme` | `String` | `"dark"` | `"dark"` or `"light"` | Anything else **warns and falls back to dark** — not an error | `ShellConfig::build_theme` |
| `idle_lock_secs` | `u64` | `0` | **Deprecated** — the key is `[access] idle_lock_secs` (§12) | None | Read in its place, with a warning at start-up, where `[access] idle_lock_secs` is 0. Where both are set, `[access]` wins and this one is warned about |
| `repaint` | `RepaintMode` | `"reactive"` | `"reactive"` or `"continuous"` (debug: redraw every frame) | A **string enum**, so anything else is `Error::Config` — a parse failure, not a fallback | `schedule_repaint` inside `Shell::frame` — [08 §3](08-troubleshooting.md#3-confirming-0-fps-when-idle) |
| `cursor` | `CursorPolicy` | `"auto"` | `"auto"`, `"hidden"` or `"visible"`. Auto hides it on touch and shows it for a mouse | String enum; anything else is `Error::Config` | |

Injecting `.theme(Theme)` in code makes `theme` irrelevant
([04 §1](04-customization.md#1-injecting-a-theme)).

---

## 3. `[status_bar]`

| Key | Type | Default | Meaning | Validation | Code |
|---|---|---|---|---|---|
| `enabled` | `bool` | `true` | Show the bar | None (per-screen control is `ChromePolicy::status_bar`) | `StatusBar::from_config` |
| `height` | `Option<f32>` | unset | The bar height in du, **pinned** over the crate's metrics spec. Unset, it is 7 mm, at least 32 du | Error when `enabled` and `height <= 0.0`, `nan` or `inf`. An integrator's `metrics_spec` or injected theme wins, and `build` warns that this was ignored | `metrics.status_bar_height` (08 D245) |
| `left` | `Vec<String>` | `["status.clock"]` | Left slot order | An undeclared id is **neither an error nor a warning** — it renders as an empty slot | [08 §2](08-troubleshooting.md#2-referring-to-an-id-that-was-never-declared) |
| `center` | `Vec<String>` | `[]` | Centre slots | Same | |
| `right` | `Vec<String>` | `["status.notifications", "status.bluetooth", "status.wifi", "status.battery"]` | Right slots | Same | |
| `icon_size` | `Option<f32>` | unset | The status icons' size in du, pinned. Unset, it is 3.6 mm, at least 18 du | Must be a number `> 0.0` (not `nan` or `inf`) **regardless of `enabled`**. The same precedence as `height` | `metrics.status_icon_size` — the status bar and the shade's notification rows ([04 §1.5](04-customization.md#15-two-things-to-watch)) |
| `icon_color` | `String` | `"on_surface"` | A palette role or `"#RRGGBB"` | **None** — a parse failure falls back to `on_surface` with no warning | `IconColor::parse` |
| `tap_opens_shade` | `bool` | `true` | Tapping the bar toggles the shade | None | With the `overlay` feature off, the tap does nothing |
| `clock_format` | `String` | `"hm"` | The clock's **shape**: `"hm"`, `"hms"`, `"date_hm"`, or their 12-hour spellings `"hm12"`, `"hms12"`, `"date_hm12"` | Anything else **warns and falls back to `hm`** | `ClockFormat`. The hour convention here is only the starting point — the `ui.clock_12h` setting (the "24-hour time" switch on `settings.datetime`) turns whichever shape you name into its other half while the shell runs |

Built-in slot ids and your own declarations are in [03 Chrome](03-chrome.md).

---

## 4. `[nav_bar]`

| Key | Type | Default | Meaning | Validation | Code |
|---|---|---|---|---|---|
| `enabled` | `bool` | `true` | Show the bar. `false` draws nothing and the content grows | None | `NavBar::from_config` |
| `style` | `String` | `"buttons"` | `"buttons"` or `"gesture"` — the home indicator, and the bottom edge's swipes in place of the buttons ([03 §2.6](03-chrome.md#26-the-gesture-style)). `items` means nothing with gestures | Anything else **warns and falls back to buttons** | `NavStyle` |
| `items` | `Vec<String>` | `["back", "home", "recents"]` | `"back"`, `"home"`, `"recents"`, `"split"`, or the id of a `nav_item` you declared | An undeclared id is neither an error nor a warning ([08 §2](08-troubleshooting.md#2-referring-to-an-id-that-was-never-declared)) | `NavItem::parse` |
| `height` | `Option<f32>` | unset | The bar height in du, pinned. Unset, it is one finger plus 8 du, at least 56 du | Error when `enabled` and `height <= 0.0`, `nan` or `inf`. The same precedence as `[status_bar] height` | `metrics.nav_bar_height` (08 D245) |
| `back_edges` | `Vec<String>` | `["left"]` | Which edges accept the back swipe | Each entry must be `"left"` or `"right"`, or it is an **error** with no fallback. An empty array means no gesture back — but combined with `style = "gesture"` that is also an error, since it leaves no way back | `gesture::Edge::parse` plus `EdgeMask` |

---

## 5. `[desktop]`

| Key | Type | Default | Meaning | Validation | Code |
|---|---|---|---|---|---|
| `columns` | `u8` | `4` | Column count; `0` means auto, from `icon_cell` | Error above `MAX_COLUMNS` (12) unless it is `0` | `DesktopView::from_config` |
| `rows` | `u8` | `3` | Row count; `0` means auto | Error above `MAX_ROWS` (8) unless it is `0` | Same |
| `dock` | `Vec<String>` | `[]` | Dock order | **No count limit.** An undeclared id **warns and is skipped** — unlike the bar slot lists, this one does warn | `apply_dock_order` |
| `dock_edge` | `String` | `"bottom"` | Which edge the dock clings to: `"bottom"`, `"top"`, `"left"`, `"right"` | Anything else warns and uses `bottom` | A bottom band is the phone convention; a portrait panel often wants a side rail and a wide instrument screen a top row |
| `dock_band` | `Option<f32>` | `None` | Set it and the dock stops clinging to an edge and becomes **a row across the desktop**, at this fraction (0..=1) of the content. The band paints no background, so only the icons sit over the wallpaper | None | |
| `pages` | `Vec<PageConfig>` | `[]` | The `[[desktop.pages]]` list | §5.1 | |
| `label_lines` | `u8` | `2` | Icon label lines | None (`0` becomes 1 internally) | |
| `wallpaper` | `String` | `"background"` | A palette role, `"#RRGGBB"`, `"abyss"` (the procedural deep-sea background) or `"file:<path>"` | **None** — a bad value warns and falls back to the `background` role. Note this is the opposite of `[theme.palette]`, which errors | `Wallpaper::Solid` / `Fixed` |
| `wallpaper_fit` | `String` | `"cover"` | For `file:` only: `"cover"`, `"contain"` or `"stretch"` | Anything else warns and uses `cover` | Same image on a different aspect ratio: `cover` crops, `contain` letterboxes, `stretch` distorts |
| `rail` | `String` | `"none"` | **Icon rail** — pin the icons down one side and open screens beside them: `"none"`, `"left"`, `"right"` | Anything else warns and uses `none` | In the default layout the icon grid owns the middle and an opening screen **covers** it. A shop kiosk wants the opposite: the menu always visible, only the content changing |
| `rail_width` | `f32` | `0.0` | Rail width as a fraction of the content width; `0` means the default 0.26 | None | A fraction on purpose — an absolute width means different things on a 480 px and a 1920 px panel. It never goes narrower than one icon cell |
| `label_legibility` | `String` | `"none"` | Label legibility over a busy wallpaper: `"none"`, `"shadow"`, `"veil"` | Anything else warns and uses `none` | See below |
| `abyss` | `AbyssConfig` | defaults | `[desktop.abyss]`, only read when `wallpaper = "abyss"` | §5.2 | |
| `long_press` | `String` | `"info"` | What holding an icon does: `"info"` brings up its info popover — the title, the level it needs, the description ([03 §3.8](03-chrome.md#38-holding-an-icon-the-info-popover)); `"none"` only reports `ShellEvent::IconLongPressed` | Anything else warns and uses `info` | `"none"` is for a device with a menu of its own on the event, so the two do not come up together |

**`label_legibility` defaults to `none` on purpose.** The shell does not lay a veil
over your background; **the picture itself is responsible for legibility.** Author
a dark background and no correction is needed, and turning correction on by
default would fog a good one.

| Value | What it does | Cost |
|---|---|---|
| `"none"` | Nothing | 0 |
| `"shadow"` | A dark shadow behind the label glyphs | Text draw calls ×2 |
| `"veil"` | A translucent `scrim` panel over the whole content area | One rectangle |

`shadow` is usually the answer — it keeps the picture and saves the label.

### 5.1 `IconOverride` in `[[desktop.pages]]`

One array element is one page; the `icons` array inside it overrides icon slots on
that page.

| Key | Type | Default | Meaning | Validation |
|---|---|---|---|---|
| `id` | `String` | `""` | The id you declared in code, no prefix | An undeclared id **warns and the whole override is dropped** |
| `label` | `Option<String>` | `None` | Label override | None |
| `description` | `Option<String>` | `None` | Description override — what the icon's info popover says under the title. A key, like `label` | None |
| `icon` | `Option<String>` | `None` | Built-in icon name override | Not in the built-in set (`fairing::icons::builtin::NAMES`) **warns and is ignored**; the original icon stays |
| `locked` | `Option<String>` | `None` | How a failed gate looks | `"show"` (padlock) or `"hide"` only; anything else **warns and is ignored** |
| `col` | `Option<u8>` | `None` | Pin the column | — |
| `row` | `Option<u8>` | `None` | Pin the row | **Both `col` and `row` are required** for a pinned position. One alone silently falls back to automatic placement |

```toml
[[desktop.pages]]
icons = [
  { id = "dashboard" },
  { id = "settings.wifi", label = "Wi-Fi", icon = "wifi", locked = "show" },
  { id = "diagnostics", locked = "hide" },
  { id = "camera", col = 2, row = 0 },
]
```

### 5.2 `[desktop.abyss]`

Only read when `wallpaper = "abyss"`, and only meaningful with the `brand` feature
on — without it the value warns and falls back to the `background` role. Full
treatment in [09 Branding](09-branding.md).

| Key | Type | Default | Meaning |
|---|---|---|---|
| `tier` | `AbyssTier` | `"lite"` | Quality tier: `"flat"`, `"lite"`, `"full"`. Higher tiers cost vertices; the shell drops a tier itself if the bake blows the budget |
| `seed` | `u32` | fixed | Seeds the procedural layout, so a given seed always draws the same scene |
| `rays` · `bubbles` · `fish` · `mantas` | `u8` | per tier | How many of each element |
| `light_x` | `f32` | — | Where the light comes from, across the width |
| `veil_top` · `veil_field` · `veil_bottom` | `f32` | — | Depth-veil density at the top, in the field, and at the bottom |
| `animate` | `bool` | `false` | Move the manta every frame. **This breaks idle 0 fps** and the shell warns when you set it |
| `bake_budget_ms` | `f32` | — | Time budget for baking the mesh. Overrunning drops a tier |
| `colors` | `String` | `"palette"` | `"palette"` (follow the theme) or `"art"` (the original artwork's colours) |

---

## 6. `[overlay]` (feature `overlay`)

`edge_px` and the shade snap ratio are not here — `theme.metrics.edge_px` and
`[motion.shade] snap_ratio` are their single sources.

| Key | Type | Default | Meaning | Validation |
|---|---|---|---|---|
| `layout` | `String` | `"unified"` | `"unified"` (Android-style): one shade wherever it is pulled from. `"split"` (One UI and iOS): two panels picked by where the pull starts — left of `split_ratio` the notifications, right of it the controls — each filling the height | An unknown value **warns and uses unified** |
| `tiles` | `Vec<String>` | `["tile.wifi", "tile.bluetooth", "tile.brightness", "tile.volume", "tile.lock", "tile.theme"]` | Tile order. Mix built-in ids and your own declarations | Neither built-in nor declared **warns and is skipped**. A declared tile missing from the list is appended at the end |
| `tile_columns` | `u8` | `6` | Tiles per row | `0` is an error |
| `footer` | `Vec<String>` | `["clear_all", "lock", "settings"]` | The footer buttons, left to right. `[]` draws none — the subject's name and the level dot stay, being state rather than controls | An unknown name **warns and is skipped** |
| `max_height_ratio` | `f32` | `0.85` | Maximum shade height as a fraction of the screen | Outside `(0.0, 1.0]` is an error |
| `peek_ms` | `u64` | `2000` | How long a peeked status bar stays | None |
| `two_step` | `bool` | `false` | Open in two steps: the first pull stops at the tiles, a second carries on to the notifications. Resting at the stop, the shade lies over the live page and a press outside closes it | Ignored, with a warning, under `layout = "split"` |
| `reveal` | `String` | `"curtain"` | How the shade comes into view. `"curtain"` draws the panel down from the top edge. `"card"` is a floating card already at its final size and place, which the pull fades and slides in; the page behind is not dimmed | An unknown name **warns and draws the curtain** |
| `card_width_ratio` | `f32` | `0.46` | A card's width as a share of the screen's. Never narrower than the tile row needs; where it would cover more than three quarters of the screen, it takes the full width less its insets | Outside `(0.0, 1.0]` is an error |
| `card_anchor` | `String` | `"press"` | Which side of a wide screen a card rests on: `"press"` (the side the pull started from, the middle when opened without a finger), `"left"`, `"right"` or `"center"` | An unknown name **warns and behaves as `"press"`** |
| `split_ratio` | `f32` | `0.5` | Where a split shade's two panels divide the top edge, as a share of the width from the left. One UI's phone layout is `0.7` | Outside `(0.0, 1.0)` is an error |
| `card_glass` | `f32` | `0.7` | How opaque a card is over its frosted backdrop: `0` is clear glass, `1` turns the glass off. Where the runner sends no screenshot back, the card stays solid. With a `shade_panel_painter` there is no frost: the painter draws the ground ([04 §10.4](04-customization.md#104-the-shades-tiles-and-panel)) | Outside `0.0..=1.0` is an error |
| `card_relief` | `f32` | `1.0` | How raised a card looks: `0` flat, `1` as designed, `2` twice it. An e-ink or 16-level panel wants `0` here and `card_glass = 1` | Outside `0.0..=2.0` is an error |

A built-in tile left out of `tiles` simply does not appear — that is how you
remove one. See [03 §4.4a](03-chrome.md#44a-removing-reordering-adding).

---

## 7. `[notify]`

| Key | Type | Default | Meaning | Validation |
|---|---|---|---|---|
| `heads_up` | `bool` | `true` | Show a new notification briefly as a banner | None |
| `max_visible` | `u8` | `2` | Toasts on screen at once | `0` is an error |
| `max_items` | `u16` | `100` | Notification centre capacity; the oldest non-persistent ones drop first | None |
| `toast_ms` | `u64` | `3000` | Default toast duration | None |
| `dismiss_button` | `bool` | `true` | Draw the `×` on a shade notification row. Turn it off and the only ways to clear are **a left swipe and "Clear all"** | None |

Always parsed regardless of features. There is no key that disables notifications
— you simply do not post any ([04 §6.1](04-customization.md#61-what-can-be-turned-off-and-how)).

---

## 8. `[osk]` (feature `osk`)

| Key | Type | Default | Meaning | Validation |
|---|---|---|---|---|
| `height_ratio` | `f32` | `0.38` | Keyboard height as a fraction of the screen, up to `osk_max_key` a row unless the code lifts that cap ([04 §1.2](04-customization.md#12-metrics-tokens)) | Outside `(0.0, 1.0]` is an error |
| `min_key_px` | `f32` | `48.0` | Minimum key height (du), the gaps between rows not counted. It wins over `osk_max_key` where the two cross; the room above the nav bar still bounds the keyboard | Must be `> 0.0` |
| `layout` | `String` | `"qwerty"` | `"qwerty"`, `"numpad"` or `"hangul"` (`"ko"` also works) | Anything else **warns and falls back to qwerty** |
| `numpad_decimal` | `bool` | `true` | A decimal point on the numpad | None |
| `numpad_sign` | `bool` | `false` | A `-` key on the numpad, which types a minus sign | None |

`layout = "hangul"` is the two-set Korean layout. Jamo do not go in raw — a
`HangulComposer` assembles them into syllables. A `한/영` key on the bottom row
switches to Latin QWERTY and back; apart from the spacebar shrinking from four
cells to three, the grid is the same.

**A syllable being composed goes into the field as real text.** `ㅎ → 하 → 한`
appears jamo by jamo, and each new jamo erases the previous glyph and types the
new syllable. Space, moving to another field, the `한/영` switch, and **a press
outside the keyboard** all end the composition.

> `ImeEvent::Preedit` is deliberately not used. It only reaches the widget that
> holds focus that frame, and one stray empty `Preedit("")` from the integration
> layer wipes the composing string — which is why composing text was invisible on
> real hardware. The cost is that there is no composition underline.

> Load a font with Hangul glyphs or the key labels render as tofu —
> [08 §6](08-troubleshooting.md#6-korean-renders-as-tofu). The font and the input
> method are independent.

---

## 9. `[gesture]`

Slop, tap and long-press durations live in `[motion]`; the edge width is
`theme.metrics.edge_px`.

| Key | Type | Default | Meaning | Validation |
|---|---|---|---|---|
| `enabled` | `bool` | `true` | Gestures on or off globally. Off still leaves **the emergency gesture** | None |
| `hold_ms` | `u64` | `150` | How long a swipe must rest to count as stopped | None |
| `emergency_ms` | `u64` | `2000` | The emergency gesture — a top-corner long press | None |
| `emergency_corner_px` | `f32` | `64.0` | The side of the corner square | None |

Gesture handles ([03 §9.2](03-chrome.md#92-gesture-handles)) are set in code, one handle at a
time: the reach, the diagonal angle and the rest are each handle's own. A finger counts as still
while it stays within half of `[motion] slop_px`. Gesture regions
([03 §9.3](03-chrome.md#93-gesture-regions-of-your-own)) are code too, with no keys of their own.
`enabled = false` turns the handles and the regions off with the shell's gestures.

---

## 10. `[motion]`

The tuning workflow (`motion_lab`) is in
[04 §8](04-customization.md#8-tuning-motion). This section is keys, defaults and
validation only.

| Key | Type | Default | Meaning | Validation |
|---|---|---|---|---|
| `reduce` | `bool` | `false` | `true` makes every tween 0 ms and every spring settle instantly | None |
| `spring` | `SpringConfig` (§10.1) | `{k=400, c=40}` | The base spring | §10.1 |
| `snap_ratio` | `f32` | `0.33` | Release distance threshold, as a fraction | Outside `[0.0, 1.0]` is an error |
| `fling_px_s` | `f32` | `800.0` | Release velocity threshold | Negative is an error |
| `slop_px` | `f32` | `12.0` | Drag detection slop | Negative is an error |
| `tap_ms` | `u64` | `300` | Tap window | None |
| `long_press_ms` | `u64` | `500` | Long press. This is also what quick-settings tiles and desktop icons use ([03 §4.4d](03-chrome.md#44d-long-pressing-a-tile), [§3.8](03-chrome.md#38-holding-an-icon-the-info-popover)) | None |
| `push` | `PushConfig` (§10.2) | below | A3 push | §10.2 |
| `pop` | `DurationConfig` (§10.3) | `{ms=200}` | A3 pop | None |
| `home` | `HomeConfig` (§10.4) | below | A2 home ↔ task | §10.4 |
| `press` | `PressConfig` (§10.5) | below | A7 press | §10.5 |
| `shade` | `ShadeConfig` (§10.6) | below | A1 shade | §10.6 |
| `page` | `PageMotionConfig` (§10.7) | below | A4 page swipe | §10.7 |
| `osk` | `OskMotionConfig` (§10.8) | below | A5 keyboard | None |
| `toast` | `ToastMotionConfig` (§10.9) | below | A6 toasts and heads-up | None |
| `switch` | `DurationConfig` (§10.10) | `{ms=140}` | A7 switch | None |
| `crossfade_ms` | `u64` | `120` | Parametric state-icon crossfade | None |
| `theme_fade_ms` | `u64` | `200` | Dark/light palette interpolation | None |
| `clear_top_ms` | `u64` | `160` | The clear-top crossfade when a Single screen is reused | None |
| `split_crossfade_ms` | `u64` | `180` | The split shade's header crossing between its two panels | None |
| `panes` | `PanesMotionConfig` (§10.11) | below | A8 the workspace's two panes | None |
| `overview` | `OverviewMotionConfig` (§10.12) | below | A10 the recent screens | None |

### 10.1 `[motion.spring]`

| Key | Default | Validation |
|---|---|---|
| `k` | `400.0` | Must be `> 0.0` |
| `c` | `40.0` | Must be `>= 0.0` (`2√k` is critical damping) |

### 10.2 `[motion.push]`

| Key | Default | Validation |
|---|---|---|
| `ms` | `220` | None |
| `parallax` | `0.25` | Outside `[0.0, 1.0]` is an error |
| `dim` | `0.15` | Outside `[0.0, 1.0]` is an error |

### 10.3 `[motion.pop]`

| Key | Default |
|---|---|
| `ms` | `200` |

### 10.4 `[motion.home]`

| Key | Default | Validation |
|---|---|---|
| `open_ms` | `240` | None |
| `close_ms` | `200` | None |
| `desktop_scale` | `0.92` | Outside `(0.0, 1.0]` is an error |

### 10.5 `[motion.press]`

| Key | Default | Validation |
|---|---|---|
| `ms` | `80` | None |
| `release_ms` | `120` | None |
| `scale` | `0.97` | Outside `(0.0, 1.0]` is an error |

### 10.6 `[motion.shade]`

| Key | Default | Validation |
|---|---|---|
| `spring` | `{k=400, c=40}` | As §10.1; write it as `[motion.shade.spring]` |
| `snap_ratio` | `0.33` | Outside `[0.0, 1.0]` is an error |
| `rubber` | `0.25` | Must be `>= 0.0` |
| `rubber_max_px` | `40.0` | None |
| `card_close_ms` | `90` | None. How long a card takes to go — `[overlay] reveal = "card"` only; 0 under `reduce` |

### 10.7 `[motion.page]`

| Key | Default | Validation |
|---|---|---|
| `spring` | `{k=300, c=35}` | As §10.1 |
| `fling_px_s` | `600.0` | Must be `>= 0.0` |
| `rubber` | `0.3` | Must be `>= 0.0` |
| `rubber_max` | `0.15` | Must be `>= 0.0` |

### 10.8 `[motion.osk]`

| Key | Default |
|---|---|
| `show_ms` | `180` |
| `hide_ms` | `160` |
| `hide_debounce_ms` | `100` |

### 10.9 `[motion.toast]`

| Key | Default |
|---|---|
| `in_ms` | `160` |
| `out_ms` | `200` — also how long a dismissed notification's slot takes to fold shut |
| `shift_ms` | `120` |
| `heads_up_in_ms` | `220` |
| `heads_up_out_ms` | `200` |
| `heads_up_hold_ms` | `4000` |

### 10.10 `[motion.switch]`

| Key | Default |
|---|---|
| `ms` | `140` |

### 10.11 `[motion.panes]`

The workspace's two panes ([02 §6.1](02-screens.md#61-two-panes)) — not the split
shade, whose crossing is `split_crossfade_ms`.

| Key | Default | Meaning |
|---|---|---|
| `enter_ms` | `220` | The pane coming in slides in while the one there shrinks to its share |
| `leave_ms` | `220` | The pane going slides out while the other grows to fill |
| `even_ms` | `200` | A double tap on the divider evens the panes out |

### 10.12 `[motion.overview]`

The recent screens ([03 §2.3](03-chrome.md#23-recent-screens-and-the-split)). A
card is thrown away past 120 du or faster than `[motion] fling_px_s`, and a drag picks its axis
after `slop_px`.

| Key | Default | Meaning |
|---|---|---|
| `in_ms` | `240` | The screen on show shrinking into its card, and growing back out |
| `cards_in_ms` | `160` | The other cards fading in, within `in_ms` |
| `throw_ms` | `200` | A thrown card leaving |

---

## 11. `[theme]` and `[theme.palette]`

| Key | Type | Default | Meaning | Validation |
|---|---|---|---|---|
| `preset` | `String` | `"base"` | Palette preset: `"base"` (neutral), `"abyss"` (pulled from the manta artwork) or `"linen"` (warm neutrals and a sage accent, built light-first for an appliance in a room). **Orthogonal** to `[shell] theme` — the preset picks the colours, `theme` picks which side | An unknown name is an **error**. Swallowing a typo would leave you hunting for why the brand never turned on |
| `palette` | `BTreeMap<String, String>` | `{}` | Role name → `"#RRGGBB"` or `"#RRGGBBAA"`. Overrides land on **both** the dark and light palettes the preset built | A name that is not a role ([04 §1.1](04-customization.md#11-the-palette-roles)) is an **error**. A value that is not `"#RRGGBB"` or `"#RRGGBBAA"` is an **error** — no short form, no colour names |

Injecting `.theme(Theme)` in code makes this whole section irrelevant, with a
warning ([04 §2](04-customization.md#2-changing-only-the-colours-with-themepalette)).

**Overrides apply to both sides.** Writing
`[theme.palette] primary = "#ff7a00"` makes primary orange in dark *and* light, so
the theme tile no longer wipes it out.

**`Palette::dark().on_primary` is white again.** It was `#0C1626` while the dark
accent was a light `#57A9FF` — a light fill needs dark text, and a light fill with
dark text on it is a pastel, which is exactly how the dark buttons came to look like
toy plastic. The accent moved instead: `#2878A9` carries white at 4.83:1, clearing the
4.5 text floor outright. The consequence is worth knowing — `Primary` measures 3.81
against `surface`, which clears the 3.0 a drawn shape needs and not the 4.5 text needs,
so **the accent is never a text colour**. An accent tick, fill or icon is fine; accent
words are not.

---

## 12. `[access]`

| Key | Type | Default | Meaning | Validation | Code |
|---|---|---|---|---|---|
| `mode` | `String` | `"prompt"` | `"prompt"`, `"routing"` or `"off"` | Anything else is an **error** | `AccessMode`. `prompt` draws the shell's prompt and lock screen through an authenticator; with none (no `[access.pin_table]` PINs or `[access.pattern_table]` patterns, no `ShellBuilder::authenticator`) it behaves as `routing` — the event only ([05 §0](05-access-control.md#0-the-principle-and-what-exists-today)) |
| `levels` | `Vec<String>` | `["default"]` | Level names, low to high | **Empty is an error.** One level means no authentication at all — every gate passes | `LevelTable::from_names` |
| `initial` | `Option<String>` | `None` | Starting level | Error when not in `levels`. Omitted means the lowest | |
| `default_gate` | `Option<String>` | `None` | Unassigned gates: `"top"`, `"bottom"` or a level name | **Error when `levels` has two or more entries and this is missing.** A level name not in `levels` is an error | `DefaultGate` |
| `gates` | `BTreeMap<String, String>` | `{}` | `[access.gates]` — §12.1 | | |
| `unlock_mode` | `String` | `"temporary"` | What a granted unlock does: `"switch"` makes it the session; `"temporary"` holds it for `temporary_secs`, then puts the previous subject back | Anything else is an **error** | `UnlockMode` — [05 §7](05-access-control.md#7-the-session--temporary-unlocks-the-timeout-the-idle-lock) |
| `temporary_secs` | `u64` | `300` | How long a temporary unlock lasts, counted **from the unlock** | **0 with `unlock_mode = "temporary"` is an error**; over a year is an error | |
| `session_timeout_secs` | `u64` | `0` | No input this long → back to the starting subject; `0` is off | Over a year is an error | `ChangeReason::Timeout` |
| `idle_lock_secs` | `u64` | `0` | No input this long → the panel locks (the lock screen, in `prompt` mode); `0` is off | Over a year is an error | `ShellEvent::LockRequested`, `ChangeReason::Lock` |
| `lock_screen` | `LockScreenConfig` | below | `[access.lock_screen]` — §12.4 | | |
| `pin_table` | `PinTableConfig` | below | `[access.pin_table]` — §12.2 | | |
| `pattern_table` | `PatternTableConfig` | below | `[access.pattern_table]` — §12.3 | | |

### 12.1 `[access.gates]`

Gate name (usually a screen or item id) → level name. The value must be `"top"`,
`"bottom"` or one of `levels`, or it is an **error**.

```toml
[access.gates]
"settings.wifi" = "operator"
"settings.network.edit" = "maintainer"
"dashboard" = "viewer"
"status.clock" = "bottom"
```

With two or more levels, any built-in gate missing from this table — so
`default_gate` will apply — is listed in an **info log at start-up**. Not an
error. The naming rules are in [05 Access control](05-access-control.md).

### 12.2 `[access.pin_table]`

The reference authenticator, `PinTable`
([05 §5.1](05-access-control.md#51-pintable--the-reference)). With any PIN in it
the table is the authenticator `prompt` mode asks, unless
`ShellBuilder::authenticator` gave another.

| Key | Type | Default | Meaning | Validation |
|---|---|---|---|---|
| `attempt_limit` | `Option<u32>` | `None` | This many wrong tries in a row lock the prompt (counted in memory, **one count with the pattern table's**) | 0 is an **error** |
| `lock_secs` | `Option<u64>` | `None` (60 s) | How long that lockout lasts | 0 is an **error**; set without any `attempt_limit` it warns and does nothing |
| `shuffle` | `bool` | `false` | A fresh digit layout each time the prompt opens | |
| `max_len` | `Option<u8>` | `None` (16) | The most digits a PIN may have: the table refuses a longer one, the keypad takes no more, `settings.credentials` sets no longer one | Outside 1–16 is an **error** |
| Every other key | `String` (flattened) | — | Level name → clear-text PIN | A key not in `levels`, a PIN that is not 1 to `max_len` digits, or two levels sharing a PIN is an **error**. Any secret present **warns at start-up** that it is clear text and file permissions are yours |

```toml
[access.pin_table]
operator = "1234"
maintainer = "987654"
attempt_limit = 5
lock_secs = 60
shuffle = true
max_len = 8
```

### 12.3 `[access.pattern_table]`

Patterns for the same `PinTable`: a level may have a PIN, a pattern or both, and
the prompt has a tab for each kind
([05 §5.1](05-access-control.md#51-pintable--the-reference)).

| Key | Type | Default | Meaning | Validation |
|---|---|---|---|---|
| `grid` | `u8` | `3` | Dots on a side | Outside 3–5 is an **error** |
| `min_points` | `u8` | `4` | The fewest dots a pattern has — the pad submits nothing shorter | Outside 1 to `grid × grid` is an **error** |
| `show_path` | `bool` | `true` | Draw the path as the finger draws it | |
| `attempt_limit` | `Option<u32>` | `None` | As `[access.pin_table]`'s, and the same count; where both set one, the stricter holds | 0 is an **error** |
| `lock_secs` | `Option<u64>` | `None` | As `[access.pin_table]`'s; where both set one, the longer holds | 0 is an **error** |
| Every other key | `String` (flattened) | — | Level name → clear-text pattern: the dots in the order drawn, row by row from 1, between `-`, `,` or spaces (`"1-2-3-6-9"`); on the 3 × 3 grid the digits alone do (`"12369"`) | A key not in `levels`, a dot off the grid or taken twice, fewer than `min_points` dots, a pattern a finger cannot draw as written (a stroke across a dot takes it — the error gives the path a finger would record), or two levels sharing a pattern is an **error** |

```toml
[access.pattern_table]
maintainer = "1-2-3-5-7-8-9"
grid = 3
min_points = 4
```

### 12.4 `[access.lock_screen]`

| Key | Type | Default | Meaning | Validation |
|---|---|---|---|---|
| `allow_continue` | `bool` | `false` | The lock screen offers Continue: leave without authenticating, as the starting subject | |

```toml
[access.lock_screen]
allow_continue = true
```

---

## 13. A complete example

Every section in one `fairing.toml`. Save it as-is and `ShellConfig::load` parses
and validates it.

```toml
[shell]
locale = "en"
theme = "dark"
idle_lock_secs = 0
repaint = "reactive"
cursor = "auto"               # auto | hidden | visible

[status_bar]
enabled = true
# height = 40.0       # pinned in du; left out, 7 mm
left  = ["status.clock"]
center = []
right = ["status.notifications", "status.bluetooth", "status.wifi", "status.battery"]
# icon_size = 20.0    # pinned in du; left out, 3.6 mm
icon_color = "on_surface"
tap_opens_shade = true
clock_format = "hm"

[nav_bar]
enabled = true
style = "buttons"
items = ["back", "home", "recents"]
# height = 64.0       # pinned in du; left out, a finger plus 8 du
back_edges = ["left"]

[desktop]
columns = 4
rows = 3
dock = ["settings.home", "dashboard"]
dock_edge = "bottom"          # bottom | top | left | right
label_lines = 2
wallpaper = "background"      # a role, #RRGGBB, "abyss", or "file:<path>"
wallpaper_fit = "cover"       # file: only. cover | contain | stretch
rail = "none"                 # none | left | right - the icon rail
rail_width = 0.0              # fraction of the content width; 0 means 0.26
label_legibility = "none"     # none | shadow | veil
long_press = "info"           # info | none - holding an icon

[[desktop.pages]]
icons = [
  { id = "dashboard" },
  { id = "settings.wifi", label = "Wi-Fi", icon = "wifi", locked = "show" },
  { id = "diagnostics", locked = "hide" },
  { id = "camera", col = 2, row = 0 },
]

[overlay]
layout = "unified"
tiles = ["tile.wifi", "tile.bluetooth", "tile.brightness", "tile.volume", "tile.lock", "tile.theme"]
tile_columns = 6
footer = ["clear_all", "lock", "settings"]
max_height_ratio = 0.85
peek_ms = 2000

[notify]
heads_up = true
max_visible = 2
max_items = 100
toast_ms = 3000
dismiss_button = true

[osk]
height_ratio = 0.38
min_key_px = 48.0
layout = "qwerty"
numpad_decimal = true
numpad_sign = false

[gesture]
enabled = true
hold_ms = 150
emergency_ms = 2000
emergency_corner_px = 64.0

[motion]
reduce = false
snap_ratio = 0.33
fling_px_s = 800.0
slop_px = 12.0
tap_ms = 300
long_press_ms = 500
crossfade_ms = 120
theme_fade_ms = 200
clear_top_ms = 160

[motion.spring]
k = 400.0
c = 40.0

[motion.push]
ms = 220
parallax = 0.25
dim = 0.15

[motion.pop]
ms = 200

[motion.home]
open_ms = 240
close_ms = 200
desktop_scale = 0.92

[motion.press]
ms = 80
release_ms = 120
scale = 0.97

[motion.shade]
snap_ratio = 0.33
rubber = 0.25
rubber_max_px = 40.0

[motion.shade.spring]
k = 400.0
c = 40.0

[motion.page]
fling_px_s = 600.0
rubber = 0.3
rubber_max = 0.15

[motion.page.spring]
k = 300.0
c = 35.0

[motion.osk]
show_ms = 180
hide_ms = 160
hide_debounce_ms = 100

[motion.toast]
in_ms = 160
out_ms = 200
shift_ms = 120
heads_up_in_ms = 220
heads_up_out_ms = 200
heads_up_hold_ms = 4000

[motion.switch]
ms = 140

[motion.panes]
enter_ms = 220
leave_ms = 220
even_ms = 200

[motion.overview]
in_ms = 240
cards_in_ms = 160
throw_ms = 200

[workspace]
split = true
split_axis = "auto"  # auto | side_by_side | stacked
overview = true

[theme]
preset = "base"      # base | abyss | linen

[theme.palette]
primary = "#2fe09b"
on_primary = "#04110b"

[access]
mode = "prompt"
levels = ["viewer", "operator", "maintainer"]
initial = "viewer"
default_gate = "top"
unlock_mode = "temporary"
temporary_secs = 300
session_timeout_secs = 0
idle_lock_secs = 0

[access.gates]
"settings.wifi" = "operator"
"settings.network.edit" = "maintainer"
"dashboard" = "viewer"
"status.clock" = "bottom"

[access.pin_table]
operator = "1234"
maintainer = "987654"
attempt_limit = 5
lock_secs = 60
```

To check a file of your own:

```rust,no_run
fn main() {
    let text = std::fs::read_to_string("fairing.toml").expect("read");
    match fairing::ShellConfig::from_toml(&text) {
        Ok(cfg) => println!("OK: levels = {:?}", cfg.access.levels),
        Err(e) => eprintln!("config error: {e}"),
    }
}
```

---

## 14. Configuring in code, with no file

You can skip `fairing.toml` entirely and build a `ShellConfig` literal. Every
sub-config implements `Default`, so fill in what you need and finish with
`..Type::default()`. All the types are public in `fairing::config`.

```rust
use fairing::config::{AccessConfig, DesktopConfig, ShellConfig, StatusBarConfig};

fn config() -> ShellConfig {
    ShellConfig {
        status_bar: StatusBarConfig {
            right: vec!["status.wifi".to_owned(), "status.battery".to_owned()],
            ..StatusBarConfig::default()
        },
        desktop: DesktopConfig {
            columns: 5,
            rows: 2,
            ..DesktopConfig::default()
        },
        access: AccessConfig {
            levels: vec!["viewer".to_owned(), "admin".to_owned()],
            default_gate: Some("top".to_owned()),
            ..AccessConfig::default()
        },
        ..ShellConfig::default()
    }
}

fn main() -> fairing::Result<()> {
    let cfg = config();
    cfg.validate()?; // the same validation ShellConfig::load runs after reading a file
    Ok(())
}
```

`ShellConfig::validate()` is `pub`, so a hand-built value gets the same checks.
Two things it does **not** cover: the level names and secrets in
`[access.pin_table]` and `[access.pattern_table]`, and the role and colour formats
in `[theme.palette]`. Those are validated by
`Access::from_config` and `ShellConfig::build_theme` — whoever knows the names does the
checking — and `Shell::builder(cfg).build(ctx)` calls all three in order.

---

## 15. Files

The shell reads `fairing.toml` once, through `ShellConfig::load` — a missing file means the
defaults — and whatever files your code hands it, a font through `FontSource::from_path` for
one. **It writes none** ([architecture §6](../architecture.md#6-state)).

What should outlive a restart is yours to keep. A setting someone changes reaches you as
`ShellEvent::SettingChanged`, to store where your device keeps such things, and
`Shell::restore_settings` puts what you stored back at start
([06 §7.1](06-services.md#71-keeping-settings-across-a-restart)). An old `state_dir` key in
`[shell]` is ignored, as any key the shell does not know is.

---

## 16. `[workspace]`

The shell's two panes and its recent screens
([03 §2.3](03-chrome.md#23-recent-screens-and-the-split),
[02 §6.1](02-screens.md#61-two-panes)).

| Key | Type | Default | Meaning | Validation | Code |
|---|---|---|---|---|---|
| `split` | `bool` | `true` | The shell's split: `cx.open_in_other_pane`, the split control and a card's split button bring the other pane up. `false` leaves the split control reported only (`ShellEvent::SplitRequested`) and opens `open_in_other_pane` in the same pane | None | `WorkspaceConfig::split` |
| `split_axis` | `String` | `"auto"` | How two panes divide the content: `"auto"` (side by side on content at least as wide as it is tall, one above the other otherwise), `"side_by_side"` or `"stacked"` | Anything else is an **error** | `WorkspaceConfig::axis` |
| `overview` | `bool` | `true` | The shell's recent screens, behind the nav bar's `"recents"` and `LaunchAction::OpenOverview`. `false` leaves them reported only (`ShellEvent::OverviewRequested`) — for a device with an overview of its own — and the split control then takes the task used last instead of offering the cards | None | `WorkspaceConfig::overview` |

Who may use them is a matter of gates, not of this section: `nav.recents` and `workspace.split`
([05 §2.1](05-access-control.md#21-built-in-gates)).

---

## Related pages

| Topic | Page |
|---|---|
| The palette roles, `Metrics`, `MotionTokens` in code | [04 Customization](04-customization.md) |
| Gate and level naming, `Authenticator` | [05 Access control](05-access-control.md) |
| `Services` and `ShellHandle::set_setting` | [06 Services](06-services.md) |
| Diagnosing typos and bad references | [08 Troubleshooting](08-troubleshooting.md) |
