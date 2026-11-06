//! Execute the unchanged production tree against explicit authority/UI contracts.
//! The generator substitutes only module paths, never executable product bodies.
extern crate self as egui;

// A width-two sequence contract. Nodes/plans live in an arena: allocator failure
// and destruction are outside this mediation proof. Push never silently drops a
// third item; exceeding the documented bound fails an assertion.
pub struct ModelVec<T: 'static> {
    data: &'static mut [Option<T>; 2],
    len: usize,
}
pub struct ModelIter<'a, T> {
    first: Option<&'a T>,
    second: Option<&'a T>,
}
impl<'a, T> Iterator for ModelIter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        match self.first.take() {
            Some(value) => Some(value),
            None => self.second.take(),
        }
    }
}
pub struct ModelIterMut<'a, T> {
    first: Option<&'a mut T>,
    second: Option<&'a mut T>,
}
impl<'a, T> Iterator for ModelIterMut<'a, T> {
    type Item = &'a mut T;
    fn next(&mut self) -> Option<Self::Item> {
        match self.first.take() {
            Some(value) => Some(value),
            None => self.second.take(),
        }
    }
}
impl<T: 'static> ModelVec<T> {
    pub fn new() -> Self {
        Self {
            data: Box::leak(Box::new([None, None])),
            len: 0,
        }
    }
    pub fn push(&mut self, value: T) {
        match self.len {
            0 => self.data[0] = Some(value),
            1 => self.data[1] = Some(value),
            _ => panic!("sequence contract capacity exceeded"),
        }
        self.len += 1;
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn iter(&self) -> ModelIter<'_, T> {
        ModelIter {
            first: self.data[0].as_ref(),
            second: self.data[1].as_ref(),
        }
    }
    pub fn iter_mut(&mut self) -> ModelIterMut<'_, T> {
        let [a, b] = &mut *self.data;
        ModelIterMut {
            first: a.as_mut(),
            second: b.as_mut(),
        }
    }
    pub fn get(&self, index: usize) -> Option<&T> {
        match index {
            0 => self.data[0].as_ref(),
            1 => self.data[1].as_ref(),
            _ => None,
        }
    }
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        match index {
            0 => self.data[0].as_mut(),
            1 => self.data[1].as_mut(),
            _ => None,
        }
    }
}
impl<'a, T: 'static> IntoIterator for &'a ModelVec<T> {
    type Item = &'a T;
    type IntoIter = ModelIter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
macro_rules! vec {
    ($($item:expr),* $(,)?) => {{ let mut values = ModelVec::new(); $(values.push($item);)* values }};
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}
impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    pub fn min(self, other: Self) -> Self {
        vec2(self.x.min(other.x), self.y.min(other.y))
    }
}
pub fn vec2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}
impl std::ops::Add for Vec2 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        vec2(self.x + other.x, self.y + other.y)
    }
}
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub min: Vec2,
    pub size: Vec2,
}
impl Rect {
    pub fn from_min_size(min: Vec2, size: Vec2) -> Self {
        Self { min, size }
    }
    pub fn intersect(self, _other: Self) -> Self {
        self
    }
}
// Widget hashing, clipping, rasterization and event realization are not modeled.
#[derive(Clone, Copy)]
pub struct Id;
impl Id {
    pub fn with<T>(self, _: T) -> Self {
        self
    }
    pub fn value(self) -> u64 {
        0
    }
}
pub struct Sense;
impl Sense {
    pub fn hover() -> Self {
        Self
    }
}
#[derive(Default)]
pub struct UiBuilder {
    rect: Rect,
}
impl UiBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn id_salt(self, _: u64) -> Self {
        self
    }
    pub fn max_rect(mut self, rect: Rect) -> Self {
        self.rect = rect;
        self
    }
}
pub struct Context;
impl Context {
    pub fn request_repaint(&self) {}
}
pub struct Ui {
    rect: Rect,
    allocations: usize,
    size: Vec2,
}
impl Ui {
    fn new() -> Self {
        Self {
            rect: Rect::default(),
            allocations: 0,
            size: Vec2::ZERO,
        }
    }
    pub fn available_size(&self) -> Vec2 {
        vec2(100.0, 100.0)
    }
    pub fn ctx(&self) -> Context {
        Context
    }
    pub fn scope_id(&self) -> Id {
        Id
    }
    pub fn allocate_exact_size(&mut self, size: Vec2, _: Sense) -> (Rect, ()) {
        self.allocations += 1;
        self.size = size;
        (Rect::from_min_size(Vec2::ZERO, size), ())
    }
    pub fn new_child(&mut self, builder: UiBuilder) -> Self {
        Self {
            rect: builder.rect,
            allocations: 0,
            size: Vec2::ZERO,
        }
    }
    pub fn clip_rect(&self) -> Rect {
        self.rect
    }
    pub fn set_clip_rect(&mut self, _: Rect) {}
}
pub enum Error {
    Config(String),
}
pub type Result<T> = std::result::Result<T, Error>;

