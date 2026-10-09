# Changelog

All notable changes to the fairing crates are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). From 0.1.0 the crates follow
[Semantic Versioning](https://semver.org/); until then, any release may break the API.

## Unreleased

Nothing has been published to crates.io yet. This will be the first release, 0.1.0.

The crates are licensed under MIT alone. The `fairing-widgets`
package carries the Lucide icon licence (`LICENSE-lucide`) next to the icon geometry it is built from.

### `fairing` — the shell

- `testing::Harness` finds a place by its label: `tap_text(label)`, `text_rect(label)` and
  `texts()` look through what the frame drew, so a test written for today's row height is
  still right when the finger, the type scale or the density changes — and a label that is not
  on the glass is an error naming what is, never a tap on nothing. The example tours press the
  same way (`Act::Tap(Spot::Text(..))`, with `Spot::Edge` and `Spot::Page` for the gestures
  and `Act::MoveBy` for every drag) and have no coordinate form at all.
- The built-in settings screens group their rows in filled cards. A title with no actions is as
  tall as its words, a section heading as tall as its text, an explanation wraps at about seventy
  characters, and the wide settings screen's two titles sit on one line.
- The status bar's item gap and the user level's dot are component tokens
  (`components.status_bar.item_gap` · `user_dot`, 10 and 8 du by default, set through
  `ShellBuilder::component_spec`) rather than constants; the audit's integrity stage checks
  that the README, CONTRIBUTING and the getting-started table quote `Cargo.toml`'s
  `rust-version`, so a bump that forgets a sentence fails.
- One source for what was written in several places: the devices the tours run on are one
  table (`tools/devices.sh`) that `tools/tours.sh` and `tools/readme-gifs.sh` both read; the
  recorder writes its frame rate beside the frames and `tools/make_gif.py` reads it there; the
  README's keyboard picture is cut by the tour (`Act::ShotBelow`) rather than at a pixel count
  in the script; CI reads the minimum Rust version from `Cargo.toml`; the examples' default
  window is `testing::DEFAULT_SIZE`; the audit names its cross target. A test checks that
  `settings::screens::ALL`, the home entries and the registrations agree, and that every
  built-in status id round-trips. The guide's metrics table says which numbers are floors under
  a finger-sized default rather than the default.
- The headless tests ask the shell where things are and how long motions take instead of
  assuming: a field is found by its hint, a row's switch by its knob, a gap between status
  items by the rects the bar laid out; waits are the motion tokens' lengths; a pull short of
  the snap is a share of the card's height. A test written for one finger or one token value
  no longer passes by coincidence.
- The example tours have no pixel distance and no frame count standing for a time: a drag goes
  to a `Spot` (`Act::DragTo`, with `Spot::Across` for a drag that keeps its line), a finger
  held still is `Act::Hold`, and a long press or a toast's lifetime is waited on the input clock
  (`Act::WaitMs`). The palette sheet checks its last row is on the glass; the gesture navigation
  tour runs in `tools/tours.sh` and CI; the kiosk's counter and compact tours run on the devices
  the example's heading lists, and the layout is picked from the panel's millimetres; a tour
  name `tools/tours.sh` does not know is an error, not an empty pass. `custom_chrome` sizes its
  chrome through `MetricsSpec` in the hand's units rather than a block of du literals, and
  `bench` drives its gestures as shares of the screen.
- Sizes that assumed a bare finger now follow the finger: the emergency ring and corner zone,
  the rail's collapsed arm and its grip strip (the edge zone), the quick-tile gap and the tile
  row's insets (`screen_inset`), a toast's and a banner's narrowest and widest and their
  margins, the nav bar's fallback glyph (`nav_icon_size`), and the gauge tile's value column
  (3.5 × the body size — `Gauge::value_width` is now `Option`, `None` for that default). Hairlines
  are `control.stroke_hairline` everywhere; the heads-up fling is `motion.fling_px_s`, the
  credentials screen's held Remove is `motion.long_press`, the shade's progress bar
  `heads_up.progress_h`.
- Numbers that belonged to a token: the status card's title and detail are set in the type
  scale's `body` and `small` (they were 0.34 and 0.23 of the row, so a gloved row made them half
  as big again), the licences list likewise; the open/close card's icon grows from the desktop
  icon's own `metrics.icon_size` to twice it (it started at a fixed 48 px); the long-press
  ring's pop and cancel run on `[motion.press]`'s tweens in `Button` and `IconButton` alike,
  and the status icons' crossfade on `motion.crossfade`, so all three follow `motion.reduce`
  and the config; the dim under a pane and the push shadow band take the palette's `Scrim`
  hue; an icon drawn by a widget is styled at the size it is drawn (`IconStyle::default()` is
  24, and a feature card's art was getting a list icon's stroke). The kiosk example picks its
  layout from the panel's size in millimetres, as its own heading says, not from the pixel
  width.
