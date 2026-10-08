# 08. Troubleshooting and operations

## 1. Errors you will actually hit

All of these come out of `ShellConfig::load` / `from_toml` or
`Shell::builder(..).build(ctx)` as `Error::Config(String)`. Print the message
straight to stderr or the log — it names the key.

| Symptom | Message (verbatim) | Cause | Fix |
|---|---|---|---|
| More than one level and it will not start | `[access] default_gate is required once levels has more than one entry` | With several levels, something has to say where an unassigned gate goes. The shell will not quietly pick a side | Add `[access] default_gate = "top"`, `"bottom"`, or one of the `levels` names |
| Typo in `default_gate` | `[access] default_gate = "op" is not in levels` | Only `"top"`, `"bottom"` or an exact `levels` name is allowed | Match the spelling, or use `top` / `bottom` |
| Gate assigned to a level that does not exist | `[access.gates] "settings.wifi" = "op" - no such level` | Values in `[access.gates]` must appear in `levels` too | Fix the typo |
| `initial` names no level | `[access] initial = "x" is not in levels` | Same rule | Fix the typo |
| `pin_table` names no level | `[access.pin_table] "x" is not a level in levels` | Every key in `[access.pin_table]` except the fixed `attempt_limit`, `lock_secs`, `shuffle` and `max_len` has to be a level name (the same in `[access.pattern_table]`, whose fixed keys are `grid`, `min_points`, `show_path`, `attempt_limit` and `lock_secs`) | Fix the typo, or add the level to `levels` |
| A PIN over `max_len` | `[access.pin_table] "x" must be 1 to 6 digits - the keypad has nothing else` | `max_len` caps the table as well as the keypad | Shorten the PIN, or raise `max_len` (16 at most) |
| A pattern that cannot be drawn | `[access.pattern_table] "x" = "1-3-6-9": a finger cannot draw it as written - a stroke across a dot takes that dot, so it records "1-2-3-6-9"` | A stroke from one dot to another across a third takes the third, as every pattern lock does — the pattern as written could never be entered | Write the path the message gives (or route round the dot) |
| Empty `levels` | `[access] levels is empty` | `[access.levels]` needs at least one entry (one means no authentication) | Put something in, even `levels = ["default"]` |
| Grid columns or rows too large | `[desktop] columns must be 0 (auto) or at most 12` (rows cap at 8) | `MAX_COLUMNS = 12`, `MAX_ROWS = 8`. A larger value allocates a `Vec` of empty cells per page and the renderer walks it every frame | Stay inside the cap, or use `0` for auto |
| Bar height rejected | `[status_bar] height must be greater than 0` / `[nav_bar] height must be greater than 0` | An enabled bar needs a positive height | Fix the value, or set `enabled = false` |
| `[motion]` value out of range | e.g. `[motion] snap_ratio must be within 0..=1`, `[motion.spring] k must be greater than 0 and c at least 0` | Springs, ratios and pixel values that make no physical sense stop the start-up | Check the table in [07 §10](07-config-reference.md#10-motion) |
| Odd value in `[nav_bar] back_edges` | ``[nav_bar] back_edges: `top` must be "left" or "right"`` | Only `"left"` and `"right"`; an empty array is fine and means "none" | Fix the typo |
| `style = "gesture"` with no back edges | `[nav_bar] style = "gesture" has no back button, so an empty back_edges leaves no way back…` | A gesture bar has no back button, so with no back edge the screen is a dead end | Give it an edge, or set `style = "buttons"` |

## 2. Referring to an id that was never declared

The rule in the [`config.rs`](../../crates/fairing/src/config.rs) module docs is
"warn and ignore", but **it differs per section**. This table is read off the
source.

| Setting | Does it warn? | Failure mode |
|---|---|---|
| `[desktop] dock` with an undeclared id | **Warns** (``desktop.dock: ignoring `{id}` - no such declaration``) | That entry drops; the rest of the dock is fine |
| `[[desktop.pages]] icons[].id` undeclared | **Warns** (``desktop.pages: ignoring `{id}` - no such declaration``) | The whole override is ignored |
| `[[desktop.pages]] icons[].icon` names no icon | **Warns** (``desktop.pages: icon name `{icon}` … is not in the built-in set``) | The original icon stays |
| `[[desktop.pages]] icons[].locked` is neither `"show"` nor `"hide"` | **Warns** | The `locked` override is ignored |
| `[overlay] tiles` with an undeclared id | **Warns** (``[overlay] tiles: `{id}` is neither built-in nor declared``) | That tile drops |
| `[status_bar] left` / `center` / `right` undeclared | **Neither warning nor error** | An empty text slot (`StatusItem::Text("")`) takes up the space and draws nothing |
| `[nav_bar] items` undeclared | **Neither warning nor error** | `NavItem::Custom(id)` with no matching `nav_item` declaration draws a fallback `•` glyph, **enabled**, and accepts taps — but there is no callback behind it, so nothing happens |
| A `nav_item` that `[nav_bar] items` does not list | **Warns** on the frame after the `add` (``nav_item `{id}` will not be drawn: …``) — also on a bar that is off or in the gesture style | Nothing is drawn: a nav item is never appended |
| `cx.open("settings.wifi")` (or any `settings.*`) without `settings::add_all` — or `settings.wifi`, `settings.bluetooth`, `settings.network` or `settings.power` with `add_all` but no backend that reports that capability | **Warns** (``launch: screen `settings.wifi` was never declared``) | Nothing opens. Add the built-in screens and the backend, or `SettingsConfig::ignoring_capabilities` ([01 §4.1](01-getting-started.md#41-the-built-in-settings-screens)) |

**So a typo in a bar slot shows up only as an empty gap or a dot that does
nothing.** When an item is missing, check the id spelling against the built-in
tables in [03 Chrome](03-chrome.md) first. To reproduce it headless, use the
harness from [01 §6](01-getting-started.md#6-testing-your-ui-headless) and check
whether `shell.status_bar().item_rect("status.xxx")` is `None`.

## 3. Confirming 0 fps when idle

With `[shell] repaint = "reactive"` (the default), the shell never calls
`ctx.request_repaint()` when nothing is animating and no backend needs waking
(`Shell::schedule_repaint`). Three ways to check.

1. **Look at the device.** Leave it alone for a few seconds and watch the process
   in `top` or `htop`. CPU should fall to roughly zero. If it keeps a core busy,
   something is asking for a repaint every frame.
2. **Compare.** Switch to `[shell] repaint = "continuous"` for a forced 60 fps and
   compare CPU. No difference means reactive is not actually taking effect.
3. **Read the log.** If a backend keeps returning a `next_wake()` in the past —
   the usual bug is not refreshing its own snapshot — the shell can never go idle,
   and every `STALE_WAKE_FRAMES` (an internal constant) you get:

   ```text
   backend `wifi` has had next_wake in the past for 240 frames - the shell cannot go idle (guide 06 §1)
   ```

   At `RUST_LOG=fairing=warn` or looser, that line means the named backend
   (`clock`, `power`, `wifi`, `bluetooth`, `display`, `audio`, `network`, `info`, or a
   custom backend by its type name) is not updating `next_wake` properly — see
   [06 Services](06-services.md).

With an `hms` clock (seconds shown), waking on a 500 ms cadence around each second
boundary is normal. Idle 0 fps does not mean "never repaints" — it means "nothing
scheduled, nothing requested".

An indeterminate `ProgressRing` on screen asks for every frame while it is there. That is
right for a few seconds of loading and wrong for a wait with no end in sight: give such a
ring `.repaint_every(Duration::from_millis(250))` and it moves in coarser steps at a few
frames a second. The unlock prompt's badge wait does this — a lock screen waiting for a
badge can stand all night.

## 4. Using an API without its feature

`overlay` and `osk` fail differently.

| Feature | With it off |
|---|---|
| `osk` | `Shell::osk()` and `osk_mut()` are behind `#[cfg(feature = "osk")]`, so they do not exist — a **compile error** (``no method named `osk` found``) |
| `overlay` | `Shell::overlay()` is likewise a **compile error**. But `LaunchAction::OpenOverlay` — the in-shell path, reached through `cx.open` and friends — always compiles. Opening the shade with the feature off quietly logs ``the `overlay` feature is off, so the shade cannot open`` and **does nothing**. Not an error, not a panic |

The `[overlay]` and `[osk]` config sections are parsed regardless
([07 §0](07-config-reference.md#0-load-rules)), so two builds with different
feature sets can share one `fairing.toml`.

## 5. Picking the Wayland or X11 backend

| What you want | How |
|---|---|
| A device (Wayland only) | `--features runner` |
| Desktop development, X11 included | `--features runner-x11` (`runner` plus `eframe/x11`; Wayland still works) |
| Force a backend where several are available | `WINIT_UNIX_BACKEND=x11` (or `wayland`), read by winit inside eframe |
| Software rendering in CI or a container with no GPU | `LIBGL_ALWAYS_SOFTWARE=1` (Mesa llvmpipe) |
| A real window with no display attached | `xvfb-run -a -s "-screen 0 1024x600x24"` for a fake X server |

The `--tour` screenshot mode in [01 §5](01-getting-started.md#5-running-the-bundled-examples)
uses exactly that combination:

```sh
xvfb-run -a -s "-screen 0 1024x600x24" env LIBGL_ALWAYS_SOFTWARE=1 WINIT_UNIX_BACKEND=x11 \
  cargo run -p fairing --features runner-x11 --example demo -- --size=1024x600
```

With only `runner` in an X11 environment — say `WINIT_UNIX_BACKEND=x11` but no
`eframe/x11` — the window either never appears or you get an initialisation
`Error::Runner`. Switch to `runner-x11`.

## 6. Korean renders as tofu

egui's `default_fonts` (Ubuntu family plus emoji) has no CJK glyphs. `fairing`
does **not** embed a font file — they run to several megabytes and how far to
subset is a per-device call. What it gives you instead is
[`fairing::fonts`](../../crates/fairing-widgets/src/fonts.rs) and `ShellBuilder::fonts`.
The shell checks once per language: on a Korean panel with no Hangul font loaded,
the log says `locale "ko": no loaded font has '가'` and names what to load.

### 6.1 Find a font already on the device

Ubuntu Core images often carry `fonts-noto-cjk`. When nothing is found you get
`None`.

```rust
# use fairing::{Services, Shell, ShellConfig};
# fn build(config: ShellConfig, services: Services, ctx: &egui::Context) -> fairing::Result<()> {
use fairing::fonts::{FontSet, FontSource};

let mut fonts = FontSet::new();
match fairing::fonts::korean_font() {
    Some(path) => fonts.push(FontSource::from_path("ko", &path)?),
    None => log::warn!("no Korean font found - Hangul will render as tofu"),
}

let mut shell = Shell::builder(config)
    .services(services)
    .fonts(fonts)
    .build(ctx)?;
# let _ = &mut shell;
# Ok(()) }
```

`korean_font()` walks `~/.local/share/fonts` → `~/.fonts` →
`/usr/local/share/fonts` → `/usr/share/fonts` and returns the first file whose
name contains `NotoSansKR`, `NotoSansCJK`, `NanumGothic` or `Pretendard`. For any
other name, call `find_system_font(&["my-font"])` yourself.

### 6.2 Embed it in the binary (the version that cannot go missing)

```rust,ignore
let fonts = FontSet::new().with(FontSource::from_static(
    "ko",
    include_bytes!("../assets/NotoSansKR-Regular.ttf"),
));
```

### 6.3 Tuning it

| What you want | How |
|---|---|
| Latin from this font too | `.priority(FontPriority::First)` — the default is `Fallback`, used only for missing glyphs |
| Body text only, or monospace only | `.families(FontFamilies::Proportional)` / `::Monospace` — the default is `Both` |
| One face out of a `.ttc` / `.otc` collection | `.index(1)` |
| Several fonts | Call `FontSet::push` more than once; they install in order |

Installation happens **once**, in `build`. `set_fonts` rebuilds the whole glyph
atlas, so never call it from the frame loop.

Hand `from_path` something that is not a font and it stops with `Error::Config`
(it checks the sfnt signature) before egui can panic.

Shipping a freely licensed CJK font with the device image — Noto Sans KR is
OFL-1.1, for instance — is the integrator's job; see "Bundling fonts and assets"
under [§10](#10-deployment).

### 6.4 Korean **input** is a separate switch

A font makes Korean visible; the OSK layout is what types it. Turn on the two-set
layout with `[osk] layout = "hangul"` — see [03 §6](03-chrome.md#6-on-screen-keyboard).
The two are independent: with the layout on and no font, the key labels are tofu;
with the font on and no layout, you can read Korean but not type it.

## 7. Known defects

The limits the crate has today are listed in the [roadmap](../roadmap.md#4-known-limits). The
defects found while checking real renders through the screenshot tour (`--tour` over Xvfb +
llvmpipe) were of these kinds, and all of them are fixed:

| Kind | Example |
|---|---|
| First-frame bugs around egui's `Area` sizing pass | The first push frame drawing empty; mis-taps after a scrim tap |
| z-order | The outgoing layer sitting under the incoming one during a pop |
| Insets | Screen content pinned to the very edge |
| Input injection timing | Synthetic events landing on the next pass |
| Missing glyphs | OSK special keys as tofu — replaced with icons |
| Render mapping | A half-pulled shade showing nothing — changed to a curtain |

Public-API and documentation mismatches found by reading the source against this
guide are closed too; the
workarounds this guide used to suggest are no longer needed (`fairing::egui` is
re-exported, `runner::run_shell_with` exists, so does
`services::clock::ChronoClock`, `Shell::remove` reaches built-in tiles, and
`Lifecycle::Resized` is emitted).

## 8. Checking performance

### 8.1 Where the numbers are

| Tool | Command | What it shows |
|---|---|---|
| `examples/bench` | `cargo run -p fairing --release --example bench` | p50/p95/max in ms per scenario, plus animation frame counts, as text on stdout |
| `examples/motion_lab` | `cargo run -p fairing --features runner-x11 --example motion_lab -- --size=1024x600` | A graph of the last 180 frames top right (interval in ms, p50, p95, the 33 ms budget line) — **by eye** |
| `Harness::frame_shapes()` | Inside a headless test | The shapes actually drawn this frame — use it to catch things being drawn that should not be |
| `Shell::is_animating()` | Anywhere | Whether an animation is live this frame. Stuck at `true` while idle means something never finishes |

### 8.2 Running the bench

```sh
cargo run -p fairing --release --example bench
```

`--release` is the baseline. The scenarios are idle desktop, icon zoom (A2),
push/pop (A3), a 200-widget screen (idle and drag-scrolling), home, shade drag
(A1), page swipe (A4) and OSK reveal (A5, feature `osk`). Run it on your device and keep the
table it prints with the hardware, the commit and the conditions; compare runs on the same
device. A p50 more than **30 %** above an earlier run is a
regression.

### 8.3 When it is slow

| Check | Why |
|---|---|
| **Idle desktop p50** | It is the proxy for per-frame heap allocation. A counting allocator needs `unsafe impl GlobalAlloc` and the workspace is `unsafe_code = "forbid"`, so this cannot be measured directly — if idle p50 moves, something in the render path started allocating |
| **How many fonts you loaded** | Every font added through `egui::Context::set_fonts` adds first-rasterisation and atlas-rebuild cost. If one Korean font is enough, do not stack several |
| **Known render allocations** | `icons::paint` allocates one `Vec<Pos2>` per sub-path with three or more points (epaint's `PathShape` wants an owned Vec), plus one each for the parametric bluetooth and volume icons. On the M1 baseline (51 icons then), 69 of 146 sub-paths were allocation-free `LineSegment`s — worth knowing when you add custom icons |
| **Debug versus release** | Debug numbers are a different order of magnitude (x86 baseline idle p95: 0.042 ms release, 0.75 ms debug). Always bench and compare with `--release` |

## 9. Passing the audit

`cargo xtask audit [--strict]` runs fourteen stages (0 and 1–13), non-compiling
checks first. CI passes `--strict`, so a missing plugin fails there; locally, a
missing `cargo-audit` or `cargo-deny` only prints an install hint and skips.

| # | Stage | Command | What it looks at |
|---|---|---|---|
| 0 | Integrity | `xtask integrity` | Tables other than `[alias]` in `.cargo/config.toml`, a `vendor/` directory, `[patch]` / `[replace]`, a missing `[lints] workspace = true`, an odd `Cargo.lock` origin |
| 1 | Format | `cargo fmt --all --check` | Formatting |
| 2 | Lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Clippy, including the lints banning `unwrap`, `panic` and `unsafe` |
| 3 | Tests | `cargo test --workspace --all-features` | Everything |
| 4 | Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` | rustdoc warnings |
| 5 | Duplicate versions | `cargo tree -e normal -d --all-features` | Duplicates outside `deny.toml [bans].skip` |
| 6 | Allow-list | `xtask deps-check` | Every crate in the tree is in `deps.allow` |
| 7 | Advisories | `cargo audit --deny warnings` | RustSec advisories (needs the plugin) |
| 8 | Licences and sources | `cargo deny check` | Licences, sources, the ban list (needs the plugin) |
| 9 | Update visibility | `cargo update --dry-run` | Reports only; never fails |
| 10 | Notices | `xtask licenses --check` | Whether `THIRD_PARTY.md` is current |
| 11 | Generated files | `xtask icons --check` | Whether `icons/generated.rs` matches `assets/icons/*.svg` |
| 12 | Target build | `cargo check -p fairing --features runner --target aarch64-unknown-linux-gnu` | aarch64 portability — **a compile check, not a binary** |
| 13 | Blocking sync | `xtask sync-check` | Bans eight identifiers across `crates/**/src/**/*.rs`: `Mutex`, `RwLock`, `Condvar`, `Barrier`, `OnceLock`, `LazyLock`, `parking_lot` and `tex_manager`. Only the last is not our own type — `Context::tex_manager()` hands back an `Arc<RwLock<..>>`, and the calling source never spells `RwLock`, so the name is the only handle. In `crates/fairing/src` and `crates/fairing-widgets/src` (the UI thread) it also bans `.recv(`, `.recv_timeout(`, a **no-argument `.join()`** and `thread::sleep`. Matching `.join(` broadly would need an exemption on every `Path::join`, so only the argument-less form counts |

Adding a dependency:

1. Add it to `Cargo.toml` — direct dependencies go in `[workspace.dependencies]`
   and members refer to them with `.workspace = true`.
2. Run `cargo xtask deps-check --write` to append a line to `deps.allow`. Name,
   version range (caret, except `egui` and `eframe` which are pinned `=X.Y.Z`)
   and licence fill themselves in; the reason column gets `TODO approve`.
3. Write the reason yourself as `explanation (approver, YYYY-MM-DD)`. A `TODO`
   with no date keeps `deps-check` failing.
4. A PR touching `Cargo.toml`, `Cargo.lock` or `deps.allow` needs the repository
   owner's review under `CODEOWNERS`. The reviewer decides from the tree delta in
   the `deps-check` output — how many new crates, how many duplicate versions.
5. Confirm `cargo xtask audit --strict` passes locally before pushing.

## 10. Deployment

### 10.1 The release profile

The workspace root deliberately has no `[profile.release]`: a profile set by a
library never propagates to the integrator's binary, and `panic = "abort"` breaks
`cargo test --release`. Put it in **your own** root `Cargo.toml`
([01 §3](01-getting-started.md#3-the-device-binary-profile)).

```toml
[profile.release]
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
opt-level = 3      # for a UI, 3 beats "s"; strip wins the size back
```

### 10.2 Cross-compiling for aarch64

```sh
rustup target add aarch64-unknown-linux-gnu   # already in rust-toolchain.toml, so `rustup show` fetches it
cargo build --release --target aarch64-unknown-linux-gnu --features runner
```

You need an aarch64 GNU cross linker on the host (the `gcc-aarch64-linux-gnu`
family) and the linker path in either
`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER` or **your own**
`.cargo/config.toml`. It cannot go in the `fairing` workspace's own
`.cargo/config.toml`, because audit stage 0 fails on any table other than
`[alias]`. CI only runs `cargo check` for this target; producing the actual
binary is the integrator's step.

### 10.3 An Ubuntu Core snap

There is no `snapcraft.yaml` in the repository yet. The list below is the set of
plugs the shell is expected to need — it is a starting
point, not something that has been packaged and verified.

| Plug | Why |
|---|---|
| `wayland` | The `ubuntu-frame` kiosk compositor |
| `opengl` | eframe's `glow` backend (GLES2) |
| `network-manager` | The Wi-Fi backend (D-Bus) |
| `bluez` | The Bluetooth backend (D-Bus) |
| `hardware-observe` / `system-observe` | Reading power and display state |
| `shutdown` | Only if you expose reboot or power-off actions |
| `network-control` | Only if you also change Wi-Fi settings |

### 10.4 Bundling fonts and assets

`fairing` handles icons (vectors, compiled into the binary) and CJK fonts (not
embedded) very differently.

| Asset | Bundled? | How |
|---|---|---|
| The 78 built-in icons | Already in the crate (`assets/icons/*.svg`, compiled by `xtask icons`) | Nothing to do |
| CJK fonts | **You bundle them with the snap or image** | Put the font file somewhere in your asset path and load it with the code in [§6](#6-korean-renders-as-tofu). Its licence (Noto Sans KR is OFL-1.1) needs the same kind of notice as `THIRD_PARTY.md` gives |
| Custom icons (`register_icon`) | Defined as vectors in code | Not a raster asset — an `IconDef` ([03 §8](03-chrome.md#8-icons)) |

## 11. FAQ

**Q0. The titles look flat, the touch targets look the wrong size.**
Two things the shell cannot know until you say: whether there is a bold face, and how big the
panel is. Both log a warning at startup. Give it a bold with `ShellBuilder::font`
([01 §4.2](01-getting-started.md#42-fonts)) and the panel's size with
`ShellBuilder::physical_mm` ([01 §4.3](01-getting-started.md#43-the-panels-size)).

**Q1. Is a screen really an app?**
No. Not a separate process, not a separate window — a virtual layer that one
`Screen` implementation (or closure) draws inside the same egui frame loop. See
the crate docs in [lib.rs](../../crates/fairing/src/lib.rs).

**Q2. Why can I not use `Mutex` or `RwLock`?**
You cannot. `cargo xtask sync-check` (audit stage 13) fails the build when
`Mutex`, `RwLock`, `Condvar`, `Barrier`, `OnceLock`, `LazyLock` or `parking_lot`
appears as an identifier in `crates/**/src/**/*.rs`, and inside
`crates/fairing/src` it also bans `.recv(`, `.join(` and `thread::sleep`. The
`disallowed-types` list in the root `clippy.toml` catches the same thing at
compile time. Blocking the UI thread stops the frame. A justified exception gets
`// sync-check: allow: <reason>` on the same line.

**Q3. How do I change the wallpaper?**
For a flat colour, `[desktop] wallpaper = "#RRGGBB"` or a palette role name.
Anything more — gradients, images, procedural patterns — goes through code:
`shell.desktop_mut().set_wallpaper(Wallpaper::Painter(..))` and friends
([04 §5](04-customization.md#5-painting-the-wallpaper-yourself),
[03 §3.7](03-chrome.md#37-wallpaper)).

**Q4. How do I turn the status bar off?**
Everywhere: `[status_bar] enabled = false`. For one screen only, that screen's
`ChromePolicy::status_bar = BarMode::Hide`
([04 §6.1](04-customization.md#61-what-can-be-turned-off-and-how)).

**Q5. Can I change the theme at runtime?**
Dark ↔ light, yes: `Shell::set_theme_dark(bool)` crossfades over 200 ms, and
`tile.theme` and its `SettingKey` both go through it. It picks from the dark/light
pair settled at start-up, and **custom colours survive the switch**: a
`[theme.palette]` override lies over both sides of the pair, and an injected
`.theme(Theme)` makes both sides its own palette, so only the `dark` flag and egui's
`Visuals` change. `ShellBuilder::palettes` (or `Shell::set_palettes` while running)
hands over a pair of your own; `Shell::set_palettes(Palette::dark(), Palette::light())`
gives back the neutral one. `Shell::apply_palette` crossfades to any palette at once.
Metrics and motion tokens are untouched by all of these. There is no API for swapping
a whole `Theme` while running; that is decided once, at start-up, through
`ShellBuilder::theme`.

**Q6. What are the limits on desktop icons, rows, columns and the dock?**
The grid caps at `columns` 12 and `rows` 8 (`0` means auto,
[07 §5](07-config-reference.md#5-desktop)). **The dock has no count limit.** Five
is a phone convention, not a device requirement — twelve along the bottom of a
1920 bar panel is fine, and so is eight down the left rail of a 480×800. What the
shell does instead is shrink the cells, and it stops shrinking at the physical
minimum touch size.

**Q7. I set `[access] mode = "prompt"` and a locked icon opens no PIN pad.**
`prompt` draws the shell's prompt only when there is something to ask: PINs in
`[access.pin_table]`, or an authenticator from `ShellBuilder::authenticator`.
With neither it behaves as `routing` — the gated launch opens nothing and emits
`AccessEvent::UnlockRequested`. A single-level table never prompts at all. Add a
PIN table (or your authenticator) and the keypad comes up
([05 §5](05-access-control.md#5-the-shells-prompt-and-the-authenticator)).

**Q8. Does `[shell] locale = "ko"` give me Korean text?**
Yes, for everything the shell draws: the shade, the lock screen, the unlock prompt
and the `settings` screens. Your screens' words come out in Korean once you look
them up with `cx.strings.get(..)` and give them a Korean entry through
`ShellBuilder::translations`; a word with no entry stays English. You need a font
with Hangul too (§6) — without one the shell logs a warning and the text is boxes.
The `ui.locale` setting switches the language while the shell runs
([04 §9](04-customization.md#9-text-and-translations)).

**Q9. Why does `cargo xtask audit` skip stages locally?**
Without `cargo-audit` (stage 7) and `cargo-deny` (stage 8) installed, the local
run prints an install hint and skips them. CI runs with `--strict`, where a
missing plugin fails. Run `cargo install cargo-audit cargo-deny` and then
`cargo xtask audit --strict` to reproduce CI before you push.

**Q10. I set `[shell] idle_lock_secs = 300` and the log warns about it.**
The key lives in `[access]` now. `[shell] idle_lock_secs` is still read where
`[access] idle_lock_secs` is 0, with that warning, so move it. Locked, the panel
shows the shell's lock screen in `prompt` mode with an authenticator; in `routing`
mode the session goes back to its start and `ShellEvent::LockRequested` is yours
to answer ([07 §12](07-config-reference.md#12-access),
[05 §7](05-access-control.md#7-the-session--temporary-unlocks-the-timeout-the-idle-lock)).

---

## Related pages

| Topic | Page |
|---|---|
| Every config key and its validation | [07 Config reference](07-config-reference.md) |
| Gates, levels, `Authenticator` | [05 Access control](05-access-control.md) |
| Backends (`ClockSource` and friends) and `next_wake` | [06 Services](06-services.md) |
| Tuning the motion tokens | [04 §8](04-customization.md#8-tuning-motion) |