#[path = "@SESSION_RULES@"]
mod session_rules;
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Gate(u8);
impl Gate {
    // Only the three named catalog constants occur in these clients. Inspecting
    // their distinct first bytes avoids introducing a C memcmp loop into the
    // permission-mapping adapter (not part of the mediation property).
    pub fn borrowed(value: &str) -> Self {
        Self(match value.as_bytes().first() {
            Some(b'r') => 0,
            Some(b'e') => 1,
            _ => 2,
        })
    }
    pub fn as_str(&self) -> &str {
        match self.0 {
            0 => "read",
            1 => "edit",
            _ => "operate",
        }
    }
}
#[derive(Clone, Copy)]
enum Right {
    Read,
    Edit,
    Operate,
}
impl Right {
    fn number(self) -> u8 {
        match self {
            Self::Read => 0,
            Self::Edit => 1,
            Self::Operate => 2,
        }
    }
}
impl access::Permission for Right {
    fn gate(self) -> Gate {
        Gate(self.number())
    }
}
#[derive(Clone, Copy)]
enum Effect {
    None,
    Revoke,
    Publish(u8),
    Replace,
}
struct Authority {
    mode: bool,
    active: bool,
    id: u64,
    revision: u64,
    grants: u8,
    policy: u8,
    measured: u8,
    painted: u8,
    measurement_stamp: [(u64, u64); 3],
    positions: [Vec2; 3],
}
impl Authority {
    fn stamp(&self) -> Option<(u64, u64)> {
        if self.mode && self.active {
            Some((self.id, self.revision))
        } else {
            None
        }
    }
    fn allowed(&self, right: Right) -> bool {
        self.mode
            && self.active
            && self.grants & (1 << right.number()) != 0
            && self.policy & (1 << right.number()) != 0
    }
    fn apply(&mut self, effect: Effect) {
        match effect {
            Effect::None => {}
            Effect::Revoke => self.active = false,
            Effect::Publish(grants) => {
                let next = session_rules::transition(
                    session_rules::ControlState {
                        revision: self.revision,
                        revoked: !self.active,
                    },
                    session_rules::ControlEvent::Publish {
                        now: 0,
                        until: u64::MAX,
                    },
                );
                self.revision = next.revision;
                self.active = !next.revoked;
                self.grants = grants;
            }
            Effect::Replace => match session_rules::next_identity(self.id) {
                Some(id) => {
                    self.id = id;
                    self.revision = 0;
                }
                None => self.active = false,
            },
        }
    }
}
mod screen {
    use super::*;
    pub struct Cx<'a> {
        pub state: &'a mut Authority,
    }
    impl Cx<'_> {
        pub fn token_mode(&self) -> bool {
            self.state.mode
        }
        pub fn allows_gate(&self, gate: &Gate) -> bool {
            self.state.mode
                && self.state.active
                && self.state.grants & (1 << gate.0) != 0
                && self.state.policy & (1 << gate.0) != 0
        }
        pub fn authority_stamp(&self) -> Option<(u64, u64)> {
            self.state.stamp()
        }
        pub fn with_protected_resources<R>(&mut self, callback: impl FnOnce(&mut Self) -> R) -> R {
            callback(self)
        }
    }
    pub trait Screen {
        fn ui(&mut self, ui: &mut Ui, cx: &mut Cx<'_>);
    }
}
mod access {
    pub use super::Gate;
    pub type SessionId = u64;
    #[path = "@BINDING@"]
    mod binding;
    #[path = "@DISPATCH@"]
    mod protected_dispatch;
    pub use self::binding::{Permission, PermissionBinding, PermissionBound};
}
#[path = "@ELEMENTS@"]
mod elements;
use access::PermissionBound;
use elements::{Element, ElementTree};
use screen::{Cx, Screen};
struct Leaf {
    index: usize,
    right: Right,
    next_right: Right,
    measure_effect: Effect,
    paint_effect: Effect,
    size: Vec2,
}
impl PermissionBound for Leaf {
    type Required = Right;
    fn required_permission(&self) -> Right {
        self.right
    }
}
impl Element for Leaf {
    fn measure(&mut self, _: Vec2, cx: &mut Cx<'_>) -> Vec2 {
        // Independent oracle: inspect this dynamic receiver's own requirement.
        assert!(
            cx.state.allowed(self.right),
            "unauthorized measurement callback"
        );
        assert_eq!(
            cx.state.measured & (1 << self.index),
            0,
            "measurement repeated"
        );
        cx.state.measured |= 1 << self.index;
        cx.state.measurement_stamp[self.index] = cx.state.stamp().unwrap();
        cx.state.apply(self.measure_effect);
        self.right = self.next_right;
        self.size
    }
    fn paint(&mut self, ui: &mut Ui, cx: &mut Cx<'_>) {
        assert!(cx.state.allowed(self.right), "unauthorized paint callback");
        assert_eq!(
            Some(cx.state.measurement_stamp[self.index]),
            cx.state.stamp(),
            "stale authority plan painted"
        );
        assert_ne!(
            cx.state.measured & (1 << self.index),
            0,
            "unmeasured leaf painted"
        );
        assert_eq!(cx.state.painted & (1 << self.index), 0, "paint repeated");
        cx.state.painted |= 1 << self.index;
        cx.state.positions[self.index] = ui.rect.min;
        cx.state.apply(self.paint_effect);
    }
}
fn right() -> Right {
    let value: u8 = kani::any();
    kani::assume(value < 3);
    match value {
        0 => Right::Read,
        1 => Right::Edit,
        _ => Right::Operate,
    }
}
fn effect() -> Effect {
    let value: u8 = kani::any();
    kani::assume(value < 4);
    match value {
        0 => Effect::None,
        1 => Effect::Revoke,
        2 => Effect::Publish(kani::any()),
        _ => Effect::Replace,
    }
}
fn state() -> Authority {
    Authority {
        mode: true,
        active: true,
        id: kani::any(),
        revision: 0,
        grants: kani::any(),
        policy: kani::any(),
        measured: 0,
        painted: 0,
        measurement_stamp: [(0, 0); 3],
        positions: [Vec2::ZERO; 3],
    }
}
fn leaf(
    index: usize,
    right: Right,
    measure_effect: Effect,
    paint_effect: Effect,
) -> ElementTree<Right> {
    ElementTree::leaf(Leaf {
        index,
        right,
        next_right: right,
        measure_effect,
        paint_effect,
        size: vec2(4.0, 6.0),
    })
}
fn row(children: ModelVec<ElementTree<Right>>) -> ElementTree<Right> {
    match ElementTree::row(Right::Read, 3.0, children) {
        Ok(tree) => tree,
        Err(_) => unreachable!(),
    }
}
fn run(mut tree: ElementTree<Right>, state: &mut Authority) -> Ui {
    let mut ui = Ui::new();
    tree.ui(&mut ui, &mut Cx { state });
    // Host destruction is outside the entry/layout contract. Keep the opaque
    // recursive input alive instead of expanding unrelated dynamic drop glue.
    std::mem::forget(tree);
    ui
}
#[kani::proof]
#[kani::unwind(2)]
fn denied_nested_tree_has_no_callbacks_or_layout() {
    let mut state = state();
    state.grants = 0;
    let tree = row(vec![
        leaf(0, right(), effect(), effect()),
        row(vec![leaf(1, right(), effect(), effect())]),
    ]);
    let ui = run(tree, &mut state);
    assert_eq!(state.measured, 0);
    assert_eq!(state.painted, 0);
    assert_eq!(ui.allocations, 0);
}

