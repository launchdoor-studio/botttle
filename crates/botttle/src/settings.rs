//! User settings: what they are, where they live, and how they reach the UI.
//!
//! [`Settings`] is the source of truth and is persisted as JSON. The [`Theme`]
//! global is derived from it, so every change goes through [`Settings::update`],
//! which re-resolves the theme and redraws.

use std::path::PathBuf;

use gpui::{App, BorrowAppContext, Global, Hsla, Rgba, WindowBackgroundAppearance};
use serde::{Deserialize, Serialize};

use crate::theme;

pub const DEFAULT_FONT_SIZE: f32 = 13.0;
pub const DEFAULT_UI_FONT_SIZE: f32 = 12.0;
pub const DEFAULT_LINE_HEIGHT: f32 = 1.4;
pub const DEFAULT_SCROLLBACK: usize = 10_000;
/// Below this the text stops being readable over anything busy.
pub const MIN_OPACITY: f32 = 0.3;
pub const DEFAULT_BLUR_RADIUS: u32 = 20;
pub const DEFAULT_SIDEBAR_WIDTH: f32 = 220.0;
/// Narrower than this and tab names are cut to a few letters.
pub const MIN_SIDEBAR_WIDTH: f32 = 140.0;
pub const MAX_SIDEBAR_WIDTH: f32 = 480.0;
pub const MAX_BLUR_RADIUS: u32 = 64;

/// How the cursor is drawn in a focused terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

impl CursorShape {
    pub const ALL: [CursorShape; 3] = [Self::Block, Self::Bar, Self::Underline];

    pub fn label(self) -> &'static str {
        match self {
            Self::Block => "Block",
            Self::Bar => "Bar",
            Self::Underline => "Underline",
        }
    }
}

/// Where the tabs live: a strip across the top, or a sidebar down the left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabLayout {
    Horizontal,
    Vertical,
}

/// Which app icon to show. Default follows the active theme: the twilight
/// logo for dark themes, the bright one for light themes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconStyle {
    /// Also what a settings file from before the fixed twilight choice was
    /// dropped reads as, rather than failing to load the whole file.
    #[default]
    #[serde(alias = "twilight")]
    Default,
    Bright,
}

impl IconStyle {
    pub const ALL: [Self; 2] = [Self::Default, Self::Bright];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default · Dynamic",
            Self::Bright => "Bright",
        }
    }
}

impl TabLayout {
    pub const ALL: [TabLayout; 2] = [Self::Horizontal, Self::Vertical];

    pub fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "Top strip",
            Self::Vertical => "Sidebar",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// A theme name from [`theme::all`], e.g. "Gruvbox Dark".
    pub theme: String,
    pub icon_style: IconStyle,
    /// `None` means "pick the best monospace font installed".
    pub terminal_font_family: Option<String>,
    pub terminal_font_size: f32,
    /// Multiplied by the font size to get the height of a terminal row.
    pub line_height: f32,
    /// Programming ligatures, where the font provides them.
    pub ligatures: bool,
    pub ui_font_family: Option<String>,
    pub ui_font_size: f32,
    /// Overrides the theme's background, as `#rrggbb`.
    pub background: Option<String>,
    /// How opaque the window is, 0.3 to 1.0. Below 1 the desktop shows through.
    pub opacity: f32,
    /// Blur whatever is behind the window. Only visible when `opacity` is below 1.
    pub blur: bool,
    /// How far the blur spreads, in points. Higher reads as frostier glass.
    pub blur_radius: u32,
    pub tab_layout: TabLayout,
    /// Whether the tab sidebar is open. Only meaningful with the sidebar layout.
    pub show_sidebar: bool,
    /// How wide the tab sidebar is, in points. Set by dragging its edge.
    pub sidebar_width: f32,
    pub cursor_shape: CursorShape,
    /// Applies to panes opened from now on.
    pub scrollback_lines: usize,
    /// Paste an image from the clipboard as a file path, the way dropping a file
    /// onto a terminal works. Turn off to send ctrl-v straight to the program.
    pub paste_images: bool,
    /// Send `ESC CR` for shift-enter so programs can tell it apart from enter.
    /// Turn off to send the plain carriage return terminals have always sent.
    pub shift_enter_newline: bool,
    /// Copy text to the clipboard as soon as it is selected, without cmd-c.
    pub copy_on_select: bool,
    /// Show which tabs are busy and which have finished, in the tab list.
    pub tab_status: bool,
    /// Chime when something finishes in a tab you aren't looking at.
    pub done_sound: bool,
}

