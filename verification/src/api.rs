//! Executable contracts use public APIs, independently tracking expected effects.

use fairing::access::{
    Access, AccessPolicy, AuthOutcome, Authenticator, Credential, Gate, Level, PinTable, Subject,
};
use fairing::notify::{CenterChange, Notification, NotificationCenter, NotificationId};
use fairing::{action, screen, LaunchAction, SettingValue, Shell, ShellConfig, ShellEvent};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

fn configured() -> ShellConfig {
    let mut cfg = fairing::testing::access_config(&["viewer", "operator", "admin"], Some("top"));
    cfg.motion.reduce = true;
    cfg
}

fn subject(level: u16) -> Subject {
    Subject {
        level: Level(level),
        ..Subject::default()
    }
}

/// Public policy construction, explicit gates, unknown gates, aliases, and the off/single-level skeleton.
pub fn access_contract(bytes: &[u8]) {
    let mut cfg = configured();
    let mode = bytes.first().copied().unwrap_or(0) % 3;
    cfg.access.mode = ["routing", "prompt", "off"][usize::from(mode)].into();
    let single = bytes.get(1).copied().unwrap_or(0) & 1 != 0;
    if single {
        cfg.access.levels.truncate(1);
    }
    cfg.access.default_gate = Some(if mode == 0 { "bottom" } else { "top" }.into());
    cfg.access
        .gates
        .insert("explicit".into(), cfg.access.levels.last().unwrap().clone());
    let mut access = Access::from_config(&cfg.access).unwrap();
    for &byte in bytes.iter().take(64) {
        let held = u16::from(byte % 3);
        access.set_subject(subject(held));
        for name in ["explicit", "unassigned", "日本語", ""] {
            let required = if name == "explicit" || mode != 0 {
                if single {
                    0
                } else {
                    2
                }
            } else {
                0
            };
            assert_eq!(
                access.allows(&Gate::from(name)),
                single || mode == 2 || held >= required
            );
        }
    }
    // Gate assignments resolve real level names before interpreting top/bottom as aliases.
    let mut cfg = configured();
    cfg.access.levels = vec!["top".into(), "middle".into(), "bottom".into()];
    cfg.access.gates.insert("real-top".into(), "top".into());
    let access = Access::from_config(&cfg.access).unwrap();
    assert!(access.allows(&Gate::from("real-top")));
    assert!(!access.allows(&Gate::from("unassigned")));
}

struct SwitchPolicy(Rc<Cell<bool>>);
impl AccessPolicy for SwitchPolicy {
    fn allows(&self, who: &Subject, _: &Gate) -> bool {
        self.0.get() && who.level >= Level(1)
    }
}

/// Every direct/queued action and setting is checked at execution, including policy changes.
pub fn shell_contract(bytes: &[u8]) {
    let ctx = fairing::egui::Context::default();
    let mut cfg = configured();
    cfg.access.mode = "routing".into();
    let mut shell = Shell::builder(cfg).build(&ctx).unwrap();
    let allowed = Rc::new(Cell::new(true));
    shell
        .set_policy(Box::new(SwitchPolicy(Rc::clone(&allowed))));
    let runs = Rc::new(Cell::new(0usize));
    let count = Rc::clone(&runs);
    shell.add(action("guarded", move |_| count.set(count.get() + 1)));
    shell.add(screen("guarded-screen", |_, _| {}));
    let mut held = 0;
    let mut expected_runs = 0;
    let mut expected_setting = None;
    for &byte in bytes.iter().take(32) {
        let next = u16::from((byte / 10) % 3);
        let passes = held >= 1 && allowed.get();
        match byte % 10 {
            0 => {
                shell.set_subject(subject(next));
                held = next;
            }
            1 => {
                shell.handle().set_subject(subject(next));
                held = next;
            }
            2 | 3 => {
                if passes {
                    expected_runs += 1;
                }
                if byte % 10 == 2 {
                    shell.launch(LaunchAction::run("guarded"));
                } else {
                    shell.handle().launch(LaunchAction::run("guarded"));
                }
            }
            4 | 5 => {
                let value = SettingValue::Int(i64::from(byte));
                if passes {
                    expected_setting = Some(value.clone());
                }
                if byte % 10 == 4 {
                    shell.set_setting("private.value".into(), value);
                } else {
                    shell.handle().set_setting("private.value", value);
                }
            }
            6 => {
                shell.launch(LaunchAction::open("guarded-screen"));
            }
            7 => allowed.set(!allowed.get()),
            8 | 9 => {
                // The same sender's authentication/launch order has observable security meaning.
                if byte % 10 == 8 {
                    shell.handle().set_subject(subject(next));
                    shell.handle().launch(LaunchAction::run("guarded"));
                    if next >= 1 && allowed.get() {
                        expected_runs += 1;
                    }
                } else {
                    shell.handle().launch(LaunchAction::run("guarded"));
                    shell.handle().set_subject(subject(next));
                    if passes {
                        expected_runs += 1;
                    }
                }
                held = next;
            }
            _ => unreachable!(),
        }
        shell.logic(&ctx);
        assert_eq!(
            runs.get(),
            expected_runs,
            "a direct/queued action bypassed its gate or ran twice"
        );
        assert_eq!(
            shell.settings().get(&"private.value".into()),
            expected_setting.as_ref(),
            "a denied setting changed memory"
        );
        assert_eq!(shell.access().session().subject.level, Level(held));
        let events = shell.poll_events();
        if byte % 10 == 6 && !passes {
            assert!(!events
                .iter()
                .any(|e| matches!(e, ShellEvent::ScreenOpened { .. })));
        }
        if held == 0 || !allowed.get() {
            assert!(
                shell.workspace().is_home(),
                "a revoked screen remained open"
            );
        }
    }
    let handle = shell.handle();
    drop(shell);
    assert!(
        !handle.send(fairing::shell::Command::Home),
        "a disconnected ShellHandle reported success"
    );
}

