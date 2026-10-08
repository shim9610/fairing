# fairing-widgets

**Touch widgets, theme tokens and a physical unit system for
[egui](https://github.com/emilk/egui).**

The element layer behind the [fairing](https://crates.io/crates/fairing) touchscreen
shell, usable on its own in any egui app. Every control is sized for a finger on a
real panel: sizes resolve through millimetres and the viewing distance, not pixels.

## What is in it

| Module | What it has |
|---|---|
| `widgets` | Button, icon button, switch, checkbox, radio group, segmented control, slider, stepper, number field, text field, dropdown, wheel picker, chip row, list row, progress bar and ring, meter, status lamp, count badge, media and feature cards, PIN pad, pattern pad |
| `theme` | Palette roles, metric and component tokens, elevation, motion tokens, light and dark presets |
| `unit` | The `du` / `mm` / finger units and the `Scale` that resolves them for a panel |
| `icons` | 78 built-in vector icons, parametric ones (Wi-Fi strength, battery level) and a polyline cache |
| `motion` | Tweens, springs and drag arithmetic |
| `fonts` | Font installation, including the bold face the weight axis needs |

## Using a widget

A widget is drawn with a `WidgetCx`: the theme, the icon set and the animation store,
which your app keeps between frames.

```rust
use fairing_widgets::icons::IconSet;
use fairing_widgets::motion::AnimationStore;
use fairing_widgets::theme::Theme;
use fairing_widgets::widgets::{Stepper, Switch};
use fairing_widgets::WidgetCx;

let theme = Theme::light();
let mut icons = IconSet::new();
let mut anims = AnimationStore::new();
let (mut on, mut count) = (true, 3);

// Inside your egui update, where you have a `ui`:
let mut cx = WidgetCx {
    theme: &theme,
    icons: &mut icons,
    anims: &mut anims,
    anim_scope: egui::Id::new("my-app"),
    frame: 0,
    inset_bottom: 0.0,
    painters: None,
};
if Switch::new(&mut on).show(ui, &mut cx).changed() {
    // `on` was flipped.
}
let _ = Stepper::new(&mut count).show(ui, &mut cx);
```

The same example runs as a doctest on the crate's front page on
[docs.rs](https://docs.rs/fairing-widgets).

## Drawing a widget your way

`painters` takes a `WidgetPainters`: one painter per kind of widget, each optional. A widget
whose kind has one is drawn by it — told its look for the frame: its rects, what it says, its
state and how far each animation has got — and keeps its press, its value and its motion. A kind
without one draws itself. See `fairing_widgets::widgets::WidgetPainters`.

## Dependencies

`egui`, `serde` and `log`. No `unsafe`.

## Licence

MIT, see [`LICENSE`](LICENSE).

The built-in icons are drawn from the geometry of a
[Lucide](https://lucide.dev) 1.39.0 subset, under the ISC licence, with some icons inherited
from Feather under the MIT licence. Both licence texts are in [`LICENSE-lucide`](LICENSE-lucide).
