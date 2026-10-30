//! botttle — an agentic development environment.
//!
//! Today it is a GPU-rendered terminal workspace: tabs, splittable panes, and a
//! real PTY behind each pane. The pane tree and the event plumbing are built so
//! that agent-driven panes can sit beside shell panes later without reshaping
//! the app around them.

mod actions;
mod assets;
mod glass;
mod menus;
mod pane;
mod settings;
mod settings_view;
mod sound;
mod terminal;
mod theme;
mod workspace;

use anyhow::Result;
use gpui::{
    point, px, size, App, AppContext, Application, Bounds, Pixels, TitlebarOptions, WindowBounds,
    WindowHandle, WindowOptions,
};

/// How far each new window steps down and right from the one before it, so it
/// lands visibly beside the current window instead of exactly on top of it.
const WINDOW_CASCADE: f32 = 28.0;

use crate::settings::{Settings, TabLayout};
use crate::workspace::Workspace;

fn main() {
    // Sets TERM and COLORTERM for every shell we spawn, and makes sure the
    // shell is the one the user chose, before any thread can read the env.
    alacritty_terminal::tty::setup_env();
    terminal::shell::use_login_shell();

    let app = Application::new().with_assets(assets::Assets);

    // Closing the last window leaves the app running, the way a mac app does.
    // Without this, clicking the dock icon then has nothing to reopen and
    // appears to do nothing at all.
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_window(cx).expect("failed to reopen a botttle window");
        }
    });

    app.run(|cx: &mut App| {
        settings::init(cx);
        theme::init(cx);
        assets::apply_app_icon(
            cx.global::<Settings>().icon_style,
            cx.global::<theme::Theme>().appearance,
        );
        actions::init(cx);

        menus::init(cx);

        cx.on_action(|_: &actions::Quit, cx| cx.quit());
        cx.on_action(|_: &actions::Hide, cx| cx.hide());
        cx.on_action(|_: &actions::HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &actions::ShowAll, cx| cx.unhide_other_apps());
        cx.on_action(|_: &actions::ToggleSidebar, cx| {
            Settings::update(cx, |settings| {
                // From the top strip, asking for the sidebar means switching to it.
                if settings.tab_layout == TabLayout::Horizontal {
                    settings.tab_layout = TabLayout::Vertical;
                    settings.show_sidebar = true;
                } else {
                    settings.show_sidebar = !settings.show_sidebar;
                }
            })
        });
        cx.on_action(|_: &actions::ToggleTabLayout, cx| {
            Settings::update(cx, |settings| {
                settings.tab_layout = match settings.tab_layout {
                    TabLayout::Horizontal => TabLayout::Vertical,
                    TabLayout::Vertical => TabLayout::Horizontal,
                }
            })
        });
        cx.on_action(|_: &actions::NewWindow, cx| {
            open_window(cx).expect("failed to open a botttle window");
        });

        open_window(cx).expect("failed to open the botttle window");
        cx.activate(true);
    });
}

/// Opens a window onto a new workspace, and focuses its first pane.
pub fn open_window(cx: &mut App) -> Result<WindowHandle<Workspace>> {
    let bounds = cascaded_bounds(cx)
        .unwrap_or_else(|| Bounds::centered(None, size(px(1120.0), px(720.0)), cx));
    let background = cx.global::<Settings>().window_background();

    let window = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("botttle".into()),
                appears_transparent: true,
                // Centred in the 32px titlebar drawn by the workspace.
                traffic_light_position: Some(point(px(14.0), px(10.0))),
            }),
            window_min_size: Some(size(px(600.0), px(400.0))),
            window_background: background,
            app_id: Some("dev.botttle".to_string()),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| Workspace::new(window, cx)),
    )?;

    let settings = cx.global::<Settings>().clone();
    window
        .update(cx, |workspace, window, cx| {
            glass::apply(window, &settings);
            workspace.focus_active_pane(window, cx);
        })
        .ok();

    Ok(window)
}

/// The current window's size, stepped down and right of it. Nothing when there
/// is no window to step from, or when the step would run off the screen it is
/// on, in which case the new window starts centred again.
fn cascaded_bounds(cx: &mut App) -> Option<Bounds<Pixels>> {
    let (current, screen) = cx
        .active_window()
        .or_else(|| cx.windows().last().copied())?
        .update(cx, |_, window, cx| {
            let screen = window.display(cx).map(|display| display.bounds());
            (window.bounds(), screen)
        })
        .ok()?;

    let step = px(WINDOW_CASCADE);
    let bounds = Bounds::new(
        point(current.origin.x + step, current.origin.y + step),
        current.size,
    );
    match screen {
        Some(screen)
            if bounds.bottom_right().x > screen.bottom_right().x
                || bounds.bottom_right().y > screen.bottom_right().y =>
        {
            None
        }
        _ => Some(bounds),
    }
}