- A `Filled` card contrasts with the panel it is on: `SurfaceVariant` on a screen, `Surface`
  on a panel that is already `SurfaceVariant` — the page inside a `layout::Rail`'s elbow, which
  sets its page `Ui`'s `panel_fill` for the purpose. A card there used to be the page's own
  colour and vanish.
- `layout::transit` tells the shell it is mid-motion, so `Shell::is_animating` — and anything
  waiting for rest on it — covers a page on its way; `Cx::keep_animating` is the call, for a
  screen with a motion of its own.
- The console example's Settings page opens the built-in settings screens in its page
  (`Cx::draw_screen` under a row back to the tiles), and a quiet tile's word is in `OnSurface`.
- The pull-down shade has a colour role of its own, `ColorRole::ShadeSurface` (`shade_surface`
  under `[theme.palette]`). Unset, it follows `surface`; set, it colours the curtain, the floating
  card and the tile pucks without touching any other surface.

- Status bar, navigation bar (buttons or edge gestures), a desktop with pages, a dock and
  badges, and a screen stack with icon-zoom and push/pop transitions.
- The shade: one panel or split (notifications on the left, controls on the right), drawn
  down as a curtain or arriving as a floating card. A card lies on frosted glass and stands
  off the page with a lit top edge.
- One gesture engine for edge swipes, flings, long presses, the shade and nested-scroll
  hand-off. A slide along an edge (`Gesture::EdgeSlide`) is recognised on the edges the shell
  asks for, and a pause in a swipe (`Gesture::SwipeHold`) is reported once a pause.
- Notifications, toasts and heads-up banners, postable from any thread.
- An on-screen keyboard: numeric, QWERTY and two-set Hangul with composition.
- Eleven built-in settings screens (network and users & access among them), and access gates
  on screens, actions, tiles and setting keys.
