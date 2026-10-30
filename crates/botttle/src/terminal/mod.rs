//! The terminal model: a PTY, an ANSI emulator, and the plumbing between the
//! emulator's IO thread and gpui's main thread.
//!
//! [`Terminal`] owns no UI. Everything that draws lives in [`crate::terminal::view`].

pub mod activity;
pub mod color;
pub mod cwd;
pub mod hyperlink;
pub mod image_paste;
pub mod keys;
pub mod shell;
pub mod view;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use alacritty_terminal::event::{
    Event as AlacrittyEvent, EventListener, Notify, OnResize, WindowSize,
};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;
use anyhow::{Context as _, Result};
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use gpui::{px, Pixels, Size};

/// The geometry of a terminal, in both pixels and cells.
///
/// The emulator only cares about rows and columns; the PTY is also told the pixel
/// size so full-screen programs can size images and sixels correctly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalSize {
    pub cell_width: Pixels,
    pub line_height: Pixels,
    pub bounds: Size<Pixels>,
}

impl TerminalSize {
    pub fn new(cell_width: Pixels, line_height: Pixels, bounds: Size<Pixels>) -> Self {
        Self {
            cell_width,
            line_height,
            bounds,
        }
    }

    pub fn columns(&self) -> usize {
        if self.cell_width <= px(0.0) {
            return 2;
        }
        ((f32::from(self.bounds.width) / f32::from(self.cell_width)).floor() as usize).max(2)
    }

    pub fn lines(&self) -> usize {
        if self.line_height <= px(0.0) {
            return 1;
        }
        ((f32::from(self.bounds.height) / f32::from(self.line_height)).floor() as usize).max(1)
    }
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self::new(
            px(8.0),
            px(18.0),
            Size {
                width: px(640.0),
                height: px(360.0),
            },
        )
    }
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        self.lines()
    }

    fn columns(&self) -> usize {
        TerminalSize::columns(self)
    }
}

impl From<TerminalSize> for WindowSize {
    fn from(size: TerminalSize) -> Self {
        WindowSize {
            num_lines: size.lines() as u16,
            num_cols: size.columns() as u16,
            cell_width: f32::from(size.cell_width) as u16,
            cell_height: f32::from(size.line_height) as u16,
        }
    }
}

/// Forwards emulator events from the PTY thread to the main thread.
#[derive(Clone)]
pub struct EventProxy(UnboundedSender<AlacrittyEvent>);

impl EventListener for EventProxy {
    fn send_event(&self, event: AlacrittyEvent) {
        let _ = self.0.unbounded_send(event);
    }
}

pub struct Terminal {
    term: Arc<FairMutex<Term<EventProxy>>>,
    notifier: Notifier,
    size: TerminalSize,
    /// A duplicate of the pty controller, used only to ask which process group
    /// is in the foreground.
    master: Option<std::os::fd::OwnedFd>,
    /// Set from OSC 0/2; `None` means "use the default label".
    pub title: Option<String>,
    /// True once the child process is gone. The pane stays open so output can be read.
    pub exited: bool,
    /// Whether whatever is running is busy, for the tab list.
    pub activity: activity::Activity,
}

