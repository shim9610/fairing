//! Assurance harnesses compile the same acknowledgement kernel and Inbox as the product.

#[path = "../../crates/fairing/src/shell/bool_ack.rs"]
pub mod bool_ack;
#[path = "../../crates/fairing/src/inbox.rs"]
pub mod inbox;

#[cfg(all(feature = "integration", not(fairing_loom)))]
pub mod api;

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
        fn parsed_configuration_satisfies_public_validation(text in if cfg!(miri) { ".{0,64}" } else { ".{0,1024}" }) {
            if let Ok(config) = fairing::ShellConfig::from_toml(&text) {
                prop_assert!(config.validate().is_ok());
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
