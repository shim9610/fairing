//! Runtime ownership and invalidation regressions for narrow host operations.
#![allow(clippy::unwrap_used, reason = "integration-test assertions")]

use fairing::services::{
    Backend, Capabilities, ServiceRequest, ServiceResult, WifiBackend, WifiSnapshot, WifiState,
};
use fairing::testing::{single_level_access, Harness};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn host() -> fairing::Result<Harness> {
    Harness::new(single_level_access(), fairing::Services::null())
}

#[test]
fn owned_metrics_edits_commit_once_and_heights_survive_theme_updates() -> fairing::Result<()> {
    let mut h = host()?;
    h.frames(2);
    let original = h.shell.theme().metrics.type_scale.body;
    let mut spec = h.shell.metrics_spec();
    spec.type_scale[1] =
        fairing::unit::Span::fixed(fairing::unit::Dim::du(original + 9.0)).pinned();
    h.frame();
    assert_eq!(h.shell.theme().metrics.type_scale.body, original);
    h.shell.set_metrics_spec(spec);
    assert!(h.shell.set_status_bar_height(43.0));
    assert!(h.shell.set_nav_height(51.0));
    assert!(!h.shell.set_status_bar_height(f32::NAN));
    assert!(!h.shell.set_nav_height(-1.0));
    h.shell.set_theme_dark(!h.shell.theme().dark);
    h.frames(2);
    assert_eq!(h.shell.theme().metrics.type_scale.body, original + 9.0);
    assert_eq!(h.shell.status_bar().height, 43.0);
    assert_eq!(h.shell.nav_bar().height, 51.0);
    Ok(())
}

#[test]
fn status_updates_reject_identity_collisions_and_invalidate_old_geometry() -> fairing::Result<()> {
    use fairing::chrome::Slot;
    let mut h = host()?;
    h.frames(2);
    let original = h.shell.status_item_spec("status.clock").unwrap();
    assert!(h.shell.status_bar().item_rect("status.clock").is_some());
    let mut changed = original.clone();
    changed.id = "wrong.identity".into();
    assert!(!h.shell.set_status_item_spec("status.clock", changed));
    assert_eq!(
        h.shell.status_item_spec("status.clock"),
        Some(original.clone())
    );
    assert!(!h
        .shell
        .set_status_items(Slot::Left, vec![original.clone(), original.clone()]));
    let current_slot = if h
        .shell
        .status_bar()
        .left
        .iter()
        .any(|x| x.id == original.id)
    {
        Slot::Right
    } else {
        Slot::Left
    };
    assert!(!h
        .shell
        .set_status_items(current_slot, vec![original.clone()]));
    let mut changed = original;
    changed.min_width = Some(180.0);
    assert!(h.shell.set_status_item_spec("status.clock", changed));
    assert!(h.shell.status_bar().item_rect("status.clock").is_none());
    h.frames(2);
    assert!(
        h.shell
            .status_bar()
            .item_rect("status.clock")
            .unwrap()
            .width()
            >= 180.0
    );
    Ok(())
}

struct Radio {
    attaches: Rc<Cell<usize>>,
    commands: Rc<RefCell<Vec<bool>>>,
    scans: Rc<Cell<usize>>,
    panic_attach: bool,
}
impl Backend for Radio {
    fn capabilities(&self) -> Capabilities {
        Capabilities::WIFI
    }
    fn attach(&mut self, waker: fairing::services::Waker) {
        self.attaches.set(self.attaches.get() + 1);
        assert!(!self.panic_attach, "synthetic backend attachment failure");
        waker.wake();
    }
}
impl WifiBackend for Radio {
    fn snapshot(&self) -> WifiSnapshot {
        WifiSnapshot {
            enabled: true,
            state: WifiState::Idle,
            networks: vec![],
            known: vec![],
        }
    }
    fn set_enabled(&mut self, on: bool) -> ServiceResult {
        self.commands.borrow_mut().push(on);
        Ok(())
    }
    fn scan(&mut self) -> ServiceResult {
        self.scans.set(self.scans.get() + 1);
        Ok(())
    }
    fn connect(&mut self, _: &str, _: Option<&str>, _: bool) -> ServiceResult {
        Ok(())
    }
    fn disconnect(&mut self) -> ServiceResult {
        Ok(())
    }
    fn forget(&mut self, _: &str) -> ServiceResult {
        Ok(())
    }
}
#[derive(Clone, Default)]
struct RadioTrace {
    attaches: Rc<Cell<usize>>,
    commands: Rc<RefCell<Vec<bool>>>,
    scans: Rc<Cell<usize>>,
}
fn radio(panic_attach: bool) -> (Radio, RadioTrace) {
    let trace = RadioTrace::default();
    (
        Radio {
            attaches: trace.attaches.clone(),
            commands: trace.commands.clone(),
            scans: trace.scans.clone(),
            panic_attach,
        },
        trace,
    )
}

#[test]
fn backend_replacement_attaches_before_commands_and_forgets_old_acknowledgements(
) -> fairing::Result<()> {
    use fairing::settings::{keys, SettingValue};
    let mut h = host()?;
    let (old, old_trace) = radio(false);
    h.shell.set_wifi_backend(old);
    h.shell
        .set_setting(keys::WIFI_ENABLED.into(), SettingValue::Bool(false));
    assert_eq!(&*old_trace.commands.borrow(), &[false]);
    let (replacement, trace) = radio(false);
    h.shell.set_wifi_backend(replacement);
    assert_eq!(trace.attaches.get(), 1);
    h.shell
        .set_setting(keys::WIFI_ENABLED.into(), SettingValue::Bool(false));
    assert_eq!(
        &*trace.commands.borrow(),
        &[false],
        "old pending work must not absorb a new request"
    );
    h.shell.request_service(ServiceRequest::WifiScan).unwrap();
    assert_eq!(trace.scans.get(), 1);
    h.shell.quarantine();
    assert!(h.shell.request_service(ServiceRequest::WifiScan).is_err());
    assert_eq!(
        trace.scans.get(),
        1,
        "quarantine must block backend requests"
    );
    Ok(())
}

#[test]
fn attachment_failure_quarantines_without_publishing_the_new_backend() -> fairing::Result<()> {
    let mut h = host()?;
    let (replacement, trace) = radio(true);
    h.shell.set_wifi_backend(replacement);
    assert_eq!(trace.attaches.get(), 1);
    assert!(h.shell.is_faulted());
    assert!(
        !h.shell.services().wifi.enabled(),
        "the previous null backend remains installed"
    );
    assert!(h.shell.request_service(ServiceRequest::WifiScan).is_err());
    assert_eq!(trace.scans.get(), 0);
    Ok(())
}