impl Global for Settings {}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: theme::DEFAULT_THEME.to_string(),
            icon_style: IconStyle::Default,
            terminal_font_family: None,
            terminal_font_size: DEFAULT_FONT_SIZE,
            line_height: DEFAULT_LINE_HEIGHT,
            ligatures: true,
            ui_font_family: None,
            ui_font_size: DEFAULT_UI_FONT_SIZE,
            background: None,
            opacity: 1.0,
            blur: false,
            blur_radius: DEFAULT_BLUR_RADIUS,
            tab_layout: TabLayout::Horizontal,
            show_sidebar: true,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            cursor_shape: CursorShape::Block,
            scrollback_lines: DEFAULT_SCROLLBACK,
            paste_images: true,
            shift_enter_newline: true,
            copy_on_select: true,
            tab_status: true,
            done_sound: true,
        }
    }
}

impl Settings {
    /// Reads the settings file, falling back to defaults if it is missing or
    /// unreadable. A malformed file is left on disk rather than overwritten.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match serde_json::from_str(&contents) {
            Ok(settings) => settings,
            Err(error) => {
                eprintln!("botttle: ignoring {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(error) = std::fs::write(&path, json) {
                    eprintln!("botttle: could not write {}: {error}", path.display());
                }
            }
            Err(error) => eprintln!("botttle: could not serialize settings: {error}"),
        }
    }

    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("botttle").join("settings.json"))
    }

    /// Clamped so the window can never be dragged into invisibility.
    pub fn opacity(&self) -> f32 {
        self.opacity.clamp(MIN_OPACITY, 1.0)
    }

    pub fn sidebar_width(&self) -> f32 {
        clamp_sidebar_width(self.sidebar_width)
    }

    pub fn is_translucent(&self) -> bool {
        self.opacity() < 0.999
    }

    /// What the platform is told to do behind the window. An opaque window gets
    /// `Opaque` whatever the blur setting says, because there is nothing to see
    /// through — and telling the compositor so saves it drawing what is behind.
    ///
    /// On macOS the blur is ours rather than GPUI's (see [`crate::glass`]), so
    /// the window is only ever asked to be see-through.
    pub fn window_background(&self) -> WindowBackgroundAppearance {
        if !self.is_translucent() {
            WindowBackgroundAppearance::Opaque
        } else if self.blur && !cfg!(target_os = "macos") {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Transparent
        }
    }

    /// The blur to put behind the window, or 0 for none.
    pub fn blur_radius(&self) -> u32 {
        if self.is_translucent() && self.blur {
            self.blur_radius.clamp(1, MAX_BLUR_RADIUS)
        } else {
            0
        }
    }

    /// The background override, if it parses.
    pub fn background_color(&self) -> Option<Hsla> {
        self.background.as_deref().and_then(parse_hex)
    }

    /// Applies an edit: saves it, re-resolves the theme, and redraws every window.
    pub fn update(cx: &mut App, edit: impl FnOnce(&mut Settings)) {
        cx.update_global::<Settings, _>(|settings, _| edit(settings));
        let settings = cx.global::<Settings>().clone();
        settings.save();
        theme::apply(&settings, cx);
        crate::assets::apply_app_icon(settings.icon_style, cx.global::<theme::Theme>().appearance);

        // Transparency is a property of the window itself, not of anything we
        // paint, so every open window has to be told.
        // Deferred on purpose. Settings usually change from a click inside the
        // settings screen, which runs inside an update of the very window we
        // need to touch — and a re-entrant window update fails rather than
        // nesting, so applied inline this silently did nothing.
        let background = settings.window_background();
        cx.defer(move |cx| {
            for window in cx.windows() {
                if window
                    .update(cx, |_, window, _| {
                        window.set_background_appearance(background);
                        crate::glass::apply(window, &settings);
                    })
                    .is_err()
                {
                    eprintln!("botttle: could not apply window transparency");
                }
            }
        });
        cx.refresh_windows();
    }
}