- The shell's own unlock prompt and lock screen in `[access] mode = "prompt"`: a PIN pad, a
  pattern, a password card with the on-screen keyboard above it, or a badge-reader wait —
  whatever the `Authenticator` offers, with its refusals in the title's place in its own
  words, a lockout countdown and the wrong-PIN shake. `PinTable` is the reference
  authenticator, read from `[access.pin_table]` (with `max_len` for the PIN's most digits)
  and `[access.pattern_table]`; `ShellBuilder::authenticator` puts in your own.
  `Authenticator::begin` is called each time a prompt opens. A lockout outlasts closing the
  prompt and a second request, and holds for a password and a badge as for the keypad; a
  grant has to be a step up to a level the table has; what was asked behind the lock screen
  is asked again once it goes. A badge reader may end a read with Enter or Tab, and a pause
  starts a new read. What the pads cannot draw as asked is logged.
- The session: temporary unlocks (`unlock_mode`, `temporary_secs`), the session timeout, the
  idle lock (`[access.lock_screen] allow_continue`), the `status.lock` padlock, and events for
  every request, grant, refusal, lockout and change of subject — with the reason. Every
  deadline runs on the wall clock, including the time an idle panel sleeps. A slow answer never
  holds the keys: while one is awaited the prompt says "Checking…", and a new attempt calls off
  the check still under way (`Authenticator::cancel`), so an answer that never comes cannot hold
  the lock screen.
- `settings.credentials`, for an authenticator that manages its own entries
  (`Authenticator::admin`). Nobody gives a level above their own or changes an entry above
  it, and `CredentialAdmin::set_actor` says who is asking. A form stays up until it is
  answered and says what went through and what did not; it goes, with what was typed, when
  the screen does.
- Two panes: `cx.open_in_other_pane`, the split control (`tile.split_screen`, the nav bar's
  `"split"`, `LaunchAction::ToggleSplit`) and a divider that follows the finger, settles where
  both screens keep their `SplitSupport` minimum, evens out on a double tap and closes a pane
  pushed to its end. The focus follows the last press on a pane's own screen; the bars hide
  only when both panes hide them; `Resized` comes once per change. `ShellEvent::SplitToggled`,
  `[workspace] split` and `split_axis`, `[motion.panes]`.
- The recent screens (`"recents"`, `LaunchAction::OpenOverview`, `[workspace] overview`): the
  screen on show shrinks into its card; tap to bring a task back, swipe up to close it, a card's
  split button puts it beside the pane on show, and "Close all". The split control opens the
  same cards as a picker, offering only what fits; where nothing can, it says why. The
  `nav.recents` and `workspace.split` gates are enforced. `[motion.overview]`.
- The string table: every word the shell draws — the shade, the lock screen and the unlock
  prompt, the recent screens, the settings screens — is a key looked up in the active
  language, and Korean is built in. `[shell] locale` is the language to start in (`ko-KR`
  finds `ko`); the `ui.locale` setting, which `settings.locale` writes, switches it from the
  next frame. `ShellBuilder::translations` adds a language or changes a built-in entry, and
  your screens look their own words up through `cx.strings`; screen titles and level labels
  are keys too. The lock screen's date and the 12-hour clock are templates for the
  language's own order (`time::Meridiem`). A language whose letters no loaded font has is
  logged. `SlotCx` and `DesktopCtx` carry the table, so a slot painter's labels follow the
  language as well.
- Gesture navigation (`[nav_bar] style = "gesture"`): a band with the home indicator. Up from
  the bottom edge is home — the screen on show follows the finger, shrinking as it rises, and
  carries on into its icon from where it was let go; up and a pause is the recent screens, the
  screen carried on into its card; along the indicator is the task used before or after (a run
  of switches keeps its order), and at home the task used last; back stays a `back_edges`
  swipe. The gestures come with the bar and keep to `edge_guard`, the shade, the unlock prompt
  and the on-screen keyboard's keys.
- A screen's widget state is its own: two tasks' screens no longer share scroll and focus by
  taking turns on the same layer.
- Holding a desktop icon brings up its info popover: the title, the level it needs where that is
  more than the lowest, and the description its declaration gives (`ScreenDecl::description`,
  `ActionDecl::description`, or `description` in `[[desktop.pages]]`). Any press puts it away
  and does nothing else; the hold's release opens nothing. `[desktop] long_press = "none"` leaves
  only `ShellEvent::IconLongPressed`. The desktop's long press is now the gesture engine's, as
  the tiles' is: it used to wait for real touch events, so it never fired on a panel that
  delivers touch as the mouse.
- Layouts and painters for the rest of the chrome, from the builder: `nav_bar_layout` and
  `status_bar_layout`;
  `toast_layout` and `toast_painter`; `heads_up_layout` and `heads_up_painter`;
  `osk_key_layout` and `osk_key_painter` (feature `osk`); `shade_tile_layout`,
  `shade_tile_painter` and `shade_panel_painter` (feature `overlay`). A layout is handed the
  built-in placement and changes only what it wants, and a rect it empties leaves that piece
  out; a painter draws one piece, background and all, and may ask for more height. The shell
  keeps when each piece shows, its gates, its presses and what they do — and what follows the
  shade's tiles (the row a tile opens, the list, the two-step stop) follows the lowest one. A
  shade panel painter draws the ground under the panel's content, a card's or a curtain's, and
  names its colour, which the notification list fades into; with one, a card takes no screenshot
  for a frost it would not draw. A nav bar painter wins over a nav bar layout.
- Painters for the lock screen and the unlock prompt: `lock_screen_painter` draws the lock
  screen's ground, clock and date, and `unlock_prompt_painter` the prompt's backdrop and card
  (`fairing::access::{LockScreenCx, UnlockPromptCx, PromptPiece}`). The shell draws the way in —
  the keypad, the dots or the fields, the tabs, the way out — over what they drew, and keeps what
  is typed, the answer, the lockout, the shake and the motion. Each painter is told its piece's
  fade and multiplies its colours by it; the card moves, scales and shakes with the way in.
- Painters for the recent screens: `recents_card_painter` draws each card and
  `recents_ground_painter` the ground under them
  (`fairing::workspace::{RecentCardCx, RecentsGroundCx, RecentsOver}`). The shell keeps where the
  cards go, the carousel's drag, the taps, the throw that ends a task and the screen shrinking into
  its card, and draws the split buttons and "Close all" over them. A card painter is told where
  the shell puts the split button; the ground painter is told whether it stands over the desktop or
  behind a task's screen, a lifted one included.
- Widget painters: `WidgetPainters` holds a painter per kind of widget, each optional, given with
  `ShellBuilder::widget_painters` or, outside the shell, in `WidgetCx::painters`. A widget whose
  kind has one is drawn by it, told its look — its rects, what it says, its state and how far
  each animation has got — and keeps its press, its drag, its value, its motion, focus and keys.
  The shell lends them to every widget it draws, its own screens, prompt and recent screens
  included. Every kind has one: `BigButton` (`ButtonLook`, with a long press's progress),
  `IconButton`, `Switch`, `Checkbox`, `Radio` and a `RadioGroup`'s marks, `SegmentedControl`,
  `Chip`, `TouchSlider`, `Stepper`, `NumberField`, `TextField`, `WheelPicker`, `Dropdown` (the
  control, and while it is open the panel and each option), `PinPad` (the dot row and each key),
  `PatternPad`, `ListRow`, `ProgressBar` and `ProgressRing` (both told a `ProgressFill`), `Meter`,
  `StatusLamp`, `CountBadge`, `MediaCard` and `FeatureCard`. What is typed in a text field or an
  editable number field stays egui's `TextEdit`, drawn over the painter's field; a pattern pad
  told to hide its path hides it from the painter too. A row's switch or radio and a card's
  action disc are still drawn as their own kinds, in the slot the look names. `paint_bar` and
  `CountBadge::paint_over` have no `WidgetCx` and draw the built-in look. A meter painter reads
  the limits back with `Limit::at`, `is_high`, `caption` and `verdict`.
- Gesture handles: `Shell::add_gesture_handle(GestureHandle::new(id, edge))` puts a thin strip,
  2 mm by default, at the very edge of the glass on the left, the right or the bottom, the way One
  Hand Operation+ does on the sides: up to three an edge, each over the stretch of the edge
  `along(from, to)` names, between the two ends `ends(from, to)` chooses (`HandleEnd::EdgeZone`
  by default, `Bar` or `Glass`). The bottom edge is the nav bar's or the handles': a bottom handle
  needs `[nav_bar] enabled = false`, and stands aside if the bar is turned on again. A swipe in
  from the strip is straight or diagonal by its angle: up or down on the sides, left or right on
  the bottom (`HandleDirection::on(edge)`). Let go past `reach_mm`, it is that way's short
  gesture; held still there for `long_after`, it is the long one, which runs at once.
  `gesture(HandleGesture, action)` binds each of the six to a `LaunchAction`, with
  `HandleAction::gate` in front where it needs one. A strip takes every press that begins in
  it, as One Hand Operation+ handles do: nothing under it sees one, the shell's edge gestures
  included, and just inward of it all is as before. The side strips end above the on-screen
  keyboard, even while it slides in or out, and a bottom strip stands aside for it; none is
  there over the shade, the unlock prompt, the recent screens or a screen with `edge_guard`.
  `ShellEvent::Gesture { handle, gesture }` reports each one. An arrow ahead of the finger says
  which gesture the swipe is, and `ShellBuilder::gesture_handle_painter` draws it your way.
  `remove_gesture_handle` and `gesture_handles` go with them.
- Gesture regions of your own: `Shell::add_gesture_region(id, region)` takes a `GestureRegion`,
  a trait that places a stretch of the glass each frame (`place`, or `None` to stand aside),
  hears every touch that begins in it from the press to the release (`touch`, with a
  `RegionTouch` of the phase, the movement since the last frame, the velocity, the time held and
  whether it went past the slop) and draws it (`paint`). Part of a screen used as a trackpad is
  one; the gesture handles are the shell's own. Nothing under a region sees its touches and no
  shell gesture starts in it, while the emergency gesture still works through it. A region runs
  the shell's actions through `RegionCx::launch` and `launch_gated` and reaches the app's state
  with `RegionCx::app_mut`. The later of two overlapping regions, or of a region and a handle,
  takes the press. Up to eight (`MAX_GESTURE_REGIONS`); `remove_gesture_region` and
  `gesture_regions` go with them. The engine reports a region's touch as `Gesture::Region`.
- Toasts stand on the on-screen keyboard while it is up, rather than over its keys. On a panel
  too short for that they come down over its top rows, never under the status bar.
- The on-screen keyboard's rows stop at one and a half fingers (`metrics.osk_max_key`, at
  least 72 du), so a tall portrait panel gets keys sized for a hand rather than a keyboard a
  third of the glass high. `[osk] min_key_px` still wins where the two cross, and `None` in a
  `MetricsSpec` lifts the cap.
- `layout::tab_bar` cuts a label wider than its cell with an ellipsis, and shrinks a glyph wider
  than its cell; a long label used to run into the next tab.
- `[status_bar] height`, `[status_bar] icon_size` and `[nav_bar] height` take effect. They are
  optional now: unset, the sizes are the crate's physical ones (7 mm, 3.6 mm, a finger plus
  8 du); set, the value is pinned in du. An integrator's `metrics_spec` or injected theme still
  wins, and `build` warns that the TOML was ignored. Before, the crate's metrics overwrote all
  three every frame.
