//! **A shop payment kiosk** — both a harness for measuring how far a commercial screen stands on the
//! crate alone, and a showcase.
//!
//! ```text
//! cargo run -p fairing --features runner-x11,mock --example kiosk -- --size=1080x2560 --panel-mm=380x900
//! cargo run -p fairing --features runner-x11,mock --example kiosk -- --size=1280x800  --panel-mm=217x136
//! cargo run -p fairing --features runner-x11,mock --example kiosk -- --size=480x800   --panel-mm=56x94
//! cargo run -p fairing --features runner-x11,mock --example kiosk -- --size=1080x2560 --panel-mm=380x900 --lang=ko
//! ```
//!
//! **`--panel-mm` is not decoration.** Without it the shell reckons the density unknown and falls
//! back to its assumption (1 du = 1/160 in), so a 38-inch panel and a 7-inch panel use the same du —
//! and this demo's whole point, that the column count comes from **the physical size** rather than
//! the resolution, dies with it.
//!
//! # What this example measures
//!
//! It is outside the library. But there is no other way to check that **things work as declared** —
//! the crate's tests check what the crate believes to be right, and this example checks what an
//! integrator actually runs into. So there is one rule:
//!
//! > **It never goes outside the public API.** No `pub(crate)`, no internal modules. Everywhere
//! > something is patched by hand is written down [below](#what-the-crate-still-does-not-have--the-places-patched-by-hand).
//!
//! # One app, three devices
//!
//! A portrait kiosk, a counter POS and a customer-facing display stand in one shop, and there is no
//! reason for the three to be different apps. **Three layouts share the same [`Order`], and the
//! screen's aspect ratio settles which layout it is** — `--layout=portrait|counter|compact` forces
//! one, for diagnosis.
//!
//! | Layout | Device | Flow |
//! |---|---|---|
//! | `portrait` | A portrait 38" kiosk (380 × 900 mm) | attract → eat in/take away → menu → confirm → method → approval → receipt |
//! | `counter` | A counter 10.1" POS (217 × 136 mm) | the menu and the cart on one screen, with only the payment pushed |
//! | `compact` | A customer-facing 4.3" display (56 × 94 mm) | it only shows the same order |
//!
//! ## Why portrait divides top and bottom — the reach bands
//!
//! Stand a 900 mm portrait panel on an 800 mm base and the screen spans 800–1700 mm from the floor.
//! The top 300 mm is around an adult's eye level, where the hand does not readily go — so **the top
//! displays and the bottom operates** ([`HERO_FRAC`]). The 0.8–1.2 m control range of the
//! accessibility regulations applies to essential controls, and in this layout that is the pinned
//! bottom bar. The bar sits at the very bottom of the screen = just above the base, so it is within
//! the regulation. A payment button buried in a scroll and a control 1.2 m up are the same kind of
//! accident. A floor-side trim ([`FLOOR_FRAC`]) is left open too, for a lower installation.
//!
//! ## Read standing, at 70 cm — the two axes of size
//!
//! **The touch target and the visual size are different axes.** `touch_target` is the *floor* the
//! finger sets (a gloved 13 mm here), and `row_height` is taken from the text, a reading size; both
//! stay around a physical 13–15 mm even on a large panel — which is right. But on a 380 × 900 mm panel 13 mm is 1.4 % of the screen, so laying out by multiples
//! alone looks like a phone screen shrunk and pasted on. So this example **derives the visible size
//! from the screen and props it up on the finger** — [`vrow`] · [`pad`] · [`bar_rows`] ·
//! [`band_text`] are all the same rule.
//!
//! Text alone is the exception and is raised through **tokens** ([`standing_metrics`]). It is the
//! viewing distance that went from 30 cm to 70 cm, not the screen that grew, so writing it in
//! physical size (`mm`) rather than a screen fraction is right. The counter POS is tapped from 45 cm
//! away, so it keeps the defaults.
//!
//! ## The language is a setting — and switching it is the harness
//!
//! The kiosk draws in whatever the shell's `ui.locale` setting says, the key the crate's own
//! Language screen writes. The switch in the top corner of the attract screen writes it, and every
//! screen follows it on the next frame ([`follow_locale`]), so the whole flow turns over at once;
//! `--lang=ko|en` only says where it starts.
//!
//! Two sets of wording ([`t`]) are in there for more than showing the showcase abroad. **English is
//! 1.5 – 2 times longer than Korean for the same meaning** — `Pay now` (7 characters) against
//! `결제하기` (4), `Cash · Call Staff` (17) against `현금 · 직원 호출` (9). Fitting to the Korean
//! lengths alone and then saying "the width fitting works" is a guess, not a check. Switching the
//! language and running it **broke two places straight away.**
//!
//! - **The bottom bar's hint was cut to `Pick somet…`.** `Label::truncate()` truncates where there
//!   is no room, and a truncated hint reads as a fault rather than as guidance. The rule was changed
//!   to shrink it in, and drop it where even that will not do ([`menu_bar`]).
//! - **The three payment-method tiles ended up at different text sizes.** [`centered_fit`] shrinks
//!   each cell separately, and in Korean all three fitted so it never showed. A control that reads
//!   as one set has its size unified against the longest ([`fit_all`]). **Fixing it improved the
//!   Korean screen too** — `간편결제` had been the only large one.
//!
//! The currency format differs as well — Korean puts the unit after (`13,800원`) and English the
//! symbol before (`₩13,800`) ([`won`]).
//!
//! # What this example had fixed in the crate
//!
//! Actually writing it turned up **four defects on the crate's side**, and all four were fixed. That
//! is the harness's value.
//!
//! - **[`layout::Grid`] overflowed the width.** The tile-width formula sets aside only `gap ×
//!   (columns + 1)`, but `ui.horizontal` inserted egui's default `item_spacing.x` (10 du) between the
//!   items as well. At 2 columns that adds 40 du and **the right-hand column was cut off the screen.**
//!   The settings screens are 1 column, so it never showed.
//! - **The text sizes were not tokens.** 13/16/17/22 were written inside `Theme::egui_style()`, so
//!   **not one rung of the override ladder reached the text size.** Now
//!   [`TypeScale`](fairing::theme::TypeScale) is on `Metrics` and can be written in `mm` through
//!   `MetricsSpec` — a necessary axis for a screen read standing in front of 21.5 inches.
//! - **Growing a [`BigButton`] left its label at 17 du.** Small text sat in the middle of a 200 du
//!   hero button. `BigButton::text_size` was added.
//! - **`[theme.palette]` could not take an alpha.** It read only the six digits of `#RRGGBB`, so
//!   there was no way at all to change [`ColorRole::Scrim`](fairing::ColorRole) or `Pressed` from the
//!   config — and being translucent is those two roles' reason to exist. It was caught while putting
//!   the brand palette onto this file, and `#RRGGBBAA` is now read too.
//!
//! # The brand goes on by overriding role colours alone
//!
//! `kiosk.toml`'s `[theme.palette]` is all of it — not one line of the layout or the widgets changes.
//! The crate's default accent is a steel blue (`#2878A9` dark, `#106AA2` light) — the arc iOS,
//! One UI and Windows 11 all land in, pulled off its pure end, because a default has to sit under
//! anyone's product without looking like a stock panel. Over this shop's warm photography it still read as *the shell's* colour rather than the
//! room's; overridden with amber (`#A4E`) it became the same colour as the photograph's key
//! light. **This is one of the things
//! the example means to show about the crate** — a product-grade look comes out of twenty lines of
//! config, with no fork.
//!
//! # What the crate still does not have — the places patched by hand
//!
//! The rest the example carries. **Five went up into the crate because of this harness** (1 · 2 · 3 ·
//! 4 · 6), and 5 sent only its aspect-ratio arithmetic up. The remaining three stand as they are. How
//! many lines each one repeats is written down too.
//!
//! 1. ~~**There is no layout for dividing the portrait reach bands.**~~ → **The decision went up into
//!    the crate** ([`layout::split_height`]). [`action_bar`](layout::action_bar) only pins a bar to
//!    the bottom and cannot move the body's **starting point** down, so [`bands`] was measuring "is
//!    it upright, and tall enough for two bands" itself. What is left in that function is **this
//!    device's share** alone — the top band's fraction ([`HERO_FRAC`]) and the floor trim
//!    ([`FLOOR_FRAC`]). Both are values the mounting height settles (an 800 mm base, a 900 mm panel)
//!    and the crate cannot know them, which is why `split_height` **takes the fraction as an
//!    argument** — unlike the four before it, this is a place where the crate does not settle the
//!    value. [`band_ui`], which produces a child `Ui`, stays: `new_child(max_rect)` is an egui idiom
//!    (written out plainly in eighteen places inside the crate too) and not a hole worth wrapping.
//! 2. ~~**[`layout::Grid`] has no column cap.**~~ → **It went up into the crate**
//!    ([`Grid::max_columns`](layout::Grid::max_columns)). "As many as the width allows" is the right
//!    principle, but a strip with a fixed count — five category chips, two of eat-in/take-away —
//!    needed an exception. Written as a fixed multiple it **folded to 1 column** in the counter POS's
//!    narrow left column, and the five lines that worked the minimum cell back from the width to
//!    avoid that (`cell_rows_for`) were **duplicated in six places.** Now all six are one line of
//!    `.max_columns(n)` and the back-calculation function is gone.
//! 3. ~~**A grid does not fill the height left over.**~~ → **It went up into the crate**
//!    ([`Grid::fill_height`](layout::Grid::fill_height)). Filling from the top left the bottom of a
//!    tall band entirely empty, and `fill_rows`, measuring `remaining_height / rows / row_height`,
//!    plus a line of `add_space` turned up in **four places**. Taking the cap as **a ratio against the
//!    width** rather than a row multiple (`6.4`) was settled then — what an integrator really wants to
//!    stop is "the tile getting taller than it is wide", not a particular row count. Now the four are
//!    one line of `.fill_height(ratio)` with neither `fill_rows` nor `add_space`.
//! 4. ~~**There is no width fitting where the drawing is done with a painter.**~~ → **It went up into
//!    the crate** ([`layout::fit_text`] · [`layout::fit_size`]). `egui::Label::truncate` requires a
//!    `Ui` and all there is inside a grid cell is a `Painter`, so the measure-and-shrink formula
//!    turned up in **sixteen places**. `fit_size`, which matches the size across sibling cells, is
//!    what `--lang=en` caught — shrinking each cell separately gives the three payment methods
//!    different text sizes. [`centered_fit`] is now a six-line shell that only bundles the centring.
//! 5. **The aspect-ratio arithmetic went up** ([`layout::contain`] · [`layout::cover_uv`]).
//!    `IconRef::Texture` holds only a `TextureId` and so **does not know the original size**, and
//!    therefore fills the Rect it is given as it stands — it is not the kind of thing the crate can
//!    fix from inside (only whoever uploaded the texture knows its size), so sending up the fitting
//!    arithmetic alone was the right line. **The drawing side stays in the example**: [`photo`] and
//!    [`backdrop`] are now four lines each, and the gradient scrim that puts text over a background
//!    photo ([`scrim`]) is built directly with an `egui::Mesh`. How strong the scrim is and which way
//!    it runs is chosen per photograph, so the crate cannot settle it.
//! 6. ~~**A [`BigButton`] is invisible on a card.**~~ → **Fixed in the crate.** `ButtonKind::Normal`
//!    is `SurfaceVariant` and vanished when laid on a card of the same colour, and this example and
//!    `demo` each wrote a different workaround **without knowing of each other** (here a dark pill
//!    beneath it, there the button moved outside the card). Two workarounds mean an API hole — now
//!    `Normal` carries an `Outline` border. [`cart_line`]'s pill was kept: a stepper's three buttons
//!    have to read as one lump, and one plate suits that better than three borders.
//! 7. **There is no quantity stepper** (deliberately). [`cart_line`] builds one out of three
//!    `BigButton`s, including the branch that stacks them over two lines in a narrow column. A
//!    hundred lines. It is right that it is not the crate's, but on a payment terminal it comes up
//!    every time.
//! 8. **There is no modal or confirmation sheet.** With no overlay to put `Please insert your card`
//!    on, a fullscreen screen ([`Progress`]) is pushed, and then it stacks on the back stack and going
//!    back has to be blocked separately with `nav_bar: Hide` plus `edge_guard`.
//! 9. **The padding tokens are not on the ladder.** `MetricsSpec` has no `screen_inset` or card gap,
//!    so `Metrics::default()`'s 12 du comes down as it is. The crate's judgement that padding is
//!    "about the field of view" and stays in `du` is right in itself — but on a 380 mm panel 12 du
//!    (≈ 4 mm) makes a card look stuck to the bezel, and **there is no knob at rung 2 to fix it.**
//!    Text can be raised through [`TypeScale`](fairing::theme::TypeScale) while the padding around
//!    that text cannot, an asymmetry, so [`pad`] derives it from the screen width on the example's
//!    side.
//!
//! Installing the Hangul font was moved down into `examples/common` (`common::korean_fonts`) — it is
//! right that the crate goes only as far as finding the path, and the ten lines of loading it into a
//! `FontSet` after that were the same in every example. This file's version swallowed the failure
//! whole, so an unreadable font left nothing but □ for no stated reason; merging them, it followed
//! `demo.rs`'s warning. The currency format ([`won`]) stays here — it differs by country, so being
//! outside the crate is right, and two examples using different currencies would not suit a shared
//! module either.
//!
//! Conversely there is **not one line of layout or styling code**. The column arithmetic, the pinned
//! bottom bar, the card backgrounds, the left/right split decision, the touch targets and the density
//! handling are all the crate's.

// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the same treatment as the crate's render modules).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod common;

use fairing::icon;
use fairing::icons::{IconColor, IconStyle};
use fairing::layout;
use fairing::notify::{Level, Notification, NotificationId, Toast};
use fairing::screen::{screen, screen_with, BarMode, ChromePolicy, Cx, Lifecycle, Screen};
use fairing::settings::SettingValue;
use fairing::theme::ColorRole;
use fairing::widgets::{
    BadgeAnchor, ChipItem, ChipRow, CountBadge, MediaCard, Stepper, ASPECT_SQUARE,
};
use fairing::widgets::{BigButton, ButtonKind, SegmentedControl};
use fairing::{icons, IconRef, LaunchAction, ShellConfig, Wallpaper};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

const CONFIG: &str = include_str!("kiosk.toml");

/// The share of the height the **display band** takes in the portrait layout.
///
/// # The mounting height settles this number
///
/// The installation this demo assumes: **a 900 mm panel stood on an 800 mm base.** The screen spans
/// 800 mm to 1700 mm from the floor — the common shape of a shop kiosk.
///
/// | Where on the screen | From the floor | What goes there |
/// |---|---|---|
/// | The top 34 % | 1394 – 1700 mm | Around eye level. It **only shows** |
/// | The bottom 66 % | 800 – 1394 mm | It is pressed. The pinned bottom bar sits at 800 – 950 mm |
///
/// The 0.8 – 1.2 m control range of the accessibility regulations applies to **essential controls**,
/// and in this layout that is the bottom bar — pay, start. The bar sits at the very bottom of the
/// screen = just above the base = 800 mm, so it is within the regulation. The menu grid goes above
/// that, but it is a secondary control with scrolling and filters.
///
/// **A different mounting height means recalculating this value.** The crate does not know where the
/// device stands, so this is the integrator's arithmetic.
const HERO_FRAC: f32 = 0.34;

/// The share of the height the **out-of-reach strip along the floor** takes in the portrait layout.
///
/// With an 800 mm base the very bottom of the screen is the control range's floor, so it is **0** —
/// there is nothing to trim. It was first set to 0.14 for an assumed 600 mm base, and that left 14 %
/// of the screen empty below the bottom bar, which looked like a half-drawn screen. **Where the
/// installation is low enough to want a strip** (a counter-top form with no base, hung low on a
/// wall), raise this — with a 600 mm base, the bottom 200 mm of the screen is below 0.8 m, so 0.22.
const FLOOR_FRAC: f32 = 0.0;

/// How long a mock card approval takes. Changed with `--dwell-ms=N`.
const DWELL_MS_DEFAULT: u64 = 2500;

// ── Language (the `ui.locale` setting; `--lang=ko|en` to start in) ───────────

/// The language of the on-screen wording.
///
/// **A showcase and a harness at once.** The English wording is 1.5 – 2 times longer than the Korean
/// for the same meaning — "Pay now" (7 characters) against "결제하기" (4), "Cash · call staff" (17)
/// against "현금 · 직원 호출" (9). Fitting to the Korean lengths alone and then saying "the width
/// fitting works" is a guess, not a check. Switching the language and running it shows whether
/// [`centered_fit`], `Label::truncate` and [`layout::fit_text`] **really** hold up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    Ko,
    En,
}

impl Lang {
    /// `--lang=ko|en`, the language to start in. Not given, it is English — the public repository's
    /// default screen is English. From there the `ui.locale` setting decides ([`follow_locale`]).
    fn parse(args: &[String]) -> Self {
        match args.iter().find_map(|a| a.strip_prefix("--lang=")) {
            Some("ko") => Self::Ko,
            Some("en") | None => Self::En,
            Some(other) => {
                log::warn!("--lang={other} is not a known language (ko | en)");
                Self::En
            }
        }
    }
}

/// The language every screen is drawing in this frame.
///
/// **It is not dragged through thirty screens as an argument.** It changes when the `ui.locale`
/// setting does, which is rarely, and passing it down would give a function like [`cart_line`]
/// seven arguments. [`follow_locale`] brings it up to the setting at the top of every screen. Yes,
/// the example is single-threaded — it is still an atomic, being a `static`.
static LANG: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// The setting the language follows: the key the crate's own Language screen writes.
const LOCALE_KEY: &str = "ui.locale";

/// The languages on offer, as `(ui.locale tag, name)`. Each name is written in its own language,
/// as a language list always is: someone looking for theirs cannot read the others.
const LANGUAGES: [(&str, &str); 2] = [("en", "English"), ("ko", "한국어")];

/// **Bring [`LANG`] up to the `ui.locale` setting**, once something has written it.
///
/// Called at the top of every screen ([`with_order`]), so a write from anywhere — the switch on
/// the attract screen, the crate's Language settings screen, a `ShellHandle` on another thread —
/// redraws every screen in that language on the next frame. Until something writes it, the
/// language stays where `--lang` started it.
fn follow_locale(cx: &Cx<'_>) {
    if let Some(SettingValue::Text(tag)) = cx.settings.get(&LOCALE_KEY.into()) {
        set_lang(if tag == "ko" { Lang::Ko } else { Lang::En });
    }
}

/// Every screen's way in: the language follows the setting ([`follow_locale`]), then the body
/// gets the order.
fn with_order(cx: &mut Cx<'_>, body: impl FnOnce(&mut Order, &mut Cx<'_>)) {
    follow_locale(cx);
    cx.with_app::<Order, _>(body);
}

fn set_lang(lang: Lang) {
    LANG.store(
        u8::from(lang == Lang::En),
        std::sync::atomic::Ordering::Relaxed,
    );
}

fn lang() -> Lang {
    if LANG.load(std::sync::atomic::Ordering::Relaxed) == 0 {
        Lang::Ko
    } else {
        Lang::En
    }
}

/// **Both sets are written side by side.** Pulling them out into a key table (`t("cart.pay")`) moves
/// the wording away from the code and hides what is used where — in a twenty-screen example, keeping
/// the two together where they are used reads better, and **a missed translation shows up at compile
/// time.** Once there are hundreds of strings, that is the moment to move to something like `fluent`.
fn t(ko: &'static str, en: &'static str) -> &'static str {
    match lang() {
        Lang::Ko => ko,
        Lang::En => en,
    }
}

// ── The domain (all the example's — the crate does not know what is being sold) ──

/// One menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Item {
    id: &'static str,
    ko: &'static str,
    en: &'static str,
    price: u32,
    cat: Cat,
    /// Today's pick — the tile's colour differs.
    pick: bool,
    /// Sold out — dimmed, and a tap does not add it.
    sold_out: bool,
}

impl Item {
    fn name(self) -> &'static str {
        t(self.ko, self.en)
    }
}

/// A category. The chips at the top of the menu screen filter by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cat {
    /// Everything. On a large portrait panel this is the default — showing a customer standing in
    /// front of 32 inches only six plates and hiding the rest behind a tab is not using the screen.
    All,
    Coffee,
    Tea,
    Dessert,
    Bean,
}

impl Cat {
    const ALL: [Self; 5] = [
        Self::All,
        Self::Coffee,
        Self::Tea,
        Self::Dessert,
        Self::Bean,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::All => t("전체", "All"),
            Self::Coffee => t("커피", "Coffee"),
            Self::Tea => t("음료", "Drinks"),
            Self::Dessert => t("디저트", "Desserts"),
            Self::Bean => t("원두", "Beans"),
        }
    }

    /// Whether this tab holds it.
    const fn holds(self, item: Cat) -> bool {
        matches!(self, Self::All) || (self as u8) == (item as u8)
    }
}

/// The product table. In a real shop it would come from a server.
const MENU: &[Item] = &[
    Item {
        id: "americano",
        ko: "아메리카노",
        en: "Americano",
        price: 4500,
        cat: Cat::Coffee,
        pick: false,
        sold_out: false,
    },
    Item {
        id: "latte",
        ko: "카페라떼",
        en: "Caffè Latte",
        price: 5000,
        cat: Cat::Coffee,
        pick: true,
        sold_out: false,
    },
    Item {
        id: "cold-brew",
        ko: "콜드브루",
        en: "Cold Brew",
        price: 5500,
        cat: Cat::Coffee,
        pick: false,
        sold_out: false,
    },
    Item {
        id: "espresso",
        ko: "에스프레소",
        en: "Espresso",
        price: 4000,
        cat: Cat::Coffee,
        pick: false,
        sold_out: true,
    },
    Item {
        id: "cappuccino",
        ko: "카푸치노",
        en: "Cappuccino",
        price: 5000,
        cat: Cat::Coffee,
        pick: false,
        sold_out: false,
    },
    Item {
        id: "vanilla",
        ko: "바닐라라떼",
        en: "Vanilla Latte",
        price: 5500,
        cat: Cat::Coffee,
        pick: false,
        sold_out: false,
    },
    Item {
        id: "earl-grey",
        ko: "얼그레이",
        en: "Earl Grey",
        price: 4500,
        cat: Cat::Tea,
        pick: false,
        sold_out: false,
    },
    Item {
        id: "peach-tea",
        ko: "복숭아 아이스티",
        en: "Peach Iced Tea",
        price: 4800,
        cat: Cat::Tea,
        pick: true,
        sold_out: false,
    },
    Item {
        id: "lemonade",
        ko: "레모네이드",
        en: "Lemonade",
        price: 5200,
        cat: Cat::Tea,
        pick: false,
        sold_out: false,
    },
    Item {
        id: "choco",
        ko: "핫초코",
        en: "Hot Chocolate",
        price: 5000,
        cat: Cat::Tea,
        pick: false,
        sold_out: false,
    },
    Item {
        id: "cheesecake",
        ko: "치즈케이크",
        en: "Cheesecake",
        price: 6500,
        cat: Cat::Dessert,
        pick: true,
        sold_out: false,
    },
    Item {
        id: "brownie",
        ko: "브라우니",
        en: "Brownie",
        price: 5000,
        cat: Cat::Dessert,
        pick: false,
        sold_out: false,
    },
    Item {
        id: "scone",
        ko: "플레인 스콘",
        en: "Plain Scone",
        price: 3800,
        cat: Cat::Dessert,
        pick: false,
        sold_out: false,
    },
    Item {
        id: "cookie",
        ko: "쿠키",
        en: "Cookie",
        price: 2500,
        cat: Cat::Dessert,
        pick: false,
        sold_out: true,
    },
    Item {
        id: "ethiopia",
        ko: "에티오피아 200g",
        en: "Ethiopia 200 g",
        price: 18000,
        cat: Cat::Bean,
        pick: false,
        sold_out: false,
    },
    Item {
        id: "colombia",
        ko: "콜롬비아 200g",
        en: "Colombia 200 g",
        price: 16000,
        cat: Cat::Bean,
        pick: false,
        sold_out: false,
    },
];

/// One cart line.
#[derive(Debug, Clone, Copy)]
struct Line {
    item: Item,
    qty: u32,
}

/// A payment method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Card,
    Easy,
    Cash,
}

impl Method {
    const ALL: [Self; 3] = [Self::Card, Self::Easy, Self::Cash];

    fn label(self) -> &'static str {
        match self {
            Self::Card => t("신용·체크카드", "Credit · Debit"),
            Self::Easy => t("간편결제", "Mobile Pay"),
            Self::Cash => t("현금 · 직원 호출", "Cash · Call Staff"),
        }
    }
}

/// The order's state. Eight screens share it. **What to share is the app's decision**.
#[derive(Debug, Default)]
struct Order {
    lines: Vec<Line>,
    dine_in: bool,
    method: Option<Method>,
    cat: Option<Cat>,
    /// The order number issued. The receipt screen reads it.
    serial: u32,
}

impl Order {
    fn add(&mut self, item: Item) {
        if let Some(line) = self.lines.iter_mut().find(|l| l.item.id == item.id) {
            line.qty += 1;
        } else {
            self.lines.push(Line { item, qty: 1 });
        }
    }

    /// Increase or decrease the quantity. At 0 the line comes out.
    fn bump(&mut self, id: &str, delta: i32) {
        let Some(index) = self.lines.iter().position(|l| l.item.id == id) else {
            return;
        };
        let Some(line) = self.lines.get_mut(index) else {
            return;
        };
        let next = i64::from(line.qty) + i64::from(delta);
        if next <= 0 {
            self.lines.remove(index);
        } else {
            line.qty = u32::try_from(next).unwrap_or(1).min(99);
        }
    }

    fn total(&self) -> u32 {
        self.lines.iter().map(|l| l.item.price * l.qty).sum()
    }

    fn count(&self) -> u32 {
        self.lines.iter().map(|l| l.qty).sum()
    }

    /// The order number string. Eat-in is `A`, take-away `P`.
    fn ticket(&self) -> String {
        format!(
            "{}-{:03}",
            if self.dine_in { 'A' } else { 'P' },
            self.serial % 1000
        )
    }

    fn clear(&mut self) {
        self.lines.clear();
        self.method = None;
        self.dine_in = false;
    }
}

