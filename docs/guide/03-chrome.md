# 03. Using the chrome

The chrome is the shell around the content: the status bar, the nav bar, the desktop, the shade,
notifications, the OSK, touch widgets, icons and gestures. It is drawn even if you register no
screens at all.

There are three ways to work with it.

| Way | What it decides | When to use it |
|---|---|---|
| Config (`fairing.toml`) | Placement, order, on/off, a few metrics | Shipping variants of a machine from the same code |
| Code declarations (`status_item` · `nav_item` · `tile` · `screen().desktop()`) | What exists | Adding your own items |
| Painter hooks (`status_bar_painter` · `nav_bar_painter` · `slot_painter`) | The drawing itself | Replacing the built-in look wholesale |

This page covers the first two. Painter hooks and theme injection are in
[04 Customization](04-customization.md). The complete config key tables are in
[07 Config reference](07-config-reference.md). How the parts fit together is in
[architecture](../architecture.md).

---

## 1. The status bar

### 1.1 Built-in items

Built-in ids all start with `status.`. In code, `fairing::chrome::BUILTIN_IDS` gives you the
list.

| id | What it draws | When it is drawn | Tap |
|---|---|---|---|
| `status.clock` | The clock (`clock_format`) | Always | none |
| `status.wifi` | A parametric Wi-Fi strength icon | The backend reports `Capabilities::WIFI` | none |
| `status.bluetooth` | Bluetooth (off / on / connected) | The backend reports `Capabilities::BLUETOOTH` | none |
| `status.battery` | Battery level and charge indicator | `services.power.battery()` is `Some` | none |
| `status.notifications` | A bell with an unread count badge | Always | Opens the shade (`LaunchAction::OpenOverlay`) |
| `status.user` | A level-coloured dot and the subject's name | Always | none |
| `status.ethernet` | Wired network, dimmed while the cable is out | The network backend reports `Capabilities::ETHERNET` | none |
| `status.volume` | Volume (four steps, muted) | The audio backend reports `Capabilities::VOLUME` | none |
| `status.brightness` | Brightness | The display backend reports `Capabilities::BRIGHTNESS` | none |
| `status.lock` | An open padlock | The session is unlocked — above `[access] initial` | A logout (`LaunchAction::Logout`) |

An item whose backend does not report the capability takes no space either. With the null
backends you are left with the clock, the notification bell and the user. Attaching backends is
[06 Services](06-services.md).

A battery at 20 % or below is drawn in `danger`, and Wi-Fi, battery and Bluetooth crossfade over
120 ms when their value changes.

### 1.2 Slot placement

Items are assigned to three slots: left, center and right. Order inside a slot is list order.

```toml
[status_bar]
enabled = true
# height = 40.0        # pin the bar height in du; left out, it is 7 mm
left = ["status.clock", "status.user"]
center = []
right = ["status.notifications", "status.bluetooth", "status.wifi", "status.battery"]
# icon_size = 20.0     # pin the icons' size in du; left out, it is 3.6 mm
icon_color = "on_surface"
tap_opens_shade = true
clock_format = "hm"
```

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `true` | `false` draws no bar and the content grows by that much |
| `height` | unset | The bar height in du, **pinned**. Unset, it is 7 mm, at least 32 du (44 du at the density the shell assumes without `physical_mm`). Zero or below, `nan` or `inf`, is a config error at startup. An integrator's `metrics_spec` or theme wins over it, with a warning |
| `left` · `center` · `right` | `["status.clock"]` · `[]` · `["status.notifications", "status.bluetooth", "status.wifi", "status.battery"]` | Ids per slot, in order |
| `icon_size` | unset | The built-in icons' side in du, pinned. Unset, 3.6 mm, at least 18 du. The same rule as `height` |
| `icon_color` | `"on_surface"` | A palette role name, or `#RRGGBB` |
| `tap_opens_shade` | `true` | A tap on empty bar space toggles the shade |
| `clock_format` | `"hm"` | `hm` (HH:MM) · `hms` (HH:MM:SS) · `date_hm` (MM-DD HH:MM), and their 12-hour spellings `hm12` · `hms12` · `date_hm12` (h:MM AM/PM). The device owner's 24-hour switch on `settings.datetime` flips between the two halves at run time |

Placement works like this: the left slot fills from the left edge, the right slot fills backwards
from the right edge, and the center slot is centred in what is left. Side padding is
`theme.metrics.status_edge_pad` (12 du) and the gap between items is fixed at 10 du (`SPACING`
in `chrome/status_bar.rs`).

When the width runs out, items collapse. Lowest `priority` first; ties collapse from the right
slot. The last remaining item stays even if it overflows.

