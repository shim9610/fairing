use super::*;
use crate::access::{Access, PermissionBound, SessionController, SessionGrant, Subject};
use crate::config::AccessConfig;
use crate::screen::cx::fixture::{pane, Fixture};
use crate::services::Waker;
use crate::workspace::InstanceId;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Clone, Copy)]
enum Rights {
    View,
    Edit,
}
impl Permission for Rights {
    fn gate(self) -> crate::Gate {
        crate::Gate::borrowed(match self {
            Self::View => "view",
            Self::Edit => "edit",
        })
    }
}

fn fixture() -> crate::Result<(Fixture, SessionController)> {
    fixture_with_policy(None)
}

fn fixture_with_policy(
    policy: Option<Box<dyn crate::access::AccessPolicy>>,
) -> crate::Result<(Fixture, SessionController)> {
    let mut runtime = Fixture::new()?;
    let mut access = Access::from_config(&AccessConfig {
        session_tokens: true,
        ..AccessConfig::default()
    })?;
    if let Some(policy) = policy {
        access.set_policy(policy);
    }
    let (owner, token) = SessionController::issue(
        Subject::default(),
        vec![
            SessionGrant::base_for(Rights::View),
            SessionGrant::inactive_for(Rights::Edit),
        ],
        None,
        runtime.now,
        Waker::new(&egui::Context::default()),
    )
    .map_err(|error| crate::Error::Config(error.to_string()))?;
    assert!(access.install_session(token, runtime.now));
    runtime.set_access(access);
    Ok((runtime, owner))
}

#[derive(Default)]
struct Seen {
    measures: Cell<usize>,
    paints: Cell<usize>,
    rect: Cell<Option<egui::Rect>>,
    id: Cell<Option<egui::Id>>,
}
struct Probe {
    required: Rc<Cell<Rights>>,
    size: Vec2,
    seen: Rc<Seen>,
    on_measure: Option<Box<dyn FnOnce()>>,
    on_paint: Option<Box<dyn FnOnce()>>,
}
impl PermissionBound for Probe {
    type Required = Rights;
    fn required_permission(&self) -> Rights {
        self.required.get()
    }
}
impl Element for Probe {
    fn measure(&mut self, _: Vec2, _: &mut Cx<'_>) -> Vec2 {
        self.seen.measures.set(self.seen.measures.get() + 1);
        if let Some(callback) = self.on_measure.take() {
            callback();
        }
        self.size
    }
    fn paint(&mut self, ui: &mut egui::Ui, _: &mut Cx<'_>) {
        self.seen.paints.set(self.seen.paints.get() + 1);
        self.seen.rect.set(Some(ui.max_rect()));
        self.seen.id.set(Some(ui.scope_id()));
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_rgb(21, 43, 65));
        if let Some(callback) = self.on_paint.take() {
            callback();
        }
    }
}
fn probe(required: Rights, size: Vec2) -> (Probe, Rc<Seen>) {
    let seen = Rc::new(Seen::default());
    (
        Probe {
            required: Rc::new(Cell::new(required)),
            size,
            seen: Rc::clone(&seen),
            on_measure: None,
            on_paint: None,
        },
        seen,
    )
}

fn draw(tree: &mut ElementTree<Rights>, runtime: &mut Fixture) -> Vec2 {
    let ctx = egui::Context::default();
    let mut consumed = Vec2::ZERO;
    let mut parts = runtime.parts();
    let mut cx = parts.cx(pane(InstanceId::NONE), None);
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(500.0, 500.0),
            )),
            ..Default::default()
        },
        |ui| {
            let before = ui.cursor().min;
            tree.ui(ui, &mut cx);
            consumed = ui.cursor().min - before;
        },
    )
    .drop_without_applying_deltas();
    consumed
}

#[test]
fn denied_elements_run_no_measure_or_paint_and_allocate_nothing() -> crate::Result<()> {
    let (mut runtime, mut owner) = fixture()?;
    let (leaf, seen) = probe(Rights::Edit, egui::vec2(100.0, 80.0));
    let mut tree = ElementTree::leaf(leaf);
    assert_eq!(draw(&mut tree, &mut runtime), Vec2::ZERO);
    assert_eq!((seen.measures.get(), seen.paints.get()), (0, 0));
    owner.revoke();
    let (leaf, seen) = probe(Rights::View, egui::vec2(100.0, 80.0));
    let mut tree = ElementTree::leaf(leaf);
    assert_eq!(draw(&mut tree, &mut runtime), Vec2::ZERO);
    assert_eq!((seen.measures.get(), seen.paints.get()), (0, 0));
    Ok(())
}