/// `4500` → `"4,500원"`. **Currency formatting is outside the crate** — it differs by country.
fn won(amount: u32) -> String {
    let digits = amount.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    // **The currency format differs by language too.** Korean puts the unit after (`4,500원`) and
    // English the symbol before (`₩4,500`). The place changes from back to front, so it affects the
    // width fitting — `₩` is narrower than one Hangul character, so the same amount is shorter in English.
    match lang() {
        Lang::Ko => {
            out.push('원');
            out
        }
        Lang::En => format!("₩{out}"),
    }
}

// ── The payment domain's icons (not in the crate — harness result 2) ──────────

/// The 33 **real assets** the example carries (`assets/kiosk/`; the specification is in `SPEC.md`).
///
/// All of them are baked into the executable with `include_bytes!` — no assets need loading onto the
/// device, and the pictures appear whichever directory the example is run from. The decoding is done
/// by [`decode_texture`](common::decode_texture) in `examples/common`, which uses the `image` crate
/// inside — **an integrator dependency rather than the crate's**, since image decoding is the
/// integrator's domain.
struct Art {
    bg_portrait: egui::TextureHandle,
    bg_landscape: egui::TextureHandle,
    bg_ticket: egui::TextureHandle,
    wordmark: egui::TextureHandle,
    mark: egui::TextureHandle,
    dinein: egui::TextureHandle,
    takeout: egui::TextureHandle,
    insert_card: egui::TextureHandle,
    done: egui::TextureHandle,
    /// Item id → the photograph.
    menu: BTreeMap<&'static str, egui::TextureHandle>,
    /// Payment method → the icon.
    pay: BTreeMap<&'static str, egui::TextureHandle>,
}

/// One `(name, bytes)` pair as a texture. The shell comes up even where the decode fails — a device
/// must not fail to start over one picture.
macro_rules! tex {
    ($ctx:expr, $name:literal) => {
        common::decode_texture(
            $ctx,
            include_bytes!(concat!("../../../assets/kiosk/", $name)),
            $name,
        )?
    };
}

impl Art {
    fn load(ctx: &egui::Context) -> fairing::Result<Self> {
        let mut menu = BTreeMap::new();
        for (id, bytes) in MENU_ART {
            menu.insert(*id, common::decode_texture(ctx, bytes, id)?);
        }
        let mut pay = BTreeMap::new();
        for (id, bytes) in PAY_ART {
            pay.insert(*id, common::decode_texture(ctx, bytes, id)?);
        }
        Ok(Self {
            bg_portrait: tex!(ctx, "bg-attract-portrait.webp"),
            bg_landscape: tex!(ctx, "bg-attract-landscape.webp"),
            bg_ticket: tex!(ctx, "bg-ticket.webp"),
            wordmark: tex!(ctx, "logo-wordmark.png"),
            mark: tex!(ctx, "logo-mark.png"),
            dinein: tex!(ctx, "mode-dinein.png"),
            takeout: tex!(ctx, "mode-takeout.png"),
            insert_card: tex!(ctx, "state-insert-card.png"),
            done: tex!(ctx, "state-done.png"),
            menu,
            pay,
        })
    }

    /// An item's photograph. `None` where there is none — the screen stands without it.
    fn item(&self, id: &str) -> Option<&egui::TextureHandle> {
        self.menu.get(id)
    }

    fn method(&self, m: Method) -> Option<&egui::TextureHandle> {
        self.pay.get(match m {
            Method::Card => "pay-card",
            Method::Easy => "pay-easy",
            Method::Cash => "pay-cash",
        })
    }
}

/// The 16 item photographs.
const MENU_ART: &[(&str, &[u8])] = &[
    (
        "americano",
        include_bytes!("../../../assets/kiosk/menu/americano.png"),
    ),
    (
        "latte",
        include_bytes!("../../../assets/kiosk/menu/latte.png"),
    ),
    (
        "cold-brew",
        include_bytes!("../../../assets/kiosk/menu/cold-brew.png"),
    ),
    (
        "espresso",
        include_bytes!("../../../assets/kiosk/menu/espresso.png"),
    ),
    (
        "cappuccino",
        include_bytes!("../../../assets/kiosk/menu/cappuccino.png"),
    ),
    (
        "vanilla",
        include_bytes!("../../../assets/kiosk/menu/vanilla.png"),
    ),
    (
        "earl-grey",
        include_bytes!("../../../assets/kiosk/menu/earl-grey.png"),
    ),
    (
        "peach-tea",
        include_bytes!("../../../assets/kiosk/menu/peach-tea.png"),
    ),
    (
        "lemonade",
        include_bytes!("../../../assets/kiosk/menu/lemonade.png"),
    ),
    (
        "choco",
        include_bytes!("../../../assets/kiosk/menu/choco.png"),
    ),
    (
        "cheesecake",
        include_bytes!("../../../assets/kiosk/menu/cheesecake.png"),
    ),
    (
        "brownie",
        include_bytes!("../../../assets/kiosk/menu/brownie.png"),
    ),
    (
        "scone",
        include_bytes!("../../../assets/kiosk/menu/scone.png"),
    ),
    (
        "cookie",
        include_bytes!("../../../assets/kiosk/menu/cookie.png"),
    ),
    (
        "ethiopia",
        include_bytes!("../../../assets/kiosk/menu/ethiopia.png"),
    ),
    (
        "colombia",
        include_bytes!("../../../assets/kiosk/menu/colombia.png"),
    ),
];

/// The 7 payment and receipt icons. **Flat white**, so they are tinted with a role colour when drawn.
const PAY_ART: &[(&str, &[u8])] = &[
    (
        "pay-card",
        include_bytes!("../../../assets/kiosk/icons/pay-card.png"),
    ),
    (
        "pay-easy",
        include_bytes!("../../../assets/kiosk/icons/pay-easy.png"),
    ),
    (
        "pay-cash",
        include_bytes!("../../../assets/kiosk/icons/pay-cash.png"),
    ),
    (
        "receipt",
        include_bytes!("../../../assets/kiosk/icons/receipt.png"),
    ),
    ("sms", include_bytes!("../../../assets/kiosk/icons/sms.png")),
    ("cup", include_bytes!("../../../assets/kiosk/icons/cup.png")),
    ("bag", include_bytes!("../../../assets/kiosk/icons/bag.png")),
];

// ── Drawing the pictures ──────────────────────────────────────────────────────

/// Draw a photograph inside `into` without cropping. The aspect ratio is [`layout::contain`]'s.
fn photo(ui: &egui::Ui, tex: &egui::TextureHandle, into: egui::Rect, tint: egui::Color32) {
    ui.painter().image(
        tex.id(),
        layout::contain(tex.size_vec2(), into),
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        tint,
    );
}

/// Draw a background filling `into` (what overflows is cropped).
fn backdrop(ui: &egui::Ui, tex: &egui::TextureHandle, into: egui::Rect) {
    ui.painter().image(
        tex.id(),
        into,
        layout::cover_uv(tex.size_vec2(), into),
        egui::Color32::WHITE,
    );
}

/// A scrim deepening towards the bottom. It has to be there to put text over a background photograph —
/// without it the text disappears in the bright parts.
fn scrim(ui: &egui::Ui, rect: egui::Rect, base: egui::Color32, top_a: u8, bottom_a: u8) {
    let mut mesh = egui::Mesh::default();
    let top = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), top_a);
    let bot = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), bottom_a);
    let uv = egui::pos2(0.0, 0.0);
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bot);
    mesh.colored_vertex(rect.right_bottom(), bot);
    for v in &mut mesh.vertices {
        v.uv = uv;
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(2, 1, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

// ── The device's form and the reach bands ─────────────────────────────────────

/// The device's form. Three layouts share the same order state.
///
/// It can be forced with `--layout=`, and without it **the screen's aspect ratio settles it** — which
/// is what this example means to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// A portrait 21.5" kiosk — a customer uses it standing. Divided into display and control bands.
    Portrait,
    /// A counter 10.1" POS — an assistant taps at it sitting down. Divided left and right.
    Counter,
    /// A customer-facing 4.3" display — it only shows.
    Compact,
}

impl Shape {
    /// `--layout=portrait|counter|compact`, or from the screen's aspect ratio.
    fn pick(args: &[String], size: Option<(f32, f32)>) -> Self {
        let forced = args
            .iter()
            .find_map(|a| a.strip_prefix("--layout="))
            .and_then(|raw| match raw {
                "portrait" => Some(Self::Portrait),
                "counter" => Some(Self::Counter),
                "compact" => Some(Self::Compact),
                other => {
                    log::warn!(
                        "--layout={other} is not a known layout (portrait | counter | compact)"
                    );
                    None
                }
            });
        if let Some(shape) = forced {
            return shape;
        }
        let (w, h) = size.unwrap_or((1024.0, 600.0));
        if w / h.max(1.0) < 1.0 {
            Self::Portrait
        } else if w < 640.0 {
            Self::Compact
        } else {
            Self::Counter
        }
    }
}

/// **The display band / the control band** — divided top and bottom on the portrait kiosk (harness
/// result 1).
///
/// Not portrait, or not tall enough for two bands, gives `None`, and then the screens use the whole
/// `ui` — [`layout::split_height`] makes that decision.
///
/// What is left in this function is **this device's share** alone: the top band's fraction
/// ([`HERO_FRAC`]) and the floor trim ([`FLOOR_FRAC`]). Both are values the mounting height settles
/// and the crate cannot know them.
fn bands(ui: &egui::Ui, cx: &Cx<'_>) -> Option<(egui::Rect, egui::Rect)> {
    let hero_h = layout::split_height(ui, cx, HERO_FRAC)?;
    let full = ui.available_rect_before_wrap();
    let top = full.top() + hero_h;
    // The floor trim is **settled by the mounting height** — see [`FLOOR_FRAC`]. With this demo's
    // 800 mm base the very bottom of the screen is the control range's floor, so it is 0.
    let bottom = full.bottom() - full.height() * FLOOR_FRAC;
    Some((
        egui::Rect::from_min_max(full.min, egui::pos2(full.right(), top)),
        egui::Rect::from_min_max(
            egui::pos2(full.left(), top),
            egui::pos2(full.right(), bottom),
        ),
    ))
}

/// A child `Ui` for one band. It is clipped so it does not overflow.
fn band_ui(ui: &mut egui::Ui, rect: egui::Rect) -> egui::Ui {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect);
    child
}

/// A text size derived from the band's height.
///
/// Written as `row_height × a bare number` it would break the ban on multiplying a token by a bare
/// number, and above all a `Heading` of 22 pt = 5.5 mm on a 21.5" portrait panel cannot be read
/// standing 70 cm away. Derived from the band, the text grows as the screen does.
fn band_text(band: egui::Rect, frac: f32) -> egui::FontId {
    egui::FontId::proportional((band.height() * frac).max(11.0))
}

/// One centred line.
fn centered(ui: &egui::Ui, at: egui::Pos2, text: &str, font: egui::FontId, color: egui::Color32) {
    ui.painter()
        .text(at, egui::Align2::CENTER_CENTER, text, font, color);
}

/// One centred line, but **shrunk where it exceeds `max_w`.**
///
/// A thin shell over [`layout::fit_text`] — this file uses centring alone in fifteen places, so
/// `Align2::CENTER_CENTER` is bundled up. The measure-and-shrink formula itself is now in the crate.
fn centered_fit(
    ui: &egui::Ui,
    at: egui::Pos2,
    text: &str,
    max_w: f32,
    font: egui::FontId,
    color: egui::Color32,
) {
    layout::fit_text(
        ui.painter(),
        at,
        egui::Align2::CENTER_CENTER,
        text,
        max_w,
        font,
        color,
    );
}

// ── The screens ───────────────────────────────────────────────────────────────

/// The pinned bottom bar's height (as a multiple of the row height).
///
/// **The touch target and the visual size are different axes.** `row_height` is a reading size of
/// about 13 mm, and on a 32-inch panel 13 mm is 2 % of the screen — written as a multiple alone the bar
/// goes thread-thin and the CTA is invisible. What can be pressed and what catches the eye are
/// different problems, so on a large panel the screen fraction is let win. The floor is still the
/// finger's.
///
/// The crate cannot settle this value for you — "what is the hero of this screen" is what the app knows.
fn bar_rows(cx: &Cx<'_>) -> f32 {
    let row = cx.theme.metrics.row_height;
    (cx.pane.rect.height() * 0.082 / row).clamp(2.0, 5.0)
}

/// The text size of a button in the bottom bar.
fn bar_text(cx: &Cx<'_>) -> f32 {
    bar_rows(cx) * cx.theme.metrics.row_height * 0.20
}

/// **The visual row height** — the vertical unit the lists and cards use.
///
/// The same point [`bar_rows`] makes, made about the body. `row_height` is **the floor the finger
/// and the eye set**, not a large panel's layout unit. Stacking a cart in 13 mm rows on a 900 mm
/// portrait panel makes the lines look like threads — they can be pressed, but they do not look
/// like goods. It is derived from the screen and propped up on `row_height`.
fn vrow(cx: &Cx<'_>) -> f32 {
    (cx.pane.rect.height() * 0.034).max(cx.theme.metrics.row_height)
}

