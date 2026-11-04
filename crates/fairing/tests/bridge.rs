//! Exercise caller adapters using the public transport traits.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "contract assertions unwrap only deterministic test-adapter results"
)]

use fairing::bridge::{CommandSink, UpdateSource};
use std::rc::Rc;

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Full,
    Closed,
}

// An application adapter used only by these tests. The library supplies no queue.
struct Slot<T> {
    pending: Option<T>,
    closed: bool,
}

impl<T> CommandSink<T> for Slot<T> {
    type Error = Refusal;

    fn try_send(&mut self, command: T) -> Result<(), (T, Self::Error)> {
        if self.closed {
            return Err((command, Refusal::Closed));
        }
        if self.pending.is_some() {
            return Err((command, Refusal::Full));
        }
        self.pending = Some(command);
        Ok(())
    }
}

impl<T> UpdateSource<T> for Slot<T> {
    type Error = Refusal;

    fn try_recv(&mut self) -> Result<Option<T>, Self::Error> {
        if let Some(value) = self.pending.take() {
            Ok(Some(value))
        } else if self.closed {
            Err(Refusal::Closed)
        } else {
            Ok(None)
        }
    }
}

// Deliberately has no Clone or Debug implementation and is not Send or Sync.
struct Payload {
    bytes: Vec<u8>,
    owner: Rc<()>,
}

#[test]
fn trait_objects_move_non_clone_local_payloads_and_preserve_rejected_ownership() {
    let mut port = Slot {
        pending: None,
        closed: false,
    };
    let first = Payload {
        bytes: vec![1, 2, 3],
        owner: Rc::new(()),
    };
    let first_buffer = first.bytes.as_ptr();
    let first_owner = Rc::as_ptr(&first.owner);
    let second = Payload {
        bytes: vec![4, 5, 6],
        owner: Rc::new(()),
    };
    let second_buffer = second.bytes.as_ptr();
    let second_owner = Rc::as_ptr(&second.owner);
    {
        let sink: &mut dyn CommandSink<Payload, Error = Refusal> = &mut port;
        assert!(sink.try_send(first).is_ok());
        let (returned, error) = sink.try_send(second).expect_err("full refusal");
        assert_eq!(error, Refusal::Full);
        assert_eq!(returned.bytes.as_ptr(), second_buffer);
        assert_eq!(Rc::as_ptr(&returned.owner), second_owner);
    }
    let source: &mut dyn UpdateSource<Payload, Error = Refusal> = &mut port;
    let delivered = source.try_recv().expect("receive").expect("pending value");
    assert_eq!(delivered.bytes.as_ptr(), first_buffer);
    assert_eq!(Rc::as_ptr(&delivered.owner), first_owner);
    assert!(source.try_recv().expect("empty receive").is_none());
}

#[test]
fn caller_owned_borrows_do_not_require_static_messages() {
    let mut value = 10;
    {
        let mut port = Slot {
            pending: None,
            closed: false,
        };
        assert!(CommandSink::try_send(&mut port, &mut value).is_ok());
        let received = UpdateSource::try_recv(&mut port)
            .expect("receive")
            .expect("borrow");
        *received = 42;
    }
    assert_eq!(value, 42);
}

#[test]
fn receive_errors_distinguish_empty_and_closed_without_losing_buffered_updates() {
    let mut port = Slot {
        pending: Some(7),
        closed: true,
    };
    assert_eq!(UpdateSource::try_recv(&mut port), Ok(Some(7)));
    assert_eq!(UpdateSource::try_recv(&mut port), Err(Refusal::Closed));
    assert_eq!(CommandSink::try_send(&mut port, 9), Err((9, Refusal::Closed)));
    port.closed = false;
    assert_eq!(UpdateSource::try_recv(&mut port), Ok(None));
}
