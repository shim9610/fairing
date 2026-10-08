# Icon name mapping (Lucide 1.39.0 → fairing)

Source: [`lucide-static@1.39.0`](https://cdn.jsdelivr.net/npm/lucide-static@1.39.0/icons/), ISC
(some are inherited from Feather and are MIT — `LICENSE-lucide` carries both licence texts in
full).

Files are saved under **our own names**. The constants are `NAMES` in
`crates/fairing/src/icons/builtin.rs`. `cargo xtask icons` uses the file name verbatim as
`IconDef::name`.

Downloading (51 icons, including the ones whose names match):

```sh
curl -sSO "https://cdn.jsdelivr.net/npm/lucide-static@1.39.0/icons/<lucide-name>.svg"
```

| Our name | Lucide file | Notes |
|---|---|---|
| `back` | `arrow-left` | Renamed |
| `home` | `house` | Renamed (`home` → `house` in Lucide 1.x) |
| `recents` | `layers` | Renamed |
| `close` | `x` | Renamed |
| `menu` | `menu` | |
| `more` | `ellipsis` | Renamed |
| `chevron-up` | `chevron-up` | |
| `chevron-down` | `chevron-down` | |
| `chevron-left` | `chevron-left` | |
| `chevron-right` | `chevron-right` | |
| `search` | `search` | |
| `wifi` | `wifi` | The static one. Strength is `icons::parametric::wifi` |
| `wifi-off` | `wifi-off` | |
| `bluetooth` | `bluetooth` | The static one. State is `icons::parametric::bluetooth` |
| `battery` | `battery` | The static one. Level is `icons::parametric::battery` |
| `signal` | `signal` | The static one. Bars are `icons::parametric::signal` |
| `bell` | `bell` | |
| `bell-off` | `bell-off` | |
| `volume` | `volume-2` | Renamed |
| `volume-off` | `volume-x` | Renamed |
| `sun` | `sun` | |
| `moon` | `moon` | |
| `clock` | `clock` | |
| `settings` | `settings` | |
| `power` | `power` | |
| `restart` | `rotate-cw` | Renamed |
| `lock` | `lock` | |
| `unlock` | `lock-open` | Renamed |
| `user` | `user` | |
| `shield` | `shield` | |
| `key` | `key` | |
| `info` | `info` | |
| `warning` | `triangle-alert` | Renamed |
| `error` | `circle-x` | Renamed |
| `check` | `check` | |
| `plus` | `plus` | |
| `minus` | `minus` | |
| `refresh` | `refresh-cw` | Renamed |
| `trash` | `trash-2` | Renamed |
| `edit` | `pencil` | Renamed |
| `gauge` | `gauge` | |
| `chart` | `chart-line` | Renamed |
| `thermometer` | `thermometer` | |
| `camera` | `camera` | |
| `cpu` | `cpu` | |
| `activity` | `activity` | |
| `wrench` | `wrench` | |
| `folder` | `folder` | |
| `display` | `monitor` | Renamed |
| `keyboard` | `keyboard` | |
| `language` | `languages` | Renamed |

## Added in M2 (27 icons)

The first set had 51 icons, and the rest were added in M2. Five of our names have no file of that name in Lucide 1.39.0 (`ethernet`, `sd-card`,
`airplane`, `brightness`, `memory`), so the closest substitute was taken instead.

```sh
curl -sSO "https://cdn.jsdelivr.net/npm/lucide-static@1.39.0/icons/<lucide-name>.svg"
```

| Our name | Lucide file | Notes |
|---|---|---|
| `arrow-up` | `arrow-up` | |
| `arrow-down` | `arrow-down` | |
| `arrow-left` | `arrow-left` | |
| `arrow-right` | `arrow-right` | |
| `split` | `split` | |
| `fullscreen` | `fullscreen` | |
| `minimize` | `minimize` | |
| `ethernet` | `ethernet-port` | Renamed — 1.39.0 has no `ethernet` |
| `usb` | `usb` | |
| `sd-card` | `card-sim` | Renamed — 1.39.0 has no `sd-card`; the clipped card shape is the closest |
| `airplane` | `plane` | Renamed — 1.39.0 has no `airplane` |
| `brightness` | `sun-medium` | Renamed — 1.39.0 has no `brightness`; this one does not collide with `sun`/`moon` |
| `calendar` | `calendar` | |
| `users` | `users` | |
| `download` | `download` | |
| `upload` | `upload` | |
| `save` | `save` | |
| `file` | `file` | |
| `grid` | `grid` | |
| `list` | `list` | |
| `printer` | `printer` | |
| `terminal` | `terminal` | |
| `fan` | `fan` | |
| `plug` | `plug` | |
| `image` | `image` | |
| `memory` | `memory-stick` | Renamed — 1.39.0 has no `memory` |
| `hard-drive` | `hard-drive` | |

That makes 51 + 27 = **78**.

## Checking

```sh
cargo run -p xtask --locked -- icons          # regenerate generated.rs
cargo run -p xtask --locked -- icons --check  # is the checked-in copy current (audit stage 11)
cargo test -p fairing --lib icons             # builtin_names_resolve and friends
```