/// Independent failure/lockout model, mixing PIN, pattern, reopening and refused grants.
pub fn authentication_contract(bytes: &[u8]) {
    let mut cfg = configured();
    cfg.access
        .pin_table
        .pins
        .insert("operator".into(), "7391".into());
    cfg.access.pin_table.attempt_limit = Some(3);
    cfg.access.pin_table.lock_secs = Some(2);
    cfg.access
        .pattern_table
        .patterns
        .insert("admin".into(), "1-2-3-6".into());
    cfg.access.pattern_table.attempt_limit = Some(4);
    cfg.access.pattern_table.lock_secs = Some(3);
    let levels = fairing::access::LevelTable::from_names(&cfg.access.levels);
    let mut table =
        PinTable::from_tables(&cfg.access.pin_table, &cfg.access.pattern_table, &levels).unwrap();
    let start = Instant::now();
    let mut seconds = 0;
    let mut failures = 0;
    let mut cleared = 0;
    let mut locked_until = None;
    let mut last_granted = false;
    for &byte in bytes.iter().take(64) {
        seconds += u64::from(byte / 32);
        let now = start + Duration::from_secs(seconds);
        let op = byte % 6;
        if op == 5 {
            table.cancel();
            // PinTable uses the default no-op begin hook. Shell challenge changes
            // are exercised through the request-bearing Reader fixture below.
            continue;
        }
        let outcome = if op == 4 {
            if !last_granted {
                continue;
            }
            last_granted = false;
            failures += cleared;
            cleared = 0;
            failures += 1;
            let outcome = table.refused(now);
            if failures >= 3 {
                failures = 0;
                locked_until = Some(seconds + 3);
                assert!(
                    matches!(outcome, Some(AuthOutcome::Locked { until, .. }) if until == now + Duration::from_secs(3))
                );
            } else {
                assert!(outcome.is_none());
            }
            continue;
        } else {
            cleared = 0;
            let credential = match op {
                0 => Credential::Pin("0000".into()),
                1 => Credential::Pin("7391".into()),
                2 => Credential::Pattern(vec![8, 7, 6, 3]),
                _ => Credential::Pattern(vec![0, 1, 2, 5]),
            };
            table.check(credential, now)
        };
        last_granted = false;
        if let Some(until) = locked_until.filter(|&until| seconds < until) {
            assert!(
                matches!(outcome, AuthOutcome::Locked { until: actual, .. } if actual == start + Duration::from_secs(until))
            );
            continue;
        }
        locked_until = None;
        if op == 1 || op == 3 {
            assert!(
                matches!(outcome, AuthOutcome::Granted(ref s) if s.level == Level(if op == 1 { 1 } else { 2 }))
            );
            cleared = failures;
            failures = 0;
            last_granted = true;
        } else {
            failures += 1;
            if failures >= 3 {
                locked_until = Some(seconds + 3);
                failures = 0;
                assert!(
                    matches!(outcome, AuthOutcome::Locked { until, .. } if until == now + Duration::from_secs(3))
                );
            } else {
                assert!(matches!(outcome, AuthOutcome::Denied { .. }));
            }
        }
    }
}

/// Notification updates must preserve deduplication, persistence, grouping, caps and gate redaction.
pub fn notification_contract(bytes: &[u8]) {
    let cap = usize::from(bytes.first().copied().unwrap_or(0) % 4 + 1);
    let mut center = NotificationCenter::new(cap);
    let mut access = Access::from_config(&configured().access).unwrap();
    for &byte in bytes.iter().skip(1).take(64) {
        let id = NotificationId(u64::from(byte % 8));
        match byte % 4 {
            0 => {
                let old = center.get(id).is_some();
                let mut n = Notification::new(id, "private title")
                    .source(format!("source-{}", byte / 16 % 3));
                n.persistent = byte & 8 != 0;
                n.gate = Some(Gate::from("private"));
                let full_protected =
                    center.len() == cap && center.iter().all(|item| item.persistent);
                assert_eq!(
                    center.push(n),
                    if old {
                        CenterChange::Updated
                    } else if full_protected {
                        CenterChange::Rejected
                    } else {
                        CenterChange::Added
                    }
                );
            }
            1 => {
                let removable = center.get(id).is_some_and(|n| !n.persistent);
                assert_eq!(center.dismiss(id), removable);
            }
            2 => center.clear(),
            _ => {
                access.set_subject(subject(u16::from(byte / 8 % 3)));
            }
        }
        let ids: BTreeSet<_> = center.iter().map(|n| n.id).collect();
        assert_eq!(ids.len(), center.len());
        assert!(center.unread() <= center.len());
        assert!(
            center.len() <= cap,
            "notification retention exceeded hard cap"
        );
        let mut groups = BTreeSet::new();
        for (source, items) in center.groups() {
            assert!(groups.insert(source));
            assert!(items.iter().all(|n| n.source == source));
        }
        assert_eq!(
            center.hidden(&access),
            if access.session().subject.level < Level(2) {
                center.len()
            } else {
                0
            }
        );
    }
}

