//! The macOS menu bar.
//!
//! Every item dispatches an action that already has a key binding, so the
//! menus show the shortcut beside each item and the two can't drift apart.
//! The dock icon gets a short menu of its own for opening windows.

use gpui::{App, Menu, MenuItem, OsAction, SystemMenuType};

use crate::actions::*;

pub fn init(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "botttle".into(),
            items: vec![
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide botttle", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit botttle", Quit),
            ],
        },
        // "File", not "Shell": it is where every mac app keeps New Window, so
        // it is where people look for it.
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("New Window", NewWindow),
                MenuItem::action("New Tab", NewTab),
                MenuItem::action("Rename Tab…", RenameTab),
                MenuItem::separator(),
                MenuItem::action("Split Right", SplitRight),
                MenuItem::action("Split Down", SplitDown),
                MenuItem::separator(),
                MenuItem::action("Close Pane", ClosePane),
                MenuItem::action("Close Tab", CloseTab),
            ],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::os_action("Copy", CopySelection, OsAction::Copy),
                MenuItem::os_action("Paste", PasteClipboard, OsAction::Paste),
                MenuItem::separator(),
                MenuItem::action("Clear Screen", ClearScreen),
            ],
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Tabs in Sidebar", ToggleTabLayout),
                MenuItem::action("Toggle Sidebar", ToggleSidebar),
                MenuItem::separator(),
                MenuItem::action("Bigger", IncreaseFontSize),
                MenuItem::action("Smaller", DecreaseFontSize),
                MenuItem::action("Actual Size", ResetFontSize),
            ],
        },
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action("Minimize", Minimize),
                MenuItem::action("Zoom", Zoom),
                MenuItem::separator(),
                MenuItem::action("Next Tab", NextTab),
                MenuItem::action("Previous Tab", PreviousTab),
                MenuItem::action("Next Pane", FocusNextPane),
                MenuItem::action("Previous Pane", FocusPreviousPane),
            ],
        },
    ]);

    // Right-clicking the dock icon. The dock adds the open windows itself.
    cx.set_dock_menu(vec![MenuItem::action("New Window", NewWindow)]);
}
