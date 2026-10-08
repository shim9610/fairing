# 02. Screens

Everything about building a screen: declaration → builder → state → `Cx` → lifecycle → stack navigation → per-screen chrome.

A screen is not an app and not a process. It is one `FnMut(&mut egui::Ui, &mut Cx)`, and the only thing asked of it is that it fills the rect the shell hands it. Inside, it is plain egui.

## 1. Five kinds of declaration

All of them go in with `shell.add(..)` and come out with `shell.remove(id)`.

> The samples in this section and in §3 · §6 call `ui.heading` · `ui.button` directly — they
> exist to show the **declaration mechanism**, so the inside of each screen is kept to nothing.
> Real screens are not written that way. Cards, grids, touch targets and scrolling are
> [§8 `layout`](#8-building-the-inside-of-a-screen-with-layout)'s job.

| Declaration | Signature | Where it shows up |
|---|---|---|
| `screen(id, ui)` | `screen(impl Into<String>, impl FnMut(&mut Ui, &mut Cx) + 'static) -> ScreenDecl` | A screen (resident) + desktop/dock |
| `screen_with(id, factory)` | `screen_with<S: Screen + 'static>(impl Into<String>, impl FnMut() -> S + 'static) -> ScreenDecl` | A screen (factory) + desktop/dock |
| `action(id, run)` | `action(impl Into<String>, impl FnMut(&mut Cx) + 'static) -> ActionDecl` | An icon with no screen. Tapping it runs `run` |
| `status_item(id, slot, ui)` | `status_item(impl Into<String>, Slot, impl FnMut(&mut Ui, &mut Cx) + 'static) -> StatusItemDecl` | The status bar (`Slot::Left` · `Center` · `Right`) |
| `nav_item(id, ui)` | `nav_item(impl Into<String>, impl FnMut(&mut Ui, &mut Cx) + 'static) -> NavItemDecl` | The nav bar, at the position that id holds in `[nav_bar] items` |
| `tile(id, kind)` | `tile(impl Into<String>, TileKind) -> TileDecl` (feature `overlay`) | A quick-settings tile in the shade |

Six constructors, five kinds — `screen` and `screen_with` produce the same kind of thing (§3 is
about which one to reach for).

Status items, nav items and tiles are covered properly in [03 Chrome](03-chrome.md). Here the
point is only that they use the same `add` / `remove`.

```rust
use fairing::{
    action, icon, nav_item, screen, screen_with, status_item, Cx, Screen, Services, Shell,
    ShellConfig, Slot,
};

/// The state a factory screen owns.
#[derive(Default)]
struct Counter {
    clicks: u32,
}

impl Screen for Counter {
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        ui.heading("Counter");
        ui.label(format!("clicks: {}", self.clicks));
        if ui.button("+1").clicked() {
            self.clicks += 1;
        }
        if ui.button("close").clicked() {
            cx.finish();
        }
    }
}

fn main() -> fairing::Result<()> {
    let ctx = egui::Context::default();
    let mut shell = Shell::new(ShellConfig::default(), Services::null(), &ctx)?;

    // 1) Resident screen. One closure *is* the screen.
    shell.add(
        screen("dashboard", |ui: &mut egui::Ui, cx: &mut Cx| {
            ui.heading("Dashboard");
            if ui.button("counter").clicked() {
                cx.open("counter");
            }
        })
        .title("Dashboard")
        .icon(icon::GAUGE)
        .desktop(),
    );

    // 2) Factory screen. Every open runs the factory and gets a fresh instance.
    shell.add(
        screen_with("counter", Counter::default)
            .title("Counter")
            .icon(icon::ACTIVITY)
            .desktop(),
    );

    // 3) Action. An icon with no screen behind it; tapping runs the closure.
    shell.add(
        action("scan-wifi", |cx: &mut Cx| {
            let _ = cx.services.wifi.scan();
        })
        .title("Scan")
        .icon(icon::WIFI)
        .desktop(),
    );

    // 4) Status bar item.
    shell.add(status_item("temp", Slot::Right, |ui: &mut egui::Ui, _cx: &mut Cx| {
        ui.label("36.5°");
    }));

    // 5) Nav bar item. It only gets a slot if "kbd" is listed in `[nav_bar] items`.
    shell.add(nav_item("kbd", |ui: &mut egui::Ui, _cx: &mut Cx| {
        let _ = ui.button("kbd");
    }));

    // 6) Quick-settings tile. `tile` and `TileKind` only exist when fairing's `overlay`
    //    feature is on (it is by default). To support builds with it off, gate this
    //    behind a feature of your own crate.
    shell.add(
        fairing::tile("heater", fairing::TileKind::Toggle("app.heater".into()))
            .label("Heater")
            .icon(icon::FAN),
    );

    // Removal is one id, whatever the kind. Built-in status items use the same id space.
    let removed = shell.remove("status.bluetooth");
    assert!(removed);
    Ok(())
}
```

### The add / remove contract

| Call | What happens |
|---|---|
| `shell.add(decl)` | Takes effect from the next frame. Legal at any time (runtime registration) |
| `add` again with the same id | Replaces it. Open instances close, and each one emits `ShellEvent::ScreenClosed` |
| `shell.remove(id) -> bool` | Drops the declaration, its icon and its item, and force-closes open instances. `true` if it was there, plus `ShellEvent::DeclRemoved(id)` |

`remove` works on built-in status items too (`status.clock` · `status.wifi` ·
`status.bluetooth` · `status.battery`). Built-in tiles (`tile.wifi` and friends) are different:
they do not come from a registry declaration but from the `[overlay] tiles` list in config, so
`remove` does not take them out — edit that list instead
([07 Config reference](07-config-reference.md)).

## 2. The `ScreenDecl` builder

`screen` and `screen_with` both return a `ScreenDecl`. Every method below is optional and the
order does not matter.

| Method | Default | What it does |
|---|---|---|
| `.title(impl Into<String>)` | the id | Icon label and recents title |
| `.icon(IconRef)` | none | The icon. Without one the screen does not appear on the desktop or the dock |
| `.description(impl Into<String>)` | none | What the icon's info popover says under the title — holding the icon brings it up ([03 §3.8](03-chrome.md#38-holding-an-icon-the-info-popover)). A key, like the title |
| `.desktop()` | not placed | Next free desktop cell. Does not override a position already fixed with `.desktop_at` |
| `.desktop_at(page: u8, col: u8, row: u8)` | not placed | Pins a desktop cell. If it is taken or out of the grid, the shell warns and auto-places |
| `.dock()` | not placed | Dock icon. Given together with `.desktop()`, only the dock wins |
| `.gate(impl Into<Gate>)` | the id | The gate that guards this screen |
| `.visibility(Visibility)` | `Visibility::Locked` | How the icon reads when the gate is closed. `Locked` (dimmed + padlock) or `Hidden` (not drawn) |
| `.launch(LaunchMode)` | `Single` | `Multi` is factory-only. On a resident screen it warns and is ignored |
| `.split(SplitSupport)` | `Yes` | How small a pane it can share the content in ([§6.1](#61-two-panes)). `Yes`: no smaller than a quarter of the content or three touch targets, whichever is larger · `MinSize(Vec2)`: no smaller than that, along the split · `No`: never in a split |
| `.fullscreen()` | none | Overwrites `chrome` with `ChromePolicy::fullscreen()` (both bars hidden, inset 0) |
| `.chrome(ChromePolicy)` | `ChromePolicy::default()` | Sets the whole policy. Overwrites an earlier `.fullscreen()` too |
| `.osk(OskMode)` | `Auto` | Changes `chrome.osk` only. `Auto` · `Manual` · `Off` |
| `.keep_awake()` | off | Stops the display idle timer. Also sets `chrome.keep_awake` |
| `.evict_after(Duration)` | off | Drops the instance once it has been `Stopped` for that long. Factory-only; on a resident screen it warns and is ignored |
| `.background(ColorRole)` | none (theme surface) | Pane background colour. Also carried in `chrome.background` |

There is a read side as well: `id()` · `label()` · `icon_ref()` · `gate_name()` (the id if none)
· `chrome_policy()` · `launch_mode()` · `is_resident()`.

`action(..)` builds with `.title` · `.icon` · `.desktop` · `.desktop_at` · `.dock` · `.gate` ·
`.visibility`, meaning the same things. Item declarations are shorter: `status_item` takes
`.gate` · `.priority(i8)` (collapse order when the bar runs out of width — lower collapses
first) · `.enabled(bool)`; `nav_item` takes `.gate` · `.enabled(bool)`; `tile` takes `.icon` ·
`.label` · `.gate` · `.enabled(bool)`.

Gates and `Visibility` are [05 Access control](05-access-control.md).

## 3. Resident and factory screens

`screen` is resident, `screen_with` is a factory, and the single difference is where the state
lives.

There are three constructors, and the table's two columns are **residency**, not closure-vs-type:
`screen(id, closure)` and `screen_of(id, value)` are both resident, `screen_with(id, factory)` is
the factory. Reach for `screen_of` when your screen is a `struct` — that is what `on_back`,
`on_lifecycle` and `on_result` need, and a resident one keeps its state and can be embedded with
`cx.draw_screen`.

| | Resident `screen(id, closure)` · `screen_of(id, value)` | Factory `screen_with(id, factory)` |
|---|---|---|
| Instances | Always one. The declaration is the instance | A new one per open |
| State lives in | What the closure captured (owned by the declaration) | The struct the factory built (owned by the instance) |
| State lifetime | `add` … `remove`. Survives closing | `Created` … `Destroyed`. Dropped when closed |
| `LaunchMode` | Always `Single` | `Single` by default, `Multi` allowed |
| `.evict_after` | Ignored (warns) | Works |
| Initialisation | `screen_of`: build the value before you register it. `screen`: by hand inside the closure, on `cx.event == Some(Lifecycle::Created)` | The factory builds a fresh value every time |
| `on_back` / `on_lifecycle` / `on_result` | `screen_of` only (a closure is `ui` and nothing else) | Yes |
| Embeddable with `cx.draw_screen` | Yes | **No** — it owns no screen between opens |
| Fits | Dashboards, settings — one of them, and the state should survive | Item details, wizards — start clean each time, reclaim on close |

A resident screen cannot be `Multi` because there is only one instance: pushing it twice would
draw the same state in two places.

### 3.1 Drawing one screen inside another

`cx.draw_screen(ui, id)` draws whatever declaration is registered under `id`, right where you call
it. A master-detail layout is that one line: the list on the left, and on the right the screen the
selected row points at. The built-in `settings.home` is written this way, which is why replacing
`settings.wifi` under the same id changes what the wide-screen right column shows.

```rust
# use fairing::Cx;
fn detail(ui: &mut egui::Ui, cx: &mut Cx<'_>, selected: &str) {
    if !cx.draw_screen(ui, selected) {
        ui.label("nothing to show here");
    }
}
```

It **draws**; it does not open. There is no instance behind it, so the embedded screen gets no
lifecycle events, no chrome of its own, no back handling and no place on the stack. Use
`cx.open(id)` when you want a real screen.

The loan lasts one call, not one frame, so a screen that is open on the stack **and** embedded
somewhere else is drawn twice in that frame. There is only one of it, so both show the same state —
but its widget ids are then used twice and egui will say so. Embed screens that are not also open.

The guest does get a `Cx` of its own wherever the host's would be a lie:

| | The guest sees |
|---|---|
| `cx.pane.rect` | The space it was really given (the `ui` you passed), not the host's pane. A scroll built on the host's height would run off the bottom of the column |
| `cx.pane.instance` | `InstanceId::NONE`. It has no instance — without this a guest calling `cx.finish()` would close **the host** |
| `cx.event` | `None`. The host's lifecycle is the host's |
| `cx.pane.inset_bottom` | However much of the on-screen keyboard reaches into the guest's own rect |

It answers `false`, and draws nothing, when:

- nothing is registered under `id`, or what is is not a screen;
- **the screen's own gate does not pass for this session** — a host that lists entries would
  otherwise be a way round access control, so the host cannot wave a guest past its gate;
- the declaration is a **factory** one — it owns no screen between opens, so there is nothing to
  lend. `open` those instead;
- the screen is already out on loan — it is the one drawing, or one further up the nesting. A
  screen that asks for itself is told no rather than recursing;
- the `Cx` is not a screen's (an action, a status item, a quick-settings tile body).

`cx.can_draw_screen(id)` answers exactly those conditions **before** you draw the row, which is how
a list decides whether a row embeds or opens.

```rust
use fairing::{screen, screen_with, Cx, LaunchMode, Screen, Services, Shell, ShellConfig};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// Factory: starts from zero every time it opens.
struct Wizard {
    step: u8,
}

impl Screen for Wizard {
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        ui.heading(format!("step {}", self.step));
        if ui.button("next").clicked() {
            self.step += 1;
        }
        if self.step >= 3 {
            cx.finish();
        }
    }
}

fn main() -> fairing::Result<()> {
    let ctx = egui::Context::default();
    let mut shell = Shell::new(ShellConfig::default(), Services::null(), &ctx)?;

    // Resident: the counter lives in the closure capture, so it survives close and reopen.
    let opens = Rc::new(Cell::new(0u32));
    let opens_in = Rc::clone(&opens);
    shell.add(screen("dashboard", move |ui: &mut egui::Ui, cx: &mut Cx| {
        if cx.event == Some(fairing::Lifecycle::Created) {
            opens_in.set(opens_in.get() + 1); // a resident screen "initialises" right here
        }
        ui.label(format!("opened {} times", opens_in.get()));
    }));

    // Factory: a new Wizard per open, dropped on close.
    shell.add(
        screen_with("wizard", || Wizard { step: 0 })
            .launch(LaunchMode::Multi)
            .evict_after(Duration::from_secs(30)),
    );

    let _ = opens.get();
    Ok(())
}
```

## 4. The `Cx` handle

`Cx` is how a screen asks the shell for something. A closure may ignore `cx` entirely, and most
screens only ever touch `ui`.

### Fields

| Field | Type | Use |
|---|---|---|
| `shell` | `&ShellHandle` | Command handle. `Clone + Send`, so it can go to a thread |
| `session` | `&Session` | Current subject and level (read-only) |
| `services` | `&mut Services` | Backend snapshots and commands |
| `settings` | `&SettingsView` | Reading settings |
| `strings` | `&Strings` | The string table: `cx.strings.get("Pump pressure")` is that text in the active language ([04 §9](04-customization.md#9-text-and-translations)) |
| `theme` | `&Theme` | Palette, metrics, motion tokens. `cx.theme.color(ColorRole::Muted)` |
| `icons` | `&mut IconSet` | Drawing built-in and custom icons |
| `pane` | `PaneInfo` | `rect` · `is_split` · `is_focused` · `inset_bottom` · `instance` |
| `now` | `Instant` | The shell's monotonic time for this frame |
| `event` | `Option<Lifecycle>` | One lifecycle event that arrived since the last `ui` |

`PaneInfo::rect` is the rect after the shell applied its insets — the same thing as
`ui.max_rect()`. `inset_bottom` is how much height the OSK is covering, which is what a scroll
area should stay clear of.

### Methods

| Method | What it does |
|---|---|
| `open(id: &str)` | Push onto the same pane. The shell checks the gate |
| `open_in_other_pane(id: &str)` | Open in the other pane ([§6.1](#61-two-panes)): with one pane, a split — yours keeps its side and `id` slides in beside it; split already, a push onto the other pane. The focus goes with it. Where no split can hold, it opens in your pane |
| `finish()` | Pop yourself |
| `finish_with(v: ScreenValue)` | Pop, then the parent's `on_result` |
| `set_chrome(p: ChromePolicy)` | Change chrome at runtime |
| `set_setting(key, value)` | Write a setting (`fairing::settings::keys` + `SettingValue`) |
| `launch(a: LaunchAction)` | Any launch action. `Open` · `Run` · `Set` · `Toggle` · `OpenOverlay` … |
| `request_power(r: PowerRequest)` | Ask for shutdown/reboot/suspend. The integrator confirms it with `Shell::commit_power` |
| `knock(id: &str)` | One knock on a hidden entry point ([05 §9](05-access-control.md)) |
| `knock_remaining(id: &str) -> Option<u8>` | Knocks left, for drawing "3 more", once they are down to the entry's `hint_from`. `None` before that, without a `hint_from`, for an unknown id or an uncountable trigger |
| `allows(gate) -> bool` | For hiding a section inside a screen. Delegates the gate decision |
| `has_screen(id: &str) -> bool` | Whether a screen is declared under that id. Registration only |
| `screen_allowed(id: &str) -> bool` | Registered **and** its own gate passes. This is the one to filter a list of entries on — a declaration's gate defaults to its id but `.gate(..)` can name another, so `allows(id)` is not the same question |
| `can_draw_screen(id: &str) -> bool` | Whether `draw_screen` would draw it: `screen_allowed`, resident, and not already out on loan |
| `draw_screen(ui, id: &str) -> bool` | **Draw another declaration's screen here, inside this one.** See §3.1 |
| `animate(id: egui::Id, target: f32, tween: Tween) -> f32` | An animated value for stateless closures. `id` only has to be unique within the instance |
| `levels() -> &LevelTable` | The access level table (name, colour, order), for a screen that wants to draw "you are here" itself |
| `theme` · `pane` · `now` | These are fields — see the table above |

`open` · `finish` · `finish_with` · `set_chrome` · `set_setting` · `launch` · `request_power` ·
`knock` do not run where you call them. They queue in order and the shell drains the queue late
in the frame, so the stack never changes mid-draw and calling one twice in a frame still lands
in order.

`cx.shell` is the handle that does the same things from outside a frame: `launch` · `back` ·
`home` · `close_screen` · `notify` · `toast` · `dismiss_notification` · `set_setting` ·
`set_badge` · `set_subject` · `logout` · `request_unlock` · `toggle_overlay` · `set_motion` ·
`waker()`. Those ride an mpsc channel and the shell drains it early in the next frame.

## 5. Lifecycle

| Event | When | What to do |
|---|---|---|
| `Created` | Right after the instance exists, before the first `ui` | Kick off the initial data request |
| `Resumed` | Visible and focused. On top after a transition settles, on task return, when the shade closes | Resume polling and animation |
| `Paused` | Visible but not focused. **The moment the shade leaves its closed state** (i.e. the first pull — the frame where `Overlay::is_closed()` turns `false`, `overlay/mod.rs:15`), or when another screen starts covering this one | Stop input timers |
| `Stopped` | Not visible. Went home, or another screen fully covers it | Stop expensive refreshes |
| `AccessChanged` | The session subject or level changed. Screens that lost their gate close first; the rest get this | Re-render anything sensitive |
| `Resized(Vec2)` | Pane size changed: the content moved (a bar, the window), a split came or went (once, as its slide ends), or the divider was let go. **Not** every frame of a divider drag — `cx.pane.rect` is live and immediate mode needs nothing more | Invalidate layout caches |
| `Destroyed` | The last notification. The drop follows immediately | Release resources |

Four rules:

1. Consecutive duplicates collapse. `Paused, Paused` becomes one; `Paused, Resumed, Paused`
   stays three.
2. Nothing is queued after `Destroyed`.
3. The same event travels two paths: `on_lifecycle` for trait implementations, `cx.event` for
   closures.
4. Only the latest size counts: a `Resized` not yet delivered takes the newer size in its place,
   so a screen in the background hears one `Resized` however often the content changed size.

A closure gets one per `ui` call. They pile up while it is not being drawn and come out in order
once it is.

A screen that comes to the top while the shade, the unlock prompt or the recent screens cover it
(the one above it finished on its own) is `Paused`, not `Resumed` — `Resumed` follows when they go.

```rust
use fairing::{screen, screen_with, Cx, Lifecycle, Screen, Services, Shell, ShellConfig};
use std::cell::Cell;
use std::rc::Rc;

/// A trait implementation receives them through `on_lifecycle`.
#[derive(Default)]
struct Sensor {
    polling: bool,
}

impl Screen for Sensor {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label(if self.polling { "polling" } else { "idle" });
    }

    fn on_lifecycle(&mut self, ev: Lifecycle, _cx: &mut Cx<'_>) {
        match ev {
            Lifecycle::Resumed => self.polling = true,
            Lifecycle::Paused | Lifecycle::Stopped | Lifecycle::Destroyed => self.polling = false,
            _ => {}
        }
    }
}

fn main() -> fairing::Result<()> {
    let ctx = egui::Context::default();
    let mut shell = Shell::new(ShellConfig::default(), Services::null(), &ctx)?;

    // A closure gets the same events through `cx.event`, one per call.
    let resumes = Rc::new(Cell::new(0u32));
    let r = Rc::clone(&resumes);
    shell.add(screen("log", move |ui: &mut egui::Ui, cx: &mut Cx| {
        if cx.event == Some(Lifecycle::Resumed) {
            r.set(r.get() + 1);
        }
        ui.label(format!("resumed {} times", r.get()));
    }));

    shell.add(screen_with("sensor", Sensor::default));
    Ok(())
}
```

A factory screen with `.evict_after(d)` gets `Destroyed` and is dropped once it has been
`Stopped` for longer than `d`. The icon stays, so the next tap runs the factory again.

## 6. Stack navigation

One pane holds one task (`Vec<Instance>`) and the top of it is what you see. Home is not a
screen — it is the workspace's `Home` view.

### Opening

`LaunchMode::Single` (the default) reuses a live instance.

| Situation | Result |
|---|---|
| Already in the current task | Everything stacked above it is cleared (clear-top) and it comes back up. Each cleared instance emits `ScreenClosed` |
| In another task | That task comes to the front |
| Not open | A new instance. From home it becomes the root of a new task; otherwise it is pushed onto the current one |

`LaunchMode::Multi` pushes every time, and only factory screens may use it.

### Going back

`shell.back()` — where the nav bar's back button and the `ShellHandle::back` command both land —
has a fixed priority:

1. If the unlock prompt is up, back cancels it and stops; on the lock screen it does nothing at
   all ([05 §5](05-access-control.md#5-the-shells-prompt-and-the-authenticator)).
2. If the shade is open, close the shade and stop.
3. If the OSK is up, close the OSK and stop.
4. At home, do nothing.
5. Call the top screen's `on_back`. `BackAction::Consumed` ends it (for a screen with navigation
   of its own).
6. On `BackAction::Pop`, pop. Popping the root ends the task and goes home (`WentHome`).

**Edge gestures do not take this path today.** An edge swipe from `[nav_bar] back_edges` is
handled separately by `Shell::gesture_back`, which pops straight through
`Workspace::release_gesture_back` — it does
**not** ask `on_back`. So a screen with internal navigation that returns `BackAction::Consumed`
still closes when a finger swipes the edge. Today the only defence is `[nav_bar] back_edges = []`
on such a screen, which turns gesture-back off.

`shell.home()` closes the shade with it and goes to the `Home` view. The task stays alive and the
whole of it goes `Stopped`, so tapping the icon again brings the stack back as it was.

### Returning a value

When a child calls `cx.finish_with(v)`, the shell pops the child and then calls the parent's
`on_result(from, v, cx)`. `from` is the child's declaration id.

`ScreenValue` is `Bool` · `Int` · `Float` · `Text` · `Bytes` · `None`. Anything richer is shared
through an `Rc<RefCell<_>>` capture ([§9.1](#91-sharing-app-state-across-screens)).

A closure screen cannot receive a result. The `Screen` implementation that wraps a closure fills
in `ui` only, and leaves `on_result` at its default (which does nothing). To receive one, write
the parent as a trait implementation.

```rust
use fairing::{screen, screen_with, Cx, Screen, ScreenValue, Services, Shell, ShellConfig};

/// Parent: receives the SSID the child picked.
#[derive(Default)]
struct WifiSettings {
    ssid: Option<String>,
}

impl Screen for WifiSettings {
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        ui.heading("Wi-Fi");
        ui.label(self.ssid.as_deref().unwrap_or("(none selected)"));
        if ui.button("Pick a network").clicked() {
            cx.open("wifi.pick");
        }
    }

    fn on_result(&mut self, from: &str, v: ScreenValue, _cx: &mut Cx<'_>) {
        if from == "wifi.pick" {
            if let ScreenValue::Text(ssid) = v {
                self.ssid = Some(ssid);
            }
        }
    }
}

fn main() -> fairing::Result<()> {
    let ctx = egui::Context::default();
    let mut shell = Shell::new(ShellConfig::default(), Services::null(), &ctx)?;

    shell.add(screen_with("wifi", WifiSettings::default).title("Wi-Fi"));

    // Child: hands the value back and closes itself.
    shell.add(screen("wifi.pick", |ui: &mut egui::Ui, cx: &mut Cx| {
        for ssid in ["lab-ap", "guest"] {
            if ui.button(ssid).clicked() {
                cx.finish_with(ScreenValue::Text(ssid.to_owned()));
            }
        }
    }));
    Ok(())
}
```

### 6.1 Two panes

The workspace can show two tasks side by side. A split comes up three ways:
`cx.open_in_other_pane(id)`, the split control — the `split` tile, the nav bar's `"split"` item,
`LaunchAction::ToggleSplit` — and a card's split button in the overview of recent screens
([03 §2.3](03-chrome.md#23-recent-screens-and-the-split)).

| | One pane | Split |
|---|---|---|
| `cx.pane.is_split` | `false` | `true`, in both panes |
| `cx.pane.is_focused` | `true` on top | `true` only in the pane last pressed, outlined in `ColorRole::Focus` |
| `cx.open(id)` | a push | a push onto **your** pane |
| `cx.open_in_other_pane(id)` | a split, `id` sliding in beside you | a push onto the other pane |
| Back | pops | pops the **focused** pane. Popping a pane's root ends its task and the split |
| Home | home | home with both. The next split starts afresh |

- **A press moves the focus** — a press on the screen itself. One on something drawn over the
  panes (the shade, the on-screen keyboard, the unlock prompt, a toast) does not: typing on the
  keyboard over the other pane keeps your pane focused. The edge back gesture belongs to the pane
  it starts in and is measured across that pane, not the screen.
- **A screen in the other pane acting on its own** — `cx.open` or `cx.finish_with` with no press
  there, a timer or an answer arriving — acts in its own pane, at once and without a slide, and
  the focus stays where you are. Its result goes to its own parent.
- **The divider** follows a finger 1:1 and, let go, settles where both screens keep their
  `SplitSupport` minimum. A double tap evens it out. Pushed to an end, the pane it is pushed into
  closes — its task lives on, one tap away in the recent screens.
- **The bars** follow the focused pane's `ChromePolicy`, except that a bar hides only when **both**
  panes hide it — a fullscreen screen beside an ordinary one keeps the bars. `keep_awake` holds
  while either pane's screen asks for it.
- **`Resized`** arrives once as a split's slide ends and once when the divider is let go — and
  whenever a task changes size without a slide of its own: put into the other pane, resumed into
  a split, sent to the background when its pane goes. A screen is told only sizes it has not
  heard. While the divider settles the panes keep taking taps.
- **The axis** follows the content's shape — wider than tall, side by side — unless
  `[workspace] split_axis` fixes it ([07 §16](07-config-reference.md#16-workspace)).
- **No split holds** where the screen on show has `.split(SplitSupport::No)` or
  `allow_split = false`, where the one coming has `SplitSupport::No`, where the two minimums do
  not fit, where `[workspace] split = false`, or where the session does not pass the
  `workspace.split` gate ([05 §2.1](05-access-control.md#21-built-in-gates)). Then
  `open_in_other_pane` opens in your pane — the screen still opens.
- A pane whose task ends — back at its root, a gate the session lost, a card thrown away in the
  recent screens — takes the split with it, and the other pane fills the content. So does a
  screen that never shares coming to the top of a pane (a task resumed into it, a pop that
  uncovers it): the focused pane keeps the content.

## 7. Per-screen chrome

A `ChromePolicy` is set in two places: on the declaration (`.chrome()` · `.fullscreen()` ·
`.osk()` · `.keep_awake()`) and at runtime (`cx.set_chrome()`). It is not a trait method.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `status_bar` | `BarMode` | `Show` | `Show` · `Hide` · `Overlay` (translucent, over the content) |
| `nav_bar` | `BarMode` | `Show` | `Show` · `Hide`. `Overlay` is treated as `Show` |
| `allow_peek` | `bool` | `true` | Let an edge pull briefly reveal a hidden bar |
| `edge_guard` | `bool` | `false` | Blocks every edge gesture. Only the emergency gesture is left |
| `osk` | `OskMode` | `Auto` | `Auto` (follows focus) · `Manual` (toggle only) · `Off` |
| `keep_awake` | `bool` | `false` | Stops the display idle timer |
| `allow_split` | `bool` | `true` | Allow a split to come up while this screen is focused. Off, `open_in_other_pane` and the split control keep to one pane |
| `background` | `Option<ColorRole>` | `None` | Overrides the pane background colour |
| `inset` | `Option<f32>` | `None` | Padding inside the rect the screen receives. `None` means `theme.metrics.screen_inset` (12) |

`ChromePolicy::fullscreen()` is `status_bar: Hide, nav_bar: Hide, inset: Some(0.0)` with
everything else at its default. `allow_peek` stays `true`, so pulling an edge brings the bars
back. `inset_or(default)` uses the value you pass when `inset` is `None`, and clamps negatives
to 0.

`edge_guard = true` blocks every edge regardless of `allow_peek`, leaving only the emergency
gesture: a 2-second press within 64 px of a top corner. It is for screens where a mis-touch
causes an accident on the spot — cutting, driving. The full contract is in
[03 Chrome](03-chrome.md).

```rust
use fairing::screen::{BarMode, OskMode}; // only ChromePolicy is re-exported at the crate root
use fairing::{screen, ChromePolicy, Cx, Services, Shell, ShellConfig};
use std::cell::Cell;
use std::rc::Rc;

fn main() -> fairing::Result<()> {
    let ctx = egui::Context::default();
    let mut shell = Shell::new(ShellConfig::default(), Services::null(), &ctx)?;

    // At declaration: fullscreen, idle timer stopped, OSK off.
    shell.add(
        screen("camera", |ui: &mut egui::Ui, _cx: &mut Cx| {
            ui.heading("camera");
        })
        .fullscreen()
        .keep_awake()
        .osk(OskMode::Off),
    );

    // For finer control, hand over the whole policy.
    shell.add(
        screen("cutting", |ui: &mut egui::Ui, _cx: &mut Cx| {
            ui.heading("cutting");
        })
        .chrome(ChromePolicy {
            status_bar: BarMode::Overlay,
            nav_bar: BarMode::Hide,
            edge_guard: true, // no mis-touch mid-cut; only the emergency gesture is left
            inset: Some(0.0),
            ..ChromePolicy::default()
        }),
    );

    // At runtime: fullscreen only while playing.
    let playing = Rc::new(Cell::new(false));
    shell.add(screen("player", move |ui: &mut egui::Ui, cx: &mut Cx| {
        if ui.button("toggle").clicked() {
            playing.set(!playing.get());
            cx.set_chrome(if playing.get() {
                ChromePolicy::fullscreen()
            } else {
                ChromePolicy::default()
            });
        }
    }));
    Ok(())
}
```

## 8. Building the inside of a screen with `layout`

Everything so far was about **declaring** a screen. The **inside** of one is built with
`fairing::layout` — the crate's own settings screens are, and so are `examples/demo.rs`
and `examples/kiosk.rs`.

Stacking `ui.heading` · `ui.button` · `egui::ScrollArea` by hand does work. It also means
re-deciding touch targets (13 mm with gloves), card backgrounds, role colours, scrolling and side
margins on every screen — and the one screen that misses a decision just quietly looks different.

### 8.1 Three layers

| Layer | What | What you use |
|---|---|---|
| **Containers** | Divide the space | `page` · `group`/`group_with` · `Grid` · `action_bar` · `split_width` · `split_height` · `title` · `section` · `note` |
| **Rows** | Place one row inside a container | `switch_row` · `slider_row` · `choice_rows` · `info_row` · `nav_row` · `icon_row` · `list_item` · `status_card` |
| **Fitting** | Fit something into a space | Text: `fit_text` · `fit_size`. Images: `contain` · `cover_uv` |

`layout`'s **row** helpers are not the same thing as the widgets in
`widgets`. `widgets::Switch` **draws** a switch;
`layout::switch_row` decides **where that switch goes** next to its label. Building a screen,
the row helpers are usually all you need.

### 8.2 One page

`page` gives you vertical scrolling and side margins in one call — no per-screen `ScrollArea`.

```rust
# fn add(shell: &mut fairing::Shell) {
use fairing::{layout, screen, Cx};

shell.add(screen("network", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
    layout::page(ui, cx, "network", |ui, cx| {
        layout::title(ui, cx, "Network");

        layout::section(ui, cx, "Wireless");
        layout::group(ui, cx, |ui, cx| {
            let mut on = true;
            // (title, subtitle, value, enabled) — returns `true` on the frame it changed.
            if layout::switch_row(ui, cx, "Wi-Fi", Some("home-5g"), &mut on, true) {
                // the value just changed
            }
            if layout::nav_row(ui, cx, "Known networks", Some("3")).clicked() {
                cx.open("network.known");
            }
        });

        layout::group(ui, cx, |ui, cx| {
            layout::info_row(ui, cx, "IP", "192.168.0.42");
            layout::info_row(ui, cx, "MAC", "a4:5e:60:11:22:33");
        });
        layout::note(ui, cx, "A static IP is wired-only.");
    });
}));
# }
```

There are no separators inside a card. The card boundary already expresses the grouping, and a
line on top of it splits one card in two. Group related rows with `group` and let the gaps
between cards do the work — the eye then reads card by card.

### 8.3 A bar pinned to the bottom

Buttons that must stay visible **however long the body gets** — confirm, pay — go in an
`action_bar`. The body flows above it and scrolling never runs underneath. When the OSK comes up,
the bar steps above it.

```rust
# use fairing::{layout, Cx};
# fn body(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
layout::action_bar(
    ui,
    cx,
    2.0, // bar height = row height × 2
    |ui, cx| { /* the body — this is what scrolls */ },
    |ui, cx| {
        if fairing::widgets::BigButton::new("Confirm")
            .kind(fairing::widgets::ButtonKind::Primary)
            .show(ui, &mut cx.widgets())
            .clicked()
        {
            cx.finish();
        }
    },
);
# }
```

### 8.4 Grids

`Grid` takes a **minimum cell width** and produces as many columns as the width allows. It is not
in absolute pixels but in multiples of the row height, so it follows the gloved-hand policy.

| Handle | When |
|---|---|
| `max_columns(n)` | A strip with a fixed count. Five category chips or three payment methods want to be on one line first, and to be wide second |
| `fill_height(ratio)` | Upright panels. A grid fills from the top and does not use the leftover height, so the bottom half goes empty. The ratio is a **height cap relative to tile width** |
| `deco(Deco)` | Colour, corners, spacing |
| `fill_with(f)` | A different background per item. Sold-out or recommended tiles |

```rust
# use fairing::{layout, Cx};
# fn body(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
# let items = ["Espresso", "Latte", "Mocha"];
layout::Grid::new(2.2, 2.1)     // min cell width 2.2 rows, floor height 2.1 rows
    .max_columns(3)
    .fill_height(1.1)           // take the spare height, but stop at 1.1 × the width
    .show(ui, cx, &items, |ui, cx, cell| {
        // cell.rect is the logical (hit) rect; cell.visual is where you draw
        layout::fit_text(
            ui.painter(),
            cell.visual.center(),
            egui::Align2::CENTER_CENTER,
            cell.item,
            cell.visual.width() * 0.9,
            egui::FontId::proportional(20.0),
            cx.theme.color(fairing::ColorRole::OnSurface),
        );
    });
# }
```

To keep the **text size consistent across sibling tiles**, settle on one size with `fit_size`
first instead of calling `fit_text` per cell. Shrinking each cell on its own only shrinks the
tiles that have long labels.

### 8.5 Wide screens and upright screens

In both cases **the crate decides whether to divide at all**, and returns `None` when it should
not. Then you just draw the screen as one piece.

```rust
# use fairing::{layout, Cx};
# fn body(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
// Wide: list on the left, body on the right. Narrow: None → draw the list, push on tap.
if let Some(list_w) = layout::split_width(cx) { /* two columns */ } else { /* one column */ }

// Upright and tall enough for two bands: the height of the top band. The ratio is an argument
// because the mounting height decides it.
if let Some(hero_h) = layout::split_height(ui, cx, 0.34) { /* top: look at. bottom: press */ }
# }
```

On a portrait kiosk the top of the screen is out of reach. `action_bar` pins a bar to the bottom
but cannot move where the body **starts**, so dividing top from bottom is `split_height`'s job.

### 8.6 Photos

`IconRef::Texture` carries only a `TextureId`, so it **does not know its own size**. It fills the
rect it is handed, which means a 3:2 photo in a square icon slot is drawn squashed. The only side
that knows the size is whoever uploaded the texture, so fit it there.

```rust
# use fairing::layout;
# fn draw(ui: &mut egui::Ui, tex: &egui::TextureHandle, into: egui::Rect, tint: egui::Color32) {
# let UV_FULL = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
// Fit inside without cropping (object-fit: contain)
let at = layout::contain(tex.size_vec2(), into);
ui.painter().image(tex.id(), at, UV_FULL, egui::Color32::WHITE);

// Fill the slot (object-fit: cover) — the slot stays, the sampled region narrows
ui.painter().image(tex.id(), into, layout::cover_uv(tex.size_vec2(), into), tint);
# }
```

## 9. Common patterns

### 9.1 Sharing app state across screens

**Your app owns its state, and owns the shell.** The shell is a field of your app, not a container
for it. Once a frame you lend the state to the screens with `Shell::frame_with`, and they reach it
through `cx.app` / `cx.app_mut` / `cx.with_app`. Nothing is captured, nothing is stored by the
crate, and there is no cell.

```rust
use fairing::{screen, Cx, Services, Shell, ShellConfig};

/// Machine state several screens look at — **yours**.
#[derive(Default)]
struct AppState {
    recipe: String,
    runs: u32,
}

struct Kiosk {
    shell: Shell,
    state: AppState,
}

impl Kiosk {
    fn new(ctx: &egui::Context) -> fairing::Result<Self> {
        let mut shell = Shell::new(ShellConfig::default(), Services::null(), ctx)?;

        // Neither screen captures anything — they ask for it.
        shell.add(screen("run", |ui: &mut egui::Ui, cx: &mut Cx| {
            let recipe = cx.app::<AppState>().map_or(String::new(), |s| s.recipe.clone());
            ui.label(format!("recipe: {recipe}"));
            if ui.button("start").clicked() {
                if let Some(state) = cx.app_mut::<AppState>() {
                    state.runs += 1;
                }
            }
            if ui.button("Pick a recipe").clicked() {
                cx.open("recipe");
            }
        }));

        shell.add(screen("recipe", |ui: &mut egui::Ui, cx: &mut Cx| {
            for name in ["A", "B"] {
                if ui.button(name).clicked() {
                    if let Some(state) = cx.app_mut::<AppState>() {
                        state.recipe = name.to_owned();
                    }
                    cx.finish();
                }
            }
        }));

        Ok(Self { shell, state: AppState::default() })
    }

    fn frame(&mut self, ui: &mut egui::Ui) {
        // The one line that lends it.
        self.shell.frame_with(ui, &mut self.state);
    }

    /// Everything outside the frame — a poll loop, an engine reply — is just your own field.
    fn on_engine_reply(&mut self) {
        self.state.runs = 0;
    }
}
```

`runner::run_app` is the ready-made loop for this shape: `build` hands back `(Shell, S)` and the
runner owns both and calls `frame_with` for you. `runner::run_shell` is the same thing for an app
with no state of its own. Driving the loop yourself (`runner::run`, or eframe directly) works too —
`frame_with` is one call.

One value goes over per frame. Several things to share means one `struct` holding them.

`cx.app_mut::<T>()` borrows all of `cx`, so you cannot hold it and call `layout::title(ui, cx, ..)`
in the same breath. When you need both, `cx.with_app` takes the reference out for the length of the
call and puts it back:

```rust
# struct Order { total: u32 }
# fn draw(ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>) {
cx.with_app::<Order, _>(|order, cx| {
    fairing::layout::title(ui, cx, "Cart");
    order.total += 1;
});
# }
```

`cx.app::<T>()` is `None` when the frame was driven by plain `Shell::frame`, or when `T` is not the
type you lent.

**Between frames the shell is not holding your state** — you are. So a call you make yourself
outside the frame, which runs your code on the spot, runs it with `None`. There are three, and each
has a pair that lends the state for its length:

| Runs your code out of frame | With the state |
|---|---|
| `shell.launch(LaunchAction::run(id))` → your `action(id, ..)` closure | `shell.launch_with(action, &mut self.state)` |
| `shell.back()` → the top screen's `on_back` | `shell.back_with(&mut self.state)` |
| `shell.knock(id)` → the entry point's action | `shell.knock_with(id, &mut self.state)` |

`shell.handle().launch(..)` works too — it queues, so it lands inside the next frame with the state
in hand. `LaunchAction::Open` needs none of this: it puts a screen up and the screen's `ui` runs in
the next frame, which has the state anyway.

> **This section used to say `Rc<RefCell<_>>`**, because there was nowhere else to put shared state.
> That was a cell with a `borrow_mut()` that panics. `Rc<T>` on its own is still fine for something
> immutable you want to share cheaply, like a loaded image set.

### 9.2 Getting results back from background work

The UI thread never blocks. A worker thread sends its result down a channel and wakes the UI with
`Waker::wake()`; the screen picks it up with `try_recv` each frame.

```rust
use fairing::{screen, Cx, Services, Shell, ShellConfig};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

fn main() -> fairing::Result<()> {
    let ctx = egui::Context::default();
    let mut shell = Shell::new(ShellConfig::default(), Services::null(), &ctx)?;

    let (tx, rx) = mpsc::channel::<String>();
    // Waker is Clone + Send. So is ShellHandle, so you can hand the whole thing over.
    let waker = shell.handle().waker().clone();
    let handle = shell.handle();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2)); // the slow part
        if tx.send("calibration done".to_owned()).is_ok() {
            waker.wake(); // wake a UI that is asleep at 0 fps
        }
        handle.toast("calibration done"); // toasts work from any thread too
    });

    let rx = Rc::new(RefCell::new(rx));
    let last = Rc::new(RefCell::new(String::from("(waiting)")));
    shell.add(screen("job", move |ui: &mut egui::Ui, _cx: &mut Cx| {
        // Never block. try_recv(), not recv().
        while let Ok(msg) = rx.borrow().try_recv() {
            *last.borrow_mut() = msg;
        }
        let text = last.borrow().clone();
        ui.label(text);
    }));
    Ok(())
}
```

Hand the whole `ShellHandle` over and the worker can call `notify` · `toast` · `launch` ·
`set_badge` itself. Those already `wake()` internally.

### 9.3 List screens

Wrap it in `layout::page` and stack rows inside cards. Scrolling and side margins come from
`page` — including shrinking by however much the OSK covers, so there is no need to subtract
`cx.pane.inset_bottom` yourself.

```rust
use fairing::{icon, layout, screen, Cx, Services, Shell, ShellConfig};

fn main() -> fairing::Result<()> {
    let ctx = egui::Context::default();
    let mut shell = Shell::new(ShellConfig::default(), Services::null(), &ctx)?;

    shell.add(screen("devices", |ui: &mut egui::Ui, cx: &mut Cx| {
        layout::page(ui, cx, "devices", |ui, cx| {
            layout::title(ui, cx, "Devices");
            layout::group(ui, cx, |ui, cx| {
                for name in ["sensor-1", "sensor-2", "pump"] {
                    if layout::nav_row(ui, cx, name, Some("OK")).clicked() {
                        cx.open("device.detail");
                    }
                }
            });
        });
    }));

    shell.add(screen("device.detail", |ui: &mut egui::Ui, cx: &mut Cx| {
        layout::page(ui, cx, "device.detail", |ui, cx| {
            layout::title(ui, cx, "sensor-1");
            layout::group(ui, cx, |ui, cx| {
                layout::info_row(ui, cx, "Slot", "0");
            });
        });
    }));
    Ok(())
}
```

When a row needs an icon, a subtitle or a badge, reach for `layout::list_item` (icon + title +
selection mark) or `widgets::ListRow` (icon · subtitle · trailing · chevron, all available).
A screen rarely has to draw its own back button — the nav bar already provides one.

> **`icon_row` is a row you press.** It always carries a chevron, so on a read-only row it reads
> as "something opens if I tap this". That row is `info_row`.

If one row has to open a different screen per item, make the detail screen a factory and pass
which item through an `Rc<RefCell<_>>` or a `ScreenValue`. Do not grow the number of declaration
ids with the number of items.

## 10. Next

| What you want | Page |
|---|---|
| Status bar, nav bar, shade, notifications, OSK | [03 Chrome](03-chrome.md) |
| Theming and repainting the shell itself | [04 Customization](04-customization.md) |
| Locking screens behind gates | [05 Access control](05-access-control.md) |
| Wiring up a backend | [06 Services](06-services.md) |
| Verifying this screen headless | [01 Getting started §6](01-getting-started.md#6-testing-your-ui-headless) |
