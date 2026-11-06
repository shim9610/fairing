//! Assurance harnesses compile the same acknowledgement kernel and Inbox as the product.

#[path = "../../crates/fairing/src/shell/bool_ack.rs"]
pub mod bool_ack;
#[path = "../../crates/fairing/src/inbox.rs"]
pub mod inbox;

#[cfg(all(feature = "integration", not(fairing_loom)))]
pub mod api;

#[cfg(all(feature = "integration", not(fairing_loom)))]
pub mod transitions;

#[cfg(not(fairing_loom))]
#[path = "../../crates/fairing/src/shell/input_route.rs"]
pub mod input_route;

#[cfg(all(feature = "integration", not(fairing_loom), not(miri)))]
pub mod reachability;

/// Exercise queued intent against a backend that acknowledges one command at a time.
pub fn exercise_protocol(events: &[u8]) {
    use bool_ack::{reconcile, Decision};
    let mut reported = false;
    let mut latest = false;
    let mut sent = None;
    let mut was_failed = false;
    let mut failed = false;
    let mut issued = 0usize;
    let mut acknowledged = 0usize;
    for &event in events.iter().take(128) {
        match event % 6 {
            0 | 1 => latest = event % 6 == 1,
            2 => {
                if let Some(target) = sent {
                    reported = target;
                    acknowledged += 1;
                }
                failed = false;
            }
            3 => failed = true,
            4 => failed = false,
            _ => {} // A repeated snapshot must not acknowledge an opposite target.
        }
        if let Some(target) = sent {
            match reconcile(target, latest, was_failed, Some(reported), failed) {
                Decision::Send(next) => {
                    assert_eq!(reported, target, "a second write preceded acknowledgement");
                    assert_eq!(next, latest, "a stale intent was replayed");
                    sent = Some(next);
                    was_failed = failed;
                    issued += 1;
                }
                Decision::Keep(previous_failure) => {
                    assert_ne!(reported, target);
                    assert_eq!(previous_failure, failed);
                    was_failed = previous_failure;
                }
                Decision::Complete => {
                    sent = None;
                    // A newly reported failure cancels the outstanding write.
                    if reported != target {
                        acknowledged += 1;
                    }
                }
            }
        } else if matches!(event % 6, 0 | 1) && latest != reported {
            sent = Some(latest);
            was_failed = failed;
            issued += 1;
        }
        assert!(issued >= acknowledged);
        assert!(
            issued - acknowledged <= 1,
            "multiple outstanding backend commands"
        );
    }
    // Once the final in-flight command is acknowledged, at most one queued target remains.
    if let Some(target) = sent {
        match reconcile(target, latest, was_failed, Some(target), false) {
            Decision::Send(next) => assert_eq!(next, latest),
            Decision::Complete => assert_eq!(target, latest),
            Decision::Keep(_) => panic!("an acknowledged command remained pending"),
        }
    }
}

#[cfg(all(test, not(fairing_loom), not(miri)))]
mod properties {
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]
        #[test]
        fn deferred_intents_never_overtake_acknowledgements(events in prop::collection::vec(any::<u8>(), 0..129)) {
            super::exercise_protocol(&events);
        }

        #[cfg(feature = "integration")]
        #[test]
        fn parsed_configuration_satisfies_public_validation(
            columns in 0u8..=12,
            rows in 0u8..=8,
            height in 1u16..=1000,
            invalid_columns in 13u8..=255,
            invalid_rows in 9u8..=255,
        ) {
            // Independent documented bounds, including automatic and maximum layouts.
            // Every generated case must enter successful parsing and semantic rejection.
            for (columns, rows) in [(0, 0), (12, 8), (columns, rows)] {
                let text = format!("[desktop]\ncolumns = {columns}\nrows = {rows}\n[status_bar]\nheight = {height}\n");
                let parsed = fairing::ShellConfig::from_toml(&text);
                prop_assert!(parsed.is_ok(), "valid structured configuration was rejected");
                let config = parsed.unwrap();
                prop_assert_eq!(config.desktop.columns, columns);
                prop_assert_eq!(config.desktop.rows, rows);
                prop_assert_eq!(config.status_bar.height, Some(f32::from(height)));
            }
            let text = format!("[desktop]\ncolumns = {invalid_columns}\n");
            prop_assert!(fairing::ShellConfig::from_toml(&text).is_err(), "columns above 12 were accepted");
            let text = format!("[desktop]\nrows = {invalid_rows}\n");
            prop_assert!(fairing::ShellConfig::from_toml(&text).is_err(), "rows above 8 were accepted");
            for text in ["[status_bar]\nheight = 0", "[status_bar]\nheight = -1",
                         "[status_bar]\nheight = nan", "[status_bar]\nheight = inf",
                         "[desktop]\ncolumns = 'twelve'", "[desktop"] {
                prop_assert!(fairing::ShellConfig::from_toml(text).is_err(), "invalid configuration was accepted: {}", text);
            }
        }

        #[cfg(feature = "integration")]
        #[test]
        fn malformed_configuration_does_not_echo_secrets(secret in "[a-zA-Z0-9]{32,64}") {
            let text = format!("[wifi]\npsk = \"{secret}");
            let error = fairing::ShellConfig::from_toml(&text).unwrap_err();
            prop_assert!(!error.to_string().contains(&secret));
        }
    }
}