/// Optional display queues are bounded and refusal preserves previously accepted work.
pub fn display_contract(bytes: &[u8]) {
    use fairing::notify::{AdmissionError, Toast, ToastQueue};
    let capacity = usize::from(bytes.first().copied().unwrap_or(0) % 4);
    let mut queue = ToastQueue::new(1, Duration::from_millis(100)).with_pending_capacity(capacity);
    let theme = fairing::Theme::default();
    let start = Instant::now();
    for (step, &byte) in bytes.iter().take(64).enumerate() {
        match byte % 4 {
            0 | 1 => {
                let pending = queue.pending();
                let visible = queue.visible().len();
                let text = ["a", "b", "c", "d"][usize::from(byte / 4 % 4)];
                if let Err(error) = queue.try_push_text(text) {
                    assert_eq!(error, AdmissionError::Full);
                    assert_eq!(pending, capacity);
                    assert_eq!(queue.pending(), pending);
                    assert_eq!(queue.visible().len(), visible);
                }
            }
            2 => {
                let _ = queue.tick(
                    0.1,
                    start + Duration::from_millis(step as u64 * 100),
                    &theme.motion,
                    theme.components.toast,
                );
            }
            _ => queue.dismiss(0, &theme.motion),
        }
        assert!(
            queue.pending() <= capacity,
            "toast retention exceeded hard cap"
        );
        assert!(queue.visible().len() <= 1);
    }
    let oversized = "x".repeat(fairing::notify::MAX_TOAST_TEXT_BYTES + 1);
    assert_eq!(Toast::try_new(&oversized), Err(AdmissionError::TooLarge));
}

/// Date conversion is checked by an inverse Gregorian calculation in wide arithmetic.
pub fn time_contract(utc_secs: u64, offset_min: i32) {
    let time = fairing::time::WallTime {
        utc_secs,
        offset_min,
    };
    let expected = (i128::from(utc_secs.min(i64::MAX as u64)) + i128::from(offset_min) * 60)
        .clamp(i128::from(i64::MIN), i128::from(i64::MAX));
    assert_eq!(i128::from(time.local_secs()), expected);
    let c = time.civil();
    assert!((1..=12).contains(&c.month));
    let leap = c.year % 4 == 0 && (c.year % 100 != 0 || c.year % 400 == 0);
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    assert!((1..=days[usize::from(c.month - 1)]).contains(&c.day));
    assert!(c.hour < 24 && c.minute < 60 && c.second < 60);
    let year = i128::from(c.year) - i128::from(c.month <= 2);
    let before_year = 365 * year + year.div_euclid(4) - year.div_euclid(100) + year.div_euclid(400);
    let month = i128::from(c.month) + if c.month > 2 { -3 } else { 9 };
    let days_since_epoch = before_year + (153 * month + 2) / 5 + i128::from(c.day) - 1 - 719468;
    assert_eq!(
        days_since_epoch * 86400
            + i128::from(c.hour) * 3600
            + i128::from(c.minute) * 60
            + i128::from(c.second),
        expected
    );
    use fairing::time::ClockFormat::*;
    for format in [Hm, Hms, Hm12, Hms12, DateHm, DateHm12] {
        let changed = format.with_hour12(!format.hour12());
        assert_eq!(changed.shows_seconds(), format.shows_seconds());
        assert_eq!(changed.with_hour12(format.hour12()), format);
        let mut output = "old secret".to_owned();
        time.format_into(&mut output, format);
        assert_eq!(output, time.format(format));
        assert!(!output.contains("old secret"));
    }
}

/// Services forwards one timestamp exactly once to every backend, then takes the earliest deadline.
pub fn services_contract(delays: [Option<u16>; 2]) {
    use fairing::services::{Backend, Services};
    struct Timer<const ID: u8> {
        deadline: Option<Instant>,
        seen: Rc<RefCell<Vec<Instant>>>,
    }
    impl<const ID: u8> Backend for Timer<ID> {
        fn poll_at(&mut self, now: Instant) {
            self.seen.borrow_mut().push(now);
        }
        fn next_wake(&self) -> Option<Instant> {
            self.deadline
        }
    }
    let now = Instant::now();
    let deadlines = delays.map(|d| d.map(|d| now + Duration::from_millis(u64::from(d))));
    let a = Rc::new(RefCell::new(Vec::new()));
    let b = Rc::new(RefCell::new(Vec::new()));
    let mut services = Services::builder()
        .clock(fairing::services::null::NullClock)
        .custom(Timer::<0> {
            deadline: deadlines[0],
            seen: Rc::clone(&a),
        })
        .custom(Timer::<1> {
            deadline: deadlines[1],
            seen: Rc::clone(&b),
        })
        .build();
    assert_eq!(services.poll(now), deadlines.into_iter().flatten().min());
    assert_eq!(&*a.borrow(), &[now]);
    assert_eq!(&*b.borrow(), &[now]);
    assert!(services.custom::<Timer<0>>().is_some());
    assert!(services.custom::<Timer<1>>().is_some());
}

