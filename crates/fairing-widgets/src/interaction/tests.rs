use super::*;
use egui::{Context, Event, Modifiers, PointerButton, RawInput};

fn pass(ctx: &Context, events: Vec<Event>, mut draw: impl FnMut()) {
    ctx.run_ui(
        RawInput {
            events,
            ..RawInput::default()
        },
        |_| draw(),
    )
    .drop_without_applying_deltas();
}

#[test]
fn only_the_mounted_owner_can_renew_its_session() {
    let ctx = Context::default();
    let id = egui::Id::unique("lease");
    pass(&ctx, vec![], || store(&ctx, id, 42u8));
    pass(&ctx, vec![], || {
        assert_eq!(load::<u8>(&ctx, id, true, Lifetime::Mounted), Some(42));
        assert_eq!(load::<u8>(&ctx, id, true, Lifetime::Mounted), Some(42));
    });
    pass(&ctx, vec![], || {
        assert_eq!(
            load::<u8>(&ctx, id, true, Lifetime::Mounted),
            None,
            "a reader renewed an absent owner's lease"
        );
        store(&ctx, id, 7u8);
    });
    for _ in 0..8 {
        pass(&ctx, vec![], || {
            assert_eq!(load::<u8>(&ctx, id, true, Lifetime::Mounted), Some(7));
            store(&ctx, id, 7u8);
        });
    }
}

#[test]
fn disabling_a_session_permanently_retires_its_old_payload() {
    let ctx = Context::default();
    let id = egui::Id::unique("disabled");
    pass(&ctx, vec![], || store(&ctx, id, true));
    pass(&ctx, vec![], || {
        assert_eq!(load::<bool>(&ctx, id, false, Lifetime::Mounted), None);
        assert_eq!(load::<bool>(&ctx, id, true, Lifetime::Mounted), None);
        store(&ctx, id, false);
        assert_eq!(load::<bool>(&ctx, id, true, Lifetime::Mounted), Some(false));
    });
}

#[test]
fn a_fresh_press_retires_capture_but_preserves_a_mounted_region() {
    let ctx = Context::default();
    let press = egui::Id::unique("press");
    let mounted = egui::Id::unique("mounted");
    pass(&ctx, vec![], || {
        store(&ctx, press, 1u8);
        store(&ctx, mounted, 2u8);
    });
    let pos = egui::pos2(20.0, 20.0);
    pass(
        &ctx,
        vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
        || {
            assert_eq!(load::<u8>(&ctx, press, true, Lifetime::Press), None);
            assert_eq!(load::<u8>(&ctx, mounted, true, Lifetime::Mounted), Some(2));
            store(&ctx, press, 3u8);
            assert_eq!(load::<u8>(&ctx, press, true, Lifetime::Press), Some(3));
        },
    );
}

#[test]
fn invalid_sessions_are_removed_without_cloning_their_payload() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Payload(Arc<AtomicUsize>);
    impl Clone for Payload {
        fn clone(&self) -> Self {
            self.0.fetch_add(1, Ordering::Relaxed);
            Self(Arc::clone(&self.0))
        }
    }
    let clones = Arc::new(AtomicUsize::new(0));
    let ctx = Context::default();
    let id = egui::Id::unique("clone");
    pass(&ctx, vec![], || {
        store(&ctx, id, Payload(Arc::clone(&clones)));
    });
    pass(&ctx, vec![], || {});
    pass(&ctx, vec![], || {
        assert!(load::<Payload>(&ctx, id, true, Lifetime::Mounted).is_none());
        assert!(take::<Payload>(&ctx, id).is_none());
    });
    assert_eq!(clones.load(Ordering::Relaxed), 0);
}

#[test]
fn future_and_wrapped_timestamps_never_resurrect_a_session() {
    let ctx = Context::default();
    let id = egui::Id::unique("clock");
    pass(&ctx, vec![], || {
        let now = ctx.cumulative_pass_nr();
        for seen in [now + 1, u64::MAX] {
            ctx.data_mut(|d| d.insert_temp(id, Session { seen, payload: 4u8 }));
            assert_eq!(load::<u8>(&ctx, id, true, Lifetime::Mounted), None);
        }
    });
}

#[test]
fn taking_a_payload_does_not_require_a_default_or_retain_a_copy() {
    #[derive(Clone, Debug, PartialEq)]
    struct Payload(u8);
    let ctx = Context::default();
    let id = egui::Id::unique("take");
    store(&ctx, id, Payload(5));
    assert_eq!(take::<Payload>(&ctx, id), Some(Payload(5)));
    assert_eq!(take::<Payload>(&ctx, id), None);
}