/// **The outer padding** — how far a card is held off the glass (harness result 9).
///
/// `screen_inset` is **not** in [`MetricsSpec`]. The crate's judgement of grouping padding as "about
/// the field of view" and leaving it in `du` is right, but it means it cannot be grown at rung 2 of
/// the token ladder — on a 380 mm panel 12 du (≈ 4 mm) makes a card look stuck to the bezel and
/// there is no knob to fix it. The example derives it from the screen and props it up on the
/// crate's value.
fn pad(cx: &Cx<'_>) -> f32 {
    (cx.pane.rect.width() * 0.030).max(cx.theme.metrics.screen_inset)
}

/// The **back** button on the left of the bottom bar. The nav bar being off, the screen provides the
/// way back itself — an Android-style system bar on a customer screen makes a kiosk look like a phone.
fn back_button(ui: &mut egui::Ui, cx: &mut Cx<'_>) -> bool {
    let ts = bar_text(cx);
    BigButton::new(t("뒤로", "Back"))
        .kind(ButtonKind::Normal)
        .icon(IconRef::Builtin("arrow-left"))
        .text_size(ts)
        .min_size(egui::vec2(ts * 5.2, ts * 2.6))
        .show(ui, &mut cx.widgets())
        .clicked()
}

/// The attract screen's today's-picks strip. Three real photographs.
fn picks_strip(ui: &mut egui::Ui, cx: &mut Cx<'_>, reach: egui::Rect, art: &Art) {
    let head = reach.height() * 0.085;
    centered(
        ui,
        egui::pos2(reach.center().x, reach.top() + head * 0.5),
        t("오늘의 추천", "Today's picks"),
        egui::FontId::proportional(head * 0.5),
        cx.theme.color(ColorRole::Muted),
    );
    let strip = egui::Rect::from_min_max(
        egui::pos2(reach.left(), reach.top() + head),
        egui::pos2(reach.right(), reach.top() + reach.height() * 0.60),
    );
    let mut top = band_ui(ui, strip);
    let picks: Vec<Item> = MENU.iter().copied().filter(|i| i.pick).collect();
    layout::Grid::new(2.0, 4.4)
        .max_columns(3)
        .deco(
            layout::Deco::new()
                .radius(22.0)
                .visual_inset(4.0)
                // A translucent plate, so the background photograph shows through. `ColorSpec::Fixed`
                // keeps the alpha as it is — a place a role colour cannot reach.
                .fill(egui::Color32::from_rgba_unmultiplied(28, 22, 17, 215)),
        )
        .show(&mut top, cx, &picks, |ui, cx, cell| {
            let v = cell.visual;
            if let Some(tex) = art.item(cell.item.id) {
                photo(
                    ui,
                    tex,
                    egui::Rect::from_min_max(
                        egui::pos2(v.left(), v.top() + v.height() * 0.03),
                        egui::pos2(v.right(), v.top() + v.height() * 0.66),
                    ),
                    egui::Color32::WHITE,
                );
            }
            centered_fit(
                ui,
                egui::pos2(v.center().x, v.bottom() - v.height() * 0.22),
                cell.item.name(),
                v.width() * 0.9,
                band_text(v, 0.13),
                cx.theme.color(ColorRole::OnSurface),
            );
            centered_fit(
                ui,
                egui::pos2(v.center().x, v.bottom() - v.height() * 0.08),
                &won(cell.item.price),
                v.width() * 0.9,
                band_text(v, 0.12),
                cx.theme.color(ColorRole::Primary),
            );
        });
}

/// **The attract screen.** A full-bleed background photograph plus a bottom scrim, with the brand and the CTA over it.
fn attract(ui: &mut egui::Ui, cx: &mut Cx<'_>, order: &mut Order, art: &Art) {
    let full = ui.available_rect_before_wrap();
    let anywhere = ui.interact(
        full,
        egui::Id::new("kiosk.attract.any"),
        egui::Sense::click(),
    );

    // **The background comes first.** Whichever of the portrait and landscape plates suits the screen
    // is drawn over it, and a scrim deepening towards the bottom laid on top — without it the text
    // disappears in the bright parts.
    let bg = if full.width() < full.height() {
        &art.bg_portrait
    } else {
        &art.bg_landscape
    };
    backdrop(ui, bg, full);
    let ground = cx.theme.color(ColorRole::Background);
    scrim(ui, full, ground, 55, 175);

    let (hero, reach) = bands(ui, cx).unwrap_or((full, full));
    let split = hero != reach;

    // The wordmark is landscape, so it is fitted by width.
    let mark_w = (hero.width() * 0.62).min(hero.height() * 1.9);
    let mark = egui::Rect::from_center_size(
        egui::pos2(hero.center().x, hero.center().y + hero.height() * 0.02),
        egui::vec2(mark_w, hero.height() * 0.34),
    );
    match lang() {
        Lang::Ko => photo(ui, &art.wordmark, mark, egui::Color32::WHITE),
        // The drawn wordmark spells the shop's name in Hangul, so the English screen sets the name
        // in the display face instead, where the drawing sits and in its cream — the palette's
        // `on_surface`, which `kiosk.toml` sets to the wordmark's own. Fourteen Latin letters held
        // to the drawing's width would stand half as tall as its five syllables, so the name takes
        // most of the hero's width; the height cap only bites on a wide hero.
        Lang::En => centered_fit(
            ui,
            mark.center(),
            "Fairing Coffee",
            hero.width() * 0.8,
            cx.theme.display(mark.height() * 0.5),
            cx.theme.color(ColorRole::OnSurface),
        ),
    }
    centered_fit(
        ui,
        egui::pos2(hero.center().x, hero.center().y + hero.height() * 0.30),
        t("주문하시려면 화면을 눌러 주세요", "Touch anywhere to order"),
        hero.width() * 0.86,
        band_text(hero, 0.058),
        cx.theme.color(ColorRole::Muted),
    );

    let m = cx.theme.metrics;
    if split {
        picks_strip(ui, cx, reach, art);
    }

    // **The language switch**, in the top corner where a kiosk keeps it. It writes the
    // `ui.locale` setting rather than the language itself: every screen follows the setting
    // ([`follow_locale`]), so the whole kiosk turns over on the next frame, and anything else
    // that writes the setting turns it over the same way.
    let inset = pad(cx);
    let switch_w = (hero.width() * 0.46).min(m.row_height * 7.0);
    let mut corner = band_ui(
        ui,
        egui::Rect::from_min_size(
            egui::pos2(hero.right() - inset - switch_w, hero.top() + inset),
            egui::vec2(switch_w, m.row_height),
        ),
    );
    let names = LANGUAGES.map(|(_, name)| name);
    let current = usize::from(lang() == Lang::Ko);
    let picked = SegmentedControl::new(&names, current)
        .show(&mut corner, &mut cx.widgets())
        .picked;
    if let Some((tag, _)) = picked.and_then(|index| LANGUAGES.get(index)) {
        cx.set_setting(LOCALE_KEY, SettingValue::Text((*tag).to_owned()));
    }

    let button_h = (reach.height() * 0.14).max(m.row_height * 2.0);
    let bar = egui::Rect::from_center_size(
        egui::pos2(
            reach.center().x,
            if split {
                reach.bottom() - button_h * 0.85
            } else {
                reach.bottom() - button_h
            },
        ),
        egui::vec2(
            (reach.width() - m.screen_inset * 2.0).min(m.row_height * 12.0),
            button_h,
        ),
    );
    let mut child = band_ui(ui, bar);
    let started = BigButton::new(t("주문 시작하기", "Start order"))
        .kind(ButtonKind::Primary)
        // **Grow the button and the label grows too.** Otherwise 17 du text sits in the middle of a
        // 200 du button — which is why `BigButton::text_size` came about.
        .text_size(button_h * 0.34)
        .min_size(bar.size())
        .show(&mut child, &mut cx.widgets())
        .clicked();
    if started || anywhere.clicked() {
        order.clear();
        cx.open("pay.mode");
    }
    ui.advance_cursor_after_rect(full);
}

/// **Eat in / take away.** Two real photograph tiles.
fn mode(ui: &mut egui::Ui, cx: &mut Cx<'_>, order: &mut Order, art: &Art) {
    let full = ui.available_rect_before_wrap();
    let (hero, reach) = bands(ui, cx).unwrap_or((full, full));
    if hero != reach {
        centered_fit(
            ui,
            egui::pos2(hero.center().x, hero.center().y - hero.height() * 0.08),
            t("어디서 드시나요", "Where will you have it?"),
            hero.width() * 0.86,
            band_text(hero, 0.16),
            cx.theme.color(ColorRole::OnSurface),
        );
        centered_fit(
            ui,
            egui::pos2(hero.center().x, hero.center().y + hero.height() * 0.12),
            t("고르시면 메뉴로 넘어갑니다", "Pick one to see the menu"),
            hero.width() * 0.86,
            band_text(hero, 0.07),
            cx.theme.color(ColorRole::Muted),
        );
    }

    let mut child = band_ui(ui, reach);
    // A screen with only two things to choose from — the band has to be filled for it to read as "one
    // of these two". A cap keeps the cards from getting much longer than they are wide.
    layout::Grid::new(2.0, 5.0)
        .max_columns(2)
        .fill_height(1.45)
        .deco(layout::Deco::new().radius(24.0).visual_inset(4.0))
        .fill_with(|dine_in: &bool| {
            if *dine_in {
                ColorRole::Primary.into()
            } else {
                ColorRole::SurfaceVariant.into()
            }
        })
        .show(&mut child, cx, &[true, false], |ui, cx, cell| {
            let dine_in = *cell.item;
            if cell.response.clicked() {
                order.dine_in = dine_in;
                cx.open("pay.menu");
            }
            let v = cell.visual;
            photo(
                ui,
                if dine_in { &art.dinein } else { &art.takeout },
                egui::Rect::from_min_max(
                    egui::pos2(v.left(), v.top() + v.height() * 0.08),
                    egui::pos2(v.right(), v.top() + v.height() * 0.66),
                ),
                egui::Color32::WHITE,
            );
            let labels = [t("매장에서", "Dine in"), t("포장", "Take out")];
            ui.painter().text(
                egui::pos2(v.center().x, v.bottom() - v.height() * 0.16),
                egui::Align2::CENTER_CENTER,
                if dine_in {
                    labels.first().copied().unwrap_or("")
                } else {
                    labels.get(1).copied().unwrap_or("")
                },
                layout::fit_size(ui.painter(), &labels, v.width() * 0.8, band_text(v, 0.15)),
                cx.theme.color(if dine_in {
                    ColorRole::OnPrimary
                } else {
                    ColorRole::OnSurface
                }),
            );
        });
    ui.advance_cursor_after_rect(full);
}

/// **The menu.** The category chips on top, the product grid below, the total and the confirm button
/// at the bottom.
///
/// In the counter layout this function fills the left column only and [`cart_body`] goes on the
/// right — the screen is not rewritten.
fn menu(ui: &mut egui::Ui, cx: &mut Cx<'_>, order: &mut Order, art: &Art, with_bar: bool) {
    let (cat, total, count) = {
        let o = &*order;
        (o.cat.unwrap_or(Cat::All), o.total(), o.count())
    };
    let mut body = |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
        // ── The category chips ──
        // A `ChipRow`, which is what this was reaching for: five choices of unequal width in a
        // lane. It used to be a `Grid` of equal cells flooded with `Primary` when picked, and the
        // chip's own selection - a fill, a label that flips its role, and a hairline that leaves -
        // says it in three channels instead of one.
        let row = cx.theme.metrics.row_height;
        let items: Vec<ChipItem<'_>> = Cat::ALL
            .iter()
            .map(|c| ChipItem {
                label: c.label(),
                icon: None,
            })
            .collect();
        let mut picked = Cat::ALL.iter().position(|c| *c == cat).unwrap_or(0);
        if ChipRow::new("kiosk.cats", &items, &mut picked)
            .show(ui, &mut cx.widgets())
            .picked
            .is_some()
        {
            order.cat = Cat::ALL.get(picked).copied();
        }

        // ── The product grid ──
        let shown: Vec<Item> = MENU.iter().copied().filter(|i| cat.holds(i.cat)).collect();
        // 3 columns where wide, 2 where narrow. The column count is settled first and the cell worked back from it.
        let avail = ui.available_width();
        let want = if avail > row * 12.0 { 3.0 } else { 2.0 };
        // **The tile height is set to fill the height left.** A product tile is a photograph plus a
        // name plus a price, so a little taller than square is as good as it gets, and stopping there
        // is what lets two rows fit on a landscape POS — the same code stands differently on two devices.
        // **No `fill_with`.** The cell used to be flooded with `Primary` when its item was picked,
        // and that one line is what made this read as a vending machine: an accent over a
        // photograph destroys the photograph, which was the reason to have a card. The card is now
        // `MediaCard`'s, and being picked is a ring.
        // **The cell's height is asked of the card, not guessed at.** It was `cell_rows = 2.1`,
        // and a square picture plus its text needs 4.6 rows at this panel — so the tile was handed
        // 282.7 du where it wanted 636.4 and the shortfall came out of the photograph, drawn at a
        // 3.435:1 crop by a caller three lines above asking for 1:1. `cell_rows` cannot express
        // this: it is a multiple of the row height and the shortfall depends on the *width* the
        // columns came out at, which is decided inside the grid.
        //
        // Every tile's text block is the same two lines, so it is measured once here rather than
        // per cell — `height_for` at zero width is the text block alone.
        let text_h = MediaCard::new(" ")
            .value(" ")
            .aspect(ASPECT_SQUARE)
            .height_for(0.0, ui, &cx.widgets());
        let cell_h = move |w: f32| w / ASPECT_SQUARE + text_h;
        layout::Grid::new(2.2, 2.1)
            .cell_height(cell_h)
            .max_columns(want as usize)
            .fill_height(1.1)
            .deco(layout::Deco::new().no_fill().visual_inset(3.0))
            .show(ui, cx, &shown, |ui, cx, cell| {
                menu_tile(ui, cx, cell, order, art);
            });
    };

    if !with_bar {
        // **The counter's menu column scrolls on its own.** With the bar the body sits in
        // `action_bar_with`'s scroll area; without it, it was drawn straight into the column and
        // the rows past the bottom could not be reached at all.
        egui::ScrollArea::vertical()
            .id_salt("kiosk.menu")
            .max_height(ui.available_height())
            .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
            .auto_shrink([false, false])
            .show(ui, |ui| body(ui, cx));
        return;
    }
    let go = layout::action_bar_with(
        ui,
        cx,
        bar_rows(cx),
        layout::Deco::new().radius(22.0),
        body,
        |ui, cx| menu_bar(ui, cx, count, total),
    );
    let (back, go) = go;
    if back {
        cx.finish();
    } else if go {
        cx.open("pay.cart");
    }
}

