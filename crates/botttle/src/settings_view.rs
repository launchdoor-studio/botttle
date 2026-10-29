//! The settings screen: a panel over the workspace.
//!
//! Every control writes straight through [`Settings::update`], which persists the
//! change and redraws, so what you see is always what is on disk.

use gpui::{
    div, img, prelude::*, px, uniform_list, App, Context, EventEmitter, FocusHandle, Focusable,
    Hsla, IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Window,
};

use crate::assets;
use crate::settings::{
    self, CursorShape, IconStyle, Settings, TabLayout, MAX_BLUR_RADIUS, MIN_OPACITY,
};
use crate::theme::{self, Appearance, FontCatalog, Theme};

/// Height of the scrollable font pickers.
const FONT_LIST_HEIGHT: f32 = 190.0;
/// Side of each app icon preview.
const ICON_PREVIEW_SIZE: f32 = 48.0;

pub enum SettingsViewEvent {
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Appearance,
    Typography,
    Terminal,
}

impl Section {
    const ALL: [Section; 3] = [Self::Appearance, Self::Typography, Self::Terminal];

    fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Typography => "Typography",
            Self::Terminal => "Terminal",
        }
    }
}

pub struct SettingsView {
    focus_handle: FocusHandle,
    section: Section,
    /// Terminal font list is filtered to likely monospace families unless this is set.
    show_all_terminal_fonts: bool,
}

