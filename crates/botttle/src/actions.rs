//! Application actions and their default key bindings.
//!
//! Bindings are matched before a terminal sees a key, so anything listed here is
//! reserved: the shell will never receive it. That is why the chords use the
//! platform modifier on macOS and `ctrl-shift` elsewhere, where a bare `ctrl`
//! belongs to the program running in the terminal.

use gpui::{actions, App, KeyBinding};

actions!(
    botttle,
    [
        NewWindow,
        NewTab,
        CloseTab,
        NextTab,
        PreviousTab,
        SplitRight,
        SplitDown,
        ClosePane,
        FocusNextPane,
        FocusPreviousPane,
        CopySelection,
        PasteClipboard,
        ClearScreen,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
        OpenSettings,
        CloseSettings,
        ToggleTabLayout,
        Minimize,
        Zoom,
        Hide,
        HideOthers,
        ShowAll,
        Quit,
    ]
);

/// The key context the workspace publishes; bindings below are scoped to it.
pub const WORKSPACE_CONTEXT: &str = "Workspace";

#[cfg(target_os = "macos")]
const MOD: &str = "cmd";
#[cfg(not(target_os = "macos"))]
const MOD: &str = "ctrl-shift";

pub fn init(cx: &mut App) {
    let context = Some(WORKSPACE_CONTEXT);

    let mut bindings = vec![
        KeyBinding::new(&format!("{MOD}-n"), NewWindow, None),
        KeyBinding::new(&format!("{MOD}-t"), NewTab, context),
        KeyBinding::new(&format!("{MOD}-w"), ClosePane, context),
        KeyBinding::new(&format!("{MOD}-shift-w"), CloseTab, context),
        KeyBinding::new(&format!("{MOD}-d"), SplitRight, context),
        KeyBinding::new(&format!("{MOD}-shift-d"), SplitDown, context),
        KeyBinding::new(&format!("{MOD}-]"), FocusNextPane, context),
        KeyBinding::new(&format!("{MOD}-["), FocusPreviousPane, context),
        KeyBinding::new(&format!("{MOD}-shift-]"), NextTab, context),
        KeyBinding::new(&format!("{MOD}-shift-["), PreviousTab, context),
        KeyBinding::new(&format!("{MOD}-c"), CopySelection, context),
        KeyBinding::new(&format!("{MOD}-v"), PasteClipboard, context),
        KeyBinding::new(&format!("{MOD}-k"), ClearScreen, context),
        KeyBinding::new(&format!("{MOD}-="), IncreaseFontSize, context),
        KeyBinding::new(&format!("{MOD}--"), DecreaseFontSize, context),
        KeyBinding::new(&format!("{MOD}-0"), ResetFontSize, context),
        KeyBinding::new(&format!("{MOD}-,"), OpenSettings, context),
        KeyBinding::new("escape", CloseSettings, Some("Settings")),
        KeyBinding::new(&format!("{MOD}-q"), Quit, None),
    ];

    // The mac's own window and app chords, which the menu bar lists.
    #[cfg(target_os = "macos")]
    bindings.extend([
        KeyBinding::new("cmd-m", Minimize, context),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
    ]);

    // Ctrl-Tab style tab cycling, for muscle memory carried over from editors.
    bindings.push(KeyBinding::new("ctrl-tab", NextTab, context));
    bindings.push(KeyBinding::new("ctrl-shift-tab", PreviousTab, context));

    cx.bind_keys(bindings);
}