/// Parses `#rrggbb` or `rrggbb`.
pub fn parse_hex(hex: &str) -> Option<Hsla> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(
        Rgba {
            r: ((value >> 16) & 0xff) as f32 / 255.0,
            g: ((value >> 8) & 0xff) as f32 / 255.0,
            b: (value & 0xff) as f32 / 255.0,
            a: 1.0,
        }
        .into(),
    )
}

/// Formats a color as `#rrggbb`, for storing a swatch choice.
pub fn to_hex(color: Hsla) -> String {
    let rgba: Rgba = color.into();
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgba.r * 255.0).round() as u8,
        (rgba.g * 255.0).round() as u8,
        (rgba.b * 255.0).round() as u8
    )
}

/// Initializes settings before anything reads the theme.
pub fn init(cx: &mut App) {
    cx.set_global(Settings::load());
}

/// Keeps a dragged or hand-edited width within what the sidebar can use.
pub fn clamp_sidebar_width(width: f32) -> f32 {
    if width.is_finite() {
        width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
    } else {
        DEFAULT_SIDEBAR_WIDTH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let color = parse_hex("#1e2030").expect("valid hex");
        assert_eq!(to_hex(color), "#1e2030");
    }

    #[test]
    fn malformed_hex_is_rejected() {
        assert!(parse_hex("nope").is_none());
        assert!(parse_hex("#12345").is_none());
    }

    #[test]
    fn defaults_round_trip_through_json() {
        let json = serde_json::to_string(&Settings::default()).expect("serializes");
        let parsed: Settings = serde_json::from_str(&json).expect("parses");
        assert_eq!(parsed.theme, Settings::default().theme);
        assert_eq!(parsed.cursor_shape, CursorShape::Block);
    }

    #[test]
    fn an_opaque_window_stays_opaque_whatever_blur_says() {
        let settings = Settings {
            opacity: 1.0,
            blur: true,
            ..Settings::default()
        };
        assert!(matches!(
            settings.window_background(),
            WindowBackgroundAppearance::Opaque
        ));
    }

    #[test]
    fn a_see_through_window_blurs_only_when_asked() {
        let blurred = Settings {
            opacity: 0.8,
            blur: true,
            ..Settings::default()
        };
        assert_eq!(blurred.blur_radius(), DEFAULT_BLUR_RADIUS);

        let plain = Settings {
            blur: false,
            ..blurred.clone()
        };
        assert_eq!(plain.blur_radius(), 0);
        assert!(matches!(
            plain.window_background(),
            WindowBackgroundAppearance::Transparent
        ));

        let opaque = Settings {
            opacity: 1.0,
            ..blurred
        };
        assert_eq!(opaque.blur_radius(), 0);
    }

    #[test]
    fn blur_radius_is_clamped() {
        let settings = Settings {
            opacity: 0.8,
            blur: true,
            blur_radius: 500,
            ..Settings::default()
        };
        assert_eq!(settings.blur_radius(), MAX_BLUR_RADIUS);
    }

    #[test]
    fn opacity_cannot_be_dragged_into_invisibility() {
        let settings = Settings {
            opacity: 0.0,
            ..Settings::default()
        };
        assert_eq!(settings.opacity(), MIN_OPACITY);

        let settings = Settings {
            opacity: 4.0,
            ..Settings::default()
        };
        assert_eq!(settings.opacity(), 1.0);
    }

    #[test]
    fn unknown_and_missing_fields_fall_back_to_defaults() {
        let parsed: Settings =
            serde_json::from_str(r#"{"terminal_font_size": 15.0}"#).expect("parses");
        assert_eq!(parsed.terminal_font_size, 15.0);
        assert_eq!(parsed.line_height, DEFAULT_LINE_HEIGHT);
        assert_eq!(parsed.tab_layout, TabLayout::Horizontal);
    }

    #[test]
    fn a_stored_twilight_icon_reads_as_the_dynamic_default() {
        let parsed: Settings =
            serde_json::from_str(r#"{"icon_style": "twilight", "terminal_font_size": 15.0}"#)
                .expect("parses");
        assert_eq!(parsed.icon_style, IconStyle::Default);
        assert_eq!(parsed.terminal_font_size, 15.0);
    }

    #[test]
    fn tab_layout_is_stored_by_name() {
        let parsed: Settings =
            serde_json::from_str(r#"{"tab_layout": "vertical"}"#).expect("parses");
        assert_eq!(parsed.tab_layout, TabLayout::Vertical);
    }
}
