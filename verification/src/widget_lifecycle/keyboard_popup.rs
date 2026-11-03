//! The real shell keyboard must remain usable after popup/focus/visibility histories.
use super::OPTIONS;
use fairing::egui::{self, Pos2, Rect};
use fairing::osk::OskLayout;
use fairing::testing::{single_level_access, Harness};
use fairing::widgets::{Dropdown, Keys, Opener, TextField};
use fairing::{screen, Cx, LaunchAction, Services};
use std::cell::RefCell;
use std::rc::Rc;

struct Form {
    visible: bool,
    enabled: bool,
    selected: usize,
    digits: String,
    latin: String,
    menu: Rect,
    first: Rect,
    second: Rect,
    second_id: egui::Id,
}
impl Form {
    fn draw(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>, opener: Opener) {
        if !self.visible {
            return;
        }
        let mut widgets = cx.widgets();
        ui.add_enabled_ui(self.enabled, |ui| {
            ui.set_width(300.0);
            self.menu = Dropdown::new("choice", &OPTIONS, &mut self.selected)
                .opener(opener)
                .show(ui, &mut widgets)
                .rect;
            self.first = TextField::new(&mut self.digits)
                .id_salt("digits")
                .keys(Keys::Digits)
                .show(ui, &mut widgets)
                .rect;
            let response = TextField::new(&mut self.latin)
                .id_salt("latin")
                .keys(Keys::Latin)
                .show(ui, &mut widgets);
            self.second = response.rect;
            self.second_id = response.id;
        });
    }
}

fn text_position(h: &mut Harness, wanted: &str) -> Pos2 {
    fn find(shape: &egui::Shape, wanted: &str) -> Option<Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == wanted => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, wanted)),
            _ => None,
        }
    }
    h.frame_shapes()
        .iter()
        .find_map(|shape| {
            find(&shape.shape, wanted)
                .map(|rect| rect.intersect(shape.clip_rect).intersect(h.screen_rect()))
                .filter(|rect| rect.is_positive())
                .map(|rect| rect.center())
        })
        .unwrap_or_else(|| panic!("required rendered control missing: {wanted}"))
}

fn tap_key(h: &mut Harness, label: &str) {
    let rect = h.shell.osk().key_rect(label).expect("required OSK key missing");
    assert!(h.screen_rect().contains_rect(rect), "OSK key outside viewport");
    h.tap(rect.center());
    h.frames(2);
}

fn type_into_each_field(h: &mut Harness, form: &Rc<RefCell<Form>>) {
    let first = form.borrow().first.center();
    h.tap(first);
    h.frames(32);
    assert!(h.shell.osk().is_shown(), "history blocked the digits keyboard");
    assert!(
        matches!(h.shell.osk().layout(), OskLayout::NumPad { .. }),
        "history retained the wrong keyboard layout"
    );
    h.key(egui::Key::End);
    let before = form.borrow().digits.clone();
    let untouched = form.borrow().latin.clone();
    tap_key(h, "7");
    assert_eq!(form.borrow().digits, before + "7", "OSK typed into the wrong field");
    assert_eq!(form.borrow().latin, untouched, "OSK changed the inactive field");

    let second = form.borrow().second.center();
    h.tap(second);
    h.frames(32);
    assert!(h.shell.osk().is_shown(), "history blocked the Latin keyboard");
    assert_eq!(h.shell.osk().layout(), &OskLayout::Qwerty);
    h.key(egui::Key::End);
    let before = form.borrow().latin.clone();
    let untouched = form.borrow().digits.clone();
    tap_key(h, "a");
    assert_eq!(form.borrow().latin, before + "a", "OSK focus was not transferred");
    assert_eq!(form.borrow().digits, untouched, "OSK leaked into the old field");
}

fn exercise(opener: Opener, animated: bool, history: &[u8]) {
    let mut config = single_level_access();
    config.motion.reduce = !animated;
    config.osk.layout = "qwerty".into();
    let mut h = Harness::new(config, Services::null())
        .unwrap()
        .with_size(1000.0, 900.0);
    let form = Rc::new(RefCell::new(Form {
        visible: true,
        enabled: true,
        selected: 0,
        digits: String::new(),
        latin: String::new(),
        menu: Rect::NOTHING,
        first: Rect::NOTHING,
        second: Rect::NOTHING,
        second_id: egui::Id::NULL,
    }));
    let state = Rc::clone(&form);
    h.shell.add(screen("form", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
        state.borrow_mut().draw(ui, cx, opener);
    }));
    h.shell.launch(LaunchAction::open("form"));
    h.frames(32);
    for &operation in history {
        match operation % 8 {
            0 => {
                let at = form.borrow().first.center();
                h.tap(at);
            }
            1 => {
                let at = form.borrow().second.center();
                h.tap(at);
            }
            2 => {
                let at = form.borrow().menu.center();
                h.tap(at);
            }
            3 => {
                form.borrow_mut().enabled = false;
                h.frames(2);
                form.borrow_mut().enabled = true;
            }
            4 => {
                form.borrow_mut().visible = false;
                h.frames(2);
                form.borrow_mut().visible = true;
            }
            5 => h.shell.handle().toggle_overlay(),
            6 => {
                h.set_size(900.0, 1000.0);
                h.frames(2);
                h.set_size(1000.0, 900.0);
            }
            _ => h.sleep(0.25),
        }
        h.frames(32);
        assert!(!h.shell.is_faulted(), "history entered fault quarantine");
    }
    if !h.shell.overlay().is_closed() {
        let panel = h.shell.overlay().frame().panel.expect("overlay panel missing");
        let viewport = h.screen_rect();
        let scrim = Rect::from_min_max(
            egui::pos2(viewport.left(), panel.bottom()),
            viewport.max,
        );
        assert!(scrim.is_positive(), "overlay dismissal has no input target");
        h.tap(scrim.center());
        h.frames(32);
        assert!(h.shell.overlay().is_closed(), "history blocked overlay dismissal");
    }
    // An ordinary outside tap dismisses an open dropdown. No focus/memory reset.
    h.tap(egui::pos2(800.0, 240.0));
    h.frames(32);
    type_into_each_field(&mut h, &form);
    form.borrow_mut().selected = 0;
    let at = form.borrow().menu.center();
    h.tap(at);
    h.frames(32);
    let target = text_position(&mut h, "Bravo");
    h.tap(target);
    h.frames(32);
    assert_eq!(form.borrow().selected, 1, "keyboard history blocked popup selection");
    type_into_each_field(&mut h, &form);
    assert!(!h.shell.is_faulted());
}

#[test]
fn popup_keyboard_and_overlay_histories_preserve_real_input_paths() {
    for opener in [Opener::Anchored, Opener::Grid, Opener::Sheet, Opener::Search] {
        for animated in [false, true] {
            for history in [
                &[][..],
                &[0, 2, 1, 3, 2, 4],
                &[1, 2, 5, 6, 5, 0],
                &[0, 3, 1, 4, 2, 7, 6],
            ] {
                exercise(opener, animated, history);
            }
        }
    }
}
