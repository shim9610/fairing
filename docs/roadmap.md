# fairing roadmap

Where the crate stands, what comes next, and the limits it has today. The parts that exist are
described in the [integrator guide](guide/README.md); how they fit together is in
[architecture](architecture.md).

## 1. Where it stands

Nothing has been published yet. Milestones M0 to M5 are done, and M6 is down to the 0.1.0
release.

| M | Name | State | Where the guide covers it |
|---|---|---|---|
| M0 | Scaffold and the audit gate | Done | [08 §9 Audit](guide/08-troubleshooting.md#9-passing-the-audit) |
| M1 | The shell skeleton | Done | 01 · 02 · [03 §1](guide/03-chrome.md#1-the-status-bar)–[§3](guide/03-chrome.md#3-the-desktop) · 06 |
| M2a | Compile-time customization hooks | Done | [04](guide/04-customization.md) |
| M2 | Shade, keyboard, icons and motion | Done | [03 §4](guide/03-chrome.md#4-the-shade)–[§8](guide/03-chrome.md#8-icons) · [04 §8](guide/04-customization.md#8-tuning-motion) |
| M2b | Physical units · the dock on any edge and the icon rail · brand | Done, with part of the plan moved after 0.1 (§3) | [01 §4](guide/01-getting-started.md#4-a-minimal-app) · [03 §3](guide/03-chrome.md#3-the-desktop) · [09](guide/09-branding.md) |
| M3 | Access control: the unlock prompt and lock screen, `Authenticator` and `PinTable`, the `PinPad` and `PatternPad` widgets, temporary unlocks, the session timeout and the idle lock, `settings.credentials` | Done | [05 §5](guide/05-access-control.md#5-the-shells-prompt-and-the-authenticator) · [05 §7](guide/05-access-control.md#7-the-session--temporary-unlocks-the-timeout-the-idle-lock) |
| M4 | Settings screens and backend traits: the `settings.*` screens, the audio, network and device-information traits with their `Null` and `Mock`, backends of your own | Done. The backends and the saving of settings are yours | [06](guide/06-services.md) · [06 §7.1](guide/06-services.md#71-keeping-settings-across-a-restart) |
| M5 | Two panes: `cx.open_in_other_pane`, the divider, the focus, `SplitSupport`; the recent screens; the split control and tile; `[workspace]` | Done | [02 §6.1](guide/02-screens.md#61-two-panes) · [03 §2.3](guide/03-chrome.md#23-recent-screens-and-the-split) · [07 §16](guide/07-config-reference.md#16-workspace) |
| M6 | Finishing: the string table with Korean built in and live language switching; gesture navigation; the info popover on a held desktop icon; layouts and painters for the nav bar, toasts, banners, the keyboard and the shade; the status bar's items placed; painters for the lock screen, the unlock prompt, the recent screens and every kind of widget; thin gesture handles on the side and bottom edges; gesture regions of your own; the prelude and the `hello` example | Under way. What is left is the 0.1.0 release | [04 §9](guide/04-customization.md#9-text-and-translations) · [03 §2.6](guide/03-chrome.md#26-the-gesture-style) · [03 §3.8](guide/03-chrome.md#38-holding-an-icon-the-info-popover) · [04 §10](guide/04-customization.md#10-moving-and-redrawing-the-other-pieces) · [03 §9](guide/03-chrome.md#9-gestures) |

The current pre-release also includes issuer-owned sessions, application-owned
permission values, protected element trees and the
[common mediation kernel/routing checks](protected-contracts.md). Queue/display
admission and host-controlled unwind recovery are described in
[memory limits](memory-limits.md). These changes are implemented in the checkout;
see the [migration guide](guide/10-migration.md) for their API and host boundaries.

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
| A software renderer for the complete interactive shell | The bundled runner uses glow/GL. Existing CPU preview/warning helpers render selected display surfaces, not the complete shell |
| Snapshot tests of what is drawn | Tests assert positions and state through `testing::Harness` |
| An accessibility tree, and right-to-left layout | No Fairing-specific screen-reader integration or right-to-left layout path is implemented |

## 4. Known limits

These are the current implementation and verification boundaries.

- **Recovery is scoped to caught unwinds and selected fallible storage paths.**
  `restart_with` replaces a shell, leaving egui/GPU and external application state
  under host control. Complete renderer heap exhaustion, aborts and business-side
  rollback are unsupported; see [memory limits](memory-limits.md).
- **Component verification is scoped.** Proofs and generated/native UI histories do
  not establish complete concrete UI-state coverage or universal interaction progress.
  See [formal operability](formal-operability.md) and [API contracts](api-contracts.md).

- **Background logic cannot present a frame.** The shell/state runners use eframe 0.37's
  `App::logic` fallback for state processing, but drawing and animations still wait for a
  UI pass. Access can be revoked in state while a compositor retains its last presented
  frame. `runner::run` supplies only a UI closure. Hosts needing background shell work
  must use a shell/state runner or implement `App::logic` and call `Shell::logic*` there.
  Backends must still use `Waker` or `next_wake`.
- **Single-pointer shell gestures.** The shell gesture engine follows one primary
  pointer; it does not recognize multi-touch shell gestures.
- **Widget hits use the previous egui pass.** Fairing's regions use current shell/pane/keyboard
  geometry and seed current guard area rectangles before user drawing. A showing keyboard
  reserves its target band immediately. However, egui has already computed widget hits before
  `Shell::frame` runs. If the region map changed and a new press belongs to a region, Fairing
  skips ordinary UI callbacks for that pass and requests a repaint. This prevents dispatch
  to both the region and a stale widget, at the cost of one omitted ordinary-view pass.
  Backend/authorization work and region callbacks continue. A key or other widget outside
  the claimed region is not retroactively given a hit swallowed by the previous geometry;
  this is not an atomic input-rerouting guarantee.
- **Corner knocks respect region ownership.** A press beginning in a gesture region does not
  count toward hidden-entry corner knocks. Moving outside and back does not become a still tap.
  The emergency hold remains global ([03 §9.3](guide/03-chrome.md#93-gesture-regions-of-your-own)).
- **The bottom edge is the nav bar's or the handles'.** With the nav bar on, a bottom gesture
  handle is refused. Turn the bar off to put one there.
- **No image decoder.** The crate draws textures you upload. Decoding is yours
  ([09 §1](guide/09-branding.md)).
- **No CJK font inside.** egui's default fonts have no Hangul. Load a font with
  `ShellBuilder::fonts` ([08](guide/08-troubleshooting.md)).

Found something else? Please open an issue with what you did, what you expected and what
happened ([CONTRIBUTING](../CONTRIBUTING.md)).