/// The menu's pinned bottom bar — `(back, confirm order)`.
///
/// **The amount and the CTA are stuck together on the right.** Put on the left with a `ui.label`, the
/// label eats all the width left and overlaps the `right_to_left` area — with a long Korean phrase
/// ("메뉴를 골라 주세요") the text really did land on top of the button.
fn menu_bar(ui: &mut egui::Ui, cx: &mut Cx<'_>, count: u32, total: u32) -> (bool, bool) {
    let ts = bar_text(cx);
    let back = back_button(ui, cx);
    let go = ui
        .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let go = BigButton::new(t("주문 확인", "Review order"))
                .kind(ButtonKind::Primary)
                .text_size(ts * 1.25)
                .min_size(egui::vec2(ts * 11.0, ts * 2.8))
                .enabled(count > 0)
                .show(ui, &mut cx.widgets())
                .clicked();
            // **Shrink it in, and drop it where it will not shrink** — the place `--lang=en` caught.
            //
            // It was `Label::truncate()` at first. The Korean "메뉴를 골라 주세요" fitted and looked
            // fine, and switching to English cut it to `Pick somet…`. **A truncated hint reads as a
            // fault rather than as guidance.** In a bar the CTA is the one that should not give up its
            // place, so the hint shrinks to what is left and drops out where even that will not do. On
            // a panel with width to spare it comes out in full — a rule is needed quite apart from
            // rewriting the wording shorter.
            let hint = if count == 0 {
                t("메뉴를 골라 주세요", "Pick an item").to_owned()
            } else {
                format!("{count}{} · {}", t("개", ""), won(total))
            };
            let base = ts * 1.15;
            let gap = pad(cx);
            let room = (ui.available_width() - gap).max(0.0);
            let width = ui
                .painter()
                .layout_no_wrap(
                    hint.clone(),
                    egui::FontId::proportional(base),
                    egui::Color32::WHITE,
                )
                .size()
                .x;
            let scale = if width > room && width > 0.0 {
                room / width
            } else {
                1.0
            };
            // Smaller than this it cannot be read in the bar — better absent then.
            if scale >= 0.72 {
                ui.add_space(gap);
                ui.label(
                    egui::RichText::new(hint)
                        .size(base * scale)
                        .color(cx.theme.color(if count == 0 {
                            ColorRole::Muted
                        } else {
                            ColorRole::OnSurface
                        })),
                );
            }
            go
        })
        .inner;
    (back, go)
}

/// One product tile.
///
/// **This was 89 lines and is now the card it was always drawing.** What it used to do by hand,
/// and what now does it:
///
/// * a photograph in a band computed as `0.04..0.62` of the cell → the card's own fixed-aspect,
///   full-bleed image box, whose outer corners are the card's and whose inner ones are square;
/// * a name and a price centred at `0.24` and `0.09` up from the bottom → the card's left-aligned
///   text block, on the type scale;
/// * a `Danger` disc with `OnPrimary` digits for the quantity → [`CountBadge`], which carries its
///   ink **per tone** because that pair measures 2.79 in base dark and a count is meant to be read;
/// * the cell flooded with `Primary` when picked → a ring, which is what every reference does;
/// * a sold-out item drawn by dropping the photo's alpha and stamping a glyph over it → the card's
///   own veil, a scrim and a word.
fn menu_tile(
    ui: &mut egui::Ui,
    cx: &mut Cx<'_>,
    cell: layout::Cell<'_, Item>,
    order: &mut Order,
    art: &Art,
) {
    let item = *cell.item;
    let qty = order
        .lines
        .iter()
        .find(|l| l.item.id == item.id)
        .map_or(0, |l| l.qty);

    let price = won(item.price);
    let mut card = MediaCard::new(item.name())
        .value(&price)
        // Square, because this shop's photographs are: a 4:3 box would crop the top off
        // every cup. That the aspect is the caller's is the whole reason it is a parameter.
        .aspect(ASPECT_SQUARE)
        .selected(qty > 0)
        .action(icon::PLUS, t("담기", "Add one"));
    if let Some(tex) = art.item(item.id) {
        card = card.image(tex.id(), tex.size_vec2());
    }
    if item.sold_out {
        card = card.veil(t("품절", "Sold out"));
    }

    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cell.visual));
    let pick = card.show(&mut child, &mut cx.widgets());
    if (pick.action || pick.response.clicked()) && !item.sold_out {
        order.add(item);
    }
    if qty > 0 {
        // Outside the cell, on its top corner — the grid puts no clip on a cell, which is what
        // makes a badge that overhangs possible.
        let _ = CountBadge::count(qty).paint_over(
            ui.painter(),
            cx.theme,
            cell.visual,
            BadgeAnchor::TopEnd,
        );
    }
}

/// **Confirm the order.** `− n +` per line, with pay and clear all at the bottom.
fn cart(ui: &mut egui::Ui, cx: &mut Cx<'_>, order: &mut Order, art: &Art) {
    let (total, count, dine_in) = {
        let o = &*order;
        (o.total(), o.count(), o.dine_in)
    };
    let full = ui.available_rect_before_wrap();
    // In portrait the summary goes up into the display band — the list has to be in the control band to be within reach.
    let inner = match bands(ui, cx) {
        Some((hero, reach)) => {
            centered(
                ui,
                egui::pos2(hero.center().x, hero.center().y - hero.height() * 0.16),
                &format!("{count}{}", t("개 담김", " in cart")),
                band_text(hero, 0.10),
                cx.theme.color(ColorRole::Muted),
            );
            centered(
                ui,
                egui::pos2(hero.center().x, hero.center().y + hero.height() * 0.06),
                &won(total),
                band_text(hero, 0.26),
                cx.theme.color(ColorRole::OnSurface),
            );
            centered_fit(
                ui,
                egui::pos2(hero.center().x, hero.center().y + hero.height() * 0.30),
                if dine_in {
                    t("매장에서 드십니다", "Dining in")
                } else {
                    t("포장해 드립니다", "Taking out")
                },
                hero.width() * 0.86,
                band_text(hero, 0.062),
                cx.theme.color(ColorRole::Primary),
            );
            band_ui(ui, reach)
        }
        None => band_ui(ui, full),
    };
    let mut inner = inner;
    let out = layout::action_bar_with(
        &mut inner,
        cx,
        bar_rows(cx),
        layout::Deco::new().radius(22.0),
        |ui, cx| cart_body(ui, cx, order, art),
        |ui, cx| {
            let m = cx.theme.metrics;
            let ts = bar_text(cx);
            // In a narrow bar such as the counter's right column the clear button is dropped — forcing
            // three in has them overlap. The amount is already shouted large by the display band in the
            // portrait layout, so it is not repeated in the bar — only a narrow landscape layout puts it
            // inside the pay button.
            let narrow = ui.available_width() < m.row_height * 9.0;
            let back = !narrow && back_button(ui, cx);
            let (pay, wipe) = ui
                .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let pay = BigButton::new(if narrow {
                        format!("{} {}", t("", "Pay"), won(total))
                    } else {
                        t("결제하기", "Pay now").to_owned()
                    })
                    .kind(ButtonKind::Primary)
                    .text_size(ts * 1.25)
                    .min_size(egui::vec2(ts * 12.0, ts * 2.8))
                    .enabled(total > 0)
                    .show(ui, &mut cx.widgets())
                    .clicked();
                    // **Clear all is a long press.** One mistaken tap must not blow the cart away — let
                    // go while the ring fills and it is cancelled. It is kept smaller than pay: the
                    // destructive one must not be what catches the eye first.
                    ui.add_space(pad(cx));
                    let wipe = !narrow
                        && BigButton::new(t("전체 취소", "Clear all"))
                            .kind(ButtonKind::Danger)
                            .text_size(ts * 0.92)
                            .min_size(egui::vec2(ts * 6.4, ts * 2.2))
                            .enabled(total > 0)
                            .long_press(Duration::from_millis(1200))
                            .show(ui, &mut cx.widgets())
                            .long_pressed();
                    (pay, wipe)
                })
                .inner;
            (back, pay, wipe)
        },
    );
    let (back, pay, wipe) = out;
    if back {
        cx.finish();
    }
    ui.advance_cursor_after_rect(full);
    if wipe {
        order.clear();
        cx.shell
            .toast(Toast::new(t("주문을 비웠습니다", "Cart cleared")));
    } else if pay {
        cx.open("pay.method");
    }
}

/// The cart's body. In the counter layout this alone is used, with no [`action_bar`](layout::action_bar).
fn cart_body(ui: &mut egui::Ui, cx: &mut Cx<'_>, order: &mut Order, art: &Art) {
    let (lines, mut dine_in) = {
        let o = &*order;
        (o.lines.clone(), o.dine_in)
    };
    if lines.is_empty() {
        layout::note(
            ui,
            cx,
            t(
                "담긴 메뉴가 없습니다. 메뉴에서 골라 주세요.",
                "Nothing in the cart yet. Pick something from the menu.",
            ),
        );
        return;
    }
    layout::section(ui, cx, t("담긴 메뉴", "In your cart"));
    // **Share the height left out among the lines** (harness result 3). A two-line order clinging
    // thinly to the top of an upright panel reads as a receipt preview rather than a cart. A cap keeps
    // one line from swallowing the screen, and the floor is the touch target's.
    let n = lines.len() as f32;
    let line_h = ((ui.available_height() - vrow(cx) * 2.2) / n.max(1.0))
        .clamp(cx.theme.metrics.touch_target * 1.3, vrow(cx) * 2.4);
    layout::group(ui, cx, |ui, cx| {
        for line in &lines {
            cart_line(ui, cx, *line, order, art, line_h);
        }
    });
    layout::group(ui, cx, |ui, cx| {
        if layout::switch_row(
            ui,
            cx,
            t("매장에서 드시나요", "Dining in?"),
            Some(if dine_in {
                t("매장 이용", "Dine in")
            } else {
                t("포장", "Take out")
            }),
            &mut dine_in,
            true,
        ) {
            order.dine_in = dine_in;
        }
    });
}