/// The widget API keeps physical terms unchanged when only the fractional reference changes.
pub fn units_contract(bits: u32, width: u16, height: u16) {
    use fairing::unit::{round_i8, round_u8, Dim, Scale};
    let value = f32::from_bits(bits);
    let reference = f64::from(value).round();
    assert_eq!(round_u8(value), reference.clamp(0.0, 255.0) as u8);
    assert_eq!(round_i8(value), reference.clamp(-127.0, 127.0) as i8);
    let scale = Scale::identity();
    let inner = scale.in_container(fairing::egui::Rect::from_min_size(
        fairing::egui::Pos2::ZERO,
        fairing::egui::vec2(f32::from(width), f32::from(height)),
    ));
    assert_eq!(inner.pixels_per_point, scale.pixels_per_point);
    assert_eq!(Dim::mm(12.0).resolve(&scale), Dim::mm(12.0).resolve(&inner));
    assert_eq!(
        Dim::finger(1.0).resolve(&scale),
        Dim::finger(1.0).resolve(&inner)
    );
    assert_eq!(
        Dim::text(1.0).resolve(&scale),
        Dim::text(1.0).resolve(&inner)
    );
    assert_eq!(Dim::frac_w(0.5).resolve(&inner), f32::from(width) * 0.5);
    let no_pixels = Scale {
        pixels_per_point: 0.0,
        ..scale
    };
    assert_eq!(
        Dim::du(5.0).resolve(&no_pixels),
        5.0,
        "an unused pixel term introduced NaN"
    );
}

/// Public parsing rejects non-finite/out-of-range ratios and grid sizes without printing credentials.
pub fn configuration_contract(bits: u32, columns: u8) {
    let ratio = f32::from_bits(bits);
    let text = format!("[motion]\nsnap_ratio = {ratio:?}\n");
    assert_eq!(
        ShellConfig::from_toml(&text).is_ok(),
        ratio.is_finite() && (0.0..=1.0).contains(&ratio)
    );
    let text = format!("[desktop]\ncolumns = {columns}\n");
    assert_eq!(ShellConfig::from_toml(&text).is_ok(), columns <= 12);
    let mut cfg = configured();
    let secret = format!("{:06}", bits % 900000 + 100000);
    cfg.access
        .pin_table
        .pins
        .insert("operator".into(), secret.clone());
    cfg.access
        .pin_table
        .pins
        .insert("admin".into(), secret.clone());
    let error = Access::from_config(&cfg.access).unwrap_err();
    assert!(!format!("{error:?}").contains(&secret));
    assert!(!format!("{:?}", cfg.access.pin_table).contains(&secret));
    for bad in ["", "missing"] {
        cfg.access.initial = Some(bad.into());
        assert!(Access::from_config(&cfg.access).is_err());
    }
}