impl SettingsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            section: Section::Appearance,
            show_all_terminal_fonts: false,
        }
    }

    fn render_appearance(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = cx.global::<Settings>().clone();
        let current = theme.name.clone();
        let cards = theme::all().iter().map(|palette| {
            let name = palette.name;
            let selected = current.as_ref() == name;
            let swatches = [
                palette.background,
                palette.accent,
                palette.ansi[2],
                palette.ansi[1],
            ];

            div()
                .id(SharedString::from(format!("theme-{name}")))
                .w(px(158.0))
                .p_2()
                .flex()
                .flex_col()
                .gap_2()
                .rounded(Theme::radius())
                .border_1()
                .border_color(if selected {
                    theme.border_focused
                } else {
                    theme.border
                })
                .bg(if selected {
                    theme.elevated
                } else {
                    theme.surface
                })
                .hover(|style| style.border_color(theme.border_focused))
                .cursor_pointer()
                .child(div().flex().gap_1().children(swatches.map(|swatch| {
                    div()
                        .w(px(28.0))
                        .h(px(16.0))
                        .rounded(px(2.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(hex(swatch))
                })))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_color(theme.text).child(palette.family))
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(theme.text_muted)
                                .child(palette.appearance.label()),
                        ),
                )
                .on_click(move |_, _, cx| {
                    Settings::update(cx, |settings| settings.theme = name.to_string());
                })
        });

        let background_options = background_choices(theme);
        let current_background = settings.background.clone();

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(section_header(
                "Theme",
                "Colors for the app and the terminal grid",
                theme,
            ))
            .child(div().flex().flex_wrap().gap_2().children(cards))
            .child(section_header(
                "App Icon",
                "Default follows the current theme; Bright stays fixed",
                theme,
            ))
            .child(div().flex().gap_2().children(IconStyle::ALL.map(|style| {
                let selected = settings.icon_style == style;
                let previews: Vec<&'static str> = if style == IconStyle::Default {
                    vec![assets::LOGO, assets::LOGO_TWILIGHT]
                } else {
                    vec![assets::logo_for(style, theme.appearance)]
                };
                div()
                    .id(SharedString::from(format!("app-icon-{}", style.label())))
                    .w(px(158.0))
                    .h(px(104.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .rounded(Theme::radius())
                    .border_1()
                    .border_color(if selected {
                        theme.border_focused
                    } else {
                        theme.border
                    })
                    .bg(if selected {
                        theme.elevated
                    } else {
                        theme.surface
                    })
                    .hover(|element| element.border_color(theme.border_focused))
                    .cursor_pointer()
                    .child(
                        div().flex().gap_2().children(
                            previews
                                .into_iter()
                                // Rounded like the Dock draws it, so the preview
                                // looks like the icon you will actually get.
                                .map(|path| {
                                    img(assets::embedded(path))
                                        .size(px(ICON_PREVIEW_SIZE))
                                        .rounded(px(ICON_PREVIEW_SIZE * 0.225))
                                }),
                        ),
                    )
                    .child(div().text_color(theme.text).child(style.label()))
                    .on_click(move |_, _, cx| {
                        Settings::update(cx, |settings| settings.icon_style = style);
                    })
            })))
            .child(section_header(
                "Tabs",
                "A strip across the top, or a sidebar down the left",
                theme,
            ))
            .child(div().flex().gap_2().children(TabLayout::ALL.map(|layout| {
                chip(
                    SharedString::from(format!("tabs-{}", layout.label())),
                    layout.label(),
                    settings.tab_layout == layout,
                    theme,
                    move |_, _, cx| Settings::update(cx, |settings| settings.tab_layout = layout),
                )
            })))
            .when(settings.tab_layout == TabLayout::Vertical, |element| {
                element.child(
                    div()
                        .flex()
                        .gap_4()
                        .items_center()
                        .child(chip(
                            "chip-glass-sidebar",
                            "Glass sidebar",
                            settings.glass_sidebar,
                            theme,
                            |_, _, cx| {
                                Settings::update(cx, |settings| {
                                    settings.glass_sidebar = !settings.glass_sidebar
                                })
                            },
                        ))
                        .when(!settings.is_translucent(), |element| {
                            element.child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child("Glass applies once opacity is below 100%"),
                            )
                        }),
                )
            })
            .child(section_header(
                "Transparency",
                "How much of the desktop shows through the window",
                theme,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .items_center()
                    .child(stepper(
                        "opacity",
                        "Opacity",
                        format!("{:.0}%", settings.opacity() * 100.0),
                        theme,
                        |cx| {
                            Settings::update(cx, |settings| {
                                settings.opacity = (settings.opacity() - 0.05).max(MIN_OPACITY)
                            })
                        },
                        |cx| {
                            Settings::update(cx, |settings| {
                                settings.opacity = (settings.opacity() + 0.05).min(1.0)
                            })
                        },
                    ))
                    .child(chip(
                        "chip-blur",
                        "Blur behind",
                        settings.blur,
                        theme,
                        |_, _, cx| Settings::update(cx, |settings| settings.blur = !settings.blur),
                    ))
                    .when(settings.blur, |element| {
                        element.child(stepper(
                            "blur-radius",
                            "Blur",
                            settings.blur_radius.min(MAX_BLUR_RADIUS).to_string(),
                            theme,
                            |cx| {
                                Settings::update(cx, |settings| {
                                    settings.blur_radius =
                                        settings.blur_radius.saturating_sub(4).max(4)
                                })
                            },
                            |cx| {
                                Settings::update(cx, |settings| {
                                    settings.blur_radius =
                                        (settings.blur_radius + 4).min(MAX_BLUR_RADIUS)
                                })
                            },
                        ))
                    })
                    .when(!settings.is_translucent(), |element| {
                        element.child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme.text_muted)
                                .child("Blur applies once opacity is below 100%"),
                        )
                    }),
            )
            .child(section_header(
                "Background",
                "Overrides the window and terminal background of the current theme",
                theme,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(chip(
                        "chip-bg-default",
                        "Theme default",
                        current_background.is_none(),
                        theme,
                        |_, _, cx| Settings::update(cx, |settings| settings.background = None),
                    ))
                    .children(background_options.into_iter().map(|(label, value)| {
                        let selected = current_background.as_deref() == Some(value.as_str());
                        let swatch = settings::parse_hex(&value).unwrap_or(theme.background);
                        let stored = value.clone();

                        div()
                            .id(SharedString::from(format!("bg-{value}")))
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(28.0))
                            .px_2()
                            .rounded(Theme::radius())
                            .border_1()
                            .border_color(if selected {
                                theme.border_focused
                            } else {
                                theme.border
                            })
                            .bg(theme.surface)
                            .hover(|style| style.border_color(theme.border_focused))
                            .cursor_pointer()
                            .child(
                                div()
                                    .w(px(14.0))
                                    .h(px(14.0))
                                    .rounded(px(2.0))
                                    .border_1()
                                    .border_color(theme.border)
                                    .bg(swatch),
                            )
                            .child(div().text_color(theme.text_muted).child(label))
                            .on_click(move |_, _, cx| {
                                let stored = stored.clone();
                                Settings::update(cx, move |settings| {
                                    settings.background = Some(stored)
                                });
                            })
                    })),
            )
    }

    fn render_typography(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = cx.global::<Settings>().clone();
        let fonts = cx.global::<FontCatalog>();
        let terminal_fonts: Vec<SharedString> = if self.show_all_terminal_fonts {
            fonts.all.clone()
        } else {
            fonts.monospace.clone()
        };
        let ui_fonts = fonts.all.clone();
        let automatic_terminal = fonts.default_monospace();
        let automatic_ui = fonts.default_ui();
        let terminal_is_automatic = settings.terminal_font_family.is_none();
        let ui_is_automatic = settings.ui_font_family.is_none();

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_header(
                        "Terminal font",
                        "Used for every terminal pane",
                        theme,
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(chip(
                                "chip-terminal-auto",
                                SharedString::from(format!("Automatic ({automatic_terminal})")),
                                terminal_is_automatic,
                                theme,
                                |_, _, cx| {
                                    Settings::update(cx, |settings| {
                                        settings.terminal_font_family = None
                                    })
                                },
                            ))
                            .child(chip(
                                "chip-all-fonts",
                                if self.show_all_terminal_fonts {
                                    "Monospace only"
                                } else {
                                    "Show all fonts"
                                },
                                false,
                                theme,
                                cx.listener(|this, _, _, cx| {
                                    this.show_all_terminal_fonts = !this.show_all_terminal_fonts;
                                    cx.notify();
                                }),
                            )),
                    ),
            )
            .child(font_list(
                "terminal-fonts",
                terminal_fonts,
                theme.font_family.clone(),
                theme,
                |family, cx| {
                    Settings::update(cx, |settings| settings.terminal_font_family = Some(family))
                },
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .child(stepper(
                        "font-size",
                        "Size",
                        format!("{:.0}px", settings.terminal_font_size),
                        theme,
                        |cx| {
                            Settings::update(cx, |settings| {
                                settings.terminal_font_size =
                                    (settings.terminal_font_size - 1.0).max(6.0)
                            })
                        },
                        |cx| {
                            Settings::update(cx, |settings| {
                                settings.terminal_font_size =
                                    (settings.terminal_font_size + 1.0).min(40.0)
                            })
                        },
                    ))
                    .child(stepper(
                        "line-height",
                        "Line height",
                        format!("{:.2}", settings.line_height),
                        theme,
                        |cx| {
                            Settings::update(cx, |settings| {
                                settings.line_height = (settings.line_height - 0.05).max(1.0)
                            })
                        },
                        |cx| {
                            Settings::update(cx, |settings| {
                                settings.line_height = (settings.line_height + 0.05).min(2.5)
                            })
                        },
                    ))
                    .child(chip(
                        "chip-ligatures",
                        "Ligatures",
                        settings.ligatures,
                        theme,
                        |_, _, cx| {
                            Settings::update(cx, |settings| {
                                settings.ligatures = !settings.ligatures
                            })
                        },
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_header(
                        "Interface font",
                        "Tabs, status bar, and this screen",
                        theme,
                    ))
                    .child(chip(
                        "chip-ui-auto",
                        SharedString::from(format!("Automatic ({automatic_ui})")),
                        ui_is_automatic,
                        theme,
                        |_, _, cx| Settings::update(cx, |settings| settings.ui_font_family = None),
                    )),
            )
            .child(font_list(
                "ui-fonts",
                ui_fonts,
                theme.ui_font_family.clone(),
                theme,
                |family, cx| {
                    Settings::update(cx, |settings| settings.ui_font_family = Some(family))
                },
            ))
            .child(stepper(
                "ui-font-size",
                "Interface size",
                format!("{:.0}px", settings.ui_font_size),
                theme,
                |cx| {
                    Settings::update(cx, |settings| {
                        settings.ui_font_size = (settings.ui_font_size - 1.0).max(8.0)
                    })
                },
                |cx| {
                    Settings::update(cx, |settings| {
                        settings.ui_font_size = (settings.ui_font_size + 1.0).min(24.0)
                    })
                },
            ))
    }

    fn render_terminal(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = cx.global::<Settings>().clone();

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(section_header(
                "Cursor",
                "How the cursor is drawn in the focused pane",
                theme,
            ))
            .child(div().flex().gap_2().children(CursorShape::ALL.map(|shape| {
                chip(
                    SharedString::from(format!("cursor-{}", shape.label())),
                    shape.label(),
                    settings.cursor_shape == shape,
                    theme,
                    move |_, _, cx| Settings::update(cx, |settings| settings.cursor_shape = shape),
                )
            })))
            .child(section_header(
                "Shift-enter",
                "Send ESC CR so programs can tell shift-enter from enter — what Claude Code and Codex need for a newline instead of a send",
                theme,
            ))
            .child(chip(
                "chip-shift-enter",
                if settings.shift_enter_newline {
                    "On"
                } else {
                    "Off — shift-enter sends a plain return"
                },
                settings.shift_enter_newline,
                theme,
                |_, _, cx| {
                    Settings::update(cx, |settings| {
                        settings.shift_enter_newline = !settings.shift_enter_newline
                    })
                },
            ))
            .child(section_header(
                "Image paste",
                "Ctrl-V writes a clipboard image to a file and types its path, which is how Claude Code and Codex take images",
                theme,
            ))
            .child(chip(
                "chip-paste-images",
                if settings.paste_images {
                    "On"
                } else {
                    "Off — ctrl-v goes to the program"
                },
                settings.paste_images,
                theme,
                |_, _, cx| {
                    Settings::update(cx, |settings| {
                        settings.paste_images = !settings.paste_images
                    })
                },
            ))
            .child(section_header(
                "Copy on select",
                "Selecting text copies it to the clipboard, without cmd-c",
                theme,
            ))
            .child(chip(
                "chip-copy-on-select",
                if settings.copy_on_select {
                    "On"
                } else {
                    "Off — copy with cmd-c"
                },
                settings.copy_on_select,
                theme,
                |_, _, cx| {
                    Settings::update(cx, |settings| {
                        settings.copy_on_select = !settings.copy_on_select
                    })
                },
            ))
            .child(section_header(
                "Scrollback",
                "Lines kept per pane. Applies to panes opened from now on.",
                theme,
            ))
            .child(stepper(
                "scrollback",
                "Lines",
                settings.scrollback_lines.to_string(),
                theme,
                |cx| {
                    Settings::update(cx, |settings| {
                        settings.scrollback_lines =
                            settings.scrollback_lines.saturating_sub(1000).max(1000)
                    })
                },
                |cx| {
                    Settings::update(cx, |settings| {
                        settings.scrollback_lines = (settings.scrollback_lines + 1000).min(200_000)
                    })
                },
            ))
            .child(section_header(
                "Settings file",
                "Edits here are written straight to disk",
                theme,
            ))
            .child(
                div().text_color(theme.text_muted).child(SharedString::from(
                    Settings::path()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "unavailable".to_string()),
                )),
            )
    }
}