impl Terminal {
    /// Spawns the user's login shell on a new PTY.
    pub fn new(
        size: TerminalSize,
        working_directory: Option<PathBuf>,
        scrollback_lines: usize,
    ) -> Result<(Self, UnboundedReceiver<AlacrittyEvent>)> {
        let (tx, rx) = unbounded();
        let proxy = EventProxy(tx);

        let config = Config {
            scrolling_history: scrollback_lines,
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, proxy.clone())));

        let mut env = HashMap::new();
        env.insert("TERM_PROGRAM".to_string(), "botttle".to_string());
        env.insert(
            "TERM_PROGRAM_VERSION".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );

        // The struct update covers a Windows-only field that unix builds don't have.
        #[cfg_attr(not(target_os = "windows"), allow(clippy::needless_update))]
        let pty_options = tty::Options {
            shell: None,
            working_directory,
            drain_on_exit: false,
            env,
            ..Default::default()
        };
        let pty = tty::new(&pty_options, size.into(), 0).context("failed to open a pty")?;
        // Our own handle on the pty, so a new pane can ask this one where its
        // shell is. Duplicated because the pty itself moves into the IO thread.
        let master = cwd::duplicate_master(pty.file());

        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false)
            .context("failed to start the pty event loop")?;
        let notifier = Notifier(event_loop.channel());
        // The join handle is intentionally dropped: the thread stops when it sees
        // `Msg::Shutdown`, which `Drop` sends.
        let _io_thread = event_loop.spawn();

        Ok((
            Self {
                term,
                notifier,
                size,
                master,
                title: None,
                exited: false,
                activity: activity::Activity::default(),
            },
            rx,
        ))
    }

    pub fn size(&self) -> TerminalSize {
        self.size
    }

    /// Where this pane's shell currently is, for new panes to start from.
    ///
    /// This follows the pty's foreground process group rather than the child we
    /// spawned: on macOS that child is a root-owned `login`, and the shell whose
    /// directory we want is its child.
    pub fn working_directory(&self) -> Option<std::path::PathBuf> {
        if self.exited {
            return None;
        }
        let pid = cwd::foreground_process(self.master.as_ref()?)?;
        cwd::of_process(pid)
    }

    /// Sends bytes to the child process.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        self.notifier.notify(bytes);
    }

    /// Sends bytes the user typed, pasted or scrolled, so the output they
    /// cause isn't mistaken for the program working on its own.
    pub fn input(&mut self, bytes: impl Into<Cow<'static, [u8]>>) {
        self.activity.input(Instant::now());
        self.write(bytes);
    }

    /// Whether the shell has the terminal, rather than a program it started.
    /// A pane we can't ask about counts as idle, so it never claims to be busy.
    pub fn at_shell(&self) -> bool {
        if self.exited {
            return true;
        }
        self.master
            .as_ref()
            .and_then(cwd::foreground_process)
            .and_then(activity::process_name)
            .is_none_or(|name| activity::is_shell(&name))
    }

    /// Resizes both the emulator's grid and the PTY. A no-op if the cell
    /// dimensions haven't changed, so it is safe to call every frame.
    pub fn resize(&mut self, size: TerminalSize) {
        if size.columns() == self.size.columns()
            && size.lines() == self.size.lines()
            && size.cell_width == self.size.cell_width
            && size.line_height == self.size.line_height
        {
            self.size = size;
            return;
        }

        self.size = size;
        // Full-screen programs redraw on resize, which isn't work either.
        self.activity.input(Instant::now());
        self.term.lock().resize(size);
        self.notifier.on_resize(size.into());
    }

    /// Scrolls the viewport through the scrollback by `lines` (positive is up).
    pub fn scroll(&self, lines: i32) {
        if lines != 0 {
            self.term.lock().scroll_display(Scroll::Delta(lines));
        }
    }

    pub fn scroll_to_top(&self) {
        self.term.lock().scroll_display(Scroll::Top);
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    /// Locks the emulator. Keep the guard short-lived: the PTY thread blocks on
    /// the same lock.
    pub fn lock(&self) -> impl std::ops::DerefMut<Target = Term<EventProxy>> + '_ {
        self.term.lock()
    }

    pub fn selected_text(&self) -> Option<String> {
        self.term.lock().selection_to_string()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn grid_text(term: &Term<EventProxy>) -> String {
        term.grid().display_iter().map(|cell| cell.c).collect()
    }

    /// End-to-end: a real shell starts on a real pty, our input reaches it, and
    /// its output lands in the grid we render from.
    #[test]
    fn a_shell_runs_and_its_output_reaches_the_grid() {
        let size = TerminalSize::new(
            px(8.0),
            px(16.0),
            Size {
                width: px(640.0),
                height: px(320.0),
            },
        );
        let (terminal, _events) = Terminal::new(size, std::env::current_dir().ok(), 1_000)
            .expect("a pty and a shell should be available");

        assert_eq!(size.columns(), 80);
        assert_eq!(size.lines(), 20);

        terminal.write(b"echo botttle-works\r".to_vec());

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let text = {
                let term = terminal.lock();
                grid_text(&term)
            };
            // Once as the echoed command line, once as the command's output.
            if text.matches("botttle-works").count() >= 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the shell never echoed the command; grid was: {text:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