- The crate carries no font of its own, on purpose: the device gives its faces
  (`ShellBuilder::fonts`), and the shell says at startup when there is no bold or no face for its
  language.
- Warnings for what would otherwise fail without a word: a `nav_item` the nav bar will not
  draw — still missing from `[nav_bar] items` on the frame after its `add`, or on a bar that is
  off or in the gesture style — and a shell with no bold face, whose titles then draw in the
  regular weight. `FontSet::has_bold` says which.
- Physical sizing: touch targets resolve through panel millimetres, with diagnostics for the
  resolved scale.
- A headless test harness, `fairing::testing::Harness`.
- The shell writes no files. A setting someone changes reaches the integrator as
  `ShellEvent::SettingChanged`, to keep wherever the device keeps such things, and
  `Shell::restore_settings` puts the kept values back at start: into the table, to the backends,
  the theme and the language, with no gate checked and no `SettingChanged` sent. The theme comes
  back at once, without the crossfade, and airplane mode is applied last and only ever turns the
  radios off. `[shell] state_dir`, which nothing read, is gone; an old key is ignored.
- `examples/hello.rs`: the smallest complete app — one screen, the built-in settings, a status
  bar item and a nav bar item — standing alone, so it can be copied into a new project.
- Backends through traits only — clock, power, Wi-Fi, Bluetooth, display, audio, network and
  device info — with `Null` defaults and a `Mock` simulator, and custom backends for anything
  else, polled every frame and reached from screens by type. The crates ship no system code;
  the device plugs in its own.