#[kani::proof]
#[kani::unwind(2)]
fn publication_while_measuring_discards_every_prepared_paint_and_allocation() {
    let mut state = state();
    state.grants = 7;
    state.policy = 7;
    let tree = row(vec![
        leaf(0, Right::Read, Effect::Publish(kani::any()), Effect::None),
        leaf(1, Right::Edit, Effect::None, Effect::None),
    ]);
    let ui = run(tree, &mut state);
    assert_eq!(state.measured, 1);
    assert_eq!(state.painted, 0);
    assert_eq!(ui.allocations, 0);
}

#[kani::proof]
#[kani::unwind(2)]
fn changed_requirement_invalidates_even_when_both_permissions_are_granted() {
    let mut state = state();
    state.grants = 7;
    state.policy = 7;
    let body = Leaf {
        index: 0,
        right: Right::Edit,
        next_right: Right::Operate,
        measure_effect: Effect::None,
        paint_effect: Effect::None,
        size: vec2(4.0, 6.0),
    };
    let tree = row(vec![ElementTree::leaf(body)]);
    let ui = run(tree, &mut state);
    assert_eq!(state.measured, 1);
    assert_eq!(state.painted, 0);
    assert_eq!(ui.allocations, 0, "stale requirement allocated geometry");
}

#[kani::proof]
#[kani::unwind(4)]
fn sequence_adapter_preserves_values_order_length_and_mutable_access() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let mut values = ModelVec::new();
    assert!(values.is_empty());
    assert_eq!(values.iter().next(), None);
    values.push(first);
    assert!(!values.is_empty());
    assert_eq!(values.get(0), Some(&first));
    assert_eq!(values.get(1), None);
    values.push(second);
    let mut iter = values.iter();
    assert_eq!(iter.next(), Some(&first));
    assert_eq!(iter.next(), Some(&second));
    assert_eq!(iter.next(), None);
    let index: usize = kani::any();
    assert_eq!(
        values.get(index).copied(),
        match index {
            0 => Some(first),
            1 => Some(second),
            _ => None,
        }
    );
    *values.get_mut(0).unwrap() = second;
    for value in values.iter_mut() {
        *value = value.wrapping_add(1);
    }
    assert_eq!(values.get(0), Some(&second.wrapping_add(1)));
    assert_eq!(values.get(1), Some(&second.wrapping_add(1)));
    assert_eq!((&values).into_iter().count(), 2);
}