#[cfg(all(test, miri))]
mod interpreted {
    #[test]
    fn queued_intents_and_failures_use_the_real_kernel() {
        for events in [
            vec![],
            vec![1, 0, 1, 2, 2],
            vec![1, 3, 0, 1, 4, 3, 2, 0, 2],
            (0..128).collect(),
        ] {
            super::exercise_protocol(&events);
        }
    }

    #[cfg(feature = "integration")]
    #[test]
    fn actual_configuration_parser_and_redaction() {
        for text in [
            "",
            "[shell]\nlocale = 'en'",
            "[theme]\ndark = true",
            "\u{1f600}",
            "[\n",
            "[wifi]\npsk = \"MIRI_PRIVATE_MARKER",
        ] {
            match fairing::ShellConfig::from_toml(text) {
                Ok(config) => assert!(config.validate().is_ok()),
                Err(error) => assert!(!error.to_string().contains("MIRI_PRIVATE_MARKER")),
            }
        }
    }
}

#[cfg(test)]
mod schedules {
    use super::inbox::Inbox;
    #[cfg(fairing_loom)]
    use loom::thread;
    #[cfg(not(fairing_loom))]
    use std::thread;

    fn scenario() {
        let (a, inbox) = Inbox::pair();
        let b = a.clone();
        let first = thread::spawn(move || {
            assert!(a.send((0, 0)).is_ok());
            assert!(a.send((0, 1)).is_ok());
        });
        let second = thread::spawn(move || {
            assert!(b.send((1, 0)).is_ok());
        });
        let mut received: Vec<_> = inbox.drain().collect();
        first.join().unwrap();
        second.join().unwrap();
        received.extend(inbox.drain());
        assert_eq!(received.len(), 3);
        assert_eq!(received.iter().filter(|m| **m == (0, 0)).count(), 1);
        assert_eq!(received.iter().filter(|m| **m == (0, 1)).count(), 1);
        assert_eq!(received.iter().filter(|m| **m == (1, 0)).count(), 1);
        let positions: Vec<_> = received
            .iter()
            .enumerate()
            .filter(|(_, m)| m.0 == 0)
            .map(|(_, m)| m.1)
            .collect();
        assert_eq!(positions, [0, 1]);
        assert_eq!(inbox.drain().count(), 0);
    }

    #[cfg(not(fairing_loom))]
    #[test]
    fn real_senders_preserve_order_and_lose_no_messages() {
        scenario();
    }

    #[cfg(fairing_loom)]
    fn bounded_scenario(close: bool) {
        use loom::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let (tx, inbox) = Inbox::bounded(1).unwrap();
        let factories = Arc::new(AtomicUsize::new(0));
        let a = tx.clone();
        let f = Arc::clone(&factories);
        let first = thread::spawn(move || {
            a.try_send_with(|| {
                f.fetch_add(1, Ordering::SeqCst);
                0
            })
            .is_ok()
        });
        let b = tx.clone();
        let f = Arc::clone(&factories);
        let second = thread::spawn(move || {
            b.try_send_with(|| {
                f.fetch_add(1, Ordering::SeqCst);
                1
            })
            .is_ok()
        });
        if close {
            let _ = first.join().unwrap();
            let _ = second.join().unwrap();
            // Loom's channel stub miscounts failed sends racing Receiver::drop.
            // Drain before close here; native/Miri cover the actual close race.
            let _ = inbox.drain().count();
            drop(inbox);
            assert!(!tx.stats().receiver_open);
            assert_eq!(tx.stats().outstanding, 0);
            assert_eq!(
                tx.try_send_with(|| panic!("closed factory ran")),
                Err(super::inbox::QueueErrorKind::Disconnected)
            );
        } else {
            let mut received: Vec<_> = inbox.try_recv().ok().into_iter().collect();
            let accepted = usize::from(first.join().unwrap()) + usize::from(second.join().unwrap());
            received.extend(inbox.drain());
            assert_eq!(received.len(), accepted);
            assert_eq!(factories.load(Ordering::SeqCst), accepted);
            assert!(received.len() < 2 || received[0] != received[1]);
            assert_eq!(tx.stats().outstanding, 0);
            assert!(tx.stats().high_water <= 1);
        }
    }