/// Actual shell-to-backend writes follow accepted intent, never a second unacknowledged write.
pub fn backend_contract(bytes: &[u8]) {
    use fairing::services::{Backend, ServiceResult, Services, WifiBackend, WifiSnapshot};
    struct Wifi {
        reported: Rc<Cell<bool>>,
        outstanding: Rc<Cell<Option<bool>>>,
        writes: Rc<RefCell<Vec<bool>>>,
    }
    impl Backend for Wifi {}
    impl WifiBackend for Wifi {
        fn snapshot(&self) -> WifiSnapshot {
            WifiSnapshot {
                enabled: self.reported.get(),
                ..WifiSnapshot::default()
            }
        }
        fn set_enabled(&mut self, on: bool) -> ServiceResult {
            assert!(
                self.outstanding.replace(Some(on)).is_none(),
                "a backend received overlapping writes"
            );
            self.writes.borrow_mut().push(on);
            Ok(())
        }
        fn scan(&mut self) -> ServiceResult {
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
    let reported = Rc::new(Cell::new(false));
    let outstanding = Rc::new(Cell::new(None));
    let writes = Rc::new(RefCell::new(Vec::new()));
    let services = Services::builder()
        .wifi(Wifi {
            reported: Rc::clone(&reported),
            outstanding: Rc::clone(&outstanding),
            writes: Rc::clone(&writes),
        })
        .build();
    let mut cfg = configured();
    cfg.access.mode = "routing".into();
    cfg.access
        .gates
        .insert("wifi.enabled".into(), "operator".into());
    let ctx = fairing::egui::Context::default();
    let mut shell = Shell::builder(cfg).services(services).build(&ctx).unwrap();
    let mut authorized = false;
    let mut desired = false;
    let mut in_flight = None;
    let mut expected_writes = Vec::new();
    for &byte in bytes.iter().take(32) {
        match byte % 6 {
            0..=2 => {
                if authorized {
                    desired = match byte % 6 {
                        0 => false,
                        1 => true,
                        _ => !desired,
                    };
                    if in_flight.is_none() && desired != reported.get() {
                        in_flight = Some(desired);
                        expected_writes.push(desired);
                    }
                }
                let launch = if byte % 6 == 2 {
                    LaunchAction::Toggle("wifi.enabled".into())
                } else {
                    LaunchAction::Set("wifi.enabled".into(), SettingValue::Bool(byte % 6 == 1))
                };
                shell.handle().launch(launch);
            }
            3 => {
                if let Some(target) = in_flight.take() {
                    assert_eq!(outstanding.take(), Some(target));
                    reported.set(target);
                    if desired != target {
                        in_flight = Some(desired);
                        expected_writes.push(desired);
                    }
                }
            }
            4 | 5 => {
                authorized = byte % 6 == 5;
                shell.handle().set_subject(subject(u16::from(authorized)));
            }
            _ => unreachable!(),
        }
        shell.logic(&ctx);
        assert_eq!(
            &*writes.borrow(),
            &expected_writes,
            "backend writes diverged from authorized, coalesced intent"
        );
        assert_eq!(outstanding.get(), in_flight);
        let _ = shell.poll_events();
    }
}

struct Reader {
    request: Option<fairing::access::AuthRequestId>,
    answer: Rc<RefCell<Option<AuthOutcome>>>,
    calls: Rc<RefCell<Vec<&'static str>>>,
}
impl Authenticator for Reader {
    fn methods(&self) -> Vec<fairing::access::AuthMethod> {
        vec![fairing::access::AuthMethod::External {
            label: "Reader".into(),
        }]
    }
    fn submit(
        &mut self,
        request: fairing::access::AuthRequestId,
        _: Credential,
        _: Instant,
    ) -> AuthOutcome {
        self.request = Some(request);
        AuthOutcome::Pending
    }
    fn begin(&mut self, request: fairing::access::AuthRequestId, _: &Gate, _: Instant) {
        self.request = Some(request);
        self.calls.borrow_mut().push("begin");
        self.answer.borrow_mut().take();
    }
    fn cancel(&mut self) {
        self.calls.borrow_mut().push("cancel");
        self.answer.borrow_mut().take();
    }
    fn refused(&mut self, _: Instant) -> Option<AuthOutcome> {
        self.calls.borrow_mut().push("refused");
        None
    }
    fn poll(&mut self, _: Instant) -> Option<fairing::access::AuthResponse> {
        let request = self.request?;
        let outcome = self.answer.borrow_mut().take();
        outcome.map(|outcome| request.respond(outcome))
    }
}

/// Exercise the public shell grant path, including actual table membership and continuation gates.
pub fn grant_contract(held: u16, candidate: u16, lock: bool, policy_passes: bool) {
    let mut cfg = configured();
    let held = held % 3;
    cfg.access.initial = Some(cfg.access.levels[usize::from(held)].clone());
    let ctx = fairing::egui::Context::default();
    let answer = Rc::new(RefCell::new(None));
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut shell = Shell::builder(cfg)
        .authenticator(Reader {
            request: None,
            answer: Rc::clone(&answer),
            calls: Rc::clone(&calls),
        })
        .build(&ctx)
        .unwrap();
    let allowed = Rc::new(Cell::new(false));
    shell
        .set_policy(Box::new(SwitchPolicy(Rc::clone(&allowed))));
    let runs = Rc::new(Cell::new(0));
    let count = Rc::clone(&runs);
    shell.add(action("guarded", move |_| count.set(count.get() + 1)));
    if lock {
        shell.launch(LaunchAction::Lock);
    }
    shell
        .handle()
        .request_unlock("guarded", Some(LaunchAction::run("guarded")));
    shell.logic(&ctx);
    assert_eq!(calls.borrow().first(), Some(&"begin"));
    allowed.set(policy_passes);
    *answer.borrow_mut() = Some(AuthOutcome::Granted(subject(candidate)));
    shell.logic(&ctx);
    let accepted = candidate < 3 && (candidate > held || (lock && candidate == held));
    assert_eq!(
        shell.access().session().subject.level,
        Level(if accepted { candidate } else { held })
    );
    assert_eq!(
        runs.get(),
        usize::from(accepted && policy_passes && candidate >= 1),
        "an authentication continuation skipped its own launch gate"
    );
    assert_eq!(calls.borrow().contains(&"refused"), !accepted);
}

/// Session transitions cancel an outstanding reader before a stale result can be polled.
pub fn cancellation_contract(boundary: u8) {
    let ctx = fairing::egui::Context::default();
    let answer = Rc::new(RefCell::new(None));
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut shell = Shell::builder(configured())
        .authenticator(Reader {
            request: None,
            answer: Rc::clone(&answer),
            calls: Rc::clone(&calls),
        })
        .build(&ctx)
        .unwrap();
    let runs = Rc::new(Cell::new(0));
    let count = Rc::clone(&runs);
    shell.add(action("guarded", move |_| count.set(count.get() + 1)));
    shell
        .handle()
        .request_unlock("first", Some(LaunchAction::run("guarded")));
    shell.logic(&ctx);
    *answer.borrow_mut() = Some(AuthOutcome::Pending);
    shell.logic(&ctx);
    *answer.borrow_mut() = Some(AuthOutcome::Granted(subject(2)));
    match boundary % 5 {
        0 => shell.handle().logout(),
        1 => shell.handle().set_subject(Subject {
            id: Some("replacement".into()),
            ..subject(0)
        }),
        2 => shell
            .handle()
            .request_unlock("second", Some(LaunchAction::run("guarded"))),
        3 => shell.handle().launch(LaunchAction::Lock),
        _ => shell.handle().back(),
    }
    shell.logic(&ctx);
    assert_eq!(runs.get(), 0);
    assert_eq!(shell.access().session().subject.level, Level(0));
    let calls = calls.borrow();
    assert!(calls.contains(&"cancel"));
    if matches!(boundary % 5, 2 | 3) {
        assert_eq!(&calls[calls.len() - 2..], &["cancel", "begin"]);
    }
}

/// Virtual input time checks just before, at, and after a timeout without sleeping.
pub fn expiry_contract(seconds: u8, boundary: u8, input: bool, kind: u8) {
    let mut cfg = configured();
    let seconds = u64::from(seconds % 5 + 1);
    let kind = kind % 3;
    cfg.access.session_timeout_secs = if kind == 0 { seconds } else { 0 };
    cfg.access.idle_lock_secs = if kind == 1 { seconds } else { 0 };
    cfg.access.temporary_secs = seconds;
    cfg.access.mode = "routing".into();
    if kind == 2 {
        cfg.access.mode = "prompt".into();
    }
    let ctx = fairing::egui::Context::default();
    let answer = Rc::new(RefCell::new(None));
    let mut shell = Shell::builder(cfg)
        .authenticator(Reader {
            request: None,
            answer: Rc::clone(&answer),
            calls: Rc::new(RefCell::new(Vec::new())),
        })
        .build(&ctx)
        .unwrap();
    let runs = Rc::new(Cell::new(0));
    let count = Rc::clone(&runs);
    shell.add(action("guarded", move |_| count.set(count.get() + 1)));
    shell.add(screen("guarded-screen", |_, _| {}));
    if kind == 2 {
        shell.handle().request_unlock("seed", None);
    } else {
        shell.handle().set_subject(subject(2));
    }
    let out = ctx.run_ui(
        fairing::egui::RawInput {
            time: Some(0.0),
            ..Default::default()
        },
        |ui| shell.frame(ui),
    );
    out.drop_without_applying_deltas();
    if kind == 2 {
        *answer.borrow_mut() = Some(AuthOutcome::Granted(subject(2)));
        let out = ctx.run_ui(
            fairing::egui::RawInput {
                time: Some(0.0),
                ..Default::default()
            },
            |ui| shell.frame(ui),
        );
        out.drop_without_applying_deltas();
    }
    let start = shell.now();
    shell.launch(LaunchAction::open("guarded-screen"));
    shell.handle().launch(LaunchAction::run("guarded"));
    shell
        .handle()
        .set_setting("private.value", SettingValue::Int(42));
    let boundary = boundary % 3;
    let elapsed = seconds as f64 + [-0.001, 0.0, 0.001][usize::from(boundary)];
    let out = ctx.run_ui(
        fairing::egui::RawInput {
            time: Some(elapsed),
            events: if input {
                vec![fairing::egui::Event::Text("fresh input".into())]
            } else {
                vec![]
            },
            ..Default::default()
        },
        |ui| shell.frame(ui),
    );
    out.drop_without_applying_deltas();
    // The UI path represents its interval as f32; compare within its rounding precision.
    assert!(
        shell
            .now()
            .duration_since(start)
            .abs_diff(Duration::from_secs_f64(elapsed))
            < Duration::from_micros(1)
    );
    let expired = boundary != 0;
    assert_eq!(
        shell.access().session().subject.level,
        Level(if expired { 0 } else { 2 })
    );
    assert_eq!(
        runs.get(),
        usize::from(!expired),
        "wake input/commands renewed expired privilege"
    );
    assert_eq!(
        shell.settings().get(&"private.value".into()).is_some(),
        !expired
    );
    if expired {
        assert!(shell.workspace().is_home());
    }
}

/// Bounded admission and inline pool handles compared with independent FIFO/live-key models.
pub fn memory_contract(bytes: &[u8]) {
    use fairing::{
        inbox::{Inbox, QueueErrorKind, TryBuildSendError},
        memory::{FixedPool, PoolKey},
    };
    use std::collections::VecDeque;
    let capacity = 1 + usize::from(bytes.first().copied().unwrap_or(0) % 8);
    let (tx, rx) = Inbox::bounded(capacity).unwrap();
    let mut queued = VecDeque::new();
    let mut pool = FixedPool::<u8, 3>::new().unwrap();
    let mut other = FixedPool::<u8, 1>::new().unwrap();
    let foreign = other.try_insert(77).unwrap();
    let mut live: Vec<(PoolKey, u8)> = Vec::new();
    let mut stale = Vec::new();
    let mut rejected = 0;
    for &byte in bytes.iter().take(64) {
        match byte % 8 {
            0 | 1 => {
                let ran = Cell::new(false);
                let result = tx.clone().try_send_with(|| {
                    ran.set(true);
                    byte
                });
                if queued.len() == capacity {
                    rejected += 1;
                    assert_eq!(result, Err(QueueErrorKind::Full));
                    assert!(!ran.get(), "a rejected factory allocated/ran");
                    let error = tx.try_send(byte).unwrap_err();
                    rejected += 1;
                    assert_eq!(error.kind(), QueueErrorKind::Full);
                    assert_eq!(error.into_inner(), byte);
                } else {
                    result.unwrap();
                    assert!(ran.get());
                    queued.push_back(byte);
                }
            }
            2 => assert_eq!(rx.try_recv().ok(), queued.pop_front()),
            3 => {
                let result = pool.try_insert(byte);
                if live.len() == 3 {
                    assert_eq!(result, Err(byte));
                } else {
                    live.push((result.unwrap(), byte));
                }
            }
            4 => {
                if !live.is_empty() {
                    let (key, value) = live.remove(usize::from(byte) % live.len());
                    assert_eq!(pool.remove(key), Some(value));
                    assert!(pool.remove(key).is_none());
                    stale.push(key);
                }
            }
            5 => {
                stale.extend(live.drain(..).map(|(key, _)| key));
                pool.clear();
            }
            6 => {
                if let Some((key, value)) = live.first_mut() {
                    *pool.get_mut(*key).unwrap() = byte;
                    *value = byte;
                }
            }
            _ => {
                let ran = Cell::new(false);
                let result = tx.try_send_fallible_with(|| {
                    ran.set(true);
                    Err::<u8, _>(byte)
                });
                if queued.len() == capacity {
                    rejected += 1;
                    assert!(matches!(
                        result,
                        Err(TryBuildSendError::Queue(QueueErrorKind::Full))
                    ));
                    assert!(!ran.get());
                } else {
                    assert!(
                        matches!(result, Err(TryBuildSendError::Build(value)) if value == byte)
                    );
                    assert!(ran.get());
                }
            }
        }
        assert_eq!(tx.stats().outstanding, queued.len());
        assert_eq!(tx.stats().rejected_full, rejected);
        assert!(tx.stats().high_water <= capacity);
        assert_eq!(pool.len(), live.len());
        assert_eq!(pool.available() + pool.len(), 3);
        for &(key, value) in &live {
            assert_eq!(pool.get(key), Some(&value));
        }
        for &key in &stale {
            assert!(pool.get(key).is_none(), "a stale handle revived");
        }
        assert!(pool.get(foreign).is_none());
    }
    assert_eq!(
        rx.drain().collect::<Vec<_>>(),
        queued.into_iter().collect::<Vec<_>>()
    );
    drop(rx);
    assert_eq!(tx.stats().outstanding, 0);
    assert_eq!(
        tx.try_send_with(|| panic!("closed factory ran")),
        Err(QueueErrorKind::Disconnected)
    );
}

#[cfg(all(test, not(miri)))]
mod generated {
    use proptest::prelude::*;
    proptest! {
        #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]
        #[test] fn public_memory(bytes in prop::collection::vec(any::<u8>(), 0..65)) { super::memory_contract(&bytes); }
        #[test] fn public_access(bytes in prop::collection::vec(any::<u8>(), 1..65)) { super::access_contract(&bytes); }
        #[test] fn public_authentication(bytes in prop::collection::vec(any::<u8>(), 0..65)) { super::authentication_contract(&bytes); }
        #[test] fn public_display(bytes in prop::collection::vec(any::<u8>(), 0..65)) { super::display_contract(&bytes); }
        #[test] fn public_bounded_display(bytes in prop::collection::vec(any::<u8>(), 0..73)) { super::bounded_display_contract(&bytes); }
        #[test] fn public_notifications(bytes in prop::collection::vec(any::<u8>(), 1..65)) { super::notification_contract(&bytes); }
        #[test] fn public_calendar(utc in any::<u64>(), offset in any::<i32>()) { super::time_contract(utc, offset); }
        #[test] fn public_services(a in any::<Option<u16>>(), b in any::<Option<u16>>()) { super::services_contract([a,b]); }
        #[test] fn public_units(bits in any::<u32>(), width in any::<u16>(), height in any::<u16>()) { super::units_contract(bits,width,height); }
        #[test] fn public_configuration(bits in any::<u32>(), columns in any::<u8>()) { super::configuration_contract(bits,columns); }
        #[test] fn credential_debug_is_independent_of_contents(a in ".{0,64}", b in ".{0,64}") {
            use fairing::access::Credential;
            prop_assert_eq!(format!("{:?}", Credential::Pin(a.clone())), format!("{:?}", Credential::Pin(b.clone())));
            prop_assert_eq!(format!("{:?}", Credential::Password { user: Some(a.clone()), secret: b.clone() }),
                            format!("{:?}", Credential::Password { user: Some(b), secret: a }));
            prop_assert_eq!(format!("{:?}", Credential::Pattern(vec![0,1,2])), format!("{:?}", Credential::Pattern(vec![8,7,6])));
            prop_assert_eq!(format!("{:?}", Credential::External(vec![0;32])), format!("{:?}", Credential::External(vec![255;32])));
        }
    }
    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]
        #[test] fn public_shell_ordering(bytes in prop::collection::vec(any::<u8>(), 0..33)) { super::shell_contract(&bytes); }
        #[test] fn public_backend_ordering(bytes in prop::collection::vec(any::<u8>(), 0..33)) { super::backend_contract(&bytes); }
        #[test] fn public_grants(held in 0u16..3, candidate in prop_oneof![0u16..5, any::<u16>()], lock in any::<bool>(), policy in any::<bool>()) {
            super::grant_contract(held, candidate, lock, policy);
        }
        #[test] fn public_expiry(seconds in any::<u8>(), boundary in 0u8..3, input in any::<bool>(), kind in 0u8..3) {
            super::expiry_contract(seconds, boundary, input, kind);
        }
    }
}

