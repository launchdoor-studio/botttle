//! Draws a terminal grid and routes input to it.
//!
//! The grid is rendered as one styled text element per row. A monospace font
//! keeps columns aligned, and per-cell colors and attributes are applied as text
//! highlights, which keeps the whole view inside gpui's normal layout system.

use std::ops::Range;
use std::path::PathBuf;

use alacritty_terminal::event::Event as AlacrittyEvent;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::CursorShape;
use anyhow::Result;
use futures::channel::mpsc::UnboundedReceiver;
use futures::StreamExt;
use gpui::{
    canvas, div, font, prelude::*, px, App, Bounds, ClipboardEntry, ClipboardItem, Context, Entity,
    EventEmitter, ExternalPaths, FocusHandle, Focusable, FontFeatures, FontStyle, FontWeight,
    HighlightStyle, Image, KeyDownEvent, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point as GpuiPoint, Render, ScrollDelta,
    ScrollWheelEvent, SharedString, StrikethroughStyle, StyledText, Task, TouchPhase,
    UnderlineStyle, Window,
};

use crate::settings::{CursorShape as CursorShapeSetting, Settings};
use crate::terminal::color::{self, resolve};
use crate::terminal::cwd;
use crate::terminal::hyperlink::{self, Target};
use crate::terminal::image_paste;
use crate::terminal::keys;
use crate::terminal::shell;
use crate::terminal::{Terminal, TerminalSize};
use crate::theme::Theme;

/// Lines scrolled per wheel notch when the platform reports discrete steps.
const LINES_PER_SCROLL_NOTCH: f32 = 3.0;
/// The scrollbar thumb: thin enough to stay out of the way of the last column,
/// and never so short that it can't be seen at the top of a long history.
const SCROLLBAR_WIDTH: f32 = 4.0;
const SCROLLBAR_MIN_THUMB: f32 = 16.0;

/// Things the workspace cares about: they change what the tab strip shows.
#[derive(Clone, Debug)]
pub enum TerminalViewEvent {
    TitleChanged,
    Exited,
}

pub struct TerminalView {
    terminal: Terminal,
    focus_handle: FocusHandle,
    /// Where the grid was last painted, used to map mouse positions to cells.
    content_bounds: Bounds<Pixels>,
    /// The scrollback offset of the last painted frame, for the same reason.
    display_offset: usize,
    /// How many lines of history sit above the screen, for the scrollbar.
    history_size: usize,
    /// The part of a line that trackpad scrolling has moved but not yet
    /// scrolled, so slow, smooth gestures add up instead of rounding to nothing.
    scroll_remainder: f32,
    selecting: bool,
    /// The link under the pointer while the command key is held, as the display
    /// cells to underline plus what a click would open.
    hovered_link: Option<HoveredLink>,
    /// Where the pointer last was, so the underline can appear when the command
    /// key goes down without the mouse moving.
    last_mouse_position: Option<GpuiPoint<Pixels>>,
    _event_pump: Task<()>,
}

impl TerminalView {
    /// Starts a shell and wraps it in a view. Failing to open a PTY is reported
    /// to the caller rather than left inside a broken pane.
    pub fn spawn(working_directory: Option<PathBuf>, cx: &mut App) -> Result<Entity<Self>> {
        // The real geometry arrives on the first paint; this is just enough for
        // the shell to start up with a sane window size.
        let working_directory = working_directory.or_else(cwd::default_directory);
        let scrollback = cx.global::<Settings>().scrollback_lines.max(100);
        let (terminal, events) =
            Terminal::new(TerminalSize::default(), working_directory, scrollback)?;
        Ok(cx.new(|cx| Self::new(terminal, events, cx)))
    }

    fn new(
        terminal: Terminal,
        mut events: UnboundedReceiver<AlacrittyEvent>,
        cx: &mut Context<Self>,
    ) -> Self {
        let event_pump = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |view, cx| view.handle_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        Self {
            terminal,
            focus_handle: cx.focus_handle(),
            content_bounds: Bounds::default(),
            display_offset: 0,
            history_size: 0,
            scroll_remainder: 0.0,
            selecting: false,
            hovered_link: None,
            last_mouse_position: None,
            _event_pump: event_pump,
        }
    }