impl EventEmitter<SettingsViewEvent> for SettingsView {}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        let section = self.section;

        let body = match section {
            Section::Appearance => self.render_appearance(&theme, cx).into_any_element(),
            Section::Typography => self.render_typography(&theme, cx).into_any_element(),
            Section::Terminal => self.render_terminal(&theme, cx).into_any_element(),
        };

        div()
            .key_context("Settings")
            .track_focus(&self.focus_handle)
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(Hsla {
                a: 0.55,
                ..gpui::black()
            })
            .child(
                div()
                    .w(px(760.0))
                    .max_h(px(620.0))
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .rounded(Theme::radius())
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.opaque_background)
                    .text_color(theme.text)
                    .text_size(theme.ui_font_size)
                    .font_family(theme.ui_font_family.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_4()
                            .py_3()
                            .bg(theme.surface)
                            .border_b_1()
                            .border_color(theme.border)
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .text_color(theme.text)
                                    .child("Settings"),
                            )
                            .child(
                                div()
                                    .id("settings-done")
                                    .px_3()
                                    .py_1()
                                    .rounded(Theme::radius())
                                    .border_1()
                                    .border_color(theme.border)
                                    .text_color(theme.text_muted)
                                    .hover(|style| {
                                        style
                                            .border_color(theme.border_focused)
                                            .text_color(theme.text)
                                    })
                                    .cursor_pointer()
                                    .child("Done")
                                    .on_click(
                                        cx.listener(|_, _, _, cx| {
                                            cx.emit(SettingsViewEvent::Close)
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .px_4()
                            .py_2()
                            .border_b_1()
                            .border_color(theme.border)
                            .children(Section::ALL.map(|candidate| {
                                chip(
                                    SharedString::from(format!("section-{}", candidate.label())),
                                    candidate.label(),
                                    candidate == section,
                                    &theme,
                                    cx.listener(move |this, _, _, cx| {
                                        this.section = candidate;
                                        cx.notify();
                                    }),
                                )
                            })),
                    )
                    .child(
                        div()
                            .id("settings-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_4()
                            .child(body),
                    ),
            )
    }
}

fn section_header(
    title: &'static str,
    description: &'static str,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_color(theme.text).child(title))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(theme.text_muted)
                .child(description),
        )
}