#[cfg(test)]
mod regressions {
    #[test]
    fn display_refusal_and_capacity_reuse() {
        for bytes in [
            vec![0; 64],
            vec![1, 0, 4, 8, 12, 2, 3, 2, 3, 2],
            (0..64).collect(),
        ] {
            super::display_contract(&bytes);
        }
    }
    #[test]
    fn native_receiver_close_races_release_all_reservations() {
        use std::sync::{Arc, Barrier};
        for _ in 0..16 {
            let (tx, rx) = fairing::inbox::Inbox::<u8>::bounded(1).unwrap();
            let start = Arc::new(Barrier::new(3));
            let workers: Vec<_> = (0..2)
                .map(|value| {
                    let tx = tx.clone();
                    let start = Arc::clone(&start);
                    std::thread::spawn(move || {
                        start.wait();
                        let _ = tx.try_send_with(|| value);
                    })
                })
                .collect();
            start.wait();
            drop(rx);
            for worker in workers {
                worker.join().unwrap();
            }
            assert_eq!(tx.stats().outstanding, 0);
            assert!(!tx.stats().receiver_open);
        }
    }

    #[test]
    fn bounded_display_without_fonts_or_dynamic_render_storage() {
        for input in [
            vec![],
            vec![0],
            vec![4, 4],
            vec![255; 72],
            (0..72).collect(),
        ] {
            super::bounded_display_contract(&input);
        }
    }