    pub fn title(&self) -> SharedString {
        match &self.terminal.title {
            Some(title) if !title.trim().is_empty() => SharedString::from(title.clone()),
            _ if self.terminal.exited => SharedString::from("exited"),
            _ => SharedString::from("shell"),
        }
    }

    pub fn has_exited(&self) -> bool {
        self.terminal.exited
    }

    pub fn working_directory(&self) -> Option<PathBuf> {
        self.terminal.working_directory()
    }

    pub fn copy(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.terminal.selected_text() {
            if !text.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        }
    }

    pub fn paste(&mut self, cx: &mut Context<Self>) {
        if self.paste_image(cx) {
            return;
        }

        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.paste_text(&text, cx);
    }

    /// Writes a clipboard image to a file and types its path, the way dropping a
    /// file onto a terminal does. Returns false when there is no image to paste,
    /// so callers can fall back to their normal behaviour.
    pub fn paste_image(&mut self, cx: &mut Context<Self>) -> bool {
        if !cx.global::<Settings>().paste_images {
            return false;
        }
        let Some(image) = cx.read_from_clipboard().and_then(clipboard_image) else {
            return false;
        };

        match image_paste::write(&image, std::time::SystemTime::now()) {
            Ok(path) => {
                // A trailing space keeps the prompt ready for the rest of the message.
                self.paste_text(&shell::quote_all([path.as_path()]), cx);
                true
            }
            Err(error) => {
                eprintln!("botttle: could not paste image: {error:#}");
                false
            }
        }
    }

    /// Files dropped onto a pane arrive as their paths, which is what dropping
    /// a file onto any terminal does — and how Claude Code and Codex take an
    /// image that is already on disk.
    fn on_drop_paths(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.insert_paths(paths.paths(), window, cx);
    }

    /// Types paths at the prompt, quoted so a name with spaces stays one word.
    fn insert_paths(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }

        // A drop is a deliberate act on this pane, so give it the keyboard too.
        window.focus(&self.focus_handle);
        self.paste_text(&shell::quote_all(paths.iter().map(PathBuf::as_path)), cx);
    }

    /// Sends text to the child, bracketed when the program asked for it.
    fn paste_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let bracketed = self
            .terminal
            .lock()
            .mode()
            .contains(TermMode::BRACKETED_PASTE);

        if bracketed {
            self.terminal.write(b"\x1b[200~".to_vec());
            self.terminal.write(text.replace('\x1b', "").into_bytes());
            self.terminal.write(b"\x1b[201~".to_vec());
        } else {
            self.terminal.write(text.replace("\r\n", "\r").into_bytes());
        }
        self.terminal.scroll_to_bottom();
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        // Ctrl-L, so the running program decides what "clear" means.
        self.terminal.write(vec![0x0c]);
        cx.notify();
    }

    fn handle_event(&mut self, event: AlacrittyEvent, cx: &mut Context<Self>) {
        match event {
            AlacrittyEvent::Wakeup => cx.notify(),
            AlacrittyEvent::Title(title) => {
                self.terminal.title = Some(title);
                cx.emit(TerminalViewEvent::TitleChanged);
                cx.notify();
            }
            AlacrittyEvent::ResetTitle => {
                self.terminal.title = None;
                cx.emit(TerminalViewEvent::TitleChanged);
                cx.notify();
            }
            AlacrittyEvent::PtyWrite(text) => self.terminal.write(text.into_bytes()),
            AlacrittyEvent::ClipboardStore(_, text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            AlacrittyEvent::ClipboardLoad(_, format) => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.terminal.write(format(&text).into_bytes());
                }
            }
            AlacrittyEvent::ColorRequest(index, format) => {
                let theme = cx.global::<Theme>();
                let hsla = match index {
                    0..=15 => theme.ansi[index],
                    256 => theme.terminal_foreground,
                    257 => theme.terminal_background,
                    _ => theme.cursor,
                };
                self.terminal
                    .write(format(color::to_rgb(hsla)).into_bytes());
            }
            AlacrittyEvent::TextAreaSizeRequest(format) => {
                self.terminal
                    .write(format(self.terminal.size().into()).into_bytes());
            }
            AlacrittyEvent::ChildExit(_) | AlacrittyEvent::Exit => {
                self.terminal.exited = true;
                cx.emit(TerminalViewEvent::Exited);
                cx.notify();
            }
            AlacrittyEvent::Bell
            | AlacrittyEvent::MouseCursorDirty
            | AlacrittyEvent::CursorBlinkingChange => {}
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        // Coding CLIs take images as file paths, and ctrl-v is the chord they
        // document for it. With no image on the clipboard this falls through, so
        // ctrl-v still reaches readline as quoted-insert.
        if is_ctrl_v(&event.keystroke) && self.paste_image(cx) {
            cx.stop_propagation();
            return;
        }

        let mode = *self.terminal.lock().mode();
        if self.scroll_by_key(&event.keystroke, mode) {
            cx.stop_propagation();
            cx.notify();
            return;
        }

        let shift_enter_newline = cx.global::<Settings>().shift_enter_newline;
        let Some(bytes) = keys::to_bytes(&event.keystroke, mode, shift_enter_newline) else {
            return;
        };

        self.terminal.write(bytes);
        self.terminal.scroll_to_bottom();
        cx.stop_propagation();
        cx.notify();
    }

    /// Shift with page up, page down, home or end moves through the scrollback,
    /// the way it does in most terminals. Full-screen programs have no
    /// scrollback, so on the alternate screen the keys go to them instead.
    fn scroll_by_key(&mut self, keystroke: &gpui::Keystroke, mode: TermMode) -> bool {
        let modifiers = keystroke.modifiers;
        if !modifiers.shift
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || mode.contains(TermMode::ALT_SCREEN)
        {
            return false;
        }

        let page = self.terminal.size().lines().saturating_sub(1).max(1) as i32;
        match keystroke.key.as_str() {
            "pageup" => self.terminal.scroll(page),
            "pagedown" => self.terminal.scroll(-page),
            "home" => self.terminal.scroll_to_top(),
            "end" => self.terminal.scroll_to_bottom(),
            _ => return false,
        }
        true
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event.touch_phase, TouchPhase::Started) {
            self.scroll_remainder = 0.0;
        }

        let line_height = f32::from(self.terminal.size().line_height).max(1.0);
        self.scroll_remainder += match event.delta {
            ScrollDelta::Lines(delta) => delta.y * LINES_PER_SCROLL_NOTCH,
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / line_height,
        };
        let lines = self.scroll_remainder.trunc();
        self.scroll_remainder -= lines;
        let lines = lines as i32;
        if lines == 0 {
            return;
        }

        // Programs that track the mouse get the wheel as mouse events, and
        // full-screen ones without mouse support get it as arrow keys, so
        // scrolling works in less, vim, and coding agents alike. Shift
        // overrides both and always scrolls our own history.
        let mode = *self.terminal.lock().mode();
        if !event.modifiers.shift && mode.intersects(TermMode::MOUSE_MODE) {
            let (row, column) = self.clamped_cell(event.position);
            let report = mouse_wheel_report(lines > 0, row, column, event.modifiers, mode);
            self.terminal
                .write(report.repeat(lines.unsigned_abs() as usize));
        } else if !event.modifiers.shift
            && mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
        {
            let arrow: &[u8] = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            self.terminal
                .write(arrow.repeat(lines.unsigned_abs() as usize));
        } else {
            self.terminal.scroll(lines);
            cx.notify();
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);

        // Cmd-click opens what is under the pointer instead of starting a
        // selection, the way it does everywhere else on the platform.
        if opens_link(event.modifiers) && self.open_link_at(event.position, cx) {
            cx.stop_propagation();
            return;
        }

        let Some((point, side)) = self.grid_point(event.position) else {
            return;
        };
        self.selecting = true;
        let selection_type = if event.click_count >= 3 {
            SelectionType::Lines
        } else if event.click_count == 2 {
            SelectionType::Semantic
        } else {
            SelectionType::Simple
        };

        let mut term = self.terminal.lock();
        term.selection = Some(Selection::new(selection_type, point, side));
        drop(term);
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.last_mouse_position = Some(event.position);
        self.update_hovered_link(event.modifiers, cx);

        if !self.selecting {
            return;
        }
        let Some((point, side)) = self.grid_point(event.position) else {
            return;
        };

        let mut term = self.terminal.lock();
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
        drop(term);
        cx.notify();
    }

    /// Ends a selection, also when the button comes up outside the pane, and
    /// copies it straight away when copy-on-select is on.
    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.selecting) {
            return;
        }
        if cx.global::<Settings>().copy_on_select {
            self.copy(cx);
        }
    }

    fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.update_hovered_link(event.modifiers, cx);
    }

    /// Recomputes the underlined link. Cheap enough to run on every move: it
    /// only touches the filesystem when the pointer changes cell.
    fn update_hovered_link(&mut self, modifiers: Modifiers, cx: &mut Context<Self>) {
        let found = if opens_link(modifiers) {
            self.last_mouse_position
                .and_then(|position| self.link_at(position))
        } else {
            None
        };

        let changed = match (&self.hovered_link, &found) {
            (Some(current), Some(new)) => current.cells != new.cells,
            (None, None) => false,
            _ => true,
        };
        if changed {
            self.hovered_link = found;
            cx.notify();
        }
    }

    fn open_link_at(&mut self, position: GpuiPoint<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some(link) = self.link_at(position) else {
            return false;
        };
        open(link.target, cx);
        self.hovered_link = None;
        cx.notify();
        true
    }

    /// The link under a window position, if there is one: a link the program
    /// attached to the text first, then one recognised in the text itself.
    fn link_at(&self, position: GpuiPoint<Pixels>) -> Option<HoveredLink> {
        let (row, column) = self.display_cell(position)?;
        if let Some(link) = self.explicit_link_at(row, column) {
            return Some(link);
        }
        let line = self.logical_line(row)?;
        let index = line.cells.iter().position(|cell| *cell == (row, column))?;

        let working_directory = self.terminal.working_directory();
        let detection = hyperlink::detect(
            &line.text,
            index,
            working_directory.as_deref(),
            cwd::home().as_deref(),
        )?;

        // Turn the character range back into cells, one contiguous span per row
        // so a wrapped link underlines across both.
        let mut cells: Vec<(usize, Range<usize>)> = Vec::new();
        for (row, column) in &line.cells[detection.range] {
            match cells.last_mut() {
                Some((last_row, span)) if last_row == row && span.end == *column => {
                    span.end = column + 1;
                }
                _ => cells.push((*row, *column..column + 1)),
            }
        }

        Some(HoveredLink {
            cells,
            target: detection.target,
        })
    }

    /// An OSC 8 hyperlink at a display cell. Its extent is every neighbouring
    /// cell carrying the same link, which may run across several rows.
    fn explicit_link_at(&self, row: usize, column: usize) -> Option<HoveredLink> {
        let term = self.terminal.lock();
        let columns = term.columns();
        let screen_lines = term.screen_lines();
        let offset = self.display_offset as i32;
        let link_at = |row: usize, column: usize| {
            term.grid()[Line(row as i32 - offset)][Column(column)].hyperlink()
        };

        let link = link_at(row, column)?;
        let target = hyperlink::from_uri(link.uri())?;

        // Walk the screen as one long run of cells in both directions.
        let index = row * columns + column;
        let same = |index: usize| link_at(index / columns, index % columns).as_ref() == Some(&link);
        let mut first = index;
        while first > 0 && same(first - 1) {
            first -= 1;
        }
        let mut last = index;
        while last + 1 < screen_lines * columns && same(last + 1) {
            last += 1;
        }

        let mut cells: Vec<(usize, Range<usize>)> = Vec::new();
        for index in first..=last {
            let (row, column) = (index / columns, index % columns);
            match cells.last_mut() {
                Some((last_row, span)) if *last_row == row => span.end = column + 1,
                _ => cells.push((row, column..column + 1)),
            }
        }

        Some(HoveredLink { cells, target })
    }

    /// The display row and column a window position falls in.
    fn display_cell(&self, position: GpuiPoint<Pixels>) -> Option<(usize, usize)> {
        let size = self.terminal.size();
        if !self.content_bounds.contains(&position) {
            return None;
        }

        let relative_x = f32::from(position.x - self.content_bounds.origin.x);
        let relative_y = f32::from(position.y - self.content_bounds.origin.y);
        let column = (relative_x / f32::from(size.cell_width).max(1.0)).floor() as usize;
        let row = (relative_y / f32::from(size.line_height).max(1.0)).floor() as usize;

        (row < size.lines() && column < size.columns()).then_some((row, column))
    }

    /// The cell under a window position, pulled onto the grid when the pointer
    /// is over the pane's padding.
    fn clamped_cell(&self, position: GpuiPoint<Pixels>) -> (usize, usize) {
        let size = self.terminal.size();
        let relative_x = f32::from(position.x - self.content_bounds.origin.x).max(0.0);
        let relative_y = f32::from(position.y - self.content_bounds.origin.y).max(0.0);
        let column = (relative_x / f32::from(size.cell_width).max(1.0)) as usize;
        let row = (relative_y / f32::from(size.line_height).max(1.0)) as usize;
        (
            row.min(size.lines().saturating_sub(1)),
            column.min(size.columns().saturating_sub(1)),
        )
    }

    /// The text a row belongs to, following the emulator's wrap markers so a
    /// path broken across two rows is still one string.
    fn logical_line(&self, row: usize) -> Option<LogicalLine> {
        let term = self.terminal.lock();
        let columns = term.columns();
        let screen_lines = term.screen_lines();
        if row >= screen_lines || columns == 0 {
            return None;
        }

        let offset = self.display_offset as i32;
        let wraps = |row: usize| -> bool {
            let line = Line(row as i32 - offset);
            term.grid()[line][Column(columns - 1)]
                .flags
                .contains(Flags::WRAPLINE)
        };

        let mut first = row;
        while first > 0 && wraps(first - 1) {
            first -= 1;
        }
        let mut last = row;
        while last + 1 < screen_lines && wraps(last) {
            last += 1;
        }

        let mut line = LogicalLine::default();
        for row in first..=last {
            for column in 0..columns {
                let cell = &term.grid()[Line(row as i32 - offset)][Column(column)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                line.text.push(cell.c);
                line.cells.push((row, column));
            }
        }

        Some(line)
    }

    /// Maps a window position to a cell, plus which half of the cell it landed on.
    fn grid_point(&self, position: GpuiPoint<Pixels>) -> Option<(Point, Side)> {
        let size = self.terminal.size();
        if !self.content_bounds.contains(&position) {
            return None;
        }

        let relative_x = f32::from(position.x - self.content_bounds.origin.x);
        let relative_y = f32::from(position.y - self.content_bounds.origin.y);
        let column = (relative_x / f32::from(size.cell_width).max(1.0)).floor();
        let row = (relative_y / f32::from(size.line_height).max(1.0)).floor() as i32;

        let side = if column.fract() >= 0.5 {
            Side::Right
        } else {
            Side::Left
        };
        let column = (column as usize).min(size.columns().saturating_sub(1));
        let line = Line(row - self.display_offset as i32);

        Some((Point::new(line, Column(column)), side))
    }

    /// Called from the paint pass with the grid's real geometry.
    fn measured(
        &mut self,
        bounds: Bounds<Pixels>,
        cell_width: Pixels,
        line_height: Pixels,
        cx: &mut Context<Self>,
    ) {
        self.content_bounds = bounds;

        let size = TerminalSize::new(cell_width, line_height, bounds.size);
        let previous = self.terminal.size();
        let reflowed = size.columns() != previous.columns() || size.lines() != previous.lines();

        self.terminal.resize(size);
        if reflowed {
            cx.notify();
        }
    }

    /// Lays out the visible grid. The cursor is drawn into the text only when a
    /// focused pane uses a block cursor; other shapes are painted as an overlay.
    fn build_grid(&mut self, theme: &Theme, focused: bool, shape: CursorShapeSetting) -> Grid {
        let link_cells = self
            .hovered_link
            .as_ref()
            .map(|link| link.cells.clone())
            .unwrap_or_default();

        let (rows, cursor, display_offset, history_size) = {
            let term = self.terminal.lock();
            let history_size = term.history_size();
            let columns = term.columns();
            let screen_lines = term.screen_lines();
            let content = term.renderable_content();
            let colors = content.colors;
            let selection = content.selection;
            let display_offset = content.display_offset as i32;

            let cursor_row = content.cursor.point.line.0 + display_offset;
            let cursor_column = content.cursor.point.column.0;
            let cursor_visible = content.cursor.shape != CursorShape::Hidden;

            let mut rows: Vec<RowBuilder> = (0..screen_lines)
                .map(|_| RowBuilder::new(columns))
                .collect();

            for indexed in content.display_iter {
                let row_index = indexed.point.line.0 + display_offset;
                if row_index < 0 || row_index as usize >= screen_lines {
                    continue;
                }
                let cell = indexed.cell;
                // The second half of a wide character is a placeholder; the font
                // already draws the glyph at double width.
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }

                let mut foreground = resolve(cell.fg, colors, theme);
                let mut background = resolve(cell.bg, colors, theme);
                if cell.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut foreground, &mut background);
                }
                if cell.flags.contains(Flags::DIM) {
                    foreground = color::dim(foreground);
                }
                if cell.flags.contains(Flags::HIDDEN) {
                    foreground = background;
                }
                if selection.is_some_and(|range| range.contains(indexed.point)) {
                    background = theme.selection;
                }
                let is_cursor_cell = cursor_visible
                    && row_index == cursor_row
                    && indexed.point.column.0 == cursor_column;
                if is_cursor_cell && focused && shape == CursorShapeSetting::Block {
                    background = theme.cursor;
                    foreground = theme.terminal_background;
                }

                // The link under the pointer is drawn as one, so it is obvious
                // what a click will open before the click happens.
                let is_link = link_cells.iter().any(|(link_row, span)| {
                    *link_row == row_index as usize && span.contains(&indexed.point.column.0)
                });
                if is_link {
                    foreground = theme.accent;
                }

                let style = HighlightStyle {
                    color: Some(foreground),
                    background_color: (background != theme.terminal_background)
                        .then_some(background),
                    font_weight: cell.flags.contains(Flags::BOLD).then_some(FontWeight::BOLD),
                    font_style: cell
                        .flags
                        .contains(Flags::ITALIC)
                        .then_some(FontStyle::Italic),
                    underline: (is_link || cell.flags.intersects(Flags::ALL_UNDERLINES)).then(
                        || UnderlineStyle {
                            thickness: px(1.0),
                            color: Some(foreground),
                            wavy: !is_link && cell.flags.contains(Flags::UNDERCURL),
                        },
                    ),
                    strikethrough: cell.flags.contains(Flags::STRIKEOUT).then(|| {
                        StrikethroughStyle {
                            thickness: px(1.0),
                            color: Some(foreground),
                        }
                    }),
                    fade_out: None,
                };

                rows[row_index as usize].push(cell.c, style);
            }

            let cursor = (cursor_visible
                && cursor_row >= 0
                && (cursor_row as usize) < screen_lines
                && cursor_column < columns)
                .then_some(CursorPlacement {
                    row: cursor_row as usize,
                    column: cursor_column,
                });

            (rows, cursor, display_offset, history_size)
        };

        self.display_offset = display_offset.max(0) as usize;
        self.history_size = history_size;
        Grid {
            rows: rows.into_iter().map(RowBuilder::finish).collect(),
            cursor,
        }
    }

    /// A thumb along the right edge while the view is scrolled back, showing
    /// where in the history it is. At the bottom there is nothing to say, so
    /// it stays hidden and out of the way of the live output.
    fn render_scrollbar(&self, theme: &Theme, height: Pixels) -> Option<gpui::Div> {
        if self.display_offset == 0 || self.history_size == 0 {
            return None;
        }

        let screen_lines = self.terminal.size().lines();
        let total = (self.history_size + screen_lines) as f32;
        let height = f32::from(height);
        let thumb = (height * screen_lines as f32 / total).max(SCROLLBAR_MIN_THUMB);
        let lines_above = self.history_size.saturating_sub(self.display_offset) as f32;
        let top = ((height - thumb) * lines_above / self.history_size as f32).max(0.0);

        Some(
            div()
                .absolute()
                .right_0()
                .top(px(top))
                .w(px(SCROLLBAR_WIDTH))
                .h(px(thumb))
                .rounded(px(SCROLLBAR_WIDTH / 2.0))
                .bg(theme.text_muted.opacity(0.5)),
        )
    }

    /// The cursor drawn on top of the text, for every case the block-in-text
    /// path doesn't cover: bar and underline shapes, and unfocused panes.
    fn render_cursor(
        &self,
        placement: CursorPlacement,
        theme: &Theme,
        focused: bool,
        shape: CursorShapeSetting,
        cell_width: Pixels,
        line_height: Pixels,
    ) -> Option<gpui::Div> {
        if focused && shape == CursorShapeSetting::Block {
            return None;
        }

        let left = px(f32::from(cell_width) * placement.column as f32);
        let top = px(f32::from(line_height) * placement.row as f32);
        let cursor = div().absolute().left(left).top(top);

        Some(if !focused {
            // A hollow box: the pane is still where it was, it just isn't listening.
            cursor
                .w(cell_width)
                .h(line_height)
                .border_1()
                .border_color(theme.cursor.opacity(0.7))
        } else {
            match shape {
                CursorShapeSetting::Bar => cursor.w(px(2.0)).h(line_height).bg(theme.cursor),
                CursorShapeSetting::Underline => cursor
                    .top(px(f32::from(top) + f32::from(line_height) - 2.0))
                    .w(cell_width)
                    .h(px(2.0))
                    .bg(theme.cursor),
                CursorShapeSetting::Block => cursor.w(cell_width).h(line_height).bg(theme.cursor),
            }
        })
    }
}