fn chip(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    selected: bool,
    theme: &Theme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id.into())
        .flex()
        .items_center()
        .h(px(28.0))
        .px_3()
        .rounded(Theme::radius())
        .border_1()
        .border_color(if selected {
            theme.border_focused
        } else {
            theme.border
        })
        .bg(if selected {
            theme.elevated
        } else {
            theme.surface
        })
        .text_color(if selected {
            theme.text
        } else {
            theme.text_muted
        })
        .hover(|style| style.border_color(theme.border_focused))
        .cursor_pointer()
        .child(label.into())
        .on_click(on_click)
}

/// A label with a value between minus and plus buttons.
fn stepper(
    id: &'static str,
    label: &'static str,
    value: String,
    theme: &Theme,
    decrease: impl Fn(&mut App) + 'static,
    increase: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    let button = |suffix: &'static str,
                  glyph: &'static str,
                  action: Box<dyn Fn(&mut App)>,
                  theme: &Theme| {
        div()
            .id(SharedString::from(format!("{id}-{suffix}")))
            .flex()
            .items_center()
            .justify_center()
            .w(px(26.0))
            .h(px(26.0))
            .rounded(Theme::radius())
            .border_1()
            .border_color(theme.border)
            .text_color(theme.text_muted)
            .hover(|style| {
                style
                    .border_color(theme.border_focused)
                    .text_color(theme.text)
            })
            .cursor_pointer()
            .child(glyph)
            .on_click(move |_, _, cx| action(cx))
    };

    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().text_color(theme.text_muted).child(label))
        .child(button("dec", "−", Box::new(decrease), theme))
        .child(
            div()
                .min_w(px(56.0))
                .flex()
                .justify_center()
                .text_color(theme.text)
                .child(SharedString::from(value)),
        )
        .child(button("inc", "+", Box::new(increase), theme))
}