#[kani::proof]
#[kani::unwind(1)]
fn complete_leaf_traversal_uses_own_live_permission_and_prepared_revision() {
    let mut state = state();
    state.mode = kani::any();
    state.active = kani::any();
    let required = right();
    let next = right();
    let change = effect();
    let allowed = state.allowed(required);
    let stable = matches!(change, Effect::None) && required.number() == next.number();
    let body = Leaf {
        index: 0,
        right: required,
        next_right: next,
        measure_effect: change,
        paint_effect: effect(),
        size: vec2(4.0, 6.0),
    };
    let ui = run(ElementTree::leaf(body), &mut state);
    assert_eq!(state.measured, u8::from(allowed));
    assert_eq!(state.painted, u8::from(allowed && stable));
    assert_eq!(ui.allocations, usize::from(allowed && stable));
}
#[kani::proof]
#[kani::unwind(1)]
fn retaining_tree_across_frames_cannot_reuse_an_earlier_admission() {
    let mut state = state();
    let required = right();
    let allowed = state.allowed(required);
    let mut tree = leaf(0, required, Effect::None, Effect::None);
    let mut first = Ui::new();
    tree.ui(&mut first, &mut Cx { state: &mut state });
    assert_eq!(state.measured, u8::from(allowed));
    assert_eq!(state.painted, u8::from(allowed));
    state.apply(Effect::Revoke);
    state.measured = 0;
    state.painted = 0;
    let mut second = Ui::new();
    tree.ui(&mut second, &mut Cx { state: &mut state });
    assert_eq!(state.measured, 0);
    assert_eq!(state.painted, 0);
    assert_eq!(second.allocations, 0);
    std::mem::forget(tree);
}
