//! Cross-pane layer identities and input beyond the former 32-slot boundary.
use fairing::testing::{single_level_access, Harness};
use fairing::{screen, Cx, LaunchAction, Shell};
use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

#[derive(Default)]
struct Clicks {
    left: Cell<u32>,
    right: Cell<u32>,
}
struct Fixture {
    h: Harness,
    left: Rc<Cell<egui::Rect>>,
    right: Rc<Cell<egui::Rect>>,
    clicks: Rc<Clicks>,
}
fn setup() -> fairing::Result<Fixture> {
    let left = Rc::new(Cell::new(egui::Rect::NOTHING));
    let right = Rc::new(Cell::new(egui::Rect::NOTHING));
    let clicks = Rc::new(Clicks::default());
    let (left_out, right_out, seen) = (Rc::clone(&left), Rc::clone(&right), Rc::clone(&clicks));
    let mut config = single_level_access();
    config.motion.reduce = false;
    let h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(config)
            .services(fairing::Services::null())
            .build(ctx)?;
        for depth in 0..34 {
            let name = format!("left-{depth}");
            let (out, count) = (Rc::clone(&left_out), Rc::clone(&seen));
            shell.add(screen(
                name.clone(),
                move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                    let response = ui.button(&name);
                    out.set(response.rect);
                    if response.clicked() {
                        count.left.set(count.left.get() + 1);
                    }
                },
            ));
        }
        shell.add(screen("right", move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            let response = ui.button("right");
            right_out.set(response.rect);
            if response.clicked() {
                seen.right.set(seen.right.get() + 1);
            }
        }));
        Ok(shell)
    })?
    .with_size(1200.0, 800.0);
    Ok(Fixture {
        h,
        left,
        right,
        clicks,
    })
}

#[test]
fn deep_stacks_keep_distinct_layers_input_and_clean_transforms() -> fairing::Result<()> {
    let Fixture {
        mut h,
        left,
        right,
        clicks,
    } = setup()?;
    h.frames(2);
    for depth in 0..33 {
        h.shell.launch(LaunchAction::open(format!("left-{depth}")));
        h.run_for(0.5);
    }
    h.shell.launch(LaunchAction::Open {
        id: "right".into(),
        in_other_pane: true,
    });
    h.run_for(0.6);
    let a = h
        .shell
        .workspace()
        .find("left-32")
        .ok_or_else(|| fairing::Error::Config("left stack missing".into()))?
        .layer_id();
    let b = h
        .shell
        .workspace()
        .find("right")
        .ok_or_else(|| fairing::Error::Config("right stack missing".into()))?
        .layer_id();
    assert_ne!(a, b, "deep pane layers must not alias");
    let slots: Vec<_> = h
        .shell
        .workspace()
        .tasks()
        .iter()
        .flat_map(fairing::workspace::Task::iter)
        .map(fairing::workspace::Instance::layer_slot)
        .collect();
    assert_eq!(
        slots.len(),
        slots.iter().copied().collect::<HashSet<_>>().len(),
        "every live pane/depth needs a distinct logical slot"
    );
    assert!(h.texts().iter().any(|(text, _)| text == "left-32"));
    assert!(h.texts().iter().any(|(text, _)| text == "right"));
    h.tap(left.get().center());
    h.frames(2);
    h.tap(right.get().center());
    h.frames(2);
    assert_eq!(
        [clicks.left.get(), clicks.right.get()],
        [1, 1],
        "both panes must accept real input"
    );
    h.tap(left.get().center());
    h.frames(2);
    h.shell.launch(LaunchAction::open("left-33"));
    h.frames(3);
    let incoming = h
        .shell
        .workspace()
        .find("left-33")
        .ok_or_else(|| fairing::Error::Config("incoming screen missing".into()))?
        .layer_id();
    assert_ne!(incoming, b);
    assert!(
        h.ctx.memory(|m| m.areas().is_visible(&incoming)),
        "deep incoming slot must already be warm"
    );
    h.run_for(0.6);
    h.shell.back();
    h.run_for(0.6);
    for layer in [a, b, incoming] {
        assert!(
            h.ctx
                .layer_transform_to_global(layer)
                .is_none_or(|v| v == egui::emath::TSTransform::IDENTITY),
            "completed transitions must reset {layer:?}"
        );
    }
    assert!(!h.shell.is_faulted());
    Ok(())
}