/// One cart line — **the example builds the quantity stepper** (harness result 3).
///
/// [`ListRow`](fairing::widgets::ListRow) has no trailing **widget** slot. The crate deliberately
/// does not open that, since what goes in a row is the integrator's, and the judgement is right,
/// but on a payment terminal these forty lines come up every time.
fn cart_line(ui: &mut egui::Ui, cx: &mut Cx<'_>, line: Line, order: &mut Order, art: &Art, h: f32) {
    let m = cx.theme.metrics;
    let inset = pad(cx);
    // **The stepper's own size, asked of the crate rather than guessed at.** This used to compute
    // `(h * 0.42).min(vrow(cx) * 0.72).max(touch_target)` = 115 px and say in a comment that the
    // stepper grew with the row — and then place the stepper at `touch_target` = 56 px, half of
    // it, because `Stepper` had no size to be told and read the finger itself. The number and the
    // claim are one thing now.
    let step = fairing::theme::control_height(&m, &cx.theme.control);
    let gap = inset * 0.5;
    let pill_w = step * 2.0 + step * 0.9 + gap * 2.0;
    // **Stacked over two lines where narrow.** In the counter POS's right column (360 du) the name and
    // the stepper did not fit on one line and overlapped. Shrinking the touch target to make it fit is
    // not the answer.
    let stacked = ui.available_width() < pill_w + step * 3.6;
    let h = if stacked { h * 1.5 } else { h };
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());

    let (text_y, pill_cy) = if stacked {
        (rect.top() + h * 0.28, rect.top() + h * 0.72)
    } else {
        (rect.center().y, rect.center().y)
    };

    // **The product photograph.** A list of nothing but stacked text reads as a statement rather than a
    // cart — the same photograph chosen from the menu has to follow through for "it went in" to be seen.
    let thumb = (h * 0.74).min(rect.width() * 0.24);
    let mut left = rect.left() + inset;
    if !stacked {
        if let Some(tex) = art.item(line.item.id) {
            photo(
                ui,
                tex,
                egui::Rect::from_center_size(
                    egui::pos2(left + thumb * 0.5, rect.center().y),
                    egui::Vec2::splat(thumb),
                ),
                egui::Color32::WHITE,
            );
        }
        left += thumb + inset;
    }

    // **The widget, not 55 lines of pill.** What this replaces drew `"+"` and `"-"` as *text*,
    // which the checkbox's own doc forbids - a typeface an integrator supplies may not carry the
    // glyph - and set its radius, its digit size and its value cell from eyeballed multiples of a
    // local variable, so a panel that retuned its metrics retuned nothing here.
    let pill = {
        let mut qty = i32::try_from(line.qty).unwrap_or(i32::MAX);
        let side = step;
        let at = egui::Rect::from_min_max(
            egui::pos2(rect.right() - inset - side * 3.0, pill_cy - side * 0.5),
            egui::pos2(rect.right() - inset, pill_cy + side * 0.5),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(at));
        let response = Stepper::new(&mut qty)
            .range(0..=99)
            .show(&mut child, &mut cx.widgets());
        if response.changed() {
            let was = i32::try_from(line.qty).unwrap_or(i32::MAX);
            order.bump(line.item.id, qty - was);
        }
        response.rect
    };

    // On the left: the name and the unit price. In a one-line layout the width available runs to the pill's left edge.
    let max_w = if stacked {
        rect.width() - inset * 2.0
    } else {
        (pill.left() - left - inset).max(step)
    };
    // **The type scale, not a fraction of the row.** This was `(h * 0.26).min(vrow(cx) * 0.52)`,
    // which resolved to 83.2 px on the portrait panel — 1.13x the theme's own `heading` and 1.93x
    // its `body`, i.e. a fifth type step nobody declared, sized off the row instead of off the
    // scale. Measured, that is the 60 px capital the 56 px stepper was standing next to.
    let name_size = cx.theme.metrics.type_scale.heading;
    let name = egui::FontId::proportional(name_size);
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(
        line.item.name().to_owned(),
        name.clone(),
        cx.theme.color(ColorRole::OnSurface),
    );
    let name = if galley.size().x > max_w && galley.size().x > 0.0 {
        egui::FontId::proportional((name.size * max_w / galley.size().x).max(9.0))
    } else {
        name
    };
    painter.text(
        egui::pos2(left, text_y - name_size * 0.62),
        egui::Align2::LEFT_CENTER,
        line.item.name(),
        name,
        cx.theme.color(ColorRole::OnSurface),
    );
    painter.text(
        egui::pos2(left, text_y + name_size * 0.72),
        egui::Align2::LEFT_CENTER,
        format!("{} × {}", won(line.item.price), line.qty),
        egui::FontId::proportional(cx.theme.metrics.type_scale.body),
        cx.theme.color(ColorRole::Muted),
    );
}

/// **The payment method.** Three icon tiles. Cash only calls an assistant — in a Korean shop, cash is
/// usually a person's job.
fn method(ui: &mut egui::Ui, cx: &mut Cx<'_>, order: &mut Order, art: &Art) {
    let total = order.total();
    let full = ui.available_rect_before_wrap();
    let (hero, reach) = bands(ui, cx).unwrap_or((full, full));
    if hero != reach {
        centered_fit(
            ui,
            egui::pos2(hero.center().x, hero.center().y - hero.height() * 0.14),
            t("결제 수단을 골라 주세요", "Choose how to pay"),
            hero.width() * 0.86,
            band_text(hero, 0.10),
            cx.theme.color(ColorRole::Muted),
        );
        centered_fit(
            ui,
            egui::pos2(hero.center().x, hero.center().y + hero.height() * 0.10),
            &won(total),
            hero.width() * 0.86,
            band_text(hero, 0.24),
            cx.theme.color(ColorRole::Primary),
        );
    }
    // **The three methods are stacked in one column and fill the band.** In 2 columns the last one is
    // left alone and the right goes empty, and on an upright panel that gap is an eighth of the screen.
    // Three stacked vertically also scan better — the hand moves up and down anyway.
    let back_h = bar_text(cx) * 4.2;
    let band = egui::Rect::from_min_max(
        reach.min,
        egui::pos2(reach.right(), (reach.bottom() - back_h).max(reach.top())),
    );
    let mut child = band_ui(ui, band);
    let picked = std::cell::Cell::new(None);
    let wide = child.available_width() > child.available_height();
    let want = if wide { 3.0 } else { 1.0 };
    layout::Grid::new(2.0, 3.0)
        .max_columns(want as usize)
        .fill_height(0.55)
        .deco(layout::Deco::new().radius(22.0).visual_inset(4.0))
        .show(&mut child, cx, &Method::ALL, |ui, cx, cell| {
            if cell.response.clicked() {
                picked.set(Some(*cell.item));
            }
            method_tile(ui, cx, cell.visual, *cell.item, art);
        });
    // With no nav bar, the screen provides the way back.
    let m = cx.theme.metrics;
    let ts = bar_text(cx);
    let back_rect = egui::Rect::from_center_size(
        egui::pos2(reach.center().x, reach.bottom() - back_h * 0.5),
        egui::vec2(ts * 9.0, ts * 3.0),
    );
    let _ = m;
    let mut back_ui = band_ui(ui, back_rect);
    if back_button(&mut back_ui, cx) {
        cx.finish();
    }
    ui.advance_cursor_after_rect(full);

    match picked.get() {
        Some(Method::Cash) => {
            cx.shell.notify(
                Notification::new(
                    NotificationId::of("kiosk.cash"),
                    t("현금 결제 요청", "Cash payment requested"),
                )
                .body(t(
                    "직원이 곧 도와드립니다.",
                    "A staff member is on the way.",
                ))
                .source(t("키오스크 1", "Kiosk 1"))
                .level(Level::Warning),
            );
            cx.shell
                .toast(Toast::new(t("직원을 호출했습니다", "Staff called")));
        }
        Some(m) => {
            order.method = Some(m);
            cx.open("pay.progress");
        }
        None => {}
    }
}

/// The inside of a payment-method tile.
///
/// Where the cell is wide, the icon stands on the left and the text goes to **its right**. Centred, a
/// long label ("Cash · call staff") rides up over the icon — which is what really happened on the
/// portrait kiosk.
fn method_tile(ui: &mut egui::Ui, cx: &mut Cx<'_>, v: egui::Rect, m: Method, art: &Art) {
    let lying = v.width() > v.height() * 1.6;
    let gap = pad(cx);
    let art_size = if lying {
        v.height() * 0.44
    } else {
        v.height() * 0.34
    };
    let art_left = v.left() + gap * 1.6;
    let art_at = if lying {
        egui::pos2(art_left + art_size * 0.5, v.center().y)
    } else {
        egui::pos2(v.center().x, v.top() + v.height() * 0.36)
    };
    if let Some(tex) = art.method(m) {
        // The icon is flat white, so **the tint takes as it stands** — change the role colour and it follows.
        photo(
            ui,
            tex,
            egui::Rect::from_center_size(art_at, egui::Vec2::splat(art_size)),
            cx.theme.color(ColorRole::OnSurface),
        );
    }
    let (at, align, frac, max_w) = if lying {
        let text_left = art_left + art_size + gap * 1.4;
        (
            egui::pos2(text_left, v.center().y),
            egui::Align2::LEFT_CENTER,
            0.28,
            (v.right() - gap * 1.6 - text_left).max(1.0),
        )
    } else {
        (
            egui::pos2(v.center().x, v.bottom() - v.height() * 0.16),
            egui::Align2::CENTER_CENTER,
            0.11,
            v.width() * 0.9,
        )
    };
    // The three methods are one set too — matched against the longest ([`fit_all`]).
    let labels: Vec<&str> = Method::ALL.iter().map(|m| m.label()).collect();
    ui.painter().text(
        at,
        align,
        m.label(),
        layout::fit_size(ui.painter(), &labels, max_w, band_text(v, frac)),
        cx.theme.color(ColorRole::OnSurface),
    );
}

/// **Payment in progress.** A screen with state, so it is built with [`screen_with`].
///
/// With no modal in the crate (harness result 4) a fullscreen screen is pushed. That stacks it on the
/// back stack, so going back has to be blocked separately with `nav_bar: Hide` plus `edge_guard` —
/// what one modal would do becomes three things: a screen declaration, a chrome policy and stack
/// management.
struct Progress {
    art: Rc<Art>,
    dwell: Duration,
    fail: bool,
    started: Option<Instant>,
    /// Raise completion only once. Otherwise the order number climbs on every frame the progress is past 1.
    settled: bool,
}

impl Screen for Progress {
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        follow_locale(cx);
        let started = *self.started.get_or_insert(cx.now);
        let elapsed = cx.now.saturating_duration_since(started);
        let done = (elapsed.as_secs_f32() / self.dwell.as_secs_f32().max(0.01)).clamp(0.0, 1.0);

        let full = ui.available_rect_before_wrap();
        let (hero, reach) = bands(ui, cx).unwrap_or((full, full));
        // The display band is filled by the card terminal's photograph — "what am I meant to do" reads faster than text.
        photo(
            ui,
            &self.art.insert_card,
            hero.shrink2(egui::vec2(hero.width() * 0.14, hero.height() * 0.08)),
            egui::Color32::WHITE,
        );
        // The ring, the hint and the amount are gathered **in the middle** of the band. Put at the top,
        // the bottom half of a large panel goes entirely empty (harness result 3).
        let ring = egui::Rect::from_center_size(
            egui::pos2(reach.center().x, reach.center().y - reach.height() * 0.20),
            egui::Vec2::splat(reach.height().min(reach.width()) * 0.22),
        );
        let style = IconStyle::sized(ring.width()).color(IconColor::Role(ColorRole::Primary));
        icons::parametric::progress_ring(ui.painter(), ring, done, &style.param_style(cx.theme));

        let total = cx.app::<Order>().map_or(0, Order::total);
        centered_fit(
            ui,
            egui::pos2(reach.center().x, reach.center().y + reach.height() * 0.03),
            t("카드를 꽂아 주세요", "Insert your card"),
            reach.width() * 0.8,
            band_text(reach, 0.075),
            cx.theme.color(ColorRole::OnSurface),
        );
        centered(
            ui,
            egui::pos2(reach.center().x, reach.center().y + reach.height() * 0.19),
            &won(total),
            band_text(reach, 0.13),
            cx.theme.color(ColorRole::Primary),
        );
        ui.advance_cursor_after_rect(full);

        if done >= 1.0 && !self.settled {
            self.settled = true;
            if self.fail {
                cx.shell.toast(
                    Toast::new(t("카드를 다시 넣어 주세요", "Please try your card again"))
                        .level(Level::Warning),
                );
                cx.finish();
            } else {
                if let Some(order) = cx.app_mut::<Order>() {
                    order.serial = order.serial.wrapping_add(1);
                }
                cx.open("pay.receipt");
            }
            return;
        }
        // **This is the only screen that breaks idling at 0 fps**. It schedules only the next
        // frame and stops by itself when it finishes.
        if !self.settled {
            cx.shell.waker().wake_after(Duration::from_millis(33));
        }
    }

    fn on_lifecycle(&mut self, ev: Lifecycle, _cx: &mut Cx<'_>) {
        // Coming back in starts over — a screen returned to after a failure must not show an already-full ring.
        if matches!(ev, Lifecycle::Created | Lifecycle::Resumed) {
            self.started = None;
            self.settled = false;
        }
    }
}

/// The three receipt-choice tiles. Split out because [`receipt`] was getting long.
fn receipt_options(
    ui: &mut egui::Ui,
    cx: &mut Cx<'_>,
    art: &Art,
    choice: &std::cell::Cell<Option<u8>>,
) {
    // Three tiles clinging to the top of the band leave the bottom entirely empty — the height left goes
    // to the cells. But **they must not get much taller than they are wide**: three cells stretching
    // tall leave the icons floating in space.
    layout::Grid::new(2.0, 3.4)
        .max_columns(3)
        .fill_height(1.12)
        .deco(layout::Deco::new().radius(22.0).visual_inset(4.0))
        .show(ui, cx, &[0_u8, 1, 2], |ui, cx, cell| {
            if cell.response.clicked() {
                choice.set(Some(*cell.item));
            }
            let v = cell.visual;
            let at = egui::Rect::from_center_size(
                egui::pos2(v.center().x, v.top() + v.height() * 0.36),
                egui::Vec2::splat(v.height() * 0.32),
            );
            let tint = cx.theme.color(ColorRole::OnSurface);
            match cell.item {
                0 => art.pay.get("receipt").map(|t| photo(ui, t, at, tint)),
                1 => art.pay.get("sms").map(|t| photo(ui, t, at, tint)),
                _ => {
                    let style =
                        IconStyle::sized(at.width()).color(IconColor::Role(ColorRole::Muted));
                    cx.icons.paint(
                        ui.painter(),
                        at,
                        &IconRef::Builtin("close"),
                        &style,
                        cx.theme,
                    );
                    None
                }
            };
            let labels = [
                t("영수증 인쇄", "Print receipt"),
                t("문자 전송", "Text it to me"),
                t("안 받을게요", "No receipt"),
            ];
            ui.painter().text(
                egui::pos2(v.center().x, v.bottom() - v.height() * 0.16),
                egui::Align2::CENTER_CENTER,
                labels.get(usize::from(*cell.item)).copied().unwrap_or(""),
                layout::fit_size(ui.painter(), &labels, v.width() * 0.9, band_text(v, 0.13)),
                cx.theme.color(ColorRole::OnSurface),
            );
        });
}