### `fairing-widgets` — the element layer

- **Phone density by default.** `ScalePolicy::default()` is a bare 9 mm finger
  (`ScalePolicy::gloved()` is 13 mm) and text read at 360 mm, a 3.1 mm body em. A row
  (`metrics.row_height`, `widget_height`) is one touch target with its padding inside, rather
  than three body ems plus a margin, and grows only when its own text needs more. A settings row
  is now about 9 mm tall where it was 14–16 mm, close to One UI on a Galaxy Fold.
- 23 touch widgets, from buttons, switches and sliders to a dropdown, a wheel picker, a
  progress ring, a PIN pad and a pattern pad. `ProgressRing::repaint_every` keeps a ring that
  may go round for hours from asking for every frame. `Dropdown::no_match` takes the words a
  search with no results shows, in the caller's language.
- Theme tokens: palette roles, metrics, components, elevation and motion, in light and dark —
  with `metrics.split_divider` and `components.overview` for the shell's two panes and recent
  screens, `metrics.nav_indicator_length` and `nav_indicator_thickness` for the home indicator,
  and `components.popover` for a desktop icon's info popover.
- The `du` / `mm` / finger unit system, 78 vector icons and parametric icons.
- `metrics.osk_max_key`, the tallest a row of on-screen keyboard keys gets, resolved in
  millimetres like the other touch sizes.

