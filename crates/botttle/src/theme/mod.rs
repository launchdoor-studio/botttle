//! The resolved look of the app: colors from a [`Palette`], typography from
//! [`Settings`].
//!
//! Views read the [`Theme`] global and never look at settings for appearance, so
//! there is one place where "what does this look like" is decided.

mod palettes;

pub use palettes::{Palette, PALETTES};

use gpui::{px, App, FontFallbacks, Global, Hsla, Pixels, Rgba, SharedString};

use crate::settings::Settings;

pub const DEFAULT_THEME: &str = "Botttle Dark";

/// One corner radius for the whole app. Kept small on purpose.
pub const RADIUS: f32 = 4.0;
/// How opaque a glass sidebar is, relative to the panes' opacity: half as
/// solid, so it reads as clearly more see-through than they do.
const GLASS_SIDEBAR_TINT: f32 = 0.5;

/// Monospace families we look for, in order, when the user hasn't chosen one.
const FONT_CANDIDATES: &[&str] = &[
    "JetBrains Mono",
    "Berkeley Mono",
    "SF Mono",
    "Menlo",
    "DejaVu Sans Mono",
    "Consolas",
];

/// Substrings that mark a family as monospace. Font files don't expose the flag
/// through gpui, so the terminal font list is filtered by name, with an escape
/// hatch in the settings screen to show every installed family.
const MONOSPACE_HINTS: &[&str] = &[
    "mono",
    "code",
    "console",
    "courier",
    "menlo",
    "monaco",
    "terminal",
    "typewriter",
    "hack",
    "iosevka",
    "fira",
    "inconsolata",
    "hasklig",
    "operator",
    "pragmata",
    "sf mono",
    "andale",
    "terminus",
    "victor",
    "dank",
    "anonymous",
    "envy",
    "input",
    "liga",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl Appearance {
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

#[derive(Clone)]
pub struct Theme {
    pub name: SharedString,
    pub appearance: Appearance,

    pub background: Hsla,
    pub surface: Hsla,
    pub elevated: Hsla,
    /// The tab sidebar's ground: the chrome surface, or a light tint of it
    /// when the sidebar is glass.
    pub sidebar: Hsla,
    /// Secondary text in the sidebar: inactive tabs, counts, and controls.
    pub sidebar_text_muted: Hsla,
    pub border: Hsla,
    pub border_focused: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub danger: Hsla,

    /// The grid's ground, always opaque: cells compare against it to decide
    /// whether they need a background of their own.
    pub terminal_background: Hsla,
    /// What a pane actually paints. Transparent while the window is see-through,
    /// so the desktop shows through once rather than through two stacked layers.
    pub pane_background: Hsla,
    /// The window ground with no transparency, for surfaces that have to stay
    /// readable whatever is behind the window.
    pub opaque_background: Hsla,
    pub terminal_foreground: Hsla,
    pub cursor: Hsla,
    pub selection: Hsla,
    pub ansi: [Hsla; 16],

    pub font_family: SharedString,
    /// Where glyphs the terminal font lacks are looked for first.
    pub font_fallbacks: Option<FontFallbacks>,
    pub ui_font_family: SharedString,
    pub font_size: Pixels,
    pub ui_font_size: Pixels,
    pub line_height_factor: f32,
    pub ligatures: bool,
}

impl Global for Theme {}

impl Theme {
    pub fn radius() -> Pixels {
        px(RADIUS)
    }

    pub fn line_height(&self) -> Pixels {
        px((f32::from(self.font_size) * self.line_height_factor).round())
    }

    /// The focused pane's border. Falls back to the accent when a palette has no
    /// distinct focus color.
    pub fn focus_ring(&self) -> Hsla {
        self.border_focused
    }

    fn resolve(palette: &Palette, settings: &Settings, fonts: &FontCatalog) -> Self {
        let mut theme = Self {
            name: SharedString::from(palette.name),
            appearance: palette.appearance,
            background: color(palette.background),
            surface: color(palette.surface),
            elevated: color(palette.elevated),
            sidebar: color(palette.surface),
            sidebar_text_muted: color(palette.text_muted),
            border: color(palette.border),
            border_focused: color(palette.border_focused),
            text: color(palette.text),
            text_muted: color(palette.text_muted),
            accent: color(palette.accent),
            danger: color(palette.danger),
            terminal_background: color(palette.terminal_background),
            pane_background: color(palette.terminal_background),
            opaque_background: color(palette.background),
            terminal_foreground: color(palette.terminal_foreground),
            cursor: color(palette.cursor),
            selection: color(palette.selection),
            ansi: palette.ansi.map(color),
            font_family: fonts.resolve_monospace(settings.terminal_font_family.as_deref()),
            font_fallbacks: fonts.symbol_fallbacks.clone(),
            ui_font_family: fonts.resolve_ui(settings.ui_font_family.as_deref()),
            font_size: px(settings.terminal_font_size.clamp(6.0, 40.0)),
            ui_font_size: px(settings.ui_font_size.clamp(8.0, 24.0)),
            line_height_factor: settings.line_height.clamp(1.0, 2.5),
            ligatures: settings.ligatures,
        };

        // A background override replaces the window and terminal grounds, but
        // leaves chrome surfaces alone so the tab strip stays readable.
        if let Some(background) = settings.background_color() {
            theme.background = background;
            theme.terminal_background = background;
        }
        theme.opaque_background = theme.background;

        // Transparency is applied last, to whatever ground we ended up with.
        let opacity = settings.opacity();
        if settings.is_translucent() {
            // Chrome stays more solid than the grid: tab and status text has to
            // hold up over whatever the desktop puts behind it.
            let chrome = (opacity + 0.22).min(1.0);
            theme.background = theme.background.alpha(opacity);
            theme.surface = theme.surface.alpha(chrome);
            theme.elevated = theme.elevated.alpha(chrome);
            // The sidebar paints the only ground beneath it (the window ground
            // stops at the content column), so this alpha is all there is.
            // Solid, it matches chrome stacked over the window ground; glass
            // keeps only a tint of the surface, so the blur shows through it
            // more than through the panes.
            theme.sidebar = if settings.glass_sidebar {
                // Dimmed text disappears against a bright, busy desktop, so
                // on glass every label is full strength, the way a mac
                // sidebar's are; the active tab still has its pill and dot.
                theme.sidebar_text_muted = theme.text;
                theme.surface.alpha(opacity * GLASS_SIDEBAR_TINT)
            } else {
                theme.surface.alpha(1.0 - (1.0 - chrome) * (1.0 - opacity))
            };
            // The window ground already carries the alpha, so the pane adds none.
            theme.pane_background = theme.terminal_background.alpha(0.0);
        }

        theme
    }
}

/// The font families installed on this machine, split into a monospace subset.
pub struct FontCatalog {
    pub all: Vec<SharedString>,
    pub monospace: Vec<SharedString>,
    default_monospace: SharedString,
    default_ui: SharedString,
    /// An installed Nerd Font, for the icons prompts and editors draw.
    symbol_fallbacks: Option<FontFallbacks>,
}

impl Global for FontCatalog {}

impl FontCatalog {
    fn new(installed: Vec<String>) -> Self {
        let mut all: Vec<SharedString> = installed
            .into_iter()
            // Hidden system faces start with a dot and can't be requested by name.
            .filter(|name| !name.starts_with('.'))
            .map(SharedString::from)
            .collect();
        all.sort_by_key(|name| name.to_lowercase());
        all.dedup();

        let monospace: Vec<SharedString> = all
            .iter()
            .filter(|name| is_monospace_name(name))
            .cloned()
            .collect();

        let default_monospace = FONT_CANDIDATES
            .iter()
            .find(|candidate| all.iter().any(|name| name.as_ref() == **candidate))
            .map(|name| SharedString::from(*name))
            .or_else(|| monospace.first().cloned())
            .unwrap_or_else(|| SharedString::from("monospace"));

        let default_ui = if all.iter().any(|name| name == "Inter") {
            SharedString::from("Inter")
        } else if cfg!(target_os = "macos") {
            SharedString::from("SF Pro Text")
        } else {
            default_monospace.clone()
        };

        let symbol_fallbacks =
            symbol_font(&all).map(|family| FontFallbacks::from_fonts(vec![family.to_string()]));

        Self {
            all,
            monospace,
            default_monospace,
            default_ui,
            symbol_fallbacks,
        }
    }

    pub fn default_monospace(&self) -> SharedString {
        self.default_monospace.clone()
    }

    pub fn default_ui(&self) -> SharedString {
        self.default_ui.clone()
    }

    /// Uses the chosen family when it is actually installed, so a settings file
    /// carried between machines degrades gracefully.
    fn resolve_monospace(&self, chosen: Option<&str>) -> SharedString {
        self.resolve(chosen, &self.default_monospace)
    }

    fn resolve_ui(&self, chosen: Option<&str>) -> SharedString {
        self.resolve(chosen, &self.default_ui)
    }

    fn resolve(&self, chosen: Option<&str>, fallback: &SharedString) -> SharedString {
        chosen
            .and_then(|chosen| {
                self.all
                    .iter()
                    .find(|name| name.as_ref() == chosen)
                    .cloned()
            })
            .unwrap_or_else(|| fallback.clone())
    }
}

/// The Nerd Font to take icons from: the symbols-only one made for exactly
/// this when it is installed, otherwise any single-width ("Mono") Nerd Font,
/// whose icons fit a terminal cell.
fn symbol_font(installed: &[SharedString]) -> Option<&SharedString> {
    let find =
        |predicate: &dyn Fn(&str) -> bool| installed.iter().find(|name| predicate(name.as_ref()));
    find(&|name| name == "Symbols Nerd Font Mono")
        .or_else(|| find(&|name| name == "Symbols Nerd Font"))
        .or_else(|| find(&|name| name.ends_with("Nerd Font Mono")))
        .or_else(|| find(&|name| name.contains("Nerd Font")))
}

fn is_monospace_name(name: &str) -> bool {
    let lowercase = name.to_lowercase();
    MONOSPACE_HINTS.iter().any(|hint| lowercase.contains(hint))
}

fn color(hex: u32) -> Hsla {
    let rgba: Rgba = gpui::rgb(hex);
    rgba.into()
}

pub fn all() -> &'static [Palette] {
    PALETTES
}

pub fn by_name(name: &str) -> &'static Palette {
    PALETTES
        .iter()
        .find(|palette| palette.name == name)
        .unwrap_or_else(|| {
            PALETTES
                .iter()
                .find(|palette| palette.name == DEFAULT_THEME)
                .expect("the default theme exists")
        })
}