#[test]
fn nested_layout_excludes_denied_sizes_and_gaps_and_reflows_after_grant_changes(
) -> crate::Result<()> {
    let (mut runtime, mut owner) = fixture()?;
    let (first, first_seen) = probe(Rights::View, egui::vec2(30.0, 10.0));
    let (hidden, hidden_seen) = probe(Rights::Edit, egui::vec2(100.0, 80.0));
    let (nested_hidden, nested_hidden_seen) = probe(Rights::Edit, egui::vec2(100.0, 80.0));
    let (last, last_seen) = probe(Rights::View, egui::vec2(20.0, 10.0));
    let nested = ElementTree::column(
        Rights::View,
        9.0,
        vec![ElementTree::leaf(nested_hidden), ElementTree::leaf(last)],
    )?;
    let mut tree = ElementTree::row(
        Rights::View,
        5.0,
        vec![ElementTree::leaf(first), ElementTree::leaf(hidden), nested],
    )?;
    draw(&mut tree, &mut runtime);
    let first = first_seen
        .rect
        .get()
        .ok_or_else(|| crate::Error::Config("first leaf not painted".into()))?;
    let last = last_seen
        .rect
        .get()
        .ok_or_else(|| crate::Error::Config("last leaf not painted".into()))?;
    assert_eq!(
        last.min,
        first.min + egui::vec2(35.0, 0.0),
        "denied children reserved layout space or gaps"
    );
    assert_eq!(
        (
            hidden_seen.measures.get(),
            hidden_seen.paints.get(),
            nested_hidden_seen.measures.get(),
            nested_hidden_seen.paints.get()
        ),
        (0, 0, 0, 0)
    );
    assert_ne!(
        first_seen.id.get(),
        last_seen.id.get(),
        "element widget identities collided"
    );
    let stable_ids = (first_seen.id.get(), last_seen.id.get());
    owner
        .replace_grants(
            vec![
                SessionGrant::base_for(Rights::View),
                SessionGrant::base_for(Rights::Edit),
            ],
            None,
            runtime.now,
        )
        .map_err(|e| crate::Error::Config(e.to_string()))?;
    draw(&mut tree, &mut runtime);
    assert!(hidden_seen.paints.get() > 0 && nested_hidden_seen.paints.get() > 0);
    assert_eq!(
        (first_seen.id.get(), last_seen.id.get()),
        stable_ids,
        "filtering siblings changed widget identity"
    );
    let expanded = last_seen
        .rect
        .get()
        .ok_or_else(|| crate::Error::Config("last leaf not painted".into()))?;
    assert!(expanded.min.x > last.min.x && expanded.min.y > last.min.y);
    owner
        .replace_grants(
            vec![SessionGrant::base_for(Rights::View)],
            None,
            runtime.now,
        )
        .map_err(|e| crate::Error::Config(e.to_string()))?;
    let counts = (
        hidden_seen.measures.get(),
        hidden_seen.paints.get(),
        nested_hidden_seen.measures.get(),
        nested_hidden_seen.paints.get(),
    );
    draw(&mut tree, &mut runtime);
    assert_eq!(last_seen.rect.get().map(|r| r.min), Some(last.min));
    assert_eq!(
        (
            hidden_seen.measures.get(),
            hidden_seen.paints.get(),
            nested_hidden_seen.measures.get(),
            nested_hidden_seen.paints.get()
        ),
        counts
    );
    Ok(())
}

#[test]
fn revocation_during_measurement_discards_all_prepared_paint_and_geometry() -> crate::Result<()> {
    let (mut runtime, mut owner) = fixture()?;
    let (first, first_seen) = probe(Rights::View, egui::vec2(30.0, 10.0));
    let (mut revoker, last_seen) = probe(Rights::View, egui::vec2(30.0, 10.0));
    revoker.on_measure = Some(Box::new(move || owner.revoke()));
    let mut tree = ElementTree::row(
        Rights::View,
        5.0,
        vec![ElementTree::leaf(first), ElementTree::leaf(revoker)],
    )?;
    assert_eq!(draw(&mut tree, &mut runtime), Vec2::ZERO);
    assert!(first_seen.measures.get() > 0 && last_seen.measures.get() > 0);
    assert_eq!(
        (first_seen.paints.get(), last_seen.paints.get()),
        (0, 0),
        "stale measurement plan painted after revocation"
    );
    Ok(())
}

#[test]
fn publication_during_measurement_invalidates_layout_even_if_the_gate_stays_allowed(
) -> crate::Result<()> {
    let (mut runtime, owner) = fixture()?;
    let now = runtime.now;
    let owner = Rc::new(RefCell::new(owner));
    let issuer = Rc::clone(&owner);
    let (mut leaf, seen) = probe(Rights::View, egui::vec2(30.0, 10.0));
    leaf.on_measure = Some(Box::new(move || {
        assert!(issuer
            .borrow_mut()
            .replace_grants(
                vec![
                    SessionGrant::base_for(Rights::View),
                    SessionGrant::base_for(Rights::Edit)
                ],
                None,
                now
            )
            .is_ok());
    }));
    let mut tree = ElementTree::leaf(leaf);
    assert_eq!(
        draw(&mut tree, &mut runtime),
        Vec2::ZERO,
        "publication reused geometry from an old authority revision"
    );
    assert_eq!(
        seen.paints.get(),
        0,
        "publication reused geometry from an old authority revision"
    );
    draw(&mut tree, &mut runtime);
    assert!(
        seen.paints.get() > 0,
        "fresh authority did not rebuild the plan"
    );
    Ok(())
}

