# Host API decisions and migration

The ordinary `Shell` is trusted application host code. Its operations configure
the host, while user-driven setting writes still use `set_setting` and protected
payloads still enter the common mediation kernel. `ProtectedShell` has no
conversion to `Shell`, mutable component loan or `request_service` method.

The compiler-indexed [decision ledger](api-decisions.json) records the exported
contracts in the minimal/all-feature inventories. The seven component-mutation
loans have the following replacements.

| Previous API and observed use | Decision and replacement | Reason and compatibility impact |
| --- | --- | --- |
| `desktop_mut().set_wallpaper`, used by branding/custom-chrome examples | `set_wallpaper(wallpaper)` | Keep ownership of textures/painters; wake the host after replacement. Source migration |
| `desktop_mut().set_legibility`, used by label-legibility tests and the demo | `set_desktop_legibility(mode)` | Change only the declared presentation value and wake. Source migration |
| `desktop_mut().set_dock_placement`, used by chrome tests and the demo | `set_dock_placement(placement)` | Keep desktop state private and request layout/redraw. Source migration |
| `desktop_mut().set_page/swipe_to`, used by gesture tests and motion-lab | `set_desktop_page(index)` / `swipe_desktop_to(index, &tokens)` | Retain existing clamping/animation semantics and request repaint. Source migration |
| `desktop_mut().set_slot_painter`, documented as a legacy painter hook | `set_desktop_slot_painter(painter)` | Preserve the explicit ordinary-host painter extension without lending the desktop. Not exposed on protected hosts. Source migration |
| Mutable status slot vectors, used by chrome layout tests | `set_status_items(slot, specs)` | Accept owned configuration, reject empty/duplicate IDs and clear old geometry/measurement caches. Register executable items through `add`. Source migration |
| `status_bar_mut().spec_mut(id)`, used by status-layout tests and the demo | `status_item_spec(id)` then `set_status_item_spec(id, spec)` | A detached copy cannot modify live state; commit rejects missing/mismatched identity and invalidates layout. Source migration |
| Mutable status enabled/height/background/icon/tap fields | `set_status_bar_enabled`, `set_status_bar_height`, `set_status_bar_background`, `set_status_bar_icon`, `set_status_tap_opens_shade` | Narrow operations retain those customization values. Heights update pinned metrics and reject negative/non-finite input. Source migration |
| `nav_bar_mut().style/enabled`, used by keyboard, warning and gesture-handle tests | `set_nav_style(style)` / `set_nav_enabled(enabled)` | The next frame retains its existing warning/layout rules and is explicitly woken. Source migration |
| Mutable navigation height/background | `set_nav_height(height)` / `set_nav_background(role)` | Height uses the metric source of truth; background remains a value change. Source migration |
| Mutable navigation `back_enabled` | No host setter; use `back`, `home`, `launch` | The renderer derives availability from workspace state each frame. A temporary direct assignment never provided a lasting override; no ineffective replacement is added |
| `metrics_spec_mut`, used by typography and keyboard-height tests | `metrics_spec()` returns an owned value; `set_metrics_spec(spec)` commits it | Derived metrics and style are refreshed through the existing theme update. Reading a copy does not change the shell. Source migration |
| Replacing `services_mut().wifi/display`, used by headless/fault tests | `set_wifi_backend`, `set_display_backend`; corresponding setters cover power/Bluetooth/audio/network/info; `set_clock` remains | Attach the new backend's waker before publication, wake the host and discard pending acknowledgements for replaced radio/audio instances. Attachment failure quarantines the shell. Source migration |
| Replacing the complete backend collection | `set_services(services)` | Supports owned custom backend collections without borrowing the live store. Attach before publication and discard old pending acknowledgements. Custom backend implementations retain their own command senders. Source migration |
| Direct service commands, used by service tests, Bluetooth pairing tests and host event handling | `request_service(ServiceRequest::...)` | Explicit synchronous commands preserve the former trusted host authority without a mutable store loan; errors are returned and quarantined shells issue no command. Borrowed credentials are neither retained nor formatted. Source migration |
| `access_mut().set_policy`, used by policy regressions and assurance clients | `set_policy(policy)` | Policies retain their existing narrowing rules and execution-time rechecks; wake after change. Source migration |
| `access_mut().set_subject`, used by identity/timeout regressions | `set_subject(subject)` or queued `handle().set_subject(subject)` | Direct host updates use the shell clock; token mode detaches authority instead of accepting an injected subject. Correlated authentication remains separate. Source migration |
| `osk_mut().set_layout/toggle_lang/toggle/hide`, used by keyboard tests and examples | `set_osk_layout`, `toggle_osk_language`, `toggle_osk`, `hide_osk` | Retain layout/composition/focus rules and request repaint; all four methods require feature `osk`. Source migration |

Caller-owned standalone `DesktopView`, `StatusBar`, `NavBar`, `Access`, `Services`
and keyboard values remain public integration types. Their public fields/methods
are individually indexed in the ledger; constructing one does not borrow a
protected shell's instance. Admitted `Cx` callbacks retain documented raw UI,
application and backend access. No additional protected dispatch entry is added
by these ordinary-host operations.

## Owned configuration example

```rust
use fairing::{Shell, desktop::Wallpaper, theme::ColorRole};
let ctx = fairing::egui::Context::default();
let mut shell = Shell::builder(Default::default()).build(&ctx)?;
shell.set_wallpaper(Wallpaper::Solid(ColorRole::Surface));
assert!(shell.set_nav_height(48.0));
if let Some(mut spec) = shell.status_item_spec("status.clock") {
    spec.min_width = Some(84.0);
    assert!(shell.set_status_item_spec("status.clock", spec));
}
let mut metrics = shell.metrics_spec();
metrics.type_scale[1] = fairing::unit::Span::fixed(fairing::unit::Dim::du(22.0)).pinned();
shell.set_metrics_spec(metrics);
# Ok::<(), fairing::Error>(())
```

The external package consumer exercises these operations against extracted
archives in minimal/default/all feature configurations. Runtime regressions in
`host_operations.rs` check detached edits, persistent heights, rejected identity
changes, stale-geometry invalidation, backend attachment, acknowledgement
ownership and faulted command refusal. Those tests complement the existing
protected revocation, expiry, failed replacement and denied setting-write tests.