    #[test]
    fn memory_capacity_reuse_and_stale_handles() {
        for bytes in [
            vec![0; 64],
            vec![3, 3, 3, 3, 4, 3, 5, 3, 6],
            (0..64).collect(),
        ] {
            super::memory_contract(&bytes);
        }
    }
    #[test]
    fn persistent_inputs_never_bypass_notification_cap() {
        super::notification_contract(&[0, 8, 12, 0]);
    }
    #[test]
    fn mixed_credentials_lock_at_the_limit_and_unlock_at_the_exact_deadline() {
        super::authentication_contract(&[0, 2, 0, 1, 97]);
        super::authentication_contract(&[0, 1, 4, 0, 1, 193]);
    }
    #[test]
    fn public_access_and_calendar_boundaries() {
        for bytes in [vec![0], vec![1], vec![2, 1], (0..64).collect()] {
            super::access_contract(&bytes);
        }
        for utc in [0, 1, 86400, i64::MAX as u64, u64::MAX] {
            for offset in [i32::MIN, -720, 0, 540, i32::MAX] {
                super::time_contract(utc, offset);
            }
        }
    }
    #[test]
    fn public_backend_and_unit_boundaries() {
        super::services_contract([None, Some(0)]);
        super::services_contract([Some(100), Some(1)]);
        for bits in [
            0,
            u32::MAX,
            f32::INFINITY.to_bits(),
            f32::NEG_INFINITY.to_bits(),
            255.5f32.to_bits(),
        ] {
            super::units_contract(bits, 0, u16::MAX);
        }
        super::configuration_contract(f32::NAN.to_bits(), 13);
        super::configuration_contract(0.5f32.to_bits(), 12);
    }
    #[cfg(not(miri))]
    #[test]
    fn direct_and_queued_paths_recheck_the_current_policy() {
        super::shell_contract(&[10, 2, 4, 6, 7, 2, 3, 4, 5, 6, 7, 8, 9, 0, 2]);
        super::backend_contract(&[5, 1, 0, 1, 2, 2, 4, 0, 3, 5, 2, 3]);
    }
    #[cfg(not(miri))]
    #[test]
    fn session_boundaries_cancel_before_polling_and_deadlines_are_inclusive() {
        for boundary in 0..5 {
            super::cancellation_contract(boundary);
        }
        for boundary in 0..3 {
            for kind in 0..3 {
                super::expiry_contract(0, boundary, true, kind);
                super::expiry_contract(0, boundary, false, kind);
            }
        }
        for held in 0..3 {
            for candidate in [0, 1, 2, 3, u16::MAX] {
                for lock in [false, true] {
                    super::grant_contract(held, candidate, lock, false);
                    super::grant_contract(held, candidate, lock, true);
                }
            }
        }
    }
}