/// Rebuilds the theme global from the current settings.
pub fn apply(settings: &Settings, cx: &mut App) {
    let palette = by_name(&settings.theme);
    let fonts = cx.global::<FontCatalog>();
    let theme = Theme::resolve(palette, settings, fonts);
    cx.set_global(theme);
}

/// Builds the font catalog, then resolves the theme. Call after settings init.
pub fn init(cx: &mut App) {
    let catalog = FontCatalog::new(cx.text_system().all_font_names());
    cx.set_global(catalog);
    let settings = cx.global::<Settings>().clone();
    apply(&settings, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_ships_both_appearances() {
        for palette in PALETTES {
            let variants: Vec<_> = PALETTES
                .iter()
                .filter(|other| other.family == palette.family)
                .map(|other| other.appearance)
                .collect();
            assert!(
                variants.contains(&Appearance::Light) && variants.contains(&Appearance::Dark),
                "{} is missing a light or dark variant",
                palette.family
            );
        }
    }

    #[test]
    fn theme_names_are_unique() {
        let mut names: Vec<_> = PALETTES.iter().map(|palette| palette.name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate theme names");
    }

    #[test]
    fn an_unknown_name_falls_back_to_the_default() {
        assert_eq!(by_name("Nonexistent").name, DEFAULT_THEME);
        assert_eq!(by_name("Gruvbox Light").name, "Gruvbox Light");
    }

    #[test]
    fn every_palette_resolves_to_a_readable_theme() {
        let catalog = FontCatalog::new(vec!["Menlo".to_string(), "Inter".to_string()]);
        let mut settings = Settings::default();

        for palette in PALETTES {
            settings.theme = palette.name.to_string();
            let theme = Theme::resolve(palette, &settings, &catalog);
            assert_ne!(theme.text, theme.background, "{}", palette.name);
            assert_ne!(
                theme.terminal_foreground, theme.terminal_background,
                "{}",
                palette.name
            );
            assert_ne!(theme.border, theme.border_focused, "{}", palette.name);
        }
    }

    #[test]
    fn a_background_override_replaces_window_and_terminal_grounds() {
        let catalog = FontCatalog::new(vec!["Menlo".to_string()]);
        let settings = Settings {
            background: Some("#123456".to_string()),
            ..Settings::default()
        };

        let theme = Theme::resolve(&PALETTES[0], &settings, &catalog);
        let expected = crate::settings::parse_hex("#123456").expect("valid hex");
        assert_eq!(theme.background, expected);
        assert_eq!(theme.terminal_background, expected);
        // Chrome keeps the palette's own surface so the tab strip stays legible.
        assert_ne!(theme.surface, expected);
    }

    #[test]
    fn a_see_through_window_paints_its_panes_transparent() {
        let catalog = FontCatalog::new(vec!["Menlo".to_string()]);
        let settings = Settings {
            opacity: 0.7,
            ..Settings::default()
        };

        let theme = Theme::resolve(&PALETTES[0], &settings, &catalog);
        // The window ground carries the transparency; the pane adds none, so the
        // desktop shows through one layer rather than two stacked ones.
        assert_eq!(theme.pane_background.a, 0.0);
        assert!((theme.background.a - 0.7).abs() < 0.001);
        // Cells compare against an opaque ground, and the settings panel needs one.
        assert_eq!(theme.terminal_background.a, 1.0);
        assert_eq!(theme.opaque_background.a, 1.0);
        // Chrome stays more solid than the grid.
        assert!(theme.surface.a > theme.background.a);
    }

    #[test]
    fn an_opaque_window_paints_the_palette_untouched() {
        let catalog = FontCatalog::new(vec!["Menlo".to_string()]);
        let theme = Theme::resolve(&PALETTES[0], &Settings::default(), &catalog);
        assert_eq!(theme.background.a, 1.0);
        assert_eq!(theme.pane_background, theme.terminal_background);
    }

    #[test]
    fn a_font_that_is_not_installed_falls_back() {
        let catalog = FontCatalog::new(vec!["Menlo".to_string()]);
        let settings = Settings {
            terminal_font_family: Some("Not Installed".to_string()),
            ..Settings::default()
        };

        let theme = Theme::resolve(&PALETTES[0], &settings, &catalog);
        assert_eq!(theme.font_family.as_ref(), "Menlo");
    }

    #[test]
    fn monospace_detection_covers_common_families() {
        assert!(is_monospace_name("JetBrains Mono"));
        assert!(is_monospace_name("Menlo"));
        assert!(is_monospace_name("Cascadia Code"));
        assert!(!is_monospace_name("Helvetica Neue"));
    }
}
