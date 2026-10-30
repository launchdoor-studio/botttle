//! The root view: the titlebar, the tabs (a strip across the top or a sidebar
//! down the left), the pane tree of the active tab, and the settings panel when
//! it is open.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    div, img, point, prelude::*, px, svg, App, Axis, ClickEvent, Context, CursorStyle,
    DragMoveEvent, Entity, EntityId, FocusHandle, Focusable, KeyDownEvent, MouseButton,
    MouseDownEvent, Render, ScrollDelta, ScrollHandle, ScrollWheelEvent, SharedString,
    Subscription, Task, Window,
};

use crate::actions::*;
use crate::assets;
use crate::pane::{PaneGroup, RemoveResult, Resize, ResizeHandler};
use crate::settings::{clamp_sidebar_width, Settings, TabLayout, DEFAULT_SIDEBAR_WIDTH};
use crate::settings_view::{SettingsView, SettingsViewEvent};
use crate::sound;
use crate::terminal::view::{TerminalView, TerminalViewEvent};
use crate::theme::Theme;

/// Room for the macOS traffic lights, which sit over our own titlebar.
#[cfg(target_os = "macos")]
const TITLEBAR_LEADING_PADDING: f32 = 76.0;
#[cfg(not(target_os = "macos"))]
const TITLEBAR_LEADING_PADDING: f32 = 10.0;

const TITLEBAR_HEIGHT: f32 = 32.0;
/// The mark in the titlebar, sized to sit inside it with room to breathe.
const LOGO_SIZE: f32 = 18.0;
const TAB_BAR_HEIGHT: f32 = 36.0;
/// Tabs keep their width instead of shrinking, so the strip scrolls once it fills.
const TAB_MIN_WIDTH: f32 = 110.0;
const TAB_MAX_WIDTH: f32 = 240.0;
/// The strip along the sidebar's edge that drags to resize it.
const SIDEBAR_HANDLE_WIDTH: f32 = 5.0;
const ICON_BUTTON_SIZE: f32 = 24.0;
const ICON_SIZE: f32 = 14.0;
const CLOSE_BUTTON_SIZE: f32 = 18.0;
const CLOSE_ICON_SIZE: f32 = 11.0;
/// Where the first button after the traffic lights starts, so the gap from
/// the green light to the first icon matches the gap between icons.
#[cfg(target_os = "macos")]
const AFTER_TRAFFIC_LIGHTS: f32 = 81.0;
#[cfg(not(target_os = "macos"))]
const AFTER_TRAFFIC_LIGHTS: f32 = 10.0;
/// The traffic lights' centre sits below the titlebar's, and the buttons
/// beside them follow it rather than the bar. Measured, not derived.
const TRAFFIC_LIGHT_NUDGE: f32 = 1.75;
const SIDEBAR_ROW_HEIGHT: f32 = 28.0;
/// A sidebar row with a status line under its title.
const SIDEBAR_STATUS_ROW_HEIGHT: f32 = 40.0;
/// How often panes are asked whether they are busy or done.
const ACTIVITY_POLL: Duration = Duration::from_millis(500);
/// The in-place editor for a tab's name. Only the end of the text is
/// editable, which is all a name this short needs.
struct Rename {
    index: usize,
    text: String,
    focus_handle: FocusHandle,
    /// Clicking away keeps what was typed, the way renaming a file does.
    _blur: Subscription,
}

/// Wheel notches to pixels when translating vertical scroll into tab movement.
const TAB_SCROLL_STEP: f32 = 40.0;

pub struct Workspace {
    tabs: Vec<Tab>,
    active_tab: usize,
    focus_handle: FocusHandle,
    settings: Option<Entity<SettingsView>>,
    /// Lets the active tab be scrolled into view when it is selected by keyboard.
    tab_scroll: ScrollHandle,
    /// Kept alive for as long as the settings panel is open.
    settings_subscription: Option<Subscription>,
    /// Shown in the status bar; set when something failed in a way worth saying
    /// out loud, such as a shell that would not start.
    status: Option<SharedString>,
    /// The sidebar row under the pointer, which shows its close button.
    hovered_tab: Option<usize>,
    /// The tab whose name is being edited in place, if any.
    renaming: Option<Rename>,
    /// Asks every pane, on a steady beat, whether it is busy or done.
    _activity: Task<()>,
    /// Followed live while the edge is dragged, and saved once it is dropped,
    /// so a drag doesn't write the settings file on every mouse move.
    sidebar_width: f32,
}