/// Bounded display copies must preserve the newest values and validate surfaces before writing.
pub fn bounded_display_contract(bytes: &[u8]) {
    use fairing::memory::{
        display_warning_tile, paint_display_warning, paint_image_preview, DisplayWindow,
    };
    let mut bulk = DisplayWindow::<u8, 8>::new();
    let status = bulk.extend(bytes);
    assert_eq!(status.discarded, bytes.len().saturating_sub(8));
    assert!(bulk
        .recent(8)
        .copied()
        .eq(bytes.iter().skip(bytes.len().saturating_sub(8)).copied()));
    assert_eq!(bulk.take_status(), status);
    bulk.clear();
    assert!(bulk.is_empty());
    let mut window = DisplayWindow::<u8, 8>::new();
    let mut expected = std::collections::VecDeque::new();
    for chunk in bytes.chunks(3).take(24) {
        let prior = expected.len();
        for &value in chunk {
            expected.push_back(value);
        }
        let lost = prior.saturating_add(chunk.len()).saturating_sub(8);
        while expected.len() > 8 {
            expected.pop_front();
        }
        assert_eq!(window.extend(chunk).discarded, lost);
        let limit = usize::from(chunk.first().copied().unwrap_or(0));
        assert!(window.recent(limit).copied().eq(expected
            .iter()
            .skip(expected.len().saturating_sub(limit))
            .copied()));
    }
    let mut target = [0u8; 96 * 12 * 4];
    paint_display_warning(&mut target, [96, 12], 96 * 12).unwrap();
    assert!(target
        .as_chunks::<4>()
        .0
        .iter()
        .any(|p| p == &[255, 208, 64, 255]));
    let (size, prepared) = display_warning_tile();
    assert_eq!(size, [96, 12]);
    assert_eq!(target.as_chunks::<4>().0, prepared);
    let before = target;
    let width = usize::from(bytes.first().copied().unwrap_or(0));
    let height = usize::from(bytes.get(1).copied().unwrap_or(0));
    let source = [0u8; 64];
    let mut small = [0u8; 16];
    let original = small;
    let result = paint_image_preview(&source, [width, height], &mut small, [2, 2], 4);
    if width.checked_mul(height) == Some(16) && width != 0 && height != 0 {
        assert!(result.is_ok());
    } else {
        assert!(result.is_err());
        assert_eq!(small, original);
    }
    assert!(paint_display_warning(&mut target, [usize::MAX, 2], usize::MAX).is_err());
    assert_eq!(target, before);
}