    #[cfg(fairing_loom)]
    #[test]
    fn modeled_bounded_admission_counts_factories_and_releases_received_slots() {
        let mut model = loom::model::Builder::new();
        model.max_threads = 3;
        model.max_branches = 1000;
        model.max_permutations = None;
        model.preemption_bound = None;
        model.max_duration = None;
        model.checkpoint_file = None;
        model.check(|| bounded_scenario(false));
    }

    #[cfg(fairing_loom)]
    #[test]
    fn modeled_close_after_drain_returns_all_reservations() {
        let mut model = loom::model::Builder::new();
        model.max_threads = 3;
        model.max_branches = 1000;
        model.max_permutations = None;
        model.preemption_bound = None;
        model.max_duration = None;
        model.checkpoint_file = None;
        model.check(|| bounded_scenario(true));
    }

    fn concurrent_drain() {
        let (a, inbox) = Inbox::pair();
        let b = a.clone();
        let first = thread::spawn(move || {
            assert!(a.send((0, 0)).is_ok());
            assert!(a.send((0, 1)).is_ok());
        });
        let second = thread::spawn(move || {
            assert!(b.send((1, 0)).is_ok());
        });
        let consumer = thread::spawn(move || {
            let mut early = Vec::new();
            for _ in 0..2 {
                if let Ok(message) = inbox.try_recv() {
                    early.push(message);
                }
            }
            (inbox, early)
        });
        first.join().unwrap();
        second.join().unwrap();
        let (inbox, mut received) = consumer.join().unwrap();
        received.extend(inbox.drain());
        assert_eq!(received.len(), 3);
        assert_eq!(received.iter().filter(|&&m| m == (0, 0)).count(), 1);
        assert_eq!(received.iter().filter(|&&m| m == (0, 1)).count(), 1);
        assert_eq!(received.iter().filter(|&&m| m == (1, 0)).count(), 1);
        let order: Vec<_> = received.iter().filter(|m| m.0 == 0).map(|m| m.1).collect();
        assert_eq!(order, [0, 1]);
        assert!(inbox.try_recv().is_err());
    }

    #[cfg(not(fairing_loom))]
    #[test]
    fn a_consumer_can_drain_while_workers_are_sending() {
        concurrent_drain();
    }

    #[cfg(fairing_loom)]
    #[test]
    fn modeled_concurrent_drain_preserves_each_message_and_sender_order() {
        let mut model = loom::model::Builder::new();
        model.max_threads = 4;
        model.max_branches = 1000;
        model.max_permutations = None;
        model.max_duration = None;
        model.preemption_bound = None;
        model.checkpoint_file = None;
        model.check(concurrent_drain);
    }

    #[cfg(fairing_loom)]
    #[test]
    fn modeled_senders_preserve_order_and_lose_no_messages() {
        let mut model = loom::model::Builder::new();
        model.max_threads = 3;
        model.max_branches = 1000;
        model.max_permutations = None;
        model.max_duration = None;
        model.preemption_bound = None;
        model.checkpoint_file = None;
        model.check(scenario);
    }
}

#[cfg(all(test, not(fairing_loom)))]
mod input_progress {
    #[test]
    fn modal_input_ownership_preempts_regions() {
        for mask in 0u8..8 {
            let overlay_closed = mask & 1 != 0;
            let prompt_open = mask & 2 != 0;
            let overview_open = mask & 4 != 0;
            let capture =
                super::input_route::regions_can_capture(overlay_closed, prompt_open, overview_open);
            if prompt_open || overview_open || !overlay_closed {
                assert!(!capture);
            } else {
                assert!(capture);
            }
        }
    }

    #[test]
    fn any_conflict_history_has_a_quiet_frame_exit() {
        for mask in 0u8..8 {
            let _ =
                super::input_route::defer_ordinary_ui(mask & 1 != 0, mask & 2 != 0, mask & 4 != 0);
            for changed in [false, true] {
                for claimed in [false, true] {
                    assert!(!super::input_route::defer_ordinary_ui(
                        changed, false, claimed
                    ));
                }
            }
        }
    }
}

#[cfg(not(fairing_loom))]
pub mod interaction;

#[cfg(all(test, feature = "integration", not(fairing_loom), not(miri)))]
mod widget_lifecycle;