/// A virtualized, clickable list of font families.
fn font_list(
    id: &'static str,
    families: Vec<SharedString>,
    selected: SharedString,
    theme: &Theme,
    on_pick: impl Fn(String, &mut App) + Clone + 'static,
) -> impl IntoElement {
    let count = families.len();
    let text = theme.text;
    let text_muted = theme.text_muted;
    let elevated = theme.elevated;

    div()
        .h(px(FONT_LIST_HEIGHT))
        .rounded(Theme::radius())
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            uniform_list(id, count, move |range, _window, _cx| {
                let mut rows = Vec::new();
                for index in range {
                    let Some(family) = families.get(index).cloned() else {
                        continue;
                    };
                    let is_selected = family == selected;
                    let on_pick = on_pick.clone();
                    let picked = family.to_string();

                    rows.push(
                        div()
                            .id(index)
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .when(is_selected, |element| element.bg(elevated))
                            .text_color(if is_selected { text } else { text_muted })
                            .hover(|style| style.bg(elevated))
                            .child(family.clone())
                            .when(is_selected, |element| {
                                element.child(div().text_color(text).child("✓"))
                            })
                            .on_click(move |_, _, cx| on_pick(picked.clone(), cx)),
                    );
                }
                rows
            })
            .h_full(),
        )
}

/// Background presets, tuned to the current theme's appearance.
fn background_choices(theme: &Theme) -> Vec<(&'static str, String)> {
    let presets: &[(&'static str, u32)] = match theme.appearance {
        Appearance::Dark => &[
            ("Black", 0x000000),
            ("Ink", 0x0b0d12),
            ("Slate", 0x151a22),
            ("Charcoal", 0x1e1e1e),
            ("Warm", 0x1c1917),
        ],
        Appearance::Light => &[
            ("White", 0xffffff),
            ("Paper", 0xfbfaf7),
            ("Mist", 0xf4f6fa),
            ("Sand", 0xf5efe4),
            ("Cool", 0xeef1f5),
        ],
    };

    presets
        .iter()
        .map(|(label, value)| (*label, settings::to_hex(hex(*value))))
        .collect()
}

fn hex(value: u32) -> Hsla {
    let rgba: gpui::Rgba = gpui::rgb(value);
    rgba.into()
}
