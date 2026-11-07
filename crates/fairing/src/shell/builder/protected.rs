//! Callback-free construction capability for the protected runtime.
#[cfg(feature = "osk")]
use super::OskHooks;
#[cfg(feature = "overlay")]
use super::ShadeHooks;
use super::{
    wallpaper_has_callback, Authenticator, ComponentSpec, ControlSpec, ElevationSpec, FontSet,
    MetricsSpec, NotifyHooks, Palette, Result, ScalePolicy, Services, Shell, ShellBuilder,
    ShellConfig, Theme, Translations,
};
use crate::desktop::Wallpaper;

// Only this constructor can establish the invariant. The recursive predicate is
// shared with the unbounded verifier; callbacks never leave a rejected wrapper.
struct CallbackFreeWallpaper(Wallpaper);
impl CallbackFreeWallpaper {
    fn new(value: Wallpaper) -> Option<Self> {
        if wallpaper_has_callback(&value) {
            None
        } else {
            Some(Self(value))
        }
    }
    fn into_inner(self) -> Wallpaper {
        self.0
    }
}

// The declaration is also the conversion inventory. Exhaustive construction of
// ShellBuilder prevents new fields from silently entering protected construction.
macro_rules! callback_free_builder {
    ($($field:ident: $kind:ty,)*) => {
        pub(in crate::shell) struct CallbackFreeBuilder {
            $($field: $kind,)*
            wallpaper: Option<CallbackFreeWallpaper>,
        }
        impl CallbackFreeBuilder {
            pub(in crate::shell) fn from_builder(builder: ShellBuilder) -> Result<Self> {
                let ShellBuilder {
                    $($field,)* wallpaper,
                    status_bar_painter: _, status_bar_layout: _,
                    nav_bar_painter: _, nav_bar_layout: _, notify_hooks: _,
                    #[cfg(feature = "osk")] osk_hooks: _,
                    #[cfg(feature = "overlay")] shade_hooks: _,
                    slot_painter: _, image_loader: _, lock_screen_painter: _,
                    unlock_prompt_painter: _, recents: _, widget_painters: _,
                    gesture_handle_painter: _,
                } = builder;
                let wallpaper = match wallpaper {
                    Some(value) => Some(CallbackFreeWallpaper::new(value).ok_or_else(||
                        crate::Error::Config("protected wallpaper contains a callback".to_owned()))?),
                    None => None,
                };
                Ok(Self { $($field,)* wallpaper })
            }
            fn into_builder(self) -> ShellBuilder {
                ShellBuilder {
                    $($field: self.$field,)*
                    wallpaper: self.wallpaper.map(CallbackFreeWallpaper::into_inner),
                    status_bar_painter: None, status_bar_layout: None,
                    nav_bar_painter: None, nav_bar_layout: None,
                    notify_hooks: NotifyHooks::default(),
                    #[cfg(feature = "osk")] osk_hooks: OskHooks::default(),
                    #[cfg(feature = "overlay")] shade_hooks: ShadeHooks::default(),
                    slot_painter: None, image_loader: None, lock_screen_painter: None,
                    unlock_prompt_painter: None,
                    recents: crate::workspace::RecentsPainters::default(),
                    widget_painters: None, gesture_handle_painter: None,
                }
            }
            pub(in crate::shell) fn build(self, ctx: &egui::Context) -> Result<Shell> {
                self.into_builder().build(ctx)
            }
        }
    };
}
callback_free_builder! {
    config: ShellConfig,
    recovery_ui: Option<crate::recovery::RecoveryUi>,
    command_capacity: usize,
    commands_per_update: usize,
    toast_capacity: usize,
    toast_overflow: crate::notify::OverflowPolicy,
    services: Option<Services>,
    theme: Option<Theme>,
    palettes: Option<(Palette, Palette)>,
    fonts: FontSet,
    translations: Vec<Translations>,
    scale_policy: ScalePolicy,
    metrics_spec: Option<MetricsSpec>,
    component_spec: Option<ComponentSpec>,
    control_spec: Option<ControlSpec>,
    elevation_spec: Option<ElevationSpec>,
    physical_mm: Option<(f32, f32)>,
    authenticator: Option<Box<dyn Authenticator>>,
}