/// **The receipt and the order number.** The whole display band is the number — it has to be visible from a distance when called at the counter.
fn receipt(ui: &mut egui::Ui, cx: &mut Cx<'_>, order: &mut Order, art: &Art) {
    let (ticket, total, dine_in) = {
        let o = &*order;
        (o.ticket(), o.total(), o.dine_in)
    };
    let full = ui.available_rect_before_wrap();
    let (hero, reach) = bands(ui, cx).unwrap_or((full, full));

    // **The bokeh is laid over the whole screen.** Laid over the display band alone it makes a cut
    // horizontal line in the photograph at the 34 % mark and looks like two plates joined — the attract
    // screen being full-bleed made it show all the more.
    backdrop(ui, &art.bg_ticket, full);
    scrim(ui, full, cx.theme.color(ColorRole::Background), 104, 232);

    photo(
        ui,
        &art.done,
        egui::Rect::from_center_size(
            egui::pos2(hero.center().x, hero.top() + hero.height() * 0.16),
            egui::Vec2::splat(hero.height() * 0.16),
        ),
        cx.theme.color(ColorRole::Success),
    );
    centered(
        ui,
        egui::pos2(hero.center().x, hero.center().y - hero.height() * 0.10),
        t("주문번호", "Order no."),
        band_text(hero, 0.08),
        cx.theme.color(ColorRole::Muted),
    );
    centered(
        ui,
        egui::pos2(hero.center().x, hero.center().y + hero.height() * 0.16),
        &ticket,
        band_text(hero, 0.38),
        cx.theme.color(ColorRole::Primary),
    );
    centered_fit(
        ui,
        egui::pos2(hero.center().x, hero.bottom() - hero.height() * 0.08),
        &format!(
            "{} · {}{}",
            if dine_in {
                t("매장", "Dine in")
            } else {
                t("포장", "Take out")
            },
            won(total),
            t(" 결제 완료", " paid")
        ),
        hero.width() * 0.86,
        band_text(hero, 0.058),
        cx.theme.color(ColorRole::OnSurface),
    );

    let mut child = band_ui(ui, reach);
    let choice = std::cell::Cell::new(None::<u8>);
    layout::action_bar_with(
        &mut child,
        cx,
        bar_rows(cx),
        layout::Deco::new().radius(22.0),
        |ui, cx| receipt_options(ui, cx, art, &choice),
        |ui, cx| {
            let ts = bar_text(cx);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                BigButton::new(t("확인", "Done"))
                    .kind(ButtonKind::Primary)
                    .text_size(ts * 1.3)
                    .min_size(egui::vec2(ts * 11.0, ts * 2.8))
                    .show(ui, &mut cx.widgets())
                    .clicked()
            })
            .inner
        },
    )
    .then(|| {
        cx.shell.notify(
            Notification::new(
                NotificationId::of("kiosk.order"),
                format!("{ticket}{}", t(" 주문 접수", " received")),
            )
            .body(format!(
                "{} · {}",
                if dine_in {
                    t("매장", "Dine in")
                } else {
                    t("포장", "Take out")
                },
                won(total)
            ))
            .source(t("주방", "Kitchen"))
            .icon(IconRef::Builtin("bell"))
            .action(LaunchAction::open("staff.orders")),
        );
        order.clear();
        cx.launch(LaunchAction::open("pay.attract"));
    });
    ui.advance_cursor_after_rect(full);
    if let Some(pick) = choice.get() {
        cx.shell.toast(Toast::new(match pick {
            0 => t("영수증을 인쇄합니다", "Printing your receipt"),
            1 => t("문자를 보냈습니다", "Text sent"),
            _ => t("영수증을 발행하지 않습니다", "No receipt issued"),
        }));
    }
}

/// **The staff screen.** It opens on knocking the four corners (the gate is `staff`).
///
/// Unlike the customer screens, information density comes first, so the reach bands are not divided —
/// an assistant stands right in front of the screen.
fn staff(ui: &mut egui::Ui, cx: &mut Cx<'_>, order: &mut Order) {
    layout::page(ui, cx, "staff.orders", |ui, cx| {
        layout::title(ui, cx, t("직원 화면", "Staff"));
        layout::section(ui, cx, t("지금 주문", "Open order"));
        let (lines, total, ticket) = {
            let o = &*order;
            (o.lines.clone(), o.total(), o.ticket())
        };
        layout::group(ui, cx, |ui, cx| {
            if lines.is_empty() {
                layout::info_row(
                    ui,
                    cx,
                    t("진행 중인 주문", "Order in progress"),
                    t("없음", "None"),
                );
            } else {
                layout::info_row(ui, cx, t("주문번호", "Order no."), &ticket);
                for line in &lines {
                    layout::info_row(
                        ui,
                        cx,
                        line.item.name(),
                        &format!("{} × {}", line.qty, won(line.item.price)),
                    );
                }
                layout::info_row(ui, cx, t("합계", "Total"), &won(total));
            }
        });
        layout::section(ui, cx, t("단말", "Terminal"));
        layout::group(ui, cx, |ui, cx| {
            layout::info_row(
                ui,
                cx,
                t("단말 번호", "Terminal ID"),
                t("키오스크 1", "Kiosk 1"),
            );
            layout::info_row(ui, cx, t("카드 리더", "Card reader"), t("정상", "OK"));
            layout::info_row(
                ui,
                cx,
                t("영수증 용지", "Receipt paper"),
                t("충분", "Plenty"),
            );
        });
        layout::note(
            ui,
            cx,
            t(
            "네 모서리를 차례로 두드리면 이 화면이 열립니다. 실제 매장에서는 게이트에 직원 카드를 겁니다.",
            "Tap the four corners in turn to open this screen. In a real store the gate is behind a staff card.",
        ),
        );
        layout::group_with(
            ui,
            cx,
            layout::Deco::new().stroke(1.0, ColorRole::Warning),
            |ui, cx| {
                if BigButton::new(t("대기 화면으로", "Back to idle"))
                    .kind(ButtonKind::Primary)
                    .text_size(cx.theme.metrics.row_height * 0.34)
                    .show(ui, &mut cx.widgets())
                    .clicked()
                {
                    cx.launch(LaunchAction::open("pay.attract"));
                }
            },
        );
    });
}

/// **The customer-facing display.** It only shows the same order.
///
/// What this screen is here to check is whether the layout has to be rewritten on a small screen —
/// deriving the text size from the screen has the same code stand at 480 × 320 and at 1024 × 600.
fn customer_display(ui: &mut egui::Ui, cx: &mut Cx<'_>, order: &mut Order, art: &Art) {
    let (total, count, ticket, method) = {
        let o = &*order;
        (o.total(), o.count(), o.ticket(), o.method)
    };
    let rect = ui.available_rect_before_wrap();
    let (head, body) = match method {
        Some(_) => (
            format!("{}{ticket}", t("주문번호 ", "Order no. ")),
            t("결제가 끝났습니다", "Payment complete").to_owned(),
        ),
        None if count == 0 => (
            t("어서 오세요", "Welcome").to_owned(),
            t("주문을 기다립니다", "Waiting for an order").to_owned(),
        ),
        None => (
            format!("{count}{}", t("개 담김", " in cart")),
            t("확인해 주세요", "Please check").to_owned(),
        ),
    };
    photo(
        ui,
        &art.mark,
        egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + rect.height() * 0.16),
            egui::Vec2::splat(rect.height() * 0.20),
        ),
        cx.theme.color(ColorRole::Primary),
    );
    centered_fit(
        ui,
        egui::pos2(rect.center().x, rect.center().y - rect.height() * 0.09),
        &head,
        rect.width() * 0.86,
        band_text(rect, 0.085),
        cx.theme.color(ColorRole::Muted),
    );
    centered_fit(
        ui,
        egui::pos2(rect.center().x, rect.center().y + rect.height() * 0.12),
        &won(total),
        rect.width() * 0.86,
        band_text(rect, 0.24),
        cx.theme.color(ColorRole::OnSurface),
    );
    centered_fit(
        ui,
        egui::pos2(rect.center().x, rect.bottom() - rect.height() * 0.08),
        &body,
        rect.width() * 0.86,
        band_text(rect, 0.07),
        cx.theme.color(ColorRole::Muted),
    );
}

/// **The counter POS.** The menu on the left, the cart on the right — one screen.
///
/// Whether it splits is settled by [`layout::split_width`]. Narrow the window and it becomes one
/// column by itself, and then it goes back to the portrait layout's flow (menu → confirm).
fn counter(ui: &mut egui::Ui, cx: &mut Cx<'_>, order: &mut Order, art: &Art) {
    let Some(list_w) = layout::split_width(cx) else {
        menu(ui, cx, order, art, true);
        return;
    };
    let full = ui.available_rect_before_wrap();
    // The left is **the menu**, so it has to be wide. `split_width` measures the case where the list is on the left, so it is flipped.
    let split = full.right() - list_w.max(full.width() * 0.34);
    let mut left = band_ui(
        ui,
        egui::Rect::from_min_max(full.min, egui::pos2(split, full.bottom())),
    );
    menu(&mut left, cx, order, art, false);

    ui.painter().vline(
        split,
        full.top()..=full.bottom(),
        egui::Stroke::new(1.0, cx.theme.color(ColorRole::Outline)),
    );
    let mut right = band_ui(
        ui,
        egui::Rect::from_min_max(egui::pos2(split + 1.0, full.top()), full.max),
    );
    cart(&mut right, cx, order, art);
    ui.advance_cursor_after_rect(full);
}

// ── The tour scripts ──────────────────────────────────────────────────────────
//
// Moving is done with `Act::Open` — a coordinate tap misses when the panel's size changes, and what
// this tour has to show is not "where was pressed" but "how each screen stands". Taps are used only
// where the press changes the outcome, as **adding to the cart** does.

/// The portrait kiosk — the whole flow.
const TOUR_PORTRAIT: &[common::Act] = &[
    common::Act::Settle,
    common::Act::Wait(15),
    common::Act::Settle,
    common::Act::Wait(6),
    common::Act::Shot("01-attract.png"),
    // **The language switch**, at 1080 × 2560: a tap on 한국어 writes the `ui.locale` setting, and
    // from the next frame the whole kiosk follows it — the attract screen, then the screens an
    // order goes through. The README's animation is this stretch (`--record`).
    common::Act::Record("kiosk-language"),
    common::Act::Wait(50),
    common::Act::Tap(common::Spot::Text("한국어")),
    common::Act::Settle,
    common::Act::Wait(60),
    common::Act::Expect(common::Expect::Text("주문 시작하기")),
    common::Act::Shot("01b-attract-ko.png"),
    common::Act::Open("pay.mode"),
    common::Act::Settle,
    common::Act::Wait(45),
    common::Act::Open("pay.menu"),
    common::Act::Settle,
    common::Act::Wait(60),
    common::Act::RecordEnd,
    common::Act::Shot("01c-menu-ko.png"),
    // Back to the attract screen and to English, for the rest of the flow.
    common::Act::Open("pay.attract"),
    common::Act::Settle,
    common::Act::Tap(common::Spot::Text("English")),
    common::Act::Settle,
    common::Act::Wait(4),
    common::Act::Expect(common::Expect::Text("Start order")),
    common::Act::Open("pay.mode"),
    common::Act::Settle,
    common::Act::Wait(4),
    common::Act::Shot("02-mode.png"),
    common::Act::Open("pay.menu"),
    common::Act::Settle,
    common::Act::Wait(4),
    common::Act::Shot("03-menu.png"),
    // Add to the cart: each tile by its name, wherever the grid put it, and the bar's count says
    // whether the three went in.
    common::Act::Tap(common::Spot::Text("Cold Brew")),
    common::Act::Settle,
    common::Act::Tap(common::Spot::Text("Cappuccino")),
    common::Act::Settle,
    common::Act::Tap(common::Spot::Text("Vanilla Latte")),
    common::Act::Settle,
    common::Act::Wait(4),
    common::Act::Shot("04-menu-filled.png"),
    common::Act::Open("pay.cart"),
    common::Act::Settle,
    common::Act::Wait(4),
    common::Act::Expect(common::Expect::Text("3 in cart")),
    common::Act::Shot("05-cart.png"),
    common::Act::Open("pay.method"),
    common::Act::Settle,
    common::Act::Wait(4),
    common::Act::Shot("06-method.png"),
    common::Act::Open("pay.progress"),
    common::Act::Settle,
    common::Act::Wait(30),
    common::Act::Shot("07-progress.png"),
    // Once the approval finishes it goes on to the receipt by itself. **Wait generously** — set to the
    // dwell (2 500 ms) exactly, a day when `Settle` spends more frames under load captures the progress
    // screen instead of the receipt. It really did fool us once.
    common::Act::Wait(300),
    common::Act::Settle,
    common::Act::Wait(6),
    common::Act::Shot("08-receipt.png"),
];

