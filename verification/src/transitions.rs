//! Independent state oracle for queued commands and retained asynchronous replies.
//! Tokens are represented by issuance ordinals in the oracle, never by observed subject/effects.
use fairing::access::{
    AccessPolicy, AuthMethod, AuthOutcome, AuthRequestId, AuthResponse, Authenticator, Credential,
    Gate, Level, Subject,
};
use fairing::shell::Command;
use fairing::{action, LaunchAction, SettingValue, Shell};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Instant;

type Requests = Rc<RefCell<Vec<(AuthRequestId, String)>>>;
type Replies = Rc<RefCell<VecDeque<AuthResponse>>>;
struct RetainingReader {
    requests: Requests,
    replies: Replies,
}
impl Authenticator for RetainingReader {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::External {
            label: "Reader".into(),
        }]
    }
    fn begin(&mut self, id: AuthRequestId, gate: &Gate, _: Instant) {
        self.requests.borrow_mut().push((id, gate.as_str().into()));
    }
    fn submit(&mut self, _: AuthRequestId, _: Credential, _: Instant) -> AuthOutcome {
        panic!("trace unexpectedly submitted a credential")
    }
    fn poll(&mut self, _: Instant) -> Option<AuthResponse> {
        self.replies.borrow_mut().pop_front()
    }
    // Deliberately retain all external replies when canceled or replaced.
}
struct Policy(Rc<Cell<bool>>);
impl AccessPolicy for Policy {
    fn allows(&self, who: &Subject, _: &Gate) -> bool {
        who.level >= Level(1) && self.0.get()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intent {
    A,
    B,
    Value(i64),
}
impl Intent {
    fn gate(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
            Self::Value(_) => "value",
        }
    }
    fn launch(self) -> LaunchAction {
        match self {
            Self::A => LaunchAction::run("a"),
            Self::B => LaunchAction::run("b"),
            Self::Value(n) => LaunchAction::Set("value".into(), SettingValue::Int(n)),
        }
    }
}
#[derive(Debug, Clone, Copy)]
enum Queued {
    Request(Intent),
    Execute(Intent),
    Back,
    Logout,
    Subject(u16),
}
impl Queued {
    fn from_byte(byte: u8) -> Self {
        match byte % 8 {
            0 => Self::Request(Intent::A),
            1 => Self::Request(Intent::B),
            2 => Self::Execute(Intent::A),
            3 => Self::Execute(Intent::B),
            4 => Self::Execute(Intent::Value(i64::from(byte))),
            5 => Self::Back,
            6 => Self::Logout,
            _ => Self::Subject(u16::from(byte & 0x80 != 0)),
        }
    }
    fn command(self) -> Command {
        match self {
            Self::Request(intent) => Command::RequestUnlock {
                gate: intent.gate().into(),
                then: Some(intent.launch()),
            },
            Self::Execute(intent) => Command::Launch(intent.launch()),
            Self::Back => Command::Back,
            Self::Logout => Command::Logout,
            Self::Subject(level) => Command::SetSubject(subject(level)),
        }
    }
}
fn subject(level: u16) -> Subject {
    Subject {
        level: Level(level),
        ..Subject::default()
    }
}
#[derive(Debug, Clone, Copy)]
enum Answer {
    Grant(u16),
    Denied,
    Pending,
}
impl Answer {
    fn outcome(self) -> AuthOutcome {
        match self {
            Self::Grant(level) => AuthOutcome::Granted(subject(level)),
            Self::Denied => AuthOutcome::Denied {
                message: "refused".into(),
            },
            Self::Pending => AuthOutcome::Pending,
        }
    }
}
#[derive(Debug, Clone, Copy)]
struct Challenge {
    ordinal: usize,
    intent: Intent,
}
#[derive(Debug, Default)]
struct Model {
    held: u16,
    challenge: Option<Challenge>,
    issued: Vec<&'static str>,
    queue: VecDeque<Queued>,
    replies: VecDeque<(usize, Answer)>,
    actions: Vec<&'static str>,
    setting: Option<i64>,
}
impl Model {
    fn request(&mut self, intent: Intent) {
        let ordinal = match self.challenge {
            Some(old) if old.intent.gate() == intent.gate() => old.ordinal,
            _ => {
                self.issued.push(intent.gate());
                self.issued.len() - 1
            }
        };
        self.challenge = Some(Challenge { ordinal, intent });
    }
    fn execute(&mut self, intent: Intent, policy: bool) {
        if self.held == 0 || !policy {
            self.request(intent);
            return;
        }
        match intent {
            Intent::A => self.actions.push("a"),
            Intent::B => self.actions.push("b"),
            Intent::Value(n) => self.setting = Some(n),
        }
    }
    fn tick(&mut self, budget: usize, policy: bool) {
        for _ in 0..budget {
            let Some(command) = self.queue.pop_front() else {
                break;
            };
            match command {
                Queued::Request(intent) => self.request(intent),
                Queued::Execute(intent) => self.execute(intent, policy),
                Queued::Back => self.challenge = None,
                Queued::Logout => {
                    self.held = 0;
                    self.challenge = None;
                }
                Queued::Subject(level) => {
                    self.held = level;
                    self.challenge = None;
                }
            }
        }
        if let Some(challenge) = self.challenge {
            if let Some((ordinal, answer)) = self.replies.pop_front() {
                if ordinal == challenge.ordinal {
                    match answer {
                        Answer::Pending => {}
                        Answer::Grant(level) if level == 1 && self.held == 0 => {
                            self.held = level;
                            self.challenge = None;
                            self.execute(challenge.intent, policy);
                        }
                        Answer::Grant(_) | Answer::Denied => {
                            self.challenge = None;
                            self.request(challenge.intent);
                        }
                    }
                }
            }
        }
    }
}
struct Trace {
    shell: Shell,
    ctx: fairing::egui::Context,
    requests: Requests,
    replies: Replies,
    actions: Rc<RefCell<Vec<&'static str>>>,
    policy: Rc<Cell<bool>>,
    model: Model,
    capacity: usize,
    budget: usize,
    history: Vec<u8>,
}
impl Trace {
    fn new(capacity: usize, budget: usize) -> Self {
        let ctx = fairing::egui::Context::default();
        let requests = Rc::new(RefCell::new(Vec::new()));
        let replies = Rc::new(RefCell::new(VecDeque::new()));
        let actions = Rc::new(RefCell::new(Vec::new()));
        let policy = Rc::new(Cell::new(true));
        let shell = Self::build(
            &ctx, &requests, &replies, &actions, &policy, capacity, budget,
        );
        Self {
            shell,
            ctx,
            requests,
            replies,
            actions,
            policy,
            model: Model::default(),
            capacity,
            budget,
            history: Vec::new(),
        }
    }
    fn build(
        ctx: &fairing::egui::Context,
        requests: &Requests,
        replies: &Replies,
        actions: &Rc<RefCell<Vec<&'static str>>>,
        policy: &Rc<Cell<bool>>,
        capacity: usize,
        budget: usize,
    ) -> Shell {
        let mut cfg = fairing::testing::access_config(&["viewer", "admin"], Some("top"));
        cfg.motion.reduce = true;
        cfg.access.unlock_mode = "switch".into();
        cfg.access.temporary_secs = 0;
        cfg.access.session_timeout_secs = 0;
        cfg.access.idle_lock_secs = 0;
        let mut shell = Shell::builder(cfg)
            .command_queue_capacity(capacity)
            .max_commands_per_update(budget)
            .authenticator(RetainingReader {
                requests: requests.clone(),
                replies: replies.clone(),
            })
            .build(ctx)
            .unwrap();
        shell
            .set_policy(Box::new(Policy(policy.clone())));
        for id in ["a", "b"] {
            let actions = actions.clone();
            shell.add(action(id, move |_| actions.borrow_mut().push(id)));
        }
        shell
    }
    fn enqueue(&mut self, command: Queued) {
        let expected = command.command();
        match self.shell.handle().try_send(expected.clone()) {
            Ok(()) => {
                assert!(
                    self.model.queue.len() < self.capacity,
                    "admission exceeded capacity"
                );
                self.model.queue.push_back(command);
            }
            Err(error) => {
                assert_eq!(error.kind(), fairing::inbox::QueueErrorKind::Full);
                assert_eq!(
                    self.model.queue.len(),
                    self.capacity,
                    "premature queue rejection"
                );
                assert_eq!(
                    error.into_inner(),
                    expected,
                    "rejection changed owned command"
                );
            }
        }
    }
    fn answer(&mut self, oldest: bool, answer: Answer) {
        let ordinal = if oldest {
            self.model.issued.first().map(|_| 0)
        } else {
            self.model.issued.len().checked_sub(1)
        };
        if let Some(ordinal) = ordinal {
            let id = self.requests.borrow()[ordinal].0;
            self.replies
                .borrow_mut()
                .push_back(id.respond(answer.outcome()));
            self.model.replies.push_back((ordinal, answer));
        }
    }
    fn tick(&mut self) {
        self.model.tick(self.budget, self.policy.get());
        self.shell.logic(&self.ctx);
        let _ = self.shell.poll_events();
    }
    fn check(&self) {
        assert!(
            !self.shell.is_faulted(),
            "trace faulted: {:?}",
            self.history
        );
        assert_eq!(
            self.shell.access().session().subject.level.0,
            self.model.held,
            "subject diverged: {:?}",
            self.history
        );
        assert_eq!(
            self.shell.unlock_prompt_visible(),
            self.model.challenge.is_some(),
            "prompt diverged: {:?}",
            self.history
        );
        assert_eq!(
            &*self.actions.borrow(),
            &self.model.actions,
            "unauthorized, lost or duplicated continuation: {:?}",
            self.history
        );
        assert_eq!(
            self.shell.settings().get(&"value".into()),
            self.model
                .setting
                .as_ref()
                .map(|n| SettingValue::Int(*n))
                .as_ref(),
            "setting diverged: {:?}",
            self.history
        );
        assert_eq!(
            self.shell.handle().queue_stats().outstanding,
            self.model.queue.len(),
            "queue reordered/evicted: {:?}",
            self.history
        );
        let issued = self.requests.borrow();
        assert_eq!(
            issued
                .iter()
                .map(|(_, gate)| gate.as_str())
                .collect::<Vec<_>>(),
            self.model.issued,
            "challenge issuance diverged: {:?}",
            self.history
        );
        for (i, (id, _)) in issued.iter().enumerate() {
            assert!(
                !issued[..i].iter().any(|(old, _)| old == id),
                "authentication token reused"
            );
        }
        assert_eq!(
            self.replies.borrow().len(),
            self.model.replies.len(),
            "reply consumption diverged: {:?}",
            self.history
        );
    }
    fn step(&mut self, byte: u8) {
        self.history.push(byte);
        match byte % 16 {
            0..=7 => self.enqueue(Queued::from_byte(byte)),
            8 => self.tick(),
            9 => self.answer(false, Answer::Grant(1)),
            10 => self.answer(true, Answer::Grant(1)),
            11 => self.answer(false, Answer::Denied),
            12 => self.answer(false, Answer::Pending),
            13 => self.answer(false, Answer::Grant(if byte & 0x80 == 0 { 0 } else { 2 })),
            14 => self.policy.set(!self.policy.get()),
            _ => {
                let old = self.shell.handle().clone();
                let next = Self::build(
                    &self.ctx,
                    &self.requests,
                    &self.replies,
                    &self.actions,
                    &self.policy,
                    self.capacity,
                    self.budget,
                );
                self.shell.restart_with(|| Ok(next)).unwrap();
                self.model.held = 0;
                self.model.challenge = None;
                self.model.queue.clear();
                self.model.setting = None;
                assert_eq!(
                    old.try_send(Command::Back).unwrap_err().kind(),
                    fairing::inbox::QueueErrorKind::Disconnected
                );
            }
        }
        self.check();
    }
    fn require_progress(&mut self) {
        // Quiesce queued work, then cancel through the same live shell (no restart shortcut).
        for _ in 0..=self.model.queue.len() {
            self.step(8);
        }
        self.policy.set(true);
        self.step(6);
        self.step(8); // logout is always admitted in an empty queue
        self.step(0);
        self.step(8); // a new A challenge, with any old worker replies retained
        let before = self.model.actions.len();
        self.step(9);
        for _ in 0..=self.model.replies.len() {
            self.step(8);
        }
        assert_eq!(
            self.model.actions.len(),
            before + 1,
            "a valid retry failed to progress"
        );
        assert_eq!(self.shell.access().session().subject.level, Level(1));
        assert!(!self.shell.unlock_prompt_visible());
    }
}

/// Up to 96 operations; generated capacities and budgets are independently 1..=3.
/// Every operation checks public observations against the independent model.
///
/// Low-nibble opcodes: 0/1 request gates A/B, 2/3 run A/B, 4 write a setting,
/// 5 Back, 6 logout, 7 assign a subject, 8 update, 9 latest-token grant,
/// 10 oldest-token grant, 11 denial, 12 pending, 13 invalid/non-elevating grant,
/// 14 change policy, 15 replace shell. The high bit selects subject/grant variants.
pub fn contract(bytes: &[u8], capacity: usize, budget: usize) {
    let mut trace = Trace::new(capacity, budget);
    for &byte in bytes.iter().take(96) {
        trace.step(byte);
    }
    trace.require_progress();
}

#[cfg(all(test, not(miri)))]
pub mod regressions {
    #[test]
    fn every_command_pair_and_reply_class_matches_the_state_oracle() {
        let mut cases = 0;
        for capacity in 1..=3 {
            for budget in 1..=3 {
                for first in [0, 1, 2, 3, 4, 5, 6, 7, 135] {
                    for second in [0, 1, 2, 3, 4, 5, 6, 7, 135] {
                        for answer in [9, 10, 11, 12, 13, 141] {
                            for policy in [true, false] {
                                let mut trace = super::Trace::new(capacity, budget);
                                // Two real challenges ensure both current and stale replies exist.
                                for op in [0, 8, 1, 8] {
                                    trace.step(op);
                                }
                                if !policy {
                                    trace.step(14);
                                }
                                for op in [answer, first, second, 8, 8, 8] {
                                    trace.step(op);
                                }
                                trace.require_progress();
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 8748);
    }
    #[test]
    fn finite_budget_separates_gate_retarget_from_reply_polling() {
        super::contract(&[0, 1, 8, 9, 8, 9, 8], 2, 1);
    }

    #[test]
    fn replacement_and_duplicate_reply_history_matches_the_state_oracle() {
        super::contract(
            &[
                0, 8, 1, 8, 10, 8, 11, 8, 9, 8, 6, 8, 0, 8, 10, 8, 9, 8, 15, 0, 8, 10, 8, 9, 8,
            ],
            2,
            1,
        );
    }
}
#[cfg(all(test, not(miri)))]
mod generated {
    use proptest::prelude::*;
    proptest! {
        #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]
        #[test]
        fn generated_command_reply_and_replacement_traces(bytes in prop::collection::vec(any::<u8>(), 0..97), capacity in 1usize..4, budget in 1usize..4) {
            super::contract(&bytes, capacity, budget);
        }
    }
}
#[cfg(all(test, miri))]
mod interpreted {
    #[test]
    fn retained_reply_and_queue_ordering_use_the_real_shell() {
        super::contract(
            &[0, 8, 1, 8, 10, 8, 11, 8, 9, 8, 15, 0, 8, 10, 8, 9, 8],
            2,
            1,
        );
    }
}

#[cfg(all(test, not(miri)))]
mod deadlines {
    use super::*;
    use fairing::egui;
    fn frame(shell: &mut Shell, ctx: &egui::Context, time: f64, fresh: bool) {
        let events = if fresh {
            vec![egui::Event::PointerMoved(egui::pos2(200.0, 180.0))]
        } else {
            Vec::new()
        };
        ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| shell.frame(ui),
        )
        .drop_without_applying_deltas();
    }
    #[test]
    fn expiry_preempts_backlogged_commands_at_the_inclusive_boundary() {
        for (capacity, budget) in [(3, 1), (5, 2)] {
            for fresh in [false, true] {
                for boundary in [0.999, 1.0, 1.001] {
                    let mut cfg =
                        fairing::testing::access_config(&["viewer", "admin"], Some("top"));
                    cfg.access.session_timeout_secs = 1;
                    cfg.access.unlock_mode = "switch".into();
                    cfg.access
                        .pin_table
                        .pins
                        .insert("admin".into(), "1234".into());
                    cfg.motion.reduce = true;
                    let ctx = egui::Context::default();
                    let runs = Rc::new(RefCell::new(Vec::new()));
                    let mut shell = Shell::builder(cfg)
                        .command_queue_capacity(capacity)
                        .max_commands_per_update(budget)
                        .build(&ctx)
                        .unwrap();
                    for id in 0..capacity {
                        let runs = runs.clone();
                        shell.add(action(format!("a{id}"), move |_| {
                            runs.borrow_mut().push(id)
                        }));
                    }
                    shell.handle().set_subject(subject(1));
                    frame(&mut shell, &ctx, 0.0, false);
                    for id in 0..capacity {
                        shell
                            .handle()
                            .try_send(Command::Launch(LaunchAction::run(format!("a{id}"))))
                            .unwrap();
                    }
                    frame(&mut shell, &ctx, 0.5, false);
                    assert_eq!(&*runs.borrow(), &(0..budget).collect::<Vec<_>>());
                    frame(&mut shell, &ctx, boundary, fresh);
                    let expected = if boundary < 1.0 {
                        (budget * 2).min(capacity)
                    } else {
                        budget
                    };
                    assert_eq!(&*runs.borrow(), &(0..expected).collect::<Vec<_>>(), "expiry/queue ordering diverged: boundary={boundary}, fresh={fresh}, budget={budget}");
                    // No activity after the pre-deadline probe; let its refreshed idle timer expire too.
                    for _ in 0..capacity {
                        frame(&mut shell, &ctx, 2.01, false);
                    }
                    assert_eq!(shell.handle().queue_stats().outstanding, 0);
                    assert_eq!(
                        &*runs.borrow(),
                        &(0..expected).collect::<Vec<_>>(),
                        "expiry/queue ordering diverged after backlog drain"
                    );
                    assert_eq!(shell.access().session().subject.level, Level(0));
                }
            }
        }
    }

    #[test]
    fn expiry_cancels_old_replies_before_replacement_and_fresh_input() {
        for boundary in [0.999, 1.0, 1.001] {
            for fresh in [false, true] {
                for replacement in [false, true] {
                    for (capacity, budget) in [(1, 1), (3, 2)] {
                        deadline_reply_case(boundary, fresh, replacement, capacity, budget);
                    }
                }
            }
        }
    }

    fn deadline_reply_case(
        boundary: f64,
        fresh: bool,
        replacement: bool,
        capacity: usize,
        budget: usize,
    ) {
        let mut cfg =
            fairing::testing::access_config(&["viewer", "operator", "admin"], Some("top"));
        cfg.motion.reduce = true;
        cfg.access.unlock_mode = "switch".into();
        cfg.access.session_timeout_secs = 1;
        cfg.access.idle_lock_secs = 0;
        cfg.access.gates.insert("sensitive".into(), "admin".into());
        let ctx = egui::Context::default();
        let requests = Rc::new(RefCell::new(Vec::new()));
        let replies = Rc::new(RefCell::new(VecDeque::new()));
        let calls = Rc::new(Cell::new(0));
        let mut shell = Shell::builder(cfg)
            .command_queue_capacity(capacity)
            .max_commands_per_update(budget)
            .authenticator(RetainingReader {
                requests: requests.clone(),
                replies: replies.clone(),
            })
            .build(&ctx)
            .unwrap();
        let seen = calls.clone();
        shell.add(action("sensitive", move |_| seen.set(seen.get() + 1)));
        shell.handle().set_subject(subject(1));
        frame(&mut shell, &ctx, 0.0, false);
        let request = || Command::RequestUnlock {
            gate: "sensitive".into(),
            then: Some(LaunchAction::run("sensitive")),
        };
        shell.handle().try_send(request()).unwrap();
        frame(&mut shell, &ctx, 0.5, false);
        assert!(shell.unlock_prompt_visible());
        assert_eq!(requests.borrow().len(), 1);
        let old = requests.borrow()[0].0;
        replies
            .borrow_mut()
            .push_back(old.respond(AuthOutcome::Granted(subject(2))));
        if replacement {
            shell.handle().try_send(request()).unwrap();
        }
        frame(&mut shell, &ctx, boundary, fresh);
        let expired = boundary >= 1.0;
        assert_eq!(
            calls.get(),
            usize::from(!expired),
            "expired challenge accepted a stale grant: boundary={boundary}, fresh={fresh}, replacement={replacement}"
        );
        assert_eq!(
            shell.access().session().subject.level,
            Level(if expired { 0 } else { 2 }),
            "expiry and reply ordering diverged"
        );
        if expired {
            if !shell.unlock_prompt_visible() {
                shell.handle().try_send(request()).unwrap();
            }
            frame(&mut shell, &ctx, 1.01, false);
            let current = requests.borrow().last().unwrap().0;
            assert_ne!(current, old, "expired authentication token was reused");
            replies
                .borrow_mut()
                .push_back(current.respond(AuthOutcome::Granted(subject(2))));
            for time in [1.02, 1.03, 1.04] {
                frame(&mut shell, &ctx, time, false);
            }
            assert_eq!(calls.get(), 1, "valid retry was blocked or executed twice");
            assert_eq!(shell.access().session().subject.level, Level(2));
            assert!(!shell.unlock_prompt_visible());
            replies
                .borrow_mut()
                .push_back(old.respond(AuthOutcome::Granted(subject(2))));
            frame(&mut shell, &ctx, 1.05, false);
            assert_eq!(
                calls.get(),
                1,
                "old reply replayed a completed continuation"
            );
        }
        assert!(!shell.is_faulted());
    }
}