#[test]
fn changed_requirement_between_measure_and_paint_discards_the_plan() -> crate::Result<()> {
    let (mut runtime, _owner) = fixture()?;
    let (mut leaf, seen) = probe(Rights::View, egui::vec2(30.0, 10.0));
    let metadata = Rc::clone(&leaf.required);
    leaf.on_measure = Some(Box::new(move || metadata.set(Rights::Edit)));
    let mut tree = ElementTree::leaf(leaf);
    assert_eq!(draw(&mut tree, &mut runtime), Vec2::ZERO);
    assert_eq!(
        seen.paints.get(),
        0,
        "changed metadata bypassed its new permission"
    );
    Ok(())
}

#[test]
fn revocation_in_one_painter_prevents_later_sibling_callbacks() -> crate::Result<()> {
    let (mut runtime, mut owner) = fixture()?;
    let (mut first, first_seen) = probe(Rights::View, egui::vec2(30.0, 10.0));
    first.on_paint = Some(Box::new(move || owner.revoke()));
    let (last, last_seen) = probe(Rights::View, egui::vec2(30.0, 10.0));
    let mut tree = ElementTree::row(
        Rights::View,
        5.0,
        vec![ElementTree::leaf(first), ElementTree::leaf(last)],
    )?;
    draw(&mut tree, &mut runtime);
    assert!(first_seen.paints.get() > 0);
    assert_eq!(
        last_seen.paints.get(),
        0,
        "later sibling used an admission from before revocation"
    );
    Ok(())
}

#[test]
fn invalid_measurements_do_not_paint_or_reserve_space() -> crate::Result<()> {
    let (mut runtime, _owner) = fixture()?;
    for size in [
        egui::vec2(f32::NAN, 20.0),
        egui::vec2(20.0, f32::INFINITY),
        egui::vec2(-1.0, 20.0),
        Vec2::ZERO,
    ] {
        let (leaf, seen) = probe(Rights::View, size);
        let mut tree = ElementTree::leaf(leaf);
        assert_eq!(draw(&mut tree, &mut runtime), Vec2::ZERO);
        assert_eq!(seen.paints.get(), 0);
    }
    Ok(())
}

#[test]
fn an_authorized_parent_does_not_authorize_child_measurement() -> crate::Result<()> {
    let (mut runtime, _owner) = fixture()?;
    let (hidden, seen) = probe(Rights::Edit, egui::vec2(100.0, 80.0));
    let mut tree = ElementTree::row(Rights::View, 5.0, vec![ElementTree::leaf(hidden)])?;
    let consumed = draw(&mut tree, &mut runtime);
    assert_eq!(
        seen.measures.get(),
        0,
        "denied child measurement executed through an authorized parent"
    );
    assert_eq!(seen.paints.get(), 0);
    assert_eq!(consumed, Vec2::ZERO);
    Ok(())
}

#[test]
fn ancestor_policy_changes_block_later_children_without_an_issuer_revision() -> crate::Result<()> {
    struct ParentPolicy(Rc<Cell<bool>>);
    impl crate::access::AccessPolicy for ParentPolicy {
        fn allows(&self, _: &Subject, gate: &crate::Gate) -> bool {
            gate.as_str() != "edit" || self.0.get()
        }
    }
    for during_paint in [false, true] {
        let parent_allowed = Rc::new(Cell::new(true));
        let (mut runtime, mut owner) =
            fixture_with_policy(Some(Box::new(ParentPolicy(Rc::clone(&parent_allowed)))))?;
        owner
            .replace_grants(
                vec![
                    SessionGrant::base_for(Rights::View),
                    SessionGrant::base_for(Rights::Edit),
                ],
                None,
                runtime.now,
            )
            .map_err(|error| crate::Error::Config(error.to_string()))?;
        let stamp = runtime.parts().access.authority_stamp();
        let (mut first, first_seen) = probe(Rights::View, egui::vec2(30.0, 10.0));
        let change = Box::new(move || parent_allowed.set(false));
        if during_paint {
            first.on_paint = Some(change);
        } else {
            first.on_measure = Some(change);
        }
        let (second, second_seen) = probe(Rights::View, egui::vec2(30.0, 10.0));
        let nested = ElementTree::column(
            Rights::Edit,
            5.0,
            vec![ElementTree::leaf(first), ElementTree::leaf(second)],
        )?;
        let mut tree = ElementTree::row(Rights::View, 5.0, vec![nested])?;
        let consumed = draw(&mut tree, &mut runtime);
        assert_eq!(runtime.parts().access.authority_stamp(), stamp);
        assert_eq!(first_seen.measures.get(), 1);
        assert_eq!(
            second_seen.paints.get(),
            0,
            "ancestor policy denial did not block a later child"
        );
        if during_paint {
            assert_eq!(first_seen.paints.get(), 1);
            assert_eq!(second_seen.measures.get(), 1);
        } else {
            assert_eq!(second_seen.measures.get(), 0);
            assert_eq!(first_seen.paints.get(), 0);
            assert_eq!(consumed, Vec2::ZERO);
        }
    }
    Ok(())
}
