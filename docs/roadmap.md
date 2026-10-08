# fairing roadmap

Where the crate stands, what comes next, and the limits it has today. The parts that exist are
described in the [integrator guide](guide/README.md); how they fit together is in
[architecture](architecture.md).

## 1. Where it stands

0.1.0 is the first published release. Milestones M0 to M6 are done.

| M | Name | State | Where the guide covers it |
|---|---|---|---|
| M0 | Scaffold and the audit gate | Done | [08 §9 Audit](guide/08-troubleshooting.md#9-passing-the-audit) |
| M1 | The shell skeleton | Done | 01 · 02 · [03 §1](guide/03-chrome.md#1-the-status-bar)–[§3](guide/03-chrome.md#3-the-desktop) · 06 |
| M2a | Compile-time customization hooks | Done | [04](guide/04-customization.md) |
| M2 | Chrome complete | Done | [03 §4](guide/03-chrome.md#4-the-shade)–[§8](guide/03-chrome.md#8-icons) · [04 §8](guide/04-customization.md#8-tuning-motion) |
| M2b | Physical units · the dock on any edge and the icon rail · brand | Done, with part of the plan moved after 0.1 (§3) | [01 §4](guide/01-getting-started.md#4-a-minimal-app) · [03 §3](guide/03-chrome.md#3-the-desktop) · [09](guide/09-branding.md) |
| M3 | Access control: the unlock prompt and lock screen, `Authenticator` and `PinTable`, the `PinPad` and `PatternPad` widgets, temporary unlocks, the session timeout and the idle lock, `settings.credentials` | Done | [05 §5](guide/05-access-control.md#5-the-shells-prompt-and-the-authenticator) · [05 §7](guide/05-access-control.md#7-the-session--temporary-unlocks-the-timeout-the-idle-lock) |
| M4 | Settings screens and backend traits: the `settings.*` screens, the audio, network and device-information traits with their `Null` and `Mock`, backends of your own | Done. The backends and the saving of settings are yours | [06](guide/06-services.md) · [06 §7.1](guide/06-services.md#71-keeping-settings-across-a-restart) |
| M5 | Two panes: `cx.open_in_other_pane`, the divider, the focus, `SplitSupport`; the recent screens; the split control and tile; `[workspace]` | Done | [02 §6.1](guide/02-screens.md#61-two-panes) · [03 §2.3](guide/03-chrome.md#23-recent-screens-and-the-split) · [07 §16](guide/07-config-reference.md#16-workspace) |
| M6 | Finishing: the string table with Korean built in and live language switching; gesture navigation; the info popover on a held desktop icon; layouts and painters for the nav bar, toasts, banners, the keyboard and the shade; the status bar's items placed; painters for the lock screen, the unlock prompt, the recent screens and every kind of widget; thin gesture handles on the side and bottom edges; gesture regions of your own; the prelude and the `hello` example | Done, released as 0.1.0 | [04 §9](guide/04-customization.md#9-text-and-translations) · [03 §2.6](guide/03-chrome.md#26-the-gesture-style) · [03 §3.8](guide/03-chrome.md#38-holding-an-icon-the-info-popover) · [04 §10](guide/04-customization.md#10-moving-and-redrawing-the-other-pieces) · [03 §9](guide/03-chrome.md#9-gestures) |

## 2. 0.1.0

The first release publishes `fairing` and `fairing-widgets` to crates.io. From 0.1.0 on, changes
follow semantic versioning: before 1.0, a release that breaks the API raises the minor version
(0.1 to 0.2), and a patch release never does. The [CHANGELOG](../CHANGELOG.md) records each one.

## 3. After 0.1

These are candidates, not promises. Each one waits for a real need.

| Item | What happens today |
|---|---|
| Bars beyond the shell's two, on any edge, and a `[[bar]]` TOML section to declare them | The shell has the status bar and the nav bar. A painter redraws either ([04 §3](guide/04-customization.md#3-painting-a-whole-bar-yourself)), and a band of your own goes inside your screens (`layout::action_bar`, `layout::tab_bar`) |
| Desktop regions, and arranging desktop icons by hand | The grid, the dock and the icon rail. An edit would come out as an event for you to keep, since the crate writes no files |
| The `[metrics]`, `[device]`, `[display]` and `[scale]` TOML sections, and unit helpers on `Cx` | Metrics are set in code with `ShellBuilder::metrics_spec` ([04 §1](guide/04-customization.md#1-injecting-a-theme)). A screen reads the resolved sizes from `cx.theme.metrics` |
| A diagnostics overlay with scale reports | The log says which density the shell assumed and why |
| Chinese and Japanese input on the keyboard | Numeric, QWERTY and two-set Hangul |
| More than two panes, and thumbnails in the recent screens | Two panes; the recent screens show cards |
| A software rasterizer for devices with no GPU | A GL driver is needed |
| Snapshot tests of what is drawn | Tests assert positions and state through `testing::Harness` |
| An accessibility tree, and right-to-left layout | Nothing is reported to a screen reader, and layouts run left to right |

## 4. Known limits

These are how the crate behaves today. Each is small, and each has a reason.

- **One finger.** The gesture engine follows a single touch. A second finger is not a gesture.
- **Strips and regions are a frame or two behind the glass.** egui decides what a press hits from
  where things were the frame before. So a gesture region that moves, grows or first appears
  takes a frame or two to catch up, and so does a handle's strip when the keyboard comes up.
  While the keyboard slides in, a side strip can cover the outer 2 mm of an edge key for up to
  two frames, about 33 ms at 60 Hz, and a bottom strip can cover the space bar's lower rim for
  one frame from the second showing on.
- **Corner knocks count inside regions.** The hidden entries' corner knocks are read from the raw
  pointer, so a tap in a region over a knock corner still counts. Keep a trackpad out of the
  corners your hidden entries use ([03 §9.3](guide/03-chrome.md#93-gesture-regions-of-your-own)).
- **The bottom edge is the nav bar's or the handles'.** With the nav bar on, a bottom gesture
  handle is refused. Turn the bar off to put one there.
- **No image decoder.** The crate draws textures you upload. Decoding is yours
  ([09 §1](guide/09-branding.md)).
- **No CJK font inside.** egui's default fonts have no Hangul. Load a font with
  `ShellBuilder::fonts` ([08](guide/08-troubleshooting.md)).

Found something else? Please open an issue with what you did, what you expected and what
happened ([CONTRIBUTING](../CONTRIBUTING.md)).
