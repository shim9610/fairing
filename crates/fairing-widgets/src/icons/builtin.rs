//! The built-in icon reference constants (the default set v1). Used as
//! `fairing_widgets::icon::GAUGE`.
//!
//! A name is the file name in `assets/icons/<name>.svg`. The source is a Lucide (ISC) subset,
//! saved under **our own names** (Lucide's `arrow-left.svg` becomes `back.svg`, for instance).
//! That every constant resolves through [`super::find`] is checked by the `builtin_names_resolve`
//! test (once the generated table is in place).

use super::IconRef;

macro_rules! icons {
    ($($(#[$doc:meta])* $name:ident = $str:literal;)*) => {
        $( $(#[$doc])* pub const $name: IconRef = IconRef::Builtin($str); )*
        /// Every built-in icon name in v1.
        pub const NAMES: &[&str] = &[$($str),*];
    };
}

icons! {
    /// Back.
    BACK = "back";
    /// Home.
    HOME = "home";
    /// Recent screens.
    RECENTS = "recents";
    /// Close.
    CLOSE = "close";
    /// Menu.
    MENU = "menu";
    /// More.
    MORE = "more";
    /// Up.
    CHEVRON_UP = "chevron-up";
    /// Down.
    CHEVRON_DOWN = "chevron-down";
    /// Left.
    CHEVRON_LEFT = "chevron-left";
    /// Right.
    CHEVRON_RIGHT = "chevron-right";
    /// Search.
    SEARCH = "search";
    /// Wi-Fi.
    WIFI = "wifi";
    /// Wi-Fi off.
    WIFI_OFF = "wifi-off";
    /// Bluetooth.
    BLUETOOTH = "bluetooth";
    /// Battery.
    BATTERY = "battery";
    /// Signal.
    SIGNAL = "signal";
    /// Notifications.
    BELL = "bell";
    /// Notifications off.
    BELL_OFF = "bell-off";
    /// Volume.
    VOLUME = "volume";
    /// Muted.
    VOLUME_OFF = "volume-off";
    /// Light.
    SUN = "sun";
    /// Dark.
    MOON = "moon";
    /// Clock.
    CLOCK = "clock";
    /// Settings.
    SETTINGS = "settings";
    /// Power.
    POWER = "power";
    /// Restart.
    RESTART = "restart";
    /// Lock.
    LOCK = "lock";
    /// Unlock.
    UNLOCK = "unlock";
    /// User.
    USER = "user";
    /// Security.
    SHIELD = "shield";
    /// Key.
    KEY = "key";
    /// Information.
    INFO = "info";
    /// Warning.
    WARNING = "warning";
    /// Error.
    ERROR = "error";
    /// Check.
    CHECK = "check";
    /// Plus.
    PLUS = "plus";
    /// Minus.
    MINUS = "minus";
    /// Refresh.
    REFRESH = "refresh";
    /// Delete.
    TRASH = "trash";
    /// Edit.
    EDIT = "edit";
    /// Gauge.
    GAUGE = "gauge";
    /// Chart.
    CHART = "chart";
    /// Temperature.
    THERMOMETER = "thermometer";
    /// Camera.
    CAMERA = "camera";
    /// CPU.
    CPU = "cpu";
    /// Activity.
    ACTIVITY = "activity";
    /// Wrench.
    WRENCH = "wrench";
    /// Folder.
    FOLDER = "folder";
    /// Display.
    DISPLAY = "display";
    /// Keyboard.
    KEYBOARD = "keyboard";
    /// Language.
    LANGUAGE = "language";
    /// Up.
    ARROW_UP = "arrow-up";
    /// Down.
    ARROW_DOWN = "arrow-down";
    /// Left.
    ARROW_LEFT = "arrow-left";
    /// Right.
    ARROW_RIGHT = "arrow-right";
    /// Split screen.
    SPLIT = "split";
    /// Fullscreen.
    FULLSCREEN = "fullscreen";
    /// Minimise.
    MINIMIZE = "minimize";
    /// Wired network.
    ETHERNET = "ethernet";
    /// USB.
    USB = "usb";
    /// SD card.
    SD_CARD = "sd-card";
    /// Airplane mode.
    AIRPLANE = "airplane";
    /// Brightness.
    BRIGHTNESS = "brightness";
    /// Calendar.
    CALENDAR = "calendar";
    /// Several users.
    USERS = "users";
    /// Download.
    DOWNLOAD = "download";
    /// Upload.
    UPLOAD = "upload";
    /// Save.
    SAVE = "save";
    /// File.
    FILE = "file";
    /// Grid.
    GRID = "grid";
    /// List.
    LIST = "list";
    /// Print.
    PRINTER = "printer";
    /// Terminal.
    TERMINAL = "terminal";
    /// Fan.
    FAN = "fan";
    /// Plug.
    PLUG = "plug";
    /// Image.
    IMAGE = "image";
    /// Memory.
    MEMORY = "memory";
    /// Storage.
    HARD_DRIVE = "hard-drive";
}

#[cfg(test)]
mod tests {
    use super::{IconRef, NAMES};
    use crate::icons::{find, ICONS};

    /// Every name in the default icon set has to be in the generated table. A failure
    /// means an SVG is missing from `assets/icons/` or a file name differs from the table —
    /// regenerate with `cargo xtask icons`.
    #[test]
    fn builtin_names_resolve() {
        let missing: Vec<&str> = NAMES
            .iter()
            .copied()
            .filter(|name| find(name).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "names missing from the generated file: {missing:?}"
        );
    }

    /// The M1 acceptance criterion: icons v1 is about 40 or more.
    #[test]
    fn the_generated_table_covers_the_v1_set() {
        assert!(ICONS.len() >= 40, "only {} icons", ICONS.len());
        assert_eq!(NAMES.len(), 78);
    }

    /// No duplicates in the name list, and the constants and the names point at the same values.
    #[test]
    fn names_are_unique_and_match_the_constants() {
        for (index, name) in NAMES.iter().enumerate() {
            let duplicate = NAMES.iter().skip(index + 1).any(|other| other == name);
            assert!(!duplicate, "the name is taken twice: {name}");
        }
        assert_eq!(super::BACK, IconRef::Builtin("back"));
        assert_eq!(super::LANGUAGE, IconRef::Builtin("language"));
        assert_eq!(NAMES.first(), Some(&"back"));
    }

    /// No icon outside the name list is left in the generated table (the remains of a deleted SVG).
    #[test]
    fn the_generated_table_has_no_strays() {
        let strays: Vec<&str> = ICONS
            .iter()
            .map(|icon| icon.name)
            .filter(|name| !NAMES.contains(name))
            .collect();
        assert!(
            strays.is_empty(),
            "generated but not in the name list: {strays:?}"
        );
    }
}
