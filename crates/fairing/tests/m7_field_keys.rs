//! **A field asks for the keys it is typed on** — `widgets::Keys`.
//!
//! The keyboard has one layout per device (`[osk] layout`), and whichever it is, it is wrong for
//! some field: a PIN on a qwerty, a password on a number pad. A field now says what it takes, and
//! the keyboard shows those keys while the field has the focus and goes back to the device's
//! layout when a field that asks for nothing takes it.

#![cfg(feature = "osk")]

use fairing::osk::OskLayout;
use fairing::testing::{single_level_access, Harness};
use fairing::widgets::{Keys, NumberField, TextField};
use fairing::{layout, screen, Cx, Shell};

/// The screen's state: one field per kind of keys.
#[derive(Default)]
struct Form {
    pin: String,
    name: String,
    host: String,
    offset: f64,
}

/// A page with a PIN field, a plain field, a Latin field and a signed decimal number field, on a
/// device whose keyboard is `layout`.
fn harness(layout: &str) -> fairing::Result<Harness> {
    let mut config = single_level_access();
    layout.clone_into(&mut config.osk.layout);
    config.motion.reduce = true;
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(config).build(ctx)?;
        shell.add(screen("p", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            layout::page(ui, cx, "p", |ui, cx| {
                cx.with_app::<Form, _>(|f, cx| {
                    TextField::new(&mut f.pin)
                        .hint("PIN")
                        .id_salt("pin")
                        .keys(Keys::Digits)
                        .show(ui, &mut cx.widgets());
                    TextField::new(&mut f.name)
                        .hint("Name")
                        .id_salt("name")
                        .show(ui, &mut cx.widgets());
                    TextField::new(&mut f.host)
                        .hint("Host")
                        .id_salt("host")
                        .keys(Keys::Latin)
                        .show(ui, &mut cx.widgets());
                    NumberField::new(&mut f.offset)
                        .range(-10.0..=10.0)
                        .decimals(1)
                        .show(ui, &mut cx.widgets());
                });
            });
        }));
        shell.launch(fairing::LaunchAction::open("p"));
        Ok(shell)
    })?
    .with_app(Form::default());
    // Tall enough that the keyboard a field brings up never covers the next field to tap.
    h.set_size(1024.0, 1400.0);
    h.run_for(0.5);
    Ok(h)
}

fn keys_after_tapping(h: &mut Harness, label: &str) -> fairing::Result<OskLayout> {
    h.tap_text(label)?;
    h.run_for(0.5);
    if !h.shell.osk().is_shown() {
        return Err(fairing::Error::Config(format!(
            "the keyboard did not come up for `{label}`"
        )));
    }
    Ok(h.shell.osk().layout().clone())
}

/// On a qwerty device: the PIN field gets the digits, the number field a pad with the point and
/// the sign its range and decimals need, and a field that asks for nothing gets the qwerty back.
#[test]
fn each_field_gets_its_own_keys_and_a_plain_one_gets_the_device_s() -> fairing::Result<()> {
    let mut h = harness("qwerty")?;
    assert_eq!(
        keys_after_tapping(&mut h, "PIN")?,
        OskLayout::NumPad {
            decimal: false,
            sign: false
        }
    );
    assert_eq!(keys_after_tapping(&mut h, "Name")?, OskLayout::Qwerty);
    assert_eq!(
        keys_after_tapping(&mut h, "0.0")?,
        OskLayout::NumPad {
            decimal: true,
            sign: true
        }
    );
    assert_eq!(keys_after_tapping(&mut h, "Host")?, OskLayout::Qwerty);
    Ok(())
}

/// On a number-pad device a Latin field gets the letters, and the pad comes back after.
#[test]
fn a_latin_field_brings_letters_to_a_number_pad_device() -> fairing::Result<()> {
    let mut h = harness("numpad")?;
    assert_eq!(keys_after_tapping(&mut h, "Host")?, OskLayout::Qwerty);
    assert!(
        matches!(
            keys_after_tapping(&mut h, "Name")?,
            OskLayout::NumPad { .. }
        ),
        "the device's pad is back for a field that asks for nothing"
    );
    Ok(())
}

/// On a Hangul device a Latin field opens on the English side and keeps the `한/영` key; a plain
/// field brings the Hangul back.
#[test]
fn a_latin_field_on_a_hangul_device_keeps_the_language_key() -> fairing::Result<()> {
    let mut h = harness("hangul")?;
    assert_eq!(keys_after_tapping(&mut h, "Host")?, OskLayout::Qwerty);
    assert!(h.shell.osk().has_lang_key(), "the 한/영 key stays");
    assert_eq!(keys_after_tapping(&mut h, "Name")?, OskLayout::Hangul);
    Ok(())
}