/// The counter POS — one screen.
const TOUR_COUNTER: &[common::Act] = &[
    common::Act::Settle,
    common::Act::Wait(15),
    common::Act::Settle,
    common::Act::Wait(6),
    common::Act::Shot("01-pos.png"),
    // The menu column scrolls: a drag up on it, held still before the release.
    common::Act::Press(common::Spot::Page(0.23, 0.75)),
    common::Act::MoveBy {
        dx: 0.0,
        dy: -350.0,
        frames: 10,
    },
    common::Act::Wait(15),
    common::Act::Release,
    common::Act::Settle,
    common::Act::Wait(4),
    common::Act::Shot("01b-pos-scrolled.png"),
    common::Act::Press(common::Spot::Page(0.23, 0.31)),
    common::Act::MoveBy {
        dx: 0.0,
        dy: 350.0,
        frames: 10,
    },
    common::Act::Wait(15),
    common::Act::Release,
    common::Act::Settle,
    // Three tiles by their names — each a name not yet in the cart beside them, where it would
    // be drawn a second time — and the pay button's sum says they went in.
    common::Act::Tap(common::Spot::Text("Americano")),
    common::Act::Settle,
    common::Act::Tap(common::Spot::Text("Cold Brew")),
    common::Act::Settle,
    common::Act::Tap(common::Spot::Text("Caffè Latte")),
    common::Act::Settle,
    common::Act::Wait(4),
    common::Act::Expect(common::Expect::Text("Pay ₩15,000")),
    common::Act::Shot("02-pos-filled.png"),
    common::Act::Open("pay.method"),
    common::Act::Settle,
    common::Act::Wait(4),
    common::Act::Shot("03-pos-method.png"),
];

/// The customer-facing display — there is only one thing to see.
const TOUR_COMPACT: &[common::Act] = &[
    common::Act::Settle,
    common::Act::Wait(15),
    common::Act::Settle,
    common::Act::Wait(6),
    common::Act::Shot("01-display.png"),
];

/// The script for a form.
const fn tour_for(shape: Shape) -> &'static [common::Act] {
    match shape {
        Shape::Portrait => TOUR_PORTRAIT,
        Shape::Counter => TOUR_COUNTER,
        Shape::Compact => TOUR_COMPACT,
    }
}

// ── The wiring ────────────────────────────────────────────────────────────────

fn main() -> fairing::Result<()> {
    common::init_logger();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // **The language to start in is settled before the shell is stood up**, so the screen titles
    // registered below are in it too. They stay in it when the `ui.locale` setting changes later,
    // but no title is ever on screen here: every screen hides both bars.
    set_lang(Lang::parse(&args));
    let size = common::arg_size(&args);
    let options = fairing::runner::Options {
        title: "fairing kiosk".to_owned(),
        size,
        // With no tour and no window size given, it is fullscreen as on a real device.
        fullscreen: size.is_none() && common::arg_tour(&args).is_none(),
    };
    let shape = Shape::pick(&args, size);
    let opts = Opts::parse(&args);
    log::info!("layout {shape:?} - language {:?} - {opts:?}", lang());
    let panel_mm = common::arg_panel_mm(&args);
    let finger_mm = common::arg_finger_mm(&args);
    if panel_mm.is_none() {
        log::warn!("no --panel-mm given - running the density-unaware fallback");
    }
    let build = move |ctx: &egui::Context| build(ctx, shape, opts, panel_mm, finger_mm);
    match common::arg_tour(&args) {
        Some(dir) => common::run_tour(options, dir, tour_for(shape), Box::new(build)),
        None => fairing::runner::run_app(options, Box::new(build)),
    }
}

/// The example's switches. Parsed by this example, not by the crate.
#[derive(Debug, Clone, Copy)]
struct Opts {
    /// Make the card approval fail. **A payment demo with only the success path is not a payment demo.**
    fail: bool,
    /// The mock approval time (ms).
    dwell_ms: u64,
    /// Open the staff gate in advance.
    staff: bool,
}

impl Opts {
    fn parse(args: &[String]) -> Self {
        Self {
            fail: args.iter().any(|a| a == "--fail"),
            dwell_ms: args
                .iter()
                .find_map(|a| a.strip_prefix("--dwell-ms="))
                .and_then(|v| v.parse().ok())
                .unwrap_or(DWELL_MS_DEFAULT),
            staff: args.iter().any(|a| a == "--staff"),
        }
    }
}

/// **Text sized for 1.15 m, on a machine used from about 70 cm** — the portrait kiosk's one knob.
///
/// This used to be a `MetricsSpec` that restated the whole type scale in millimetres, because
/// `ScalePolicy` had no knob for the eye and the only physical term going was the finger. It was a
/// working answer to the right question and it cost the screen its coherence: the text moved 2.30x,
/// the rows moved 2.49x, and **every control stayed at exactly one finger**, because a control's
/// size came from `metrics.touch_target`. A stepper measured 56 px beside a 60 px capital.
///
/// One line now says what the override was trying to say. A 10.1" counter POS is tapped by an
/// assistant 45 cm away, so it keeps the default and is right to — the distance is per-shape, which
/// is exactly what a policy knob is for.
///
/// **Why 1150 and not 700.** The old override drew a 10 mm body, and 10 mm is what 1.15 m asks for;
/// the 70 cm in its own title was never the number it was using. Stating 700 here would shrink the
/// text 1.64x, so the figure is kept and the reason is written down instead: the crate's scale puts
/// a capital at 20.9 arcmin, ISO 9241-303's floor for *comfortable* reading by someone who chose to
/// sit down and read. A café kiosk is read in passing, over a shoulder, by people who did not
/// choose their eyesight, and the way to buy that headroom is to size for further away than the
/// customer stands. It reproduces the shipped render to 0.1 %.
fn standing_policy(shape: Shape, finger_mm: Option<f32>) -> fairing::unit::ScalePolicy {
    // **Gloved, 13 mm**, not the crate's bare-finger default: on a shared screen where winter
    // gloves and elderly hands both turn up, the larger target is right. `--finger-mm=9` tries the
    // bare-finger policy. And a customer stands, so the text is sized for further away than the
    // hand-held default - 500 mm on the counter unit, more on the tall one.
    let mut policy = fairing::unit::ScalePolicy::gloved().with_viewing_distance_mm(500.0);
    if let Some(mm) = finger_mm {
        policy = policy.with_finger_mm(mm);
    }
    if shape == Shape::Portrait {
        policy = policy.with_viewing_distance_mm(1150.0);
    }
    policy
}

fn build(
    ctx: &egui::Context,
    shape: Shape,
    opts: Opts,
    panel_mm: Option<(f32, f32)>,
    finger_mm: Option<f32>,
) -> fairing::Result<(fairing::Shell, Order)> {
    let mut config = ShellConfig::from_toml(CONFIG)?;
    if shape == Shape::Compact {
        // The display is only looked at by the customer — it needs no back or home, and on a 320 du
        // height the nav bar would eat a fifth of the screen.
        config.nav_bar.enabled = false;
    }
    let mut builder = fairing::Shell::builder(config)
        .services(fairing::services::mock::services())
        .fonts(common::korean_fonts());
    // **Pin the physical size** (`--panel-mm=380x900`). Without it the density is reckoned unknown and
    // it falls back to the assumption (1 du = 1/160 in), so a 32-inch panel and a 7-inch panel use the
    // same du — and this demo's whole point, that the column count comes from the physical size rather
    // than the resolution, dies with it. Not given, a warning is logged at startup.
    if let Some((w, h)) = panel_mm {
        builder = builder.physical_mm(w, h);
    }
    builder = builder.scale_policy(standing_policy(shape, finger_mm));
    // **Rows sized for reading, not for the hand.** The crate's row is one touch target, which is
    // right for a panel held at hand-held distance. A customer stands in front of this one and the
    // screens are laid out in multiples of the row, so it takes the row from the text instead:
    // three body ems and a hair, which follows `viewing_distance_mm`. The finger still floors
    // every control through `touch_target`.
    builder = builder.metrics_spec({
        use fairing::unit::{Dim, Span};
        let read = Span::fixed(Dim::text(3.0) + Dim::du(8.0))
            .min(Dim::du(56.0))
            .reanchored();
        fairing::theme::MetricsSpec {
            row_height: read,
            widget_height: read,
            ..fairing::theme::MetricsSpec::default()
        }
    });
    let mut shell = builder.build(ctx)?;
    shell
        .desktop_mut()
        .set_wallpaper(Wallpaper::Solid(ColorRole::Background));
    // The 33 real assets are decoded here, once — outside the frame loop, so it finishes before the
    // first screen appears. On a failure the shell is not stood up: the photographs are the screens, so
    // a fallback would be meaningless.
    let art = Rc::new(Art::load(ctx)?);
    add_screens(&mut shell, shape, &art, opts);
    // **The app owns the order** and lends it to the screens each frame.
    Ok((shell, Order::default()))
}

fn add_screens(shell: &mut fairing::Shell, shape: Shape, art: &Rc<Art>, opts: Opts) {
    // **A customer screen has no chrome.** The clock, the Wi-Fi and the battery on a status bar are
    // none of a customer's business, and an Android-style back bar makes a kiosk look like a phone. The
    // flow is driven by the screens, and the way back is a large button on each screen's bottom bar.
    //
    // The edge gestures are blocked too — a customer must not swipe the shade down. Only the emergency
    // gesture is left.
    let guarded = ChromePolicy {
        status_bar: BarMode::Hide,
        nav_bar: BarMode::Hide,
        allow_peek: false,
        edge_guard: true,
        keep_awake: true,
        ..ChromePolicy::default()
    };

    if shape == Shape::Compact {
        let show_art = Rc::clone(art);
        shell.add(
            screen("pay.display", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                with_order(cx, |order, cx| {
                    customer_display(ui, cx, order, &show_art);
                });
            })
            .title(t("결제", "Payment"))
            .chrome(ChromePolicy {
                keep_awake: true,
                ..ChromePolicy::fullscreen()
            }),
        );
        shell.launch(LaunchAction::open("pay.display"));
        return;
    }

    if shape == Shape::Counter {
        let pos_art = Rc::clone(art);
        shell.add(
            screen("pos.order", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                with_order(cx, |order, cx| counter(ui, cx, order, &pos_art));
            })
            .title(t("주문", "Order"))
            .icon(IconRef::Builtin("grid"))
            .chrome(guarded),
        );
    } else {
        // ── The portrait kiosk: attract → eat in/take away → menu ──
        let attract_art = Rc::clone(art);
        shell.add(
            screen("pay.attract", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                with_order(cx, |order, cx| attract(ui, cx, order, &attract_art));
            })
            .title(t("대기", "Idle"))
            .chrome(ChromePolicy {
                edge_guard: true,
                keep_awake: true,
                ..ChromePolicy::fullscreen()
            }),
        );
        let mode_art = Rc::clone(art);
        shell.add(
            screen("pay.mode", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                with_order(cx, |order, cx| mode(ui, cx, order, &mode_art));
            })
            .title(t("매장 · 포장", "Dine in · Take out"))
            .chrome(guarded),
        );
        let menu_art = Rc::clone(art);
        shell.add(
            screen("pay.menu", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                with_order(cx, |order, cx| menu(ui, cx, order, &menu_art, true));
            })
            .title(t("메뉴", "Menu"))
            .chrome(guarded),
        );
        let cart_art = Rc::clone(art);
        shell.add(
            screen("pay.cart", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                with_order(cx, |order, cx| cart(ui, cx, order, &cart_art));
            })
            .title(t("주문 확인", "Review order"))
            .chrome(guarded),
        );
    }

    add_payment_screens(shell, art, opts, guarded);
    add_staff_screen(shell, opts);

    shell.launch(LaunchAction::open(if shape == Shape::Counter {
        "pos.order"
    } else {
        "pay.attract"
    }));
}

/// The three payment screens — shared by two layouts.
fn add_payment_screens(
    shell: &mut fairing::Shell,
    art: &Rc<Art>,
    opts: Opts,
    guarded: ChromePolicy,
) {
    let method_art = Rc::clone(art);
    shell.add(
        screen("pay.method", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            with_order(cx, |order, cx| method(ui, cx, order, &method_art));
        })
        .title(t("결제 수단", "Payment method"))
        .chrome(guarded),
    );
    let progress_art = Rc::clone(art);
    shell.add(
        screen_with("pay.progress", move || Progress {
            art: Rc::clone(&progress_art),
            dwell: Duration::from_millis(opts.dwell_ms),
            fail: opts.fail,
            started: None,
            settled: false,
        })
        .title(t("결제 중", "Processing"))
        // **There is no back during payment.** Leaving the screen mid-approval puts the card terminal
        // and the screen out of step.
        .chrome(guarded),
    );
    let receipt_art = Rc::clone(art);
    shell.add(
        screen("pay.receipt", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            with_order(cx, |order, cx| receipt(ui, cx, order, &receipt_art));
        })
        .title(t("영수증", "Receipt"))
        .chrome(guarded),
    );
}

/// The staff screen — knocking the four corners plus a gate.
fn add_staff_screen(shell: &mut fairing::Shell, opts: Opts) {
    shell.add(
        screen("staff.orders", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            with_order(cx, |order, cx| staff(ui, cx, order));
        })
        .title(t("직원", "Staff"))
        .gate("staff"),
    );
    shell.add_hidden_entry(
        fairing::access::HiddenEntry::corners(
            "staff",
            fairing::access::Corner::ALL,
            LaunchAction::open("staff.orders"),
        )
        .gate("staff")
        .hint_from(2),
    );
    if opts.staff {
        // `--staff` skips the authentication. On a real device a staff card would go here.
        shell.handle().set_subject(fairing::access::Subject {
            id: Some("staff-demo".to_owned()),
            level: fairing::access::Level(1),
            attrs: std::collections::BTreeMap::default(),
        });
    }
}