impl EventEmitter<TerminalViewEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        let cursor_shape = cx.global::<Settings>().cursor_shape;
        let focused = self.focus_handle.is_focused(window);

        let font_size = theme.font_size;
        let line_height = theme.line_height();
        let font_id = window
            .text_system()
            .resolve_font(&font(theme.font_family.clone()));
        let cell_width = window
            .text_system()
            .em_advance(font_id, font_size)
            .unwrap_or(px(8.0));

        let drop_tint = theme.accent.opacity(0.12);
        let theme_accent = theme.accent;
        let grid = self.build_grid(&theme, focused, cursor_shape);
        let cursor = grid.cursor.and_then(|placement| {
            self.render_cursor(
                placement,
                &theme,
                focused,
                cursor_shape,
                cell_width,
                line_height,
            )
        });
        let scrollbar = self.render_scrollbar(&theme, self.content_bounds.size.height);
        let entity = cx.entity().downgrade();

        let mut root = div()
            // Identified so it can track hover state, which is what clears the
            // link underline when the pointer leaves the pane.
            .id("terminal")
            .key_context("Terminal")
            .track_focus(&self.focus_handle)
            .size_full()
            .p_2()
            .bg(theme.pane_background)
            .text_color(theme.terminal_foreground)
            .font_family(theme.font_family.clone())
            .text_size(font_size)
            .line_height(line_height)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
            .on_drop(cx.listener(Self::on_drop_paths))
            // A tint while files are over the pane, so it is clear which one
            // will receive them when there is more than one open.
            .drag_over::<ExternalPaths>(move |style, _, _, _| {
                style.bg(drop_tint).border_color(theme_accent)
            })
            // Leaving the pane with the command key still down would otherwise
            // leave an underline behind in a pane the pointer has left.
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered && this.hovered_link.take().is_some() {
                    this.last_mouse_position = None;
                    cx.notify();
                }
            }))
            .when(self.hovered_link.is_some(), |element| {
                element.cursor_pointer()
            });

        if !theme.ligatures {
            root.text_style()
                .get_or_insert_with(Default::default)
                .font_features = Some(FontFeatures::disable_ligatures());
        }

        root.child(
            div()
                .relative()
                .size_full()
                .overflow_hidden()
                .child(
                    canvas(
                        move |bounds, _, cx| {
                            entity
                                .update(cx, |view, cx| {
                                    view.measured(bounds, cell_width, line_height, cx)
                                })
                                .ok();
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .children(grid.rows.into_iter().map(|row| {
                    div()
                        .h(line_height)
                        .child(StyledText::new(row.text).with_highlights(row.highlights))
                }))
                .children(cursor)
                .children(scrollbar),
        )
    }
}

/// Where the cursor sits in the visible grid.
#[derive(Clone, Copy)]
struct CursorPlacement {
    row: usize,
    column: usize,
}

/// A laid-out frame of the terminal.
struct Grid {
    rows: Vec<Row>,
    cursor: Option<CursorPlacement>,
}

/// One rendered row: the text plus the styles that apply to byte ranges of it.
struct Row {
    text: SharedString,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
}

/// Accumulates cells into a row, merging neighbours that share a style so the
/// text system sees as few runs as possible.
struct RowBuilder {
    text: String,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    current: Option<(usize, HighlightStyle)>,
}

impl RowBuilder {
    fn new(columns: usize) -> Self {
        Self {
            text: String::with_capacity(columns),
            highlights: Vec::new(),
            current: None,
        }
    }

    fn push(&mut self, c: char, style: HighlightStyle) {
        let offset = self.text.len();
        match &self.current {
            Some((_, current)) if *current == style => {}
            Some((start, current)) => {
                self.highlights.push((*start..offset, *current));
                self.current = Some((offset, style));
            }
            None => self.current = Some((offset, style)),
        }
        self.text.push(c);
    }

    fn finish(mut self) -> Row {
        if let Some((start, style)) = self.current.take() {
            self.highlights.push((start..self.text.len(), style));
        }
        Row {
            text: SharedString::from(self.text),
            highlights: self.highlights,
        }
    }
}

/// Picks the image out of a clipboard item, if it carries one.
fn clipboard_image(item: gpui::ClipboardItem) -> Option<Image> {
    item.into_entries().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(image),
        ClipboardEntry::String(_) => None,
    })
}