To place the items otherwise — the clock in the middle whatever its slot, a collapsed item on a
row of its own — give the builder a layout: it is handed this placement and moves what it wants,
and the bar still draws and presses the items where it says
([04 §10.5](04-customization.md#105-the-status-bars-items)).

### 1.3 Turning items on and off

| What you want | How |
|---|---|
| Not visible at all | Remove the id from the slot list |
| Remove it at runtime | `shell.remove("status.clock")` — built-in items share the declaration id space, so `ShellEvent::DeclRemoved` fires |
| Hide it from lower-level users | Assign that id in `[access.gates]` ([05 Access control](05-access-control.md)) |
| Turn the whole bar off | `[status_bar] enabled = false` |
| Hide it on one screen | `ChromePolicy::status_bar = BarMode::Hide` ([02 Screens](02-screens.md)) |

A gate name is the item id. In a config with `default_gate = "top"`, not assigning
`status.clock` makes the clock disappear entirely — which is why status bar items are usually
assigned to `bottom`.

### 1.4 Adding your own item

One `status_item(id, slot, |ui, cx| ..)` is one item. It goes in with `shell.add(..)` and comes
out with `shell.remove(id)`.

```rust
use fairing::{status_item, ColorRole, Cx, Shell, Slot};

fn add_temperature_item(shell: &mut Shell) {
    shell.add(
        status_item("temp", Slot::Right, |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let color = cx.theme.color(ColorRole::Muted);
            ui.colored_label(color, "36.5 °C");
        })
        .gate("status.temp")
        .priority(-1),
    );
}
```

| Builder | Default | Meaning |
|---|---|---|
| `.gate(name)` | the same name as the id | Fails the gate and the closure is not called |
| `.priority(n)` | `0` | Lower collapses first (`i8`) |
| `.enabled(bool)` | `true` | Off means not drawn and no space taken |

Its position comes from one of two places.

- If the id is in a slot list (`[status_bar] right = [.., "temp"]`), it is drawn there.
- If it is not, it goes at the end of the slot given in the declaration.

The `ui` the closure receives is a child `Ui` inside the item rect the shell laid out. On the
first frame the width is unknown, so it is estimated at 72 px; from the next frame the width
actually drawn is used. An item that collapses on that estimate is run once out of sight (nothing
it draws is shown, and its widgets take no input) so its real width is known; after that a
collapsed item is not run until it has room again.

Taps are the closure's own widgets' business. The only thing the shell handles for you is the
`tap_action` of built-in items.

### 1.5 Tap behaviour, in one table

| Where you press | What happens |
|---|---|
| Empty space with no item | With `tap_opens_shade = true`, the shade toggles |
| `status.notifications` | Opens the shade |
| Any other built-in item | Nothing |
| Your own item | The widgets inside your closure take it |

---

## 2. The nav bar

### 2.1 Item kinds

```toml
[nav_bar]
enabled = true
style = "buttons"
items = ["back", "home", "recents"]
# height = 64.0        # pin the bar height in du; left out, it is a finger plus 8 du
back_edges = ["left"]
```

| `items` value | What it does | State |
|---|---|---|
| `"back"` | Pops the stack | Disabled when the stack is empty |
| `"home"` | Goes home | Always enabled |
| `"recents"` | The recent screens (§2.3) | Always enabled. Reported as `ShellEvent::OverviewRequested` too |
| `"split"` | The split control (§2.3) | Always enabled. Reported as `ShellEvent::SplitRequested` too |
| `"osk"` | Manual OSK toggle | Pairs with a `nav_item("osk", ..)` declaration (§6.3) |
| Any other string | The slot for a `nav_item(id, ..)` declaration | An empty slot if there is no declaration |

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `true` | `false` means no bar and wider content |
| `style` | `"buttons"` | `"gesture"`: a band with the home indicator, and the bottom edge's swipes in place of the buttons (§2.6) |
| `items` | `["back", "home", "recents"]` | The slots and their order |
| `height` | unset | The bar height in du, pinned. Unset, it is one finger plus 8 du, at least 56 du (65 du at the assumed density). The same rule as `[status_bar] height` |
| `back_edges` | `["left"]` | Which screen edges accept a back gesture. Only `"left"` and `"right"` are valid; an empty array means no gesture back |

Item widths are distributed evenly between one `touch_target` (a 9 mm finger by default) and
`nav_item_max_span` of them (3), centred. To place them yourself, see
[04 §10.1](04-customization.md#101-the-nav-bars-items).

> `style = "gesture"` has no back button — back is a `back_edges` swipe — so combined with an
> empty `back_edges` it is an **`Error::Config`** that stops the shell from starting: that
> combination leaves a machine with no way back at all.

### 2.2 The disabled rule

A disabled item is drawn muted and does not take taps.

- `back` is refreshed every frame by the shell from `!workspace.is_home()`. At home it is
  disabled.
- `recents` and `split` are **not** disabled. The shell acts on them (§2.3) and reports them as
  `ShellEvent::OverviewRequested` / `SplitRequested` either way, so a device with an overview of
  its own turns the shell's off in `[workspace]` and takes the events. A session short of their
  gates (`nav.recents`, `workspace.split`) still sees them; a press asks for an unlock.
- A `nav_item` that fails its gate is not drawn. Its slot is empty and `NavBar::item_rect`
  returns `None`.

### 2.3 Recent screens and the split

**The recent screens** — `"recents"`, `LaunchAction::OpenOverview` — come up over what is on
show: the screen shrinks into its card, and every live task has one, with its icon, its title,
when it was last used, and the level it needs where that is more than everyone has.

| On the cards | Does |
|---|---|
| Tap a card | Brings its task forward |
| Swipe a card up | Ends its task — `ScreenClosed` for each of its screens |
| A card's split button | Puts its task beside the pane on show |
| "Close all" | Ends every task and goes home |
| Tap past the cards · back · a back swipe · `"recents"` again | Goes back to what was on show — a back swipe over the cards never pops the screen behind them |

**The split control** — `"split"`, `tile.split_screen`, `LaunchAction::ToggleSplit` — goes back
to one pane when split, keeping the focused one. With one pane it brings the cards up as a picker
("Choose a screen for the other side") offering only the tasks whose screens fit beside the one on
show, and the card tapped goes beside; with the recent screens turned off it takes the task used
last of those. Where no split can come up — at home, over a screen that keeps the content to
itself, or with nothing that fits — it says so in a toast rather than doing nothing. Pressed again
over its cards, it takes them down. What two panes mean for a screen is
[02 §6.1](02-screens.md#61-two-panes).

- **Both are always reported** — `ShellEvent::OverviewRequested` / `SplitRequested` — and
  `ShellEvent::SplitToggled(bool)` follows a split coming up or going.
- **The cards make way.** A screen opened from anywhere else while they are up — a launch, a
  notification, another thread — takes them down first, so it is on show; home takes them down
  too, the shade goes up when they come up, and the words on them pass through `Strings` like the
  shade's.
- **`[workspace] overview = false` / `split = false`** leave them reported only, for a device with
  an overview or a layout of its own ([07 §16](07-config-reference.md#16-workspace)).
- **Both pass a gate**: `nav.recents` and `workspace.split`
  ([05 §2.1](05-access-control.md#21-built-in-gates)). Short of one, a press asks for an unlock.
  Going back from the cards, or to one pane, is never gated. The cards follow the session: a
  drop below the gate they came up behind takes them down, and their split buttons come and go
  with `workspace.split`.
- **A card thrown away is a decision.** Home, or anything else that takes the cards down while
  it is still flying, ends its task all the same.
- **Timings** are `[motion.panes]` and `[motion.overview]`
  ([07 §10.11](07-config-reference.md#1011-motionpanes)), the throw speed is
  `[motion] fling_px_s`.
- **The cards and the ground under them can be drawn your way** — `recents_card_painter`,
  `recents_ground_painter` ([04 §10.7](04-customization.md#107-the-recent-screens)). Everything in
  the table above stays the shell's.
- **Sizes are tokens**: the divider is `metrics.split_divider` (8 du); the cards' gap and
  narrowest width are `components.overview.card_gap` and `card_min_width`
  ([04 §1.2](04-customization.md#12-metrics-tokens) · [§1.6](04-customization.md#16-widget-metrics--componentspec)).

### 2.4 Controls the shell draws but does not act on

Two presses leave only as events. The icon is the crate's; what it means is yours — with one
exception: in `[access] mode = "prompt"` with an authenticator, the shell carries out the lock
(its lock screen) and the logout (back to `[access] initial`) as well, and still reports them
([05 §7](05-access-control.md#7-the-session--temporary-unlocks-the-timeout-the-idle-lock)).

| Where | Event |
|---|---|
| `[overlay] footer = ["lock"]` · `tile.lock` · `LaunchAction::Lock` | `ShellEvent::LockRequested` |
| `LaunchAction::Logout` · `ShellHandle::logout` | `ShellEvent::LogoutRequested` |

```rust
# use fairing::{Shell, ShellEvent};
# fn my_lock_screen(_: &mut Shell) {}
# fn sign_out(_: &mut Shell) {}
fn handle(shell: &mut Shell) {
    for event in shell.poll_events() {
        match event {
            ShellEvent::LockRequested => my_lock_screen(shell),
            ShellEvent::LogoutRequested => sign_out(shell),
            _ => {}
        }
    }
}
```

Outside `prompt` mode, locking and logging out are not carried out by the shell — what "locked"
is on your device, and which subject "logged out" is, are yours. `shell.handle().set_subject(..)`
is the lever when they mean an access level. The shade does close itself first on a lock, so that
whatever you open is visible.

Not taking an event leaves the control drawn and the press doing nothing, which is the same
contract as a Wi-Fi row tap. Where that is not what you want, take the control out of the list:
`[overlay] footer`, `[nav_bar] items`, `[overlay] tiles`.

### 2.5 Adding your own item

The same shape as a status bar item, except that the slots are the layout: a declaration not
listed in `items` is not drawn. It does not get appended. On the frame after you add one, the
shell logs a warning if the list still does not name it, or if the bar is off or in the gesture
style and so will never draw it. Listing it in code after the `add`
(`shell.nav_bar_mut().style`) is fine: the check waits for that frame.

```rust
use fairing::{nav_item, Cx, LaunchAction, Shell};

fn add_keyboard_item(shell: &mut Shell) {
    shell.add(
        nav_item("kbd", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            if ui.button("KBD").clicked() {
                cx.launch(LaunchAction::open("settings.keyboard"));
            }
        })
        .gate("nav.kbd"),
    );
}
```

```toml
[nav_bar]
items = ["back", "home", "kbd"]
```

The shell draws press tint and scale for built-in items only. Your own item makes its own with
`cx.animate(..)`.

The items go in even cells across the bar. To place them yourself — back hard against an edge, a
wider home, a gap — give the builder a layout; the shell still draws and presses them where you
put them ([04 §10.1](04-customization.md#101-the-nav-bars-items)).

### 2.6 The gesture style

```toml
[nav_bar]
style = "gesture"
back_edges = ["left"]
```

The bar becomes a band with the home indicator — a pill `metrics.nav_indicator_length` long and
`metrics.nav_indicator_thickness` thick ([04 §1.2](04-customization.md#12-metrics-tokens)) — and
the bottom edge takes the buttons' work:

| Gesture | Does | Like |
|---|---|---|
| Up from the bottom edge, let go | Home. The screen on show follows the finger, shrinking as it rises, and carries on into its icon from where it was let go | `"home"` |
| Up, then a pause | The recent screens: the lifted screen carries on into its card | `"recents"` — reported as `ShellEvent::OverviewRequested`, gated by `nav.recents`, acted on only with `[workspace] overview` |
| Along the indicator, right | The task used before the one on show slides in from the left | — |
| Along the indicator, left | The task used after it — back where a run of switches came from | — |
| Along the indicator, at home | The task used last comes back | — |
| In from a `back_edges` edge | Back, as in the buttons style | `"back"` |

- **Home or not is the release's.** Let go past `[motion] snap_ratio` (a third, by default) of
  the lift's travel — 35 % of the pane's height, between 15 and 60 mm — or flung up faster than
  `[motion] fling_px_s`, it is home; short of that, the screen springs back down and goes on. A
  slide along the indicator goes through by the same two rules, across the pane's width. A pause
  counts only a quarter of the way up: lower, it is a hand making up its mind, and a later pause
  higher up still brings the cards.
- **A run of switches keeps its order.** Right, right, left goes back one, rather than bouncing
  between the last two. A touch anywhere but the indicator's edge ends the run.
- **Over the recent screens**, up from the bottom edge is home.
- **The gestures come with the bar.** `[nav_bar] enabled = false` takes them with it; a screen
  hiding the bar (`ChromePolicy::nav_bar = BarMode::Hide`) does not, unless it also sets
  `allow_peek = false` or `edge_guard = true`. Over an open shade, behind the unlock prompt and
  on the on-screen keyboard's keys, nothing starts.
- **Lifecycle**: the screen on show is `Paused` when a lift or a slide begins, and `Resumed` if
  it comes back; one that goes is `Stopped`, and the task that came in is `Resumed`.

The shell has no other way into the recent screens in this style; `LaunchAction::OpenOverview`
and `ShellHandle::launch` still reach them.

---

## 3. The desktop

The desktop is not a screen; it is the workspace's home view. Icons **always originate in code
declarations**, and the config file only overrides position, label, icon name and lock
presentation. An id in config with no declaration in code logs a warning and is ignored.

### 3.1 Putting icons on from code

Only screen and action declarations that have an icon are candidates. Without `.icon(..)` a
declaration is not one.

```rust
use fairing::{action, icon, screen, Cx, Shell};

fn add_desktop_icons(shell: &mut Shell) {
    // Auto-placed in the next free cell.
    shell.add(
        screen("dashboard", |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
            ui.heading("Dashboard");
        })
        .title("Dashboard")
        .icon(icon::GAUGE)
        .desktop(),
    );
    // Pinned to page 0, cell (2, 1).
    shell.add(
        screen("valve", |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
            ui.label("Valve");
        })
        .title("Valve")
        .icon(icon::WRENCH)
        .desktop_at(0, 2, 1),
    );
    // Pinned to the dock (it does not also go on the grid).
    shell.add(
        screen("settings.home", |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
            ui.label("Settings");
        })
        .title("Settings")
        .icon(icon::SETTINGS)
        .dock(),
    );
    // An action icon: runs immediately, opens no screen.
    shell.add(
        action("scan-wifi", |cx: &mut Cx<'_>| {
            let _ = cx.services.wifi.scan();
        })
        .title("Scan")
        .icon(icon::WIFI)
        .desktop(),
    );
}
```

| Builder | Meaning |
|---|---|
| `.desktop()` | Auto-placed in the next free cell |
| `.desktop_at(page, col, row)` | Pinned. If the cell is taken or outside the grid, it warns and auto-places |
| `.dock()` | Puts it on the dock. An id on the dock is not placed on the grid again |
| `.visibility(Visibility)` | How a failed gate presents (§3.6) |
| `.description(text)` | What the icon's info popover says under the title (§3.8) |

### 3.2 Overriding from config

```toml
[desktop]
columns = 4
rows = 3
dock = ["settings.home", "dashboard"]
label_lines = 2
wallpaper = "background"

[[desktop.pages]]
icons = [
  { id = "dashboard" },
  { id = "valve", label = "Valve control", col = 2, row = 1 },
  { id = "settings.wifi", label = "Wi-Fi", icon = "wifi", locked = "show" },
]
```

| `[desktop]` key | Default | Meaning |
|---|---|---|
| `columns` | `4` | Column count. `0` means automatic from the cell size. Capped at 12 (`MAX_COLUMNS`) |
| `rows` | `3` | Row count. `0` is automatic. Capped at 8 (`MAX_ROWS`) |
| `dock` | `[]` | Dock ids, in order. **No count limit**; an id with no declaration logs a warning and is skipped |
| `dock_edge` | `"bottom"` | Which edge the dock sits on: `bottom` · `top` · `left` · `right` |
| `dock_band` | none | A floating band across the desktop instead of an edge, at this fraction (0..1) of the content |
| `label_lines` | `2` | Label line count |
| `wallpaper` | `"background"` | A palette role name · `#RRGGBB` · `"abyss"` · `"file:<path>"` (§3.7) |
| `wallpaper_fit` | `"cover"` | The fit for `wallpaper = "file:…"`: `cover` · `contain` · `stretch` |
| `rail` | `"none"` | **The icon rail** — `left` or `right` keeps the icons in a vertical column and opens screens beside it (the kiosk layout) |
| `rail_width` | `0` | Rail width as a fraction of the content width. `0` means the default, 0.26 |
| `label_legibility` | `"none"` | Label correction over a photo background: `none` · `shadow` · `veil` ([09 §2.2a](09-branding.md#22a-when-you-cannot-change-the-artwork--label_legibility)) |
| `long_press` | `"info"` | What holding an icon does: `info` brings up its info popover (§3.8), `none` only reports `ShellEvent::IconLongPressed` |

| `[[desktop.pages]].icons` field | Meaning |
|---|---|
| `id` | The declaration id (required) |
| `label` | Label override |
| `description` | Description override — what the info popover says under the title (§3.8) |
| `icon` | Built-in icon name override. Not in the built-in set warns and is ignored |
| `locked` | `"show"` (padlock) or `"hide"` |
| `col` · `row` | Pins the cell. Both are needed; with only one, it becomes auto-placement on that page |

`[[desktop.pages]]` is an array, so writing it several times gives you page 0, page 1 and so on.
Ids listed there are placed on that page.

Placement order is: ① the dock (`[desktop] dock` order, then `.dock()` declaration order),
② pinned cells, ③ everything else in declaration order.

The `[desktop.abyss]` section that tunes the built-in procedural background is in
[09 Appendix A](09-branding.md#appendix-a-every-desktopabyss-key).

### 3.3 Pages

Pages are added automatically when the cells run out. The page indicator strip only takes space
with two or more pages.

| Gesture | Result |
|---|---|
| Horizontal drag over the grid | The page follows the finger 1:1; on release, distance and velocity decide |
| Tapping an indicator dot | Springs to that page |
| Dragging past the ends | Rubber-band resistance (`[motion.page] rubber`) |

From the frame a horizontal drag passes the slop, icon taps in that gesture are cancelled. In
code, move with `shell.desktop_mut().set_page(i)` (immediate) or `swipe_to(i, &tokens)` (spring).

### 3.4 The dock

The dock stays in the same place regardless of the page. **There is no count limit** — the icons
share the band's width. When it is empty the band is not drawn at all (and the grid grows by
that much).

### 3.5 Badges

Badges are set from code only. There is no config key.

```rust
use fairing::desktop::Badge;
use fairing::{Shell, ShellHandle};

fn mark_pending(shell: &mut Shell, count: u32) {
    // Directly on the UI thread. `false` if there is no such id.
    let _ = shell.set_badge("inbox", Some(&Badge::Count(count)));
    let _ = shell.set_badge("inbox", None); // clear
}

fn mark_from_worker(handle: &ShellHandle) {
    // From another thread. Applied on the next frame.
    handle.set_badge("inbox", Some(Badge::Dot));
}
```

| Variant | What it draws |
|---|---|
| `Badge::Count(u32)` | A number pill |
| `Badge::Dot` | A dot |
| `Badge::Text(String)` | Short text |

Badges survive rebuilding the declaration list (adding or removing screens).

### 3.6 Lock presentation

`Visibility` decides how an icon whose gate is closed presents.

| Value | Presentation | How to set it |
|---|---|---|
| `Visibility::Locked` (default) | Grey with a padlock badge. A tap emits `AccessEvent::UnlockRequested` | `.visibility(Visibility::Locked)`, or `locked = "show"` |
| `Visibility::Hidden` | Not drawn at all | `.visibility(Visibility::Hidden)`, or `locked = "hide"` |

The full rules are in [05 Access control](05-access-control.md).

### 3.7 Wallpaper

There are nine `Wallpaper` variants.

| Variant | Argument | From config |
|---|---|---|
| `Wallpaper::Solid(ColorRole)` | A palette role | A role name, e.g. `wallpaper = "background"` |
| `Wallpaper::Fixed(Color32)` | A fixed colour | `wallpaper = "#101418"` |
| `Wallpaper::Gradient { top, bottom }` | Two colours, vertical | not available (code only) |
| `Wallpaper::Texture { id }` | An `egui::TextureId` — **stretches** | not available (code only) |
| `Wallpaper::TextureFit { id, source_size, fit }` | Keeps the aspect but **does not hold the handle** | not available (code only) |
| **`Wallpaper::owned(texture, fit)`** | **Holds** the handle. Normally the one you want | `wallpaper = "file:…"` (needs a loader) |
| `Wallpaper::Painter(Box<dyn Fn(&Painter, Rect)>)` | Procedural drawing | not available (code only) |
| `Wallpaper::ThemedPainter(..)` | Procedural drawing that receives the theme | `wallpaper = "abyss"` (the one built-in) |
| `Wallpaper::themed(dark, light)` | Two wallpapers, one for each palette; the shell picks by `theme.dark` as the theme switches | not available (code only) |

It is `#[non_exhaustive]`, so a `match` outside the crate needs a `_` arm.

**The crate has no image decoder.** To use a PNG, JPEG or WebP as the background, plug a decoder
into `ShellBuilder::image_loader` — ten lines, and with it installed `wallpaper = "file:<path>"`
in the config file starts working ([09 Branding §1](09-branding.md#1-the-crate-does-not-decode-images--you-plug-in-a-loader)).

```rust
use fairing::desktop::{Fit, Wallpaper};
use fairing::Shell;

fn set_gradient(shell: &mut Shell) {
    shell.desktop_mut().set_wallpaper(Wallpaper::Gradient {
        top: egui::Color32::from_rgb(0x10, 0x18, 0x28),
        bottom: egui::Color32::from_rgb(0x04, 0x06, 0x0a),
    });
}

/// The integrator uploads the texture with `ctx.load_texture(..)`. **Hand the handle over** and
/// the wallpaper holds it.
fn set_texture(shell: &mut Shell, texture: egui::TextureHandle) {
    shell.desktop_mut().set_wallpaper(Wallpaper::owned(texture, Fit::Cover));
}

/// With a loader installed, straight from a file at runtime.
fn set_from_file(shell: &mut Shell, ctx: &egui::Context) -> fairing::Result<()> {
    shell.load_wallpaper(ctx, "/opt/acme/brand/bg.webp", Fit::Cover)
}
```

Pass only an `id` to `Wallpaper::TextureFit` (or `Texture`) and the texture is freed the moment
you drop the `TextureHandle`, leaving a black background. Unless you manage an atlas yourself,
**use `Wallpaper::owned`** — the wallpaper holds the handle.

`Wallpaper::Painter` is in
[04 Customization §5](04-customization.md#5-painting-the-wallpaper-yourself).

### 3.8 Holding an icon: the info popover

Hold a finger on an icon — on the grid, the dock or the rail — and a card comes up over it saying
what it is, before anyone opens it:

- the icon and its **title**;
- the **level it needs** (`Required level · maintenance`), where that is more than the lowest. On
  an icon the session cannot open yet the level is in the warning colour, beside a closed padlock;
- its **description**, if the declaration gives one.

```rust
use fairing::{icon, screen, Cx, Shell};

fn add_pumps(shell: &mut Shell) {
    shell.add(
        screen("pumps", |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
            ui.heading("Pumps");
        })
        .title("Pumps")
        .description("Starts and stops the feed pumps, and shows their run hours.")
        .icon(icon::GAUGE)
        .desktop(),
    );
}
```

The description is a key, like the title: it is looked up in the string table where it is drawn
([04 §9](04-customization.md#9-text-and-translations)). An action takes `.description(..)` the
same way, and `[[desktop.pages]]` can give one from the config (§3.2).

The card stands over the icon and points at it; on the top row, where there is no room above, it
hangs below. Either way it keeps inside the space between the bars.

**Any press puts it away, and that press does nothing else**: it does not open the icon under the
finger, turn the page or reach a bar. Back puts it away too, and so does anything that takes the
desktop's place — the shade, the unlock prompt, a screen opening, the recent screens. Letting go
of the hold that brought it up opens nothing.

The hold is `[gesture] long_press_ms` (500 ms by default), and it goes out as
`ShellEvent::IconLongPressed { id }` whatever the shell draws. A device with a menu of its own for
the event sets `[desktop] long_press = "none"`: then only the event goes out — and the release
still opens nothing. With `[gesture] enabled = false` there is no long press at all.

The card's widest, its caret and the gap beside its icons are `components.popover`
([04 §1.6](04-customization.md#16-widget-metrics--componentspec)); its padding, corner and type
are the global tokens a card uses.

---

## 4. The shade

The shade is the panel you pull down from the top: a tile grid, a notification list and a footer.
It exists when the `overlay` feature is on (it is by default).

### 4.1 Opening it

| Path | Condition |
|---|---|
| Swipe down from the top edge | `[gesture] enabled = true` |
| Tap empty status bar space | `[status_bar] tap_opens_shade = true` |
| Tap `status.notifications` | That item is in a slot list |
| `cx.launch(LaunchAction::OpenOverlay)` | Anywhere |
| `shell.handle().toggle_overlay()` | Including from another thread |

Every path goes through the `overlay.open` gate. Fail it and only `AccessEvent::UnlockRequested`
goes out; the shade does not open.

The moment the shade **leaves its closed state** (the first pull, not the fully-open position)
the focused screen gets `Lifecycle::Paused`, and it gets `Resumed` when the shade is fully closed
again (`overlay/mod.rs:15` — the shell decides from the change in `Overlay::is_closed`). Opening
it takes the unread notification count to zero and absorbs any heads-up banner immediately.

### 4.2 Built-in tiles

| id | Kind | Setting key · action | Notes |
|---|---|---|---|
| `tile.wifi` | `Toggle` | `wifi.enabled` | The backend is the truth |
| `tile.bluetooth` | `Toggle` | `bluetooth.enabled` | The backend is the truth |
| `tile.brightness` | `Slider` | `display.brightness` | Dimmed when the backend returns `None` |
| `tile.volume` | `Slider` | `audio.volume` | An in-memory value in M2 (0.5 by default) |
| `tile.lock` | `Action` | `LaunchAction::Lock` | |
| `tile.theme` | `Toggle` | `theme.dark` | A 200 ms palette crossfade |
| `tile.airplane` | `Toggle` | `radio.airplane` | |
| `tile.rotation_lock` | `Toggle` | `display.rotation_lock` | Enabled only when the display backend reports the capability |
| `tile.split_screen` | `Action` | `LaunchAction::ToggleSplit` | The split control (§2.3) |
| `tile.settings` | `Action` | Opens `settings.home` | |

In code, `fairing::overlay::BUILTIN_IDS` gives you the list.

### 4.3 Tile order and count

```toml
[overlay]
layout = "unified"
tiles = ["tile.wifi", "tile.bluetooth", "tile.brightness", "tile.volume", "tile.lock", "tile.theme"]
tile_columns = 6
footer = ["clear_all", "lock", "settings"]
max_height_ratio = 0.85
peek_ms = 2000
```

| Key | Default | Meaning |
|---|---|---|
| `layout` | `"unified"` | `"split"` (One UI and iOS): notifications on the left, controls on the right, picked by where the pull starts |
| `tiles` | The six above | Tile ids in order. A `tile()` declaration not listed here goes at the end |
| `tile_columns` | `6` | Tiles per row. Zero is a config error at startup |
| `footer` | The three above | The footer buttons, left to right. `[]` draws none of them |
| `max_height_ratio` | `0.85` | Maximum shade height = `min(content, ratio × screen)` |
| `peek_ms` | `2000` | How long a peeked hidden status bar stays revealed |

The card shade (`reveal = "card"`, `card_width_ratio`, `card_anchor`, `card_glass`, `card_relief`),
the split's `split_ratio` and the two-step pull (`two_step`) are in
[07 §6](07-config-reference.md#6-overlay-feature-overlay).

An id in `tiles` that is neither built in nor declared logs a warning and is ignored.

The three footer buttons are `clear_all` · `lock` · `settings`, and **none of them
is unconditional** — a device with no lock concept, or one that never registered
`settings.home` (which the settings button opens), leaves them out of the list.
The subject's name and the level dot are not in the list: those are state rather
than controls, and they always draw. An unknown name warns and is skipped.

Note that `lock` and the `tile.lock` tile issue the same `LaunchAction::Lock`,
so by default the shade offers it twice.

The tiles go in centred rows of `tile_columns`. To place them otherwise — one column, a gap, the
tiles a session may not use left out — give the builder a layout; the row a tile opens, the list
and the two-step stop follow the lowest tile. To draw them otherwise, or the panel under them,
give it a painter: the shell still takes the taps and opens the rows
([04 §10.4](04-customization.md#104-the-shades-tiles-and-panel)).

### 4.4 Adding your own tile

```rust
use fairing::settings::SettingKey;
use fairing::{icon, tile, Shell, TileKind};

fn add_heater_tile(shell: &mut Shell) {
    shell.add(
        tile("tile.heater", TileKind::Toggle(SettingKey::from("app.heater")))
            .icon(icon::THERMOMETER)
            .label("Heater")
            .gate("tile.heater"),
    );
}
```

| `TileKind` | What it draws | Tap |
|---|---|---|
| `Toggle(SettingKey)` | Two states, on and off | Flips the value |
| `Slider(SettingKey)` | 0..=100 | Opens an expanded row under the tile row. The value tracks the finger 1:1 |
| `Action(LaunchAction)` | Off, or lit from `.lit_by(key)` (below) | Runs the action |
| `Status` | Read-only and dimmed, or lit from `.lit_by(key)` (below) | none |
| `Gauges { rows, label_width }` | **Several gauge rows** — you declare the count, names, colours and units (§4.4c) | Opens an expanded row |
| `Panel { height }` | An expanded row **you draw** (§4.4c, `tile_panel`) | Opens an expanded row |

| Builder | Default |
|---|---|
| `.icon(IconRef)` | `icon::SETTINGS` |
| `.label(text)` | The id string |
| `.gate(name)` | The same name as the id |
| `.enabled(bool)` | `true` |
| `.long_press(LaunchAction)` | none (built-in tiles have the pairings in §4.4d) |
| `.lit_by(key)` | none — lights a tile that is not a switch, below |

The setting key of a `Toggle` or `Slider` is checked twice. The render filter looks at the tile's
gate, and the actual write makes the shell check the key name's gate again. So making `tile.wifi`
visible means assigning **both** `tile.wifi` and `wifi.enabled`.

#### Lighting a tile that is not a switch

A tile that opens a screen is not a switch, so `Action`, `Status`, `Gauges` and `Panel` have no
on/off of their own and the crate draws them unlit. Your device often does know: a conveyor is
running, a heater is at temperature, a door is open. `.lit_by(key)` names the setting that answer
lives in, and the tile is lit while it holds `Bool(true)` — its puck is filled with the accent, the
same as a toggle that is on. Without `.lit_by` these kinds are never filled: a button that looks
latched is a lie.

```rust
# fn f(shell: &mut fairing::Shell) {
use fairing::overlay::{tile, TileKind};
use fairing::settings::SettingValue;
use fairing::{icon, LaunchAction};

shell.add(
    tile("tile.conveyor", TileKind::Action(LaunchAction::open("app.conveyor")))
        .icon(icon::GAUGE)
        .label("Conveyor")
        .lit_by("app.conveyor.running"),
);

// …from wherever the truth is, on any thread:
shell.handle().set_setting("app.conveyor.running", SettingValue::Bool(true));
# }
```

Do not reach for a `Toggle` to fake this — it turns a tile that opens a screen into a switch, and
a press then writes the value instead of opening anything. `Toggle` and `Slider` already read their
own key, so `.lit_by` on one of those is ignored with a warning.

`Overlay::tile_state(id)` reads back what a tile is drawn as, which is the quick way to check the
key is arriving.

**The key is a gate name**, like every setting (§[05 §2.3](05-access-control.md)). With an
`[access] default_gate` in force, the write is refused unless `[access.gates]` names the key at a
level the writer holds. Machine state is not something the person at the panel sets, so give those
keys the lowest level:

```toml
[access.gates]
"app.conveyor.running" = "viewer"
```

### 4.4a Removing, reordering, adding

What stands on the panel is decided by **one line**, `[overlay] tiles`. Mix built-in ids with
your own declarations, and they stand in the order you wrote. A built-in tile not in the list is
**not shown**; a declaration not in the list goes at the end.

```toml
[overlay]
tiles = ["tile.wifi", "tile.hopper", "tile.jog", "tile.brightness"]
tile_columns = 4
```

| What you want | How |
|---|---|
| Remove a built-in tile | Delete it from the list |
| Reorder | List order |
| How many per row | `tile_columns` (the rest wrap) |
| Just rename one | A `tile(...)` declaration with the same id, plus `.label(..)` |
| Lock one | The tile id in `[access.gates]` (the gate defaults to the same name as the id) |
| Add something that does not exist | §4.4b below |

### 4.4b The icon spec

`.icon(..)` takes any of the four `IconRef` variants. For all four **the shell decides the size**
(a tile draws a puck sized from `metrics.touch_target`) and the colour follows a palette role —
so that icons do not stand at four different sizes.

| What you put in | How | Spec |
|---|---|---|
| A built-in icon | `icon::GAUGE` (the `fairing::icon` constants) | — |
| **Your own vector icon** | `icons.register(IconDef { name, segs, fill })` → `IconRef::Custom(id)` | **A 24 × 24 grid**; coordinates must not leave it. Stroke width is 2.0 on that grid and scales with the size |
| A bitmap | `IconRef::Texture { id, tint }` | An egui texture id. `tint = true` dyes it the icon colour; `false` keeps the source colours (logos, photos) |
| A glyph or emoji | `IconRef::Glyph("⚑".into())` | It has to exist in the fonts you installed — otherwise □ |
| A drawing callback | `icons.register_painter(..)` → `IconRef::Custom(id)` | Draws directly into the rect the shell gives it. For icons whose shape depends on state |

A vector icon's path is a list of `Seg::{M, L, Q, C, Z}`. Porting from SVG, set the `viewBox` to
`0 0 24 24` and copy the coordinates across — coordinates outside it are caught by the tests.

```rust
# use fairing::overlay::{tile, TileKind};
# fn add(shell: &mut fairing::Shell, kind: TileKind) {
use fairing::icons::{IconDef, IconRef, Seg};

let id = shell.register_icon(IconDef {
    name: "hopper",
    segs: &[Seg::M(5.0, 4.0), Seg::L(19.0, 4.0), Seg::L(14.0, 20.0),
            Seg::L(10.0, 20.0), Seg::Z],
    fill: false,
});
shell.add(tile("tile.hopper", kind).icon(IconRef::Custom(id)));
# }
```

### 4.4c Tiles the crate has no kind for

For something that fits none of the built-in kinds (toggle, slider, action, status) there are two
routes. **Look at the declarative one first** — letting the crate draw keeps the finger height,
the press feedback and the colours in line with every other tile.

#### Declarative: `TileKind::Gauges`

Several gauge rows open. You write down **how many rows, and each row's name, colour, unit and
whether it can be touched**; the crate does the drawing.

```rust
# fn add(shell: &mut fairing::Shell) {
use fairing::overlay::{tile, Gauge, TileKind};
use fairing::ColorRole;

shell.add(
    tile("tile.hopper", TileKind::Gauges {
        rows: vec![
            Gauge::new("hopper.a", "Resin A").color(ColorRole::Primary),
            Gauge::new("hopper.b", "Resin B").color(ColorRole::Warning),
            Gauge::new("hopper.c", "Solvent").read_only(true),   // a sensor value
        ],
        label_width: 96.0,
    })
    .label("Hoppers")
    .icon(fairing::icon::GAUGE),
);
# }
```

| What you write | What it is | Default |
|---|---|---|
| The length of `rows` | **How many rows appear** | — (empty means it does not open) |
| `label_width` | The name column's width (du). `0` draws no names and widens the track | — |
| `Gauge::new(key, label)` | The setting key the value lives in · that row's name | — |
| `.color(role)` | That row's track colour (a palette role) | `Primary` |
| `.unit("%")` | The unit after the value. **An empty string draws no value readout** | `"%"` |
| `.value_width(du)` | The value column's width | 3.5 × the body type size (`100 %` fits) |
| `.read_only(true)` | Untouchable — drawn dimmed and refuses drags | `false` |

Values live in settings as integers in `0..=100`. It is the same contract as the built-in
brightness and volume sliders, so **the write gate is the key name** in exactly the same way
(05 §3), and a change emits `ShellEvent::SettingChanged`. The row height is one finger per row,
so there is nothing to write for it.

#### Drawing it yourself: `tile_panel`

For something that does not fit the spec at all (a two-axis jog pad, a small chart, a
machine-specific control) you take the whole row. You get the same `(&mut Ui, &mut Cx)` a screen
body gets.

```rust
# fn add(shell: &mut fairing::Shell) {
use fairing::overlay::tile_panel;

shell.add(
    tile_panel("tile.jog", 132.0, move |ui, cx| { /* all of this is yours */ })
        .label("Jog")
        .icon(fairing::icon::SPLIT),
);
# }
```

Either way, the shell's job is the same.

| The shell does | You do |
|---|---|
| The tile puck, label, press feedback, gate decision | `Gauges`: the declaration. `tile_panel`: everything inside the row |
| Opening and closing, and the transition where the tile drops into the row | — |
| Leaving the icon space free on the left of the row (the tile fills it) | Lay out inside the rest of the rect |
| Owning drags in that row (the shade does not steal them) | Sliders and drag widgets, freely |

It uses **the same space** as a Slider tile, so only one expanding tile is open at a time. A
locked tile does not open its row.

### 4.4d Long-pressing a tile

**Long-pressing a tile** goes to that tile's settings screen. How long is set by
`[gesture] long_press_ms` (500 ms by default), and where it goes is attached to the declaration.

```rust
# fn add(shell: &mut fairing::Shell) {
use fairing::overlay::{tile, TileKind};
use fairing::settings::SettingKey;
use fairing::LaunchAction;

shell.add(
    tile("heater", TileKind::Toggle(SettingKey::from("app.heater")))
        .long_press(LaunchAction::open("app.heater.settings")),
);
# }
```

Built-in tiles go to their paired built-in settings screen **without being told to**.

| Tile | Long press goes to |
|---|---|
| `tile.wifi` | `settings.wifi` |
| `tile.bluetooth` | `settings.bluetooth` |
| `tile.brightness` · `tile.theme` | `settings.display` |
| `tile.volume` | `settings.sound` |

Only if that screen is actually registered — turn it off with `[settings] only`, or drop the
`settings` feature, and it goes nowhere.

Attached or not, **the long press itself always** goes out as
`ShellEvent::TileLongPressed { id }`. Take that if you want a menu or a dialog instead of a
screen. A locked tile goes nowhere on a long press — the unlock prompt is raised on taps only
(raise it on a mere press-and-hold and it is easy to summon by accident). A press consumed by a
long press **does not emit a tap** on release: otherwise Wi-Fi would turn off *and* the settings
screen would open.

Desktop icons follow the same contract and emit `ShellEvent::IconLongPressed { id }`; the shell
brings up the icon's info popover besides (§3.8).

> Both are caught with **the engine's `Gesture::LongPress`**, not egui's
> `Response::long_touched()`. The latter only fires when real touch events arrive, so on a panel
> that funnels touch through as mouse input it never fires at all.

### 4.5 The notification list and gate redaction

Below the tiles is the notification list, and below that a "clear all" footer.

| Situation | What the list shows |
|---|---|
| No notifications | One line, `No notifications` |
| An ordinary notification | Icon · title · body · time (+ a progress bar) |
| A notification that fails its gate | One line, `1 notification`. Title, body and progress are hidden |
| A persistent notification | Not removed by a swipe or by "clear all" |

It does not hide the fact that *something* is there. That decision lives in one place,
`fairing::notify::shows_content(&notification, access)`, and the hidden count comes from
`NotificationCenter::hidden(access)`.

The list handles gestures like this.

| Gesture | Result |
|---|---|
| Tap a row | Runs `Notification::action` |
| **Swipe left** | Dismisses a non-persistent notification. On release the row **flies out** and is then removed |
| The `×` on the right of a row | The same dismissal, down the same path. With `[notify] dismiss_button = false` it is **not drawn** |
| **Swipe right** | Runs `Notification::action` (the same as a tap). **Only notifications that have an `action`** drag to the right |
| "Clear all" in the footer | Dismisses every non-persistent notification |

**Direction carries the meaning.** Left discards, right opens. While you drag, the outcome shows
in the space you uncover — a bin on a red ground going left, an arrow on an accent ground going
right. A notification with nowhere to go (`action` is `None`) **does not drag right at all**:
better than looking like it will do something and then doing nothing.

The commit threshold is a third of the list's width, or a fling in the same direction — the same as pulling the shade.

**Dismissal is two steps.** Once the card is fully out sideways, its **slot** collapses to zero
height over `[motion.toast] out_ms`, and the notification leaves the list on the frame the
collapse finishes. While it collapses the rows below rise into the gap, so you can see what left
— removing it immediately makes the rows below jump and you cannot tell a dismissal from a
glitch. A row that is collapsing does not take taps.

The `×` can be turned off. On a finger-only machine one swipe is enough and the card is cleaner.
On a machine where swipes register poorly — gloves, a resistive panel — the button has to be
there, so it is **policy, not taste**. Turning it off never removes the way to dismiss (the
swipe and "clear all" stay).

With `motion.reduce = true`, all three disappear instantly as they used to.

```rust
# use fairing::{LaunchAction, Notification, NotificationId};
# fn notify(shell: &mut fairing::Shell) {
// Give it somewhere to go and it can be opened with a right swipe.
shell.notify(
    Notification::new(NotificationId::of("sensor.3"), "Sensor 3 offline")
        .body("check wiring")
        .action(LaunchAction::open("device.detail")),
);
# }
```

Notifications from the same `source` stay together as a group, and a new one from that source
brings the whole group to the front.

---

## 5. Notifications and toasts

Both live in memory only. A restart clears them.

| | Notification (`Notification`) | Toast (`Toast`) |
|---|---|---|
| Lives in | The notification centre (the shade) | A queue at the bottom of the content — above the on-screen keyboard while it is up |
| Lifetime | Until dismissed | Leaves automatically after `duration` |
| Gate | Yes (content is redacted) | No |
| Dedup | The same id updates in place | None |
| On arrival | A brief heads-up banner | Rises from the bottom |

### 5.1 Notification fields

| Field | Type | Default | Builder |
|---|---|---|---|
| `id` | `NotificationId` | — | `Notification::new(id, title)` |
| `title` | `String` | — | `Notification::new(id, title)` |
| `body` | `String` | empty | `.body(text)` |
| `source` | `String` | empty | `.source(text)` — the grouping key |
| `icon` | `IconRef` | `icon::BELL` | `.icon(icon)` |
| `level` | `Level` | `Level::Info` | `.level(level)` |
| `at` | `WallTime` | default | Filled in by the shell from `services.clock` |
| `persistent` | `bool` | `false` | `.persistent()` |
| `progress` | `Option<f32>` | `None` | `.progress(0.0..=1.0)` |
| `action` | `Option<LaunchAction>` | `None` | `.action(action)` |
| `gate` | `Option<Gate>` | `None` | `.gate(name)` |

`Level` is `Info` · `Success` · `Warning` · `Error`. `.level(..)` only changes the icon to the
severity default when you did not give one. The accent colours are `Success` → `success`,
`Warning` → `warning`, `Error` → `danger`; `Info` uses no accent.

`NotificationId::of("wifi.lost")` is a stable hash of the string. Sending the same key again
updates in place, keeping its position.

### 5.2 Toast fields

| Field | Type | Default | Builder |
|---|---|---|---|
| `text` | `String` | — | `Toast::new(text)` |
| `level` | `Level` | `Level::Info` | `.level(level)` |
| `duration` | `Duration` | `Duration::ZERO` = `[notify] toast_ms`; `Duration::MAX` = until tapped | `.duration(d)` |
| `icon` | `Option<IconRef>` | `None` | `.icon(icon)` |

`&str` and `String` are `Into<Toast>`, so `handle.toast("Saved")` works directly.

### 5.3 Sending from another thread

`ShellHandle` is `Clone + Send`. Inside it is one channel and a wake handle — no locks. Sending
wakes the UI, and it is processed in stage 2 of the next frame.

```rust
use fairing::notify::Level;
use fairing::{Notification, NotificationId, Shell, ShellHandle, Toast};
use std::time::Duration;

fn spawn_update_worker(shell: &Shell) {
    let handle: ShellHandle = shell.handle();
    std::thread::spawn(move || {
        handle.toast(Toast::new("Downloading firmware").duration(Duration::from_secs(2)));
        // Repeating the same id updates it in place (dedup).
        for (progress, step) in [(0.33, "1/3"), (0.66, "2/3"), (1.0, "3/3")] {
            handle.notify(
                Notification::new(NotificationId::of("update"), "Firmware update")
                    .source("update")
                    .body(step)
                    .progress(progress)
                    .persistent(),
            );
        }
        handle.notify(
            Notification::new(NotificationId::of("update.done"), "Update complete")
                .source("update")
                .level(Level::Success),
        );
    });
}
```

The same handle is used from the UI thread (inside a screen closure): `cx.shell.notify(..)` ·
`cx.shell.toast(..)`. Holding a `Shell` directly, there are also `shell.notify(..)` ·
`shell.toast(..)`.

### 5.4 Heads-up

A new notification appears briefly as a banner at the top.

| Condition | Value |
|---|---|
| It appears when | `[notify] heads_up = true` · the notification is new (`Added`) · the shade is closed (all three) |
| How long it stays | `[motion.toast] heads_up_hold_ms` (4000 by default) |
| While held down | The timer stops |
| Swipe up | Dismisses it |
| Tap | Runs `Notification::action`, then leaves |
| If the shade opens | Absorbed immediately (no exit animation) |
| A gate the session fails | Redacted as in the shade: one line of "1 notification", the bell, no body and no progress (a `.heads_up_painter(..)` is handed the same) |
| Over the lock screen | Shown, redacted where gated as above; a tap only dismisses it |
| The same id updated while it is up | The banner shows the new content (no new heads-up) |

Only one at a time.

Where the toasts and the banner go, and how each is drawn, can be changed from code — the queue,
the timing and the taps stay the shell's
([04 §10.2](04-customization.md#102-toasts-and-the-heads-up-banner)).

### 5.5 Dedup rules

| Situation | Result |
|---|---|
| A new id | Added to the list, unread +1, heads-up |
| The same id and the same `source` | Content replaced in place. No unread increment, **no heads-up** |
| The same id, a different `source` | Taken out of the list and moved to the new source group. Also treated as an update |
| Over the retention limit | Above `[notify] max_items` (100 by default), the oldest non-persistent ones are dropped |

The unread count goes to zero the moment the shade opens. That is the number in
`status.notifications`'s badge, drawn as `99+` above 99.

### 5.6 `[notify]` keys

| Key | Default | Meaning |
|---|---|---|
| `heads_up` | `true` | Banners for new notifications |
| `max_visible` | `2` | Toasts visible at once. Zero is a config error at startup |
| `max_items` | `100` | Notification centre retention limit |
| `toast_ms` | `3000` | How long a toast with no `duration` shows |

---

## 6. On-screen keyboard

It exists when the `osk` feature is on (it is by default).

### 6.1 When it shows automatically

Decided every frame:

| Screen policy | Shows when |
|---|---|
| `OskMode::Auto` (default) | `ctx.egui_wants_keyboard_input()` is true |
| `OskMode::Manual` | Only on a manual toggle (§6.3) |
| `OskMode::Off` | Never |

Hiding is not immediate. `wants` has to stay false for `[motion.osk] hide_debounce_ms` (100 by
default) before it goes down — that is what stops the keyboard flickering as you tap between
fields.

Closing it with back or with the keyboard's hide key (▾) means it does not come back on its own,
even in `Auto`. Two signals reopen it:

- `wants` going from false to true
- A tap on the screen after it was closed, with focus still there once the finger lifts (so
  re-tapping the same field, or tapping another one, opens it again; a tap on empty space takes
  the focus away and leaves it closed)

### 6.2 Layouts

```toml
[osk]
height_ratio = 0.38
min_key_px = 48.0
layout = "qwerty"
numpad_decimal = true
numpad_sign = false
```

| Key | Default | Meaning |
|---|---|---|
| `height_ratio` | `0.38` | Height = screen height × this. Above 0 and at most 1 |
| `min_key_px` | `48.0` | Minimum height of a key, the gaps between rows not counted. If the ratio gives less, it is raised |

The ratio has a ceiling too. A row of keys is never taller than the theme's `osk_max_key` — one
and a half fingers, and at least 72 du (04 §1.2) — so a tall portrait panel gets a keyboard sized
for a hand rather than a third of the glass. Where the floor and the ceiling cross (a big
`min_key_px` on a small panel), the floor wins. Either way the keyboard is never taller than the
room above the nav bar (the screen, when the nav bar is off), so its top row is always on the
glass — on a panel too short for the floor, the keys come out shorter than `min_key_px`.

The ceiling is a theme token, so it changes in code: give the builder a `MetricsSpec` with an
`osk_max_key` of your own, or `None` to lift it and let the ratio alone decide.

```rust
use fairing::theme::MetricsSpec;
use fairing::unit::{Dim, Span};
use fairing::{Shell, ShellConfig};

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    let spec = MetricsSpec {
        // Rows of at most a finger and a quarter, never under 60 du. `None` lifts the cap.
        osk_max_key: Some(Span::fixed(Dim::finger(1.25)).min(Dim::du(60.0))),
        ..MetricsSpec::default()
    };
    Shell::builder(ShellConfig::default())
        .metrics_spec(spec)
        .build(ctx)
}
```
| `layout` | `"qwerty"` | `"qwerty"`, `"numpad"` or `"hangul"` (`"ko"` too). An unknown value warns and uses qwerty |
| `numpad_decimal` | `true` | A `.` key on the numpad |
| `numpad_sign` | `false` | A `-` key on the numpad |

| `OskLayout` | Faces | Notes |
|---|---|---|
| `Qwerty` | Three: lowercase, uppercase, symbols | ⇧ capitalises one character and returns. Pressed twice within 400 ms it locks |
| `NumPad { decimal, sign }` | One | 3×4 plus a right column (⌫ / ▾ / ⏭ / ✓) |
| `Hangul` | Three, laid out like `Qwerty`'s | Two-set Hangul, composing syllables as you type. A `한/영` key switches to and from QWERTY |
| `Custom(KeyLayout)` | Integrator-defined | Any number of faces. The one-character ⇧ rule is not attached |

To change it at runtime, use `shell.osk_mut().set_layout(..)`.

```rust
use fairing::osk::{KeyAction, KeyDef, KeyFace, KeyLayout, KeyRow, OskLayout};
use fairing::Shell;

fn use_numpad(shell: &mut Shell) {
    shell.osk_mut().set_layout(OskLayout::NumPad {
        decimal: true,
        sign: false,
    });
}

fn use_custom_pad(shell: &mut Shell) {
    let face = KeyFace {
        rows: vec![
            KeyRow {
                keys: vec![KeyDef::text("A"), KeyDef::text("B"), KeyDef::text("C")],
            },
            KeyRow {
                keys: vec![
                    KeyDef::special(KeyAction::Backspace, "⌫", 1.0),
                    KeyDef::special(KeyAction::Enter, "↵", 2.0),
                ],
            },
        ],
    };
    shell.osk_mut().set_layout(OskLayout::Custom(KeyLayout {
        name: "abc".into(),
        faces: vec![face],
    }));
}
```

The `span` in `KeyDef::special(action, label, span)` is a multiple of one default key, held to
`0.5..=6` and rounded to a tenth: narrower than half a key is no target for a finger, and one key
wider than six squeezes the rest of its face. Every row
in a face divides up the width of the longest row, so keys stay the same size even when rows
have different key counts.

`KeyAction` is `Text` · `Backspace` · `Enter` · `Space` · `NextFocus` · `PrevFocus` · `Left` ·
`Right` · `Face(usize)` · `Lang` · `Hide`.

A layout says what the keys are. Where they go on the panel and how each is drawn are two hooks
of their own ([04 §10.3](04-customization.md#103-the-keyboards-keys)).

### 6.3 Per-screen policy

```rust
use fairing::screen::OskMode;
use fairing::{nav_item, screen, ChromePolicy, Cx, Shell};

fn add_screens(shell: &mut Shell) {
    // A machine with a hardware keyboard: never raise the OSK on this screen.
    shell.add(
        screen("console", |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
            ui.label("console");
        })
        .osk(OskMode::Off),
    );
    // Manual toggle only. The nav bar's "osk" slot is the toggle.
    shell.add(
        screen("editor", |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
            ui.label("editor");
        })
        .chrome(ChromePolicy {
            osk: OskMode::Manual,
            ..ChromePolicy::default()
        }),
    );
    shell.add(nav_item("osk", |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
        ui.label("⌨");
    }));
}
```

```toml
[nav_bar]
items = ["back", "home", "osk"]
```

The id `osk` is special-cased by the shell: tapping that slot calls `Osk::toggle()`. To change
the policy at runtime, use `cx.set_chrome(policy)`.

### 6.4 How content gets out of the OSK's way

The OSK does not push the content. `Layout.content` is unchanged; instead the covered height is
handed over every frame as `cx.pane.inset_bottom`. It is the real y even mid-animation, so it
rises smoothly from zero to the full height.

```rust
use fairing::widgets::TextField;
use fairing::{layout, screen, Cx, Shell};

fn add_form(shell: &mut Shell) {
    let mut name = String::new();
    shell.add(
        screen("form", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            // `page` shrinks the scroll area by the covered amount — do not subtract it yourself.
            layout::page(ui, cx, "form", |ui, cx| {
                layout::group(ui, cx, |ui, cx| {
                    TextField::new(&mut name).hint("Machine name").show(ui, &mut cx.widgets());
                });
            });
        })
        .title("Form"),
    );
}
```

The OSK sits above the nav bar, or at the bottom of the screen when the nav bar is off. That is
the rule that keeps the back button from being covered by the key bed.

The moment a key is pressed, egui takes focus away from the text field. The shell remembers the
previous pass's focus and restores it after injecting, so there is nothing for the integrator to
do.

### 6.5 Text fields — `widgets::TextField`

Use `egui::TextEdit` as-is and its height comes from the font size, which does not reach a gloved
hand. It also only follows the theme as far as `Theme::egui_style` pushed, and its padding is
egui's default, so it does not line up with the other rows. `TextField` fixes both — the height
is `touch_target` and the text is centred vertically inside it.

| Handle | What it does |
|---|---|
| `hint(impl Into<String>)` | Dimmed guidance shown when empty |
| `multiline(bool)` | Three rows high. Notes, addresses |
| `password(bool)` | Masking |
| `width(f32)` | A fixed width. The default is the remaining width |
| `enabled(bool)` | Locks it |
| `id_salt(&'static str)` | For several fields on one screen |

```rust
# use fairing::widgets::TextField;
# fn body(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
# let mut ssid = String::new();
let r = TextField::new(&mut ssid).hint("SSID").show(ui, &mut cx.widgets());
if r.lost_focus() { /* entry finished */ }
# }
```

It returns an `egui::Response` unchanged, so `changed()` · `lost_focus()` · `has_focus()` all
work. Raising and lowering the OSK is the shell's job, from focus.

---

## 7. Touch widgets

Every widget in `fairing::widgets` draws its pressed state from the first frame. The tint is
immediate, the scale goes `1 → 0.97` over 80 ms and returns over 120 ms on release. egui's own
widgets work too, but then touch targets and press feedback are yours to handle.

| Widget | Construction | `show` returns | When to use it |
|---|---|---|---|
| `BigButton` | `BigButton::new(label)` | `LongPressResponse { response, completed, progress }` | A large button for gloved hands. Dangerous actions get `.long_press(d)` |
| `Switch` | `Switch::new(&mut bool)` | `egui::Response` | On/off that applies immediately |
| `TouchSlider` | `TouchSlider::new(&mut f32, range)` | `egui::Response` | Continuous values like brightness or speed. Tracks the finger 1:1 |
| `ListRow` | `ListRow::new(title)` | `egui::Response` | One row of a settings screen |

All of them draw with `.show(ui, &mut cx.widgets())`. `cx.widgets()` lends them the part of the
screen's `Cx` a control needs — the theme, and the animation values, which live in the shell's
store. That is what keeps press feedback alive in a stateless closure screen.

```rust
use fairing::widgets::{BigButton, ButtonKind, ListRow, Switch, TouchSlider};
use fairing::{icon, screen, Cx, Shell};
use std::time::Duration;

fn add_widget_screen(shell: &mut Shell) {
    let mut heater = false;
    let mut setpoint = 40.0_f32;
    shell.add(
        screen("panel", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            // A dangerous action: a one-second hold completes it (the ring draws the progress).
            let purge = BigButton::new("Purge")
                .icon(icon::POWER)
                .kind(ButtonKind::Danger)
                .long_press(Duration::from_secs(1))
                .show(ui, &mut cx.widgets());
            if purge.completed {
                cx.shell.toast("Purge started");
            }
            ui.horizontal(|ui| {
                ui.label("Heater");
                let _ = Switch::new(&mut heater).show(ui, &mut cx.widgets());
            });
            let _ = TouchSlider::new(&mut setpoint, 0.0..=100.0).show(ui, &mut cx.widgets());
            if ListRow::new("Network")
                .subtitle("Wi-Fi · Ethernet")
                .icon(icon::WIFI)
                .trailing("Connected")
                .show(ui, &mut cx.widgets())
                .clicked()
            {
                cx.open("settings.network");
            }
        })
        .title("Panel"),
    );
}
```

| Builder | `BigButton` | `Switch` | `TouchSlider` | `ListRow` |
|---|---|---|---|---|
| `.enabled(bool)` | ○ | ○ | ○ | ○ |
| `.icon(IconRef)` | ○ | | | ○ |
| `.kind(ButtonKind)` | `Normal` · `Primary` · `Danger` | | | |
| `.min_size(Vec2)` | 120×56 by default | | | |
| `.long_press(Duration)` | ○ | | | |
| `.subtitle(text)` · `.trailing(text)` · `.chevron(bool)` | | | | ○ |

`PinPad` — the digits of a PIN with their dot row — and `PatternPad` — a path through a square
of dots — are what the unlock prompt draws, and yours for a login screen of your own
([05 §5.5](05-access-control.md#55-a-keypad-or-a-pattern-in-a-login-screen-of-your-own)).

---

## 8. Icons

### 8.1 The 78 built-ins

```rust
use fairing::{icon, IconRef};

fn pick() -> IconRef {
    icon::GAUGE
}
```

`fairing::icon` is an alias for `icons::builtin`. The constant names are SCREAMING_SNAKE and the
real name is the file name in `assets/icons/<name>.svg` (so `icon::CHEVRON_UP` = `"chevron-up"`).
The full list of names is in `fairing::icons::builtin::NAMES`, and the count has a single source
of truth in `assert_eq!(NAMES.len(), 78)` in `crates/fairing-widgets/src/icons/builtin.rs`.

To refer to one by string, use `IconRef::Builtin("wifi")`. An unknown name draws nothing and
passes quietly (`IconSet::paint` returns `false`).

There are four `IconRef` variants.

| Variant | Use |
|---|---|
| `Builtin(&'static str)` | The built-in set |
| `Custom(CustomIconId)` | Something registered with `register_icon` or `register_icon_painter` |
| `Texture { id, tint }` | A texture the integrator uploaded. `tint` dyes it the icon colour |
| `Glyph(String)` | A font glyph or emoji (within the installed fonts) |

Size and colour come from `IconStyle`.

```rust
use fairing::icons::{IconColor, IconStyle};
use fairing::{icon, ColorRole, Cx};

fn draw_badge(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    let rect = ui.max_rect();
    let style = IconStyle::sized(24.0)
        .color(IconColor::Role(ColorRole::Primary))
        .enabled(true);
    let painter = ui.painter().clone();
    let _ = cx.icons.paint(&painter, rect, &icon::BELL, &style, cx.theme);
}
```

| `IconStyle` field | Default | Meaning |
|---|---|---|
| `size` | `24.0` | The side of the square (px) |
| `color` | `IconColor::Role(ColorRole::OnSurface)` | A role colour, or a fixed one |
| `stroke` | `None` (= 2.0) | Stroke in 24-grid units. The pixel width is `stroke × size / 24` |
| `enabled` | `true` | `false` means the muted colour at alpha 0.6 |

### 8.2 The six parametric icons

Their state is an argument, so an `IconRef` cannot point at them. Whoever knows the state calls
the function.

| Function | Arguments |
|---|---|
| `wifi(painter, rect, level: u8, off: bool, style)` | `level` 0..=4 |
| `battery(painter, rect, percent: u8, charging: bool, style)` | 20 % or below is the danger colour |
| `volume(painter, rect, level: u8, muted: bool, style)` | `level` 0..=3 |
| `bluetooth(painter, rect, state: BtIconState, style)` | `Off` · `On` · `Connected` |
| `signal(painter, rect, bars: u8, style)` | `bars` 0..=4 |
| `progress_ring(painter, rect, t: f32, style)` | `t` 0..=1, clockwise from 12 o'clock |

`style` is a `ParamStyle`, built with `IconStyle::param_style(theme)` — that is what keeps the
colour and stroke rules the same as the static icons.

```rust
use fairing::icons::parametric;
use fairing::icons::IconStyle;
use fairing::Cx;

fn draw_wifi(ui: &mut egui::Ui, cx: &mut Cx<'_>, level: u8) {
    let rect = ui.max_rect();
    let style = IconStyle::sized(18.0).param_style(cx.theme);
    parametric::wifi(ui.painter(), rect, level, false, &style);
}

/// The 120 ms crossfade on a state change is the caller's.
fn draw_wifi_crossfade(ui: &mut egui::Ui, cx: &mut Cx<'_>, prev: u8, next: u8, u: f32) {
    let rect = ui.max_rect();
    let style = IconStyle::sized(18.0).param_style(cx.theme);
    parametric::wifi(ui.painter(), rect, prev, false, &style.with_alpha(1.0 - u));
    parametric::wifi(ui.painter(), rect, next, false, &style.with_alpha(u));
}
```

Only `battery` uses the whole of a wide `rect`. The rest draw into the largest square inside it.

### 8.3 Registering custom icons

Two ways. Registering a vector path shares the polyline cache; registering a painter lets you
draw whatever you like.

```rust
use fairing::icons::{IconDef, Seg};
use fairing::{IconRef, Shell};

fn register_icons(shell: &mut Shell) -> (IconRef, IconRef) {
    // (1) A vector path. Coordinates are the 24×24 design grid.
    const DIAMOND: IconDef = IconDef {
        name: "app-diamond",
        segs: &[
            Seg::M(12.0, 3.0),
            Seg::L(21.0, 12.0),
            Seg::L(12.0, 21.0),
            Seg::L(3.0, 12.0),
            Seg::Z,
        ],
        fill: false,
    };
    let diamond = IconRef::Custom(shell.register_icon(DIAMOND));

    // (2) An arbitrary drawing callback. The shell resolves the colour from the style for you.
    let ring = IconRef::Custom(shell.register_icon_painter(Box::new(
        |painter, rect, style, color| {
            let radius = rect.width() / 2.0 - 2.0;
            painter.circle_stroke(
                rect.center(),
                radius,
                egui::Stroke::new(style.stroke_px(), color),
            );
            painter.circle_filled(rect.center(), radius * 0.35, color);
        },
    )));

    (diamond, ring)
}
```

| | `register_icon` | `register_icon_painter` |
|---|---|---|
| Argument | `IconDef { name, segs, fill }` | `Box<dyn Fn(&Painter, Rect, &IconStyle, Color32)>` |
| Coordinates | A 24×24 grid, scaled uniformly into the target rect by the painter | The rect the callback receives, as-is |
| Polyline cache | Used | Not used |
| Constraint | Subpaths with `fill: true` must be **convex** | None |

`Seg` is `M` (move) · `L` (line) · `Q` (quadratic Bézier) · `C` (cubic Bézier) · `Z` (close). The
name is only a label: an icon is drawn by the `CustomIconId` `register_icon` returns, and the
polyline cache keys on the path, so a name shared with a built-in or another registration still
draws as itself.

### 8.4 Adding a new SVG to the built-in set

To grow the built-in set itself, put the SVG in `assets/icons/` and run the compiler. This is for
when you use the crate as a fork; if you only need icons for your own machine, §8.3's
`register_icon` is the right tool.

```sh
# 1) Put assets/icons/<name>.svg in place
# 2) Regenerate
cargo xtask icons

# Just check it is up to date (this is the form CI runs)
cargo xtask icons --check
```

| Rule | Detail |
|---|---|
| File name | `[a-z0-9][a-z0-9_-]*` — this becomes the icon's name |
| viewBox | Fit it to a 24×24 grid (the compiler handles the scaling) |
| Elements you may use | `path` (`d`) · `circle` · `ellipse` · `rect` (`rx`/`ry`) · `line` · `polyline` · `polygon`, and the `svg` · `g` that contain them |
| Containers that are ignored | Shapes inside `defs` · `clipPath` · `mask` · `symbol` · `pattern` · `marker` · `metadata` · `title` · `desc` |
| **What is an error** | A `transform` attribute, and elements like `use` · `text` · `image`. Rather than dropping them silently, the compile stops |
| Fill detection | One shape without `fill="none"` is enough to make it `fill: true` |
| Convexity | A concave subpath with `fill: true` is an error — `epaint`'s fan fill draws it wrong |

The output is `crates/fairing-widgets/src/icons/generated.rs`. It is sorted by name and looked up with a
binary search, so it must not be edited by hand.

To grow the `fairing::icon::*` constants alongside it, add the name to the `icons!` table in
`crates/fairing-widgets/src/icons/builtin.rs`. A constant out of step with the generated table is caught
by the `builtin_names_resolve` test.

---

## 9. Gestures

### 9.1 The shell's own

| Edge | Gesture | When |
|---|---|---|
| Top | The shade, pulled down | Always. The status bar's tap and `LaunchAction::OpenOverlay` are the other ways in (§4.1) |
| Bottom | Home, the recent screens, the task before or after | `[nav_bar] style = "gesture"` (§2.6) |
| Left, right | Back | The edges in `[nav_bar] back_edges` — the left one by default (§2) |
| A top corner, held 2 s | The emergency gesture: the shade, or the prompt for `chrome.emergency` | Always — with `edge_guard` and with gestures off too ([05](05-access-control.md#one-entry-point-already-exists)) |

`[gesture] enabled = false` turns them all off but the emergency gesture, and a screen's
`ChromePolicy::edge_guard` turns them off over that screen. A gesture handle (§9.2) takes the
strip it sits on: there, back starts just inward of it. A gesture region of your own (§9.3) takes
every press that begins in it, wherever you put it.

### 9.2 Gesture handles

Thin strips at the edge of the glass, swiped the way the handles of Samsung's One Hand Operation+
are. A handle answers six gestures: straight in and diagonally either way, each let go at once
(short) or after a rest (long). Each runs a `LaunchAction`, the same actions a desktop icon or a
hidden entry runs. Handles go on the left and the right edges, and on the bottom one where the
nav bar is off.

```rust
# fn add(shell: &mut fairing::Shell) -> fairing::Result<()> {
use fairing::gesture::{
    Edge, GestureHandle, HandleAction, HandleDirection, HandleEnd, HandleGesture,
};
use fairing::LaunchAction;

shell.add_gesture_handle(
    GestureHandle::new("right", Edge::Right)
        // Measured from under the status bar down to the glass's bottom, over the nav bar...
        .ends(HandleEnd::EdgeZone, HandleEnd::Glass)
        // ...its lower two thirds.
        .along(0.33, 1.0)
        // Straight in: the batch list.
        .gesture(HandleGesture::short(HandleDirection::Straight), LaunchAction::open("batches"))
        // In and up: the recent screens.
        .gesture(HandleGesture::short(HandleDirection::DiagonalUp), LaunchAction::OpenOverview)
        // Straight in, then a rest: the service menu, behind its gate.
        .gesture(
            HandleGesture::long(HandleDirection::Straight),
            HandleAction::new(LaunchAction::open("service_menu")).gate("service"),
        ),
)?;
// With the nav bar off (`[nav_bar] enabled = false`), the bottom edge is the handles'.
shell.add_gesture_handle(
    GestureHandle::new("bottom", Edge::Bottom)
        // The middle third of the width.
        .along(0.33, 0.67)
        // Straight up: the batch list. Up and to the left: the recent screens.
        .gesture(HandleGesture::short(HandleDirection::Straight), LaunchAction::open("batches"))
        .gesture(HandleGesture::short(HandleDirection::DiagonalLeft), LaunchAction::OpenOverview),
)?;
# Ok(())
# }
```

| Setting | Default | What it is |
|---|---|---|
| `along(from, to)` | `0.0, 1.0` | Its stretch of the edge, as shares of the edge between its two ends: 0 at the top on the sides, 0 at the left on the bottom |
| `ends(from, to)` | `EdgeZone, EdgeZone` | Where the edge it is measured over ends: the top and the bottom on the sides, the left and the right on the bottom. `HandleEnd::EdgeZone` is the shell's band there, the bar where it shows and `edge_px` deep where it does not; `Bar` is the bar, or the glass's edge where it is hidden; `Glass` is the glass's edge, over a bar |
| `thickness_mm` | 2 | How thick the strip is |
| `reach_mm` | 10 | How far a swipe runs before it counts |
| `diagonal_from` | 25° | Where diagonal begins, off straight in. Below 70° |
| `long_after` | 400 ms | How long a rest past the reach makes the long gesture |
| `visible` | `false` | Whether the strip shows at rest. Shown or not, it takes the presses in it |

- **The strip takes every press that begins in it**, the way a One Hand Operation+ handle takes
  its edge. Nothing under it sees one: not a screen's list or button, not a desktop page, and not
  the shell's own edge gesture. A stroke that runs in from the edge is the handle's swipe; a tap,
  or a stroke along the edge, does nothing. Just inward of the strip everything is as it would
  have been, so a handle on the left edge leaves back the rest of that edge. This is why the
  strips are thin, and why they fit the keyboard.
- **The angle picks the way.** Within `diagonal_from` of straight in, it is straight. Past it, it
  is diagonal: up or down on the sides (`DiagonalUp`, `DiagonalDown`), left or right on the
  bottom (`DiagonalLeft`, `DiagonalRight`). Past 70°, it runs along the edge and is no handle's.
  The way is decided once the swipe passes the slop: a stroke that starts along the edge and then
  turns in stays nobody's. `HandleDirection::on(edge)` gives an edge's three ways, and
  `add_gesture_handle` refuses a way foreign to the handle's edge.
- **Let go, it is the short gesture**, once the swipe has run `reach_mm`. Short of that, nothing
  happens. A slow swipe that never stops is short too.
- **Held still past the reach, it is the long gesture**, and it runs there and then. The rest of
  that swipe reaches nothing. Where the handle has no long gesture that way, the rest changes
  nothing and the release runs the short one.
- **Left, right and bottom edges, three an edge, their stretches apart.** One Hand Operation+ has
  the sides; the bottom is fairing's own. The top edge is the shade's, so `add_gesture_handle`
  refuses it. A handle with an id already there replaces the old one, and an id one of your
  gesture regions has (§9.3) is refused.
- **The bottom edge is the nav bar's or the handles', not both.** With the nav bar on, buttons
  or gestures, `add_gesture_handle` refuses a bottom handle; turn the bar off
  (`[nav_bar] enabled = false`) to put handles there. Their strip is then the glass's last
  millimetres, like the side ones, and turned on again at run time, the nav bar has the edge back
  while a bottom handle stands aside.
- **Where a strip ends is the handle's choice** (`ends`). By default it keeps off the other edges'
  bands, so the shade's pull, the home swipe and back keep their corners; `HandleEnd::Glass` runs
  it to the glass's very end instead, over a bar where one shows.
- **It fits the keyboard.** While the on-screen keyboard is up, the side strips end above the keys
  and `along` is measured over what is left, and a bottom strip stands aside: the keys lie where
  it was. They keep off the keys while it slides, too: from the moment it starts coming up they
  end where the keys will stop, and on its way down they end above the keys still showing. Two
  limits remain, both because egui senses a strip's guard where it was a frame before. For the
  first two frames after the keyboard starts to appear, about 33 ms at 60 Hz, a press on the outer
  2 mm of a key that reaches a side strip, such as `⌫` or the numeric keypad's right column, can
  still go to the handle. On the very frame the keyboard comes up again after the first time,
  about 17 ms, a press on the lower edge of the space bar, where a bottom strip lies, can too. A
  strip shown with `visible(true)` is likewise drawn over the keys for the keyboard's first frame.
- **Over a screen or the desktop only.** Not over the shade, the unlock prompt or the recent
  screens, not on a screen with `edge_guard`, and not with `[gesture] enabled = false`. There the
  strip is not there at all, and a press at the edge reaches the screen: a screen that needs its
  very edge sets `edge_guard`.
- **It never goes by quietly.** `ShellEvent::Gesture { handle, gesture }` comes out when one
  completes, gate or no gate. With a gate the session does not pass, `AccessEvent::UnlockRequested`
  follows, the unlock prompt comes up, and the action runs once the gate is passed.
- **Nothing follows the finger but an arrow** that says which gesture the swipe is. The built-in
  one is a disc ahead of the finger, lit in the accent once the swipe counts and runs something,
  ringed while a rest makes it long. `ShellBuilder::gesture_handle_painter` draws it your way
  ([04](04-customization.md)): a `HandleLook` carries the strip, and a `HandleSwipe` while it is
  swiped, with the gesture it would be, its reach and rest, and whether it has run.
- **Give every gesture another way in too**, such as an icon, a nav bar item or a button, as every
  gesture of the shell's has.

`remove_gesture_handle(id)` takes one off; `gesture_handles()` lists them. A handle is a gesture
region the shell places itself (§9.3).

### 9.3 Gesture regions of your own

A gesture region is a stretch of the glass that takes every touch beginning in it, and hears that
touch from its press to its release. You write it: a `GestureRegion` says where it is each frame,
follows its touches and draws itself. A part of a screen used as a trackpad is one; a jog dial or a
strip for a gesture of your own is another. The gesture handles of §9.2 are the shell's own regions,
built on the same trait.

```rust
# fn add(shell: &mut fairing::Shell) -> fairing::Result<()> {
use fairing::gesture::{GestureRegion, Phase, RegionCx, RegionPlaceCx, RegionTouch};

/// The app's pointer: the trackpad moves it, and the screen draws it.
#[derive(Default)]
struct Pointer {
    at: egui::Pos2,
    clicks: u32,
}

/// The lower right of the `remote` screen, four centimetres a side.
struct Trackpad;

impl GestureRegion for Trackpad {
    fn place(&mut self, cx: &RegionPlaceCx<'_>) -> Option<egui::Rect> {
        // Only over the screen it serves, and never over the keyboard.
        if cx.focused != Some("remote") || cx.keys.is_some() {
            return None;
        }
        let side = egui::Vec2::splat(cx.scale.mm_to_du(40.0));
        Some(egui::Rect::from_min_size(cx.pane.max - side, side))
    }

    fn touch(&mut self, touch: &RegionTouch, cx: &mut RegionCx<'_>) {
        let Some(pointer) = cx.app_mut::<Pointer>() else {
            return;
        };
        match touch.phase {
            // The pointer goes twice as far as the finger.
            Phase::Moved => pointer.at += touch.delta * 2.0,
            // Let go before it went anywhere, it is a click.
            Phase::Ended if !touch.moved => pointer.clicks += 1,
            _ => {}
        }
    }
}

shell.add_gesture_region("trackpad", Trackpad)?;
# Ok(())
# }
```

The pointer lives in the state you lend the shell with `Shell::frame_with`, so the `remote`
screen reads the same `Pointer` through `cx.app::<Pointer>()` and draws it.

- **`place` says where it is**, every frame the regions are in play, or `None` to stand aside. A
  `RegionPlaceCx` carries the glass, the content, the focused screen's id and pane, the
  keyboard's band while it is up (`keys`), the edges the screen keeps from edge gestures
  (`blocked`, all four with `edge_guard`), the shell's own edge bands (`edge_zone(edge)`), and the
  scale for millimetres. A region is yours, so the shell places it wherever `place` says, on a
  screen with `edge_guard` too: a region that should stand aside there checks `blocked`. The
  rect is clipped to the glass, so `Rect::EVERYTHING` is the whole of it.
- **The touch is the region's from the press to the release.** Nothing under the region sees it,
  not a widget and not a desktop page, and no gesture of the shell's starts from it.
  `touch` hears it every frame as a `RegionTouch`: `Started` on the press, `Moved` on each frame
  after (a finger at rest included, as frames keep coming while a finger is down), `Ended` on the
  release, and `Cancelled` when the touch is taken: the shade, the unlock prompt or the recent
  screens coming over the regions, the pointer lost, gestures turned off. A region taken off or
  replaced mid-touch hears no more of it, and neither does its replacement. It carries where the
  touch came down and where the finger is, the movement since the last frame, the velocity, how
  long it has been held, and whether it has gone past the slop (`moved`): a touch let go before
  it moved is a tap. A press let go within one frame is heard whole, `Started` then `Ended`.
- **A region runs the shell's actions** with `cx.launch(action)`, and `cx.launch_gated(action,
  gate)` puts a gate in front: a session that does not pass it gets the unlock prompt. The
  actions run once the region has heard the touch. `cx.app::<T>()` and `cx.app_mut::<T>()`
  reach the app's state, and `cx.ctx()` the egui context, for what else the input holds, such
  as a second finger.
- **Where regions overlap, the one added later is on top** and takes the press. That holds for
  handles too: a region added after a handle takes the presses in the strip it covers.
- **The emergency gesture works through every region**: two seconds still in a top corner. So do
  the hidden entries' corner knocks ([05](05-access-control.md#hidden-entry-points--the-service-menu)),
  which are read from the raw pointer: a tap in a region over a knock corner still counts as a
  knock, so keep a trackpad out of the corners your hidden entries use.
- **Over a screen or the desktop only.** Not over the shade, the unlock prompt or the recent
  screens, and not at all with `[gesture] enabled = false`. There `place` is not asked and a press
  reaches what is under the region.
- **`paint` draws it**, every frame it is placed, on a layer over the screens and the shade.
  A `RegionLook` carries its rect, the touch in it while there is one, the shell's time, the
  scale, the theme and the icons. Nothing is drawn by default.
- **A frame behind, like the handles.** A region is placed from last frame's layout, and egui
  finds what a press hits from where things were the frame before, so a region that moves, grows
  or first appears is a frame or two behind the glass ([roadmap §4](../roadmap.md#4-known-limits)).

Eight regions at most (`MAX_GESTURE_REGIONS`). The ids of the handles and the regions are one
namespace, and a region with an id already there replaces the old one in its place.
`remove_gesture_region(id)` takes one off; `gesture_regions()` lists them.

---

## Related

| Topic | Page |
|---|---|
| Declaring screens · `ChromePolicy` · `Cx` | [02 Screens](02-screens.md) |
| Painter hooks · theming · turning parts off · tuning motion | [04 Customization](04-customization.md) |
| Hidden entry points — knocks, not gestures | [05 Hidden entry points](05-access-control.md#hidden-entry-points--the-service-menu) |
| Gate names · the level table · lock presentation | [05 Access control](05-access-control.md) |
| Backend capability reporting · setting values | [06 Services](06-services.md) |
| The full config key tables | [07 Config reference](07-config-reference.md) |
| How the parts fit together · what comes next | [Architecture](../architecture.md) · [Roadmap](../roadmap.md) |
