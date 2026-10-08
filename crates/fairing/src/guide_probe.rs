//! The guide doctest gate. Each page of `docs/guide`, and the crate README that crates.io shows,
//! becomes the doc comment of one item here, so rustdoc compiles every Rust block on it. Only built for doctests with every feature on — see
//! where `lib.rs` declares it. The paths reach outside the crate, which is why the module must
//! never be compiled in a normal build: a packaged crate does not carry `docs/`.

#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/01-getting-started.md")]
pub struct Guide01GettingStarted;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/02-screens.md")]
pub struct Guide02Screens;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/03-chrome.md")]
pub struct Guide03Chrome;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/04-customization.md")]
pub struct Guide04Customization;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/05-access-control.md")]
pub struct Guide05AccessControl;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/06-services.md")]
pub struct Guide06Services;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/07-config-reference.md")]
pub struct Guide07ConfigReference;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/08-troubleshooting.md")]
pub struct Guide08Troubleshooting;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/09-branding.md")]
pub struct Guide09Branding;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/README.md")]
pub struct GuideReadme;

#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct CrateReadme;