/// One wheel step as a mouse report, in SGR form when the program asked for
/// it and the legacy byte encoding otherwise. Rows and columns are zero-based.
fn mouse_wheel_report(
    up: bool,
    row: usize,
    column: usize,
    modifiers: Modifiers,
    mode: TermMode,
) -> Vec<u8> {
    let mut button: u32 = if up { 64 } else { 65 };
    if modifiers.shift {
        button += 4;
    }
    if modifiers.alt {
        button += 8;
    }
    if modifiers.control {
        button += 16;
    }

    if mode.contains(TermMode::SGR_MOUSE) {
        format!("\x1b[<{button};{};{}M", column + 1, row + 1).into_bytes()
    } else {
        // Each value travels as one byte offset by 32, so positions past 223
        // can't be expressed and are pinned to the last one that can.
        let encode = |value: usize| (32 + 1 + value).min(255) as u8;
        vec![
            0x1b,
            b'[',
            b'M',
            32 + button as u8,
            encode(column),
            encode(row),
        ]
    }
}

fn is_ctrl_v(keystroke: &gpui::Keystroke) -> bool {
    let modifiers = keystroke.modifiers;
    keystroke.key == "v" && modifiers.control && !modifiers.platform && !modifiers.alt
}

/// A link under the pointer: the cells to underline, and what a click opens.
struct HoveredLink {
    /// One contiguous column span per display row.
    cells: Vec<(usize, Range<usize>)>,
    target: Target,
}

/// A row's text together with the cell each character came from, so a detection
/// made on the text can be drawn back onto the grid.
#[derive(Default)]
struct LogicalLine {
    text: Vec<char>,
    cells: Vec<(usize, usize)>,
}

/// Cmd on macOS, super elsewhere. Ctrl is deliberately not used: on macOS it is
/// the system's secondary-click gesture, and in a terminal it belongs to the
/// program anyway.
fn opens_link(modifiers: Modifiers) -> bool {
    modifiers.platform && !modifiers.control && !modifiers.alt
}

/// Hands a target to the system opener, off the main thread so the window does
/// not stall while Finder or a browser starts.
fn open(target: Target, cx: &mut App) {
    let argument = match target {
        Target::Url(url) => url,
        Target::Path(path) => path.display().to_string(),
    };

    cx.background_executor()
        .spawn(async move {
            let opener = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            if let Err(error) = std::process::Command::new(opener).arg(&argument).status() {
                eprintln!("botttle: could not open {argument}: {error}");
            }
        })
        .detach();
}