/// What the sidebar's edge carries while it is dragged.
struct SidebarEdge;

/// The edge itself is the feedback, so nothing follows the pointer.
struct SidebarEdgePreview;

impl Render for SidebarEdgePreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

struct Tab {
    /// A name the user gave the tab. While set, it is shown instead of the
    /// focused pane's title, which programs change as they please.
    name: Option<SharedString>,
    group: PaneGroup,
    active_pane: EntityId,
    /// Per pane: title/exit notifications, and focus tracking so clicking a pane
    /// makes it the target of the next split or close.
    subscriptions: HashMap<EntityId, Vec<Subscription>>,
    /// Some pane in the tab is producing output on its own.
    working: bool,
    /// Something in the tab finished while nobody was looking at it, and how
    /// long ago that reads as. Cleared once the tab is looked at.
    done: Option<(Instant, SharedString)>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut workspace = Self {
            tabs: Vec::new(),
            active_tab: 0,
            focus_handle: cx.focus_handle(),
            settings: None,
            tab_scroll: ScrollHandle::new(),
            settings_subscription: None,
            status: None,
            hovered_tab: None,
            renaming: None,
            sidebar_width: cx.global::<Settings>().sidebar_width(),
            _activity: cx.spawn_in(window, async move |this, cx| loop {
                cx.background_executor().timer(ACTIVITY_POLL).await;
                if this
                    .update_in(cx, |this, window, cx| this.poll_activity(window, cx))
                    .is_err()
                {
                    break;
                }
            }),
        };
        workspace.open_tab(window, cx);
        workspace
    }

    fn open_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.spawn_pane(cx) else {
            return;
        };

        let subscriptions = self.watch_pane(&pane, window, cx);
        let mut map = HashMap::new();
        map.insert(pane.entity_id(), subscriptions);

        self.finish_rename(true, cx);
        self.tabs.push(Tab {
            name: None,
            group: PaneGroup::new(pane.clone()),
            active_pane: pane.entity_id(),
            subscriptions: map,
            working: false,
            done: None,
        });
        self.active_tab = self.tabs.len() - 1;
        self.tab_scroll.scroll_to_item(self.active_tab);
        self.focus_pane(&pane, window, cx);
        cx.notify();
    }

    fn spawn_pane(&mut self, cx: &mut Context<Self>) -> Option<Entity<TerminalView>> {
        // A new pane opens where the current one is, the way a new tab in any
        // terminal does. Falling back to the launch directory covers the first
        // pane, and a GUI launch (where that is `/`) falls back to home.
        let working_directory = self
            .active_pane()
            .and_then(|pane| pane.read(cx).working_directory());

        match TerminalView::spawn(working_directory, cx) {
            Ok(pane) => {
                self.status = None;
                Some(pane)
            }
            Err(error) => {
                self.status = Some(SharedString::from(format!("{error:#}")));
                cx.notify();
                None
            }
        }
    }

    fn watch_pane(
        &self,
        pane: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        let id = pane.entity_id();
        let focus_handle = pane.focus_handle(cx);

        vec![
            cx.subscribe_in(
                pane,
                window,
                move |this, _, event, window, cx| match event {
                    // Both change what the tab strip should say.
                    TerminalViewEvent::TitleChanged | TerminalViewEvent::Exited => cx.notify(),
                    TerminalViewEvent::Bell => this.mark_done(id, window, cx),
                },
            ),
            // Clicking into a pane focuses it directly, so the workspace learns
            // which pane is current by watching focus rather than clicks.
            cx.on_focus_in(&focus_handle, window, move |this, _, cx| {
                if let Some(tab) = this.tabs.get_mut(this.active_tab) {
                    if tab.active_pane != id {
                        tab.active_pane = id;
                        cx.notify();
                    }
                }
            }),
        ]
    }

    /// Notes which tabs are busy and which have finished something, so the tab
    /// list can say where attention is needed.
    fn poll_activity(&mut self, window: &Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let seen = window.is_window_active().then_some(self.active_tab);
        let mut changed = false;
        let mut newly_done = false;

        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let mut working = false;
            let mut finished = false;
            for pane in tab.group.panes() {
                let status = pane.update(cx, |pane, _| pane.poll_activity(now));
                working |= status.working;
                finished |= status.finished;
            }

            let done = if seen == Some(index) {
                None
            } else if finished {
                newly_done = true;
                Some(now)
            } else {
                tab.done.as_ref().map(|(at, _)| *at)
            };
            let done = done.map(|at| (at, age(now.duration_since(at))));

            changed |= working != tab.working || done != tab.done;
            tab.working = working;
            tab.done = done;
        }

        if newly_done {
            chime(cx);
        }
        if changed {
            cx.notify();
        }
    }

    /// A pane asked for attention. Its tab shows as done unless it is the one
    /// already being looked at.
    fn mark_done(&mut self, pane: EntityId, window: &Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| {
            tab.group
                .panes()
                .iter()
                .any(|candidate| candidate.entity_id() == pane)
        }) else {
            return;
        };
        if index == self.active_tab && window.is_window_active() {
            return;
        }
        self.tabs[index].done = Some((Instant::now(), age(Duration::ZERO)));
        chime(cx);
        cx.notify();
    }

    fn focus_pane(
        &mut self,
        pane: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.active_pane = pane.entity_id();
            // Looking at it is the acknowledgement.
            tab.done = None;
        }
        window.focus(&pane.focus_handle(cx));
        cx.notify();
    }

    fn active_pane(&self) -> Option<Entity<TerminalView>> {
        let tab = self.tabs.get(self.active_tab)?;
        tab.group
            .panes()
            .into_iter()
            .find(|pane| pane.entity_id() == tab.active_pane)
    }

    fn split(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.tabs.get(self.active_tab).map(|tab| tab.active_pane) else {
            return;
        };
        let Some(pane) = self.spawn_pane(cx) else {
            return;
        };

        let subscriptions = self.watch_pane(&pane, window, cx);
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        if !tab.group.split(target, axis, pane.clone()) {
            return;
        }
        tab.subscriptions.insert(pane.entity_id(), subscriptions);
        self.focus_pane(&pane, window, cx);
    }

    fn close_active_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        let target = tab.active_pane;

        match tab.group.remove(target) {
            RemoveResult::Removed => {
                tab.subscriptions.remove(&target);
                let next = tab.group.panes().first().cloned();
                if let Some(pane) = next {
                    self.focus_pane(&pane, window, cx);
                }
            }
            RemoveResult::GroupEmpty => self.close_tab(self.active_tab, window, cx),
            RemoveResult::NotFound => {}
        }
        cx.notify();
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        // Indices shift under an open editor, so it is closed first.
        self.finish_rename(true, cx);
        self.tabs.remove(index);

        // The last tab takes the window with it, but not the app: a mac app
        // stays running with no windows, and the dock icon reopens one.
        if self.tabs.is_empty() {
            window.remove_window();
            return;
        }

        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        self.tab_scroll.scroll_to_item(self.active_tab);
        self.focus_active_pane(window, cx);
        cx.notify();
    }

    fn activate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.active_tab = index;
        self.tab_scroll.scroll_to_item(index);
        self.focus_active_pane(window, cx);
        cx.notify();
    }

    /// Opens the name editor on a tab, starting from what the tab shows now.
    fn start_rename(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.finish_rename(true, cx);

        let text = self.tab_title(&self.tabs[index], cx).to_string();
        let focus_handle = cx.focus_handle();
        let blur = cx.on_blur(&focus_handle, window, |this, _, cx| {
            this.finish_rename(true, cx);
        });
        window.focus(&focus_handle);
        self.renaming = Some(Rename {
            index,
            text,
            focus_handle,
            _blur: blur,
        });
        cx.notify();
    }

    /// Closes the name editor, keeping the new name when `commit` is set. An
    /// empty name hands the tab back to its pane's title.
    fn finish_rename(&mut self, commit: bool, cx: &mut Context<Self>) {
        let Some(rename) = self.renaming.take() else {
            return;
        };
        if commit {
            if let Some(tab) = self.tabs.get_mut(rename.index) {
                let name = rename.text.trim();
                tab.name = (!name.is_empty()).then(|| SharedString::from(name.to_string()));
            }
        }
        cx.notify();
    }

    fn on_rename_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(rename) = self.renaming.as_mut() else {
            return;
        };
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;

        match keystroke.key.as_str() {
            "enter" | "escape" => {
                let commit = keystroke.key == "enter";
                self.finish_rename(commit, cx);
                self.focus_active_pane(window, cx);
            }
            "backspace" if modifiers.platform || modifiers.alt => {
                // Whole words, or everything, as in any other text field.
                let keep = if modifiers.platform {
                    0
                } else {
                    let trimmed = rename.text.trim_end();
                    trimmed
                        .rfind(char::is_whitespace)
                        .map_or(0, |space| space + 1)
                };
                rename.text.truncate(keep);
            }
            "backspace" => {
                rename.text.pop();
            }
            _ if modifiers.platform || modifiers.control || modifiers.function => return,
            _ => {
                let Some(typed) = keystroke
                    .key_char
                    .as_deref()
                    .filter(|text| !text.is_empty())
                else {
                    return;
                };
                rename.text.push_str(typed);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn paste_into_rename(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if let Some(rename) = self.renaming.as_mut() {
            // A name is one line.
            rename
                .text
                .push_str(text.lines().next().unwrap_or_default());
            cx.notify();
        }
    }

    /// The editor drawn in place of a tab's title.
    fn render_rename_field(
        &self,
        rename: &Rename,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("rename-tab")
            .key_context("TabRename")
            .track_focus(&rename.focus_handle)
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .overflow_hidden()
            .px_1()
            .rounded(px(2.0))
            .bg(theme.background)
            .border_1()
            .border_color(theme.accent)
            .text_color(theme.text)
            .on_key_down(cx.listener(Self::on_rename_key_down))
            .on_action(cx.listener(|this, _: &PasteClipboard, _, cx| this.paste_into_rename(cx)))
            // Clicks inside the field shouldn't reach the tab underneath.
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex_none()
                    .child(SharedString::from(rename.text.clone())),
            )
            .child(div().flex_none().w(px(1.0)).h(px(14.0)).bg(theme.accent))
    }

    /// A tab's title wherever it is drawn: the name it was given, or else its
    /// focused pane's title.
    fn tab_title(&self, tab: &Tab, cx: &Context<Self>) -> SharedString {
        if let Some(name) = &tab.name {
            return name.clone();
        }
        let panes = tab.group.panes();
        panes
            .iter()
            .find(|pane| pane.entity_id() == tab.active_pane)
            .or_else(|| panes.first())
            .map(|pane| pane.read(cx).title())
            .unwrap_or_else(|| SharedString::from("shell"))
    }

    /// Single clicks select a tab; a double click also starts renaming it.
    fn on_tab_click(
        &mut self,
        index: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.click_count() >= 2 {
            self.activate_tab(index, window, cx);
            self.start_rename(index, window, cx);
        } else {
            self.activate_tab(index, window, cx);
        }
    }

    pub fn focus_active_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.active_pane() {
            window.focus(&pane.focus_handle(cx));
        }
    }

    fn cycle_tab(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() < 2 {
            return;
        }
        let count = self.tabs.len() as isize;
        let next = (self.active_tab as isize + delta).rem_euclid(count) as usize;
        self.activate_tab(next, window, cx);
    }

    fn cycle_pane(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return;
        };
        let panes = tab.group.panes();
        if panes.len() < 2 {
            return;
        }

        let current = panes
            .iter()
            .position(|pane| pane.entity_id() == tab.active_pane)
            .unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(panes.len() as isize) as usize;
        let pane = panes[next].clone();
        self.focus_pane(&pane, window, cx);
    }

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            return;
        }

        let view = cx.new(SettingsView::new);
        self.settings_subscription = Some(cx.subscribe_in(
            &view,
            window,
            |this, _, _: &SettingsViewEvent, window, cx| this.close_settings(window, cx),
        ));
        window.focus(&view.focus_handle(cx));
        self.settings = Some(view);
        cx.notify();
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.take().is_some() {
            self.settings_subscription = None;
            self.focus_active_pane(window, cx);
            cx.notify();
        }
    }

    fn adjust_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        Settings::update(cx, |settings| {
            settings.terminal_font_size = (settings.terminal_font_size + delta).clamp(6.0, 40.0);
        });
    }

    fn reset_font_size(&mut self, cx: &mut Context<Self>) {
        Settings::update(cx, |settings| {
            settings.terminal_font_size = crate::settings::DEFAULT_FONT_SIZE;
        });
    }

    /// The window's own bar: traffic lights, the app name, and drag area. Kept
    /// separate from the tabs so the two rows can't be mistaken for each other.
    fn render_titlebar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .flex_none()
            .h(px(TITLEBAR_HEIGHT))
            .pl(px(TITLEBAR_LEADING_PADDING))
            .pr_3()
            .bg(theme.surface)
            .border_b_1()
            .border_color(theme.border)
            .text_size(px(11.0))
            .text_color(theme.text_muted)
            .gap_2()
            .child(
                img(assets::embedded(assets::logo_for(
                    cx.global::<Settings>().icon_style,
                    theme.appearance,
                )))
                .size(px(LOGO_SIZE))
                .flex_none(),
            )
            .child("botttle")
            .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
                if event.click_count == 2 {
                    window.zoom_window();
                } else {
                    window.start_window_move();
                }
            })
    }

    fn render_tab_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| self.render_tab(index, tab, theme, cx))
            .collect::<Vec<_>>();

        div()
            .flex()
            .items_center()
            .flex_none()
            .h(px(TAB_BAR_HEIGHT))
            .px_1()
            .gap_1()
            .bg(theme.background)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .id("tab-strip")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap_1()
                    .overflow_x_scroll()
                    .track_scroll(&self.tab_scroll)
                    // Trackpads and mice that only scroll vertically still need to
                    // reach tabs that ran off the edge.
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                        let vertical = match event.delta {
                            ScrollDelta::Lines(delta) => delta.y * TAB_SCROLL_STEP,
                            ScrollDelta::Pixels(delta) => f32::from(delta.y),
                        };
                        if vertical == 0.0 {
                            return;
                        }
                        let offset = this.tab_scroll.offset();
                        this.tab_scroll
                            .set_offset(point(offset.x + px(vertical), offset.y));
                        cx.notify();
                    }))
                    .children(tabs),
            )
            .child(new_tab_button(theme, cx))
    }

    /// What a tab shows wherever it is drawn: its focused pane's title, the
    /// colour of its status dot, and how many panes it holds.
    fn tab_summary(
        &self,
        index: usize,
        tab: &Tab,
        theme: &Theme,
        cx: &Context<Self>,
    ) -> (SharedString, gpui::Hsla, usize) {
        let panes = tab.group.panes();
        let focused_pane = panes
            .iter()
            .find(|pane| pane.entity_id() == tab.active_pane)
            .or_else(|| panes.first());
        let title = self.tab_title(tab, cx);
        let exited = focused_pane.is_some_and(|pane| pane.read(cx).has_exited());
        let show_status = cx.global::<Settings>().tab_status;
        let dot = if exited {
            theme.danger
        } else if show_status && tab.done.is_some() {
            theme.ansi[2]
        } else if show_status && tab.working {
            theme.ansi[3]
        } else if index == self.active_tab {
            theme.accent
        } else {
            theme.text_muted
        };
        (title, dot, panes.len())
    }

    fn render_tab(
        &self,
        index: usize,
        tab: &Tab,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_active = index == self.active_tab;
        let (title, dot, pane_count) = self.tab_summary(index, tab, theme, cx);

        div()
            .id(("tab", index))
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(26.0))
            .px_2()
            .min_w(px(TAB_MIN_WIDTH))
            .max_w(px(TAB_MAX_WIDTH))
            .rounded(Theme::radius())
            .cursor_pointer()
            .child(div().size(px(6.0)).flex_none().rounded(px(3.0)).bg(dot))
            .when(pane_count > 1, |element| {
                element.child(
                    div()
                        .flex_none()
                        .text_color(theme.text_muted)
                        .child(format!("{pane_count}")),
                )
            })
            .map(|element| match self.renaming.as_ref() {
                Some(rename) if rename.index == index => {
                    element.child(self.render_rename_field(rename, theme, cx))
                }
                _ => element.child(div().flex_1().min_w_0().truncate().child(title)),
            })
            .when(is_active, |element| {
                element.bg(theme.elevated).text_color(theme.text)
            })
            .when(!is_active, |element| {
                element
                    .text_color(theme.text_muted)
                    .hover(|style| style.bg(theme.elevated))
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.on_tab_click(index, event, window, cx)
            }))
            .child(
                div()
                    .id(("close-tab", index))
                    .flex_none()
                    .px_1()
                    .rounded(px(2.0))
                    .text_color(theme.text_muted)
                    .hover(|style| style.text_color(theme.danger))
                    .child("×")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.close_tab(index, window, cx);
                    })),
            )
    }

    /// Tabs as a list down the left, with the traffic lights above it the way
    /// a mac sidebar has them. The sidebar's top is the window's only drag
    /// area in this layout; the panes run to the top edge.
    fn render_sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| self.render_sidebar_row(index, tab, theme, cx))
            .collect::<Vec<_>>();

        div()
            .flex()
            .flex_col()
            .flex_none()
            .relative()
            .w(px(self.sidebar_width))
            .h_full()
            // The window ground stops at the content column, so the sidebar
            // paints the same ground itself, transparency and all.
            .bg(theme.background)
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .flex_none()
                    .h(px(TITLEBAR_HEIGHT))
                    .pl(px(TITLEBAR_LEADING_PADDING))
                    .pr_2()
                    .pt(px(TRAFFIC_LIGHT_NUDGE))
                    .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
                        if event.click_count == 2 {
                            window.zoom_window();
                        } else {
                            window.start_window_move();
                        }
                    })
                    .child(sidebar_button(theme))
                    .child(new_tab_button(theme, cx)),
            )
            .child(
                div()
                    .id("tab-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .py_1()
                    .gap(px(2.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.tab_scroll)
                    .children(rows),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_none()
                    .h(px(TITLEBAR_HEIGHT))
                    .px_2()
                    .child(
                        icon_button("sidebar-settings", assets::SETTINGS_ICON, theme).on_click(
                            cx.listener(|this, _, window, cx| this.open_settings(window, cx)),
                        ),
                    ),
            )
            .child(
                // Drag the edge to resize; double-click it to put it back.
                div()
                    .id("sidebar-edge")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(px(SIDEBAR_HANDLE_WIDTH))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .hover(|style| style.bg(theme.border))
                    .on_drag(SidebarEdge, |_, _, _, cx| cx.new(|_| SidebarEdgePreview))
                    .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                        if event.click_count() == 2 {
                            this.sidebar_width = DEFAULT_SIDEBAR_WIDTH;
                            Settings::update(cx, |settings| {
                                settings.sidebar_width = DEFAULT_SIDEBAR_WIDTH
                            });
                        }
                    })),
            )
    }

    /// With the sidebar closed, the traffic lights still need somewhere to sit
    /// and the window something to be dragged by, so a slim bar stays behind
    /// holding just the button that brings the sidebar back.
    fn render_collapsed_bar(&self, theme: &Theme) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .flex_none()
            .h(px(TITLEBAR_HEIGHT))
            .pl(px(AFTER_TRAFFIC_LIGHTS))
            .pt(px(TRAFFIC_LIGHT_NUDGE))
            .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
                if event.click_count == 2 {
                    window.zoom_window();
                } else {
                    window.start_window_move();
                }
            })
            .child(sidebar_button(theme))
    }

    fn render_sidebar_row(
        &self,
        index: usize,
        tab: &Tab,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_active = index == self.active_tab;
        let (title, dot, pane_count) = self.tab_summary(index, tab, theme, cx);
        // Only the active row and the one under the pointer offer to close, so
        // a long list of tabs doesn't read as a column of ×s.
        let closable = is_active || self.hovered_tab == Some(index);
        let status = match &tab.done {
            _ if !cx.global::<Settings>().tab_status => None,
            Some((_, age)) => Some(("Done", Some(age.clone()))),
            None => tab.working.then_some(("Working…", None)),
        };
        let height = if status.is_some() {
            SIDEBAR_STATUS_ROW_HEIGHT
        } else {
            SIDEBAR_ROW_HEIGHT
        };

        div()
            .id(("sidebar-tab", index))
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(height))
            .px_2()
            .rounded(Theme::radius())
            .cursor_pointer()
            .child(div().size(px(6.0)).flex_none().rounded(px(3.0)).bg(dot))
            .map(|element| match self.renaming.as_ref() {
                Some(rename) if rename.index == index => {
                    element.child(self.render_rename_field(rename, theme, cx))
                }
                _ => element.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().child(title))
                        .when_some(status, |column, (label, age)| {
                            column.child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .gap_2()
                                    .text_size(px(11.0))
                                    .text_color(theme.text_muted)
                                    .child(label)
                                    .children(age),
                            )
                        }),
                ),
            })
            .when(pane_count > 1, |element| {
                element.child(
                    div()
                        .flex_none()
                        .text_size(px(10.0))
                        .text_color(theme.text_muted)
                        .child(format!("{pane_count}")),
                )
            })
            .when(is_active, |element| {
                element.bg(theme.elevated).text_color(theme.text)
            })
            .when(!is_active, |element| {
                element
                    .text_color(theme.text_muted)
                    .hover(|style| style.bg(theme.elevated))
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.on_tab_click(index, event, window, cx)
            }))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                let before = this.hovered_tab;
                if *hovered {
                    this.hovered_tab = Some(index);
                } else if this.hovered_tab == Some(index) {
                    this.hovered_tab = None;
                }
                if this.hovered_tab != before {
                    cx.notify();
                }
            }))
            .child(
                // The slot is always there, so the title doesn't shift when the
                // button comes and goes.
                div()
                    .id(("close-sidebar-tab", index))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(CLOSE_BUTTON_SIZE))
                    .rounded(Theme::radius())
                    .when(closable, |button| {
                        button
                            .cursor_pointer()
                            .hover(|style| style.bg(theme.border))
                            .child(
                                svg()
                                    .path(assets::CLOSE_ICON)
                                    .size(px(CLOSE_ICON_SIZE))
                                    .text_color(icon_color(theme)),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_tab(index, window, cx);
                            }))
                    }),
            )
    }

    /// Only there when it has something to say. On a mac the menu bar and the
    /// sidebar already cover what it used to show, so it appears just to
    /// report a failure; elsewhere it also carries the shortcut hint.
    fn render_status_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.status.is_none() && cfg!(target_os = "macos") {
            return None;
        }
        let panes = self
            .tabs
            .get(self.active_tab)
            .map(|tab| tab.group.panes().len())
            .unwrap_or(0);

        let left = match &self.status {
            Some(status) => div().text_color(theme.danger).child(status.clone()),
            None => div().text_color(theme.text_muted).child(format!(
                "{} tab{} · {} pane{} · {}",
                self.tabs.len(),
                if self.tabs.len() == 1 { "" } else { "s" },
                panes,
                if panes == 1 { "" } else { "s" },
                theme.name
            )),
        };

        Some(
            div()
                .flex()
                .items_center()
                .justify_between()
                .h(px(24.0))
                .px_3()
                .bg(theme.surface)
                .border_t_1()
                .border_color(theme.border)
                .text_size(px(11.0))
                .child(left)
                // On a mac the menu bar lists every command with its shortcut, so
                // the status bar doesn't repeat them.
                .when(!cfg!(target_os = "macos"), |element| {
                    element.child(
                        div()
                            .id("open-settings")
                            .px_2()
                            .rounded(px(2.0))
                            .text_color(theme.text_muted)
                            .hover(|style| style.text_color(theme.text))
                            .cursor_pointer()
                            .child(SharedString::from(SHORTCUT_HINT))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_settings(window, cx)),
                            ),
                    )
                }),
        )
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        let layout = cx.global::<Settings>().tab_layout;
        let show_sidebar = cx.global::<Settings>().show_sidebar;
        let active = self.tabs.get(self.active_tab);
        let on_resize: ResizeHandler = Rc::new(cx.listener(|this, resize: &Resize, _, cx| {
            if let Some(tab) = this.tabs.get_mut(this.active_tab) {
                tab.group.resize(resize);
                cx.notify();
            }
        }));
        let content = active.map(|tab| tab.group.render(Some(tab.active_pane), &theme, &on_resize));
        let content = div().flex().flex_1().min_h_0().p_2().children(content);

        div()
            .key_context(WORKSPACE_CONTEXT)
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .text_color(theme.text)
            .text_size(theme.ui_font_size)
            .font_family(theme.ui_font_family.clone())
            .on_action(cx.listener(|this, _: &NewTab, window, cx| this.open_tab(window, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                this.close_tab(this.active_tab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &RenameTab, window, cx| {
                this.start_rename(this.active_tab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| this.cycle_tab(1, window, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousTab, window, cx| this.cycle_tab(-1, window, cx)),
            )
            .on_action(cx.listener(|this, _: &SplitRight, window, cx| {
                this.split(Axis::Horizontal, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SplitDown, window, cx| {
                    this.split(Axis::Vertical, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &ClosePane, window, cx| this.close_active_pane(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FocusNextPane, window, cx| this.cycle_pane(1, window, cx)),
            )
            .on_action(cx.listener(|this, _: &FocusPreviousPane, window, cx| {
                this.cycle_pane(-1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CopySelection, _, cx| {
                if let Some(pane) = this.active_pane() {
                    pane.update(cx, |pane, cx| pane.copy(cx));
                }
            }))
            .on_action(cx.listener(|this, _: &PasteClipboard, _, cx| {
                if let Some(pane) = this.active_pane() {
                    pane.update(cx, |pane, cx| pane.paste(cx));
                }
            }))
            .on_action(cx.listener(|this, _: &ClearScreen, _, cx| {
                if let Some(pane) = this.active_pane() {
                    pane.update(cx, |pane, cx| pane.clear(cx));
                }
            }))
            .on_action(
                cx.listener(|this, _: &IncreaseFontSize, _, cx| this.adjust_font_size(1.0, cx)),
            )
            .on_action(
                cx.listener(|this, _: &DecreaseFontSize, _, cx| this.adjust_font_size(-1.0, cx)),
            )
            .on_action(cx.listener(|this, _: &ResetFontSize, _, cx| this.reset_font_size(cx)))
            .on_action(
                cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CloseSettings, window, cx| this.close_settings(window, cx)),
            )
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &Zoom, window, _| window.zoom_window())
            .map(|root| match layout {
                TabLayout::Horizontal => root
                    .bg(theme.background)
                    .child(self.render_titlebar(&theme, cx))
                    .child(self.render_tab_bar(&theme, cx))
                    .child(content)
                    .children(self.render_status_bar(&theme, cx)),
                TabLayout::Vertical => root.child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .when(show_sidebar, |row| {
                            row.child(self.render_sidebar(&theme, cx))
                        })
                        // The whole row hears the drag, so the edge keeps
                        // following the pointer when it runs ahead of it.
                        .on_drag_move(cx.listener(
                            |this, event: &DragMoveEvent<SidebarEdge>, _, cx| {
                                let width = event.event.position.x - event.bounds.origin.x;
                                this.sidebar_width = clamp_sidebar_width(f32::from(width));
                                cx.notify();
                            },
                        ))
                        .on_drop(cx.listener(|this, _: &SidebarEdge, _, cx| {
                            let width = this.sidebar_width;
                            Settings::update(cx, |settings| settings.sidebar_width = width);
                        }))
                        .child(
                            // The window ground goes behind the panes only, so a
                            // sidebar, which paints its own, isn't doubled up.
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .bg(theme.background)
                                .when(!show_sidebar, |column| {
                                    column.child(self.render_collapsed_bar(&theme))
                                })
                                .child(content)
                                .children(self.render_status_bar(&theme, cx)),
                        ),
                ),
            })
            .children(self.settings.clone())
    }
}

/// Lets the user know something finished, if they want to hear about it.
fn chime(cx: &App) {
    if cx.global::<Settings>().done_sound {
        sound::play_done();
    }
}

/// How long ago something finished, as briefly as a sidebar has room for.
fn age(elapsed: Duration) -> SharedString {
    let minutes = elapsed.as_secs() / 60;
    SharedString::from(match minutes {
        0 => "now".to_string(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    })
}

/// Icons are line drawings, thinner than text, so the muted text colour that
/// suits a label leaves them looking faded. They get most of the full one.
fn icon_color(theme: &Theme) -> gpui::Hsla {
    theme.text.opacity(0.8)
}

/// Shows or hides the tab sidebar.
fn sidebar_button(theme: &Theme) -> impl IntoElement {
    icon_button("toggle-sidebar", assets::SIDEBAR_ICON, theme)
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleSidebar), cx))
}

fn new_tab_button(theme: &Theme, cx: &mut Context<Workspace>) -> impl IntoElement {
    icon_button("new-tab", assets::PLUS_ICON, theme)
        .on_click(cx.listener(|this, _, window, cx| this.open_tab(window, cx)))
}

/// A square titlebar button around a mask icon. Every such button shares one
/// size and one icon size, so a row of them is evenly spaced by construction.
fn icon_button(id: &'static str, icon: &'static str, theme: &Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .group(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(ICON_BUTTON_SIZE))
        .rounded(Theme::radius())
        .hover(|style| style.bg(theme.elevated))
        .cursor_pointer()
        .child(
            // An svg takes no colour from its parent, so it is set here and
            // brightened along with the button on hover.
            svg()
                .path(icon)
                .size(px(ICON_SIZE))
                .text_color(icon_color(theme))
                .group_hover(id, |style| style.text_color(theme.text)),
        )
        // These buttons sit in the drag area; without this a click would also
        // start moving the window.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

const SHORTCUT_HINT: &str = "ctrl+shift T tab · D split · ] pane · , settings";