### The public API

- `fairing::prelude`: the names most apps reach for in one `use` — the shell and its builder,
  the declarations, `Cx`, the events and what they carry, the palette roles, the built-in icons,
  the `layout` and `widgets` modules and `egui`. `ScreenDecl`, `Services`, `PowerRequest`,
  `SettingKey`, `SettingValue` and `InstanceId` are at the crate root too.
- Internal submodules are private and their items are reached through their parent module
  (`overlay::Shade`, not `overlay::shade::Shade`); the shell's internal methods are
  crate-visible. `unreachable_pub` keeps it that way.
- Enums whose set grows with the crate are `#[non_exhaustive]` — `ShellEvent`,
  `LaunchAction`, `AccessEvent`, `SettingValue`, `TileKind`, `Wallpaper`, `ColorRole`,
  `IconRef`, `ButtonKind`, the error type, the overlay's layout, reveal and panel, and the
  authentication and network enums among them. Everything else is matched exhaustively on
  purpose: a new variant or field there breaks your `match` or struct literal, so the compiler
  shows you where to handle it.
- On docs.rs every item behind a feature carries the feature's name.
- The shell's layout step and the egui area ids it uses are crate-internal; `Shell::layout`
  is how you read the result.
- What only tests reach into — the shell's declaration registry, gesture engine, toast and
  heads-up queues and policy driver, and state probes such as the shade's scrim alpha or a
  screen's layer id — is `#[doc(hidden)]`. It stays callable from your own tests, but it is
  not API and may change in any release.
- The shade opens and closes only through the shell (`LaunchAction::OpenOverlay`,
  `ShellHandle::toggle_overlay`), so the `overlay.open` gate always applies.

### Known gaps

- No bars beyond the status bar and the nav bar, and so no `[[bar]]` TOML section to declare
  one: both come after 0.1, as does arranging desktop icons. A band of your own goes inside your
  screens (`layout::action_bar`, `layout::tab_bar`).
- No accessibility tree and no right-to-left layout.
