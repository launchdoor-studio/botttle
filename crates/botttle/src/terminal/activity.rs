//! Whether a pane is busy, and when it has just finished, so the tab list can
//! point at the tabs worth going back to.
//!
//! Nothing tells a terminal "the program is done": shells, build tools and
//! coding agents all behave differently. Two cheap signals cover most of it: a
//! command handing the terminal back to the shell, and a program that keeps
//! the terminal (an agent, say) going quiet after a stretch of output.

use std::time::{Duration, Instant};

/// Output this soon after the user typed, pasted or scrolled is the program
/// echoing or redrawing for them, not working on its own.
const ECHO_WINDOW: Duration = Duration::from_millis(300);
/// How long output has to stop before a program counts as quiet again.
const QUIET_AFTER: Duration = Duration::from_millis(2500);
/// Output has to keep going this long before the pane shows as working, so a
/// one-off redraw doesn't flash it.
const WORKING_AFTER: Duration = Duration::from_secs(1);
/// Anything quicker than this was over before anyone looked away.
const WORTH_REPORTING: Duration = Duration::from_secs(3);

/// The names a process at a prompt goes by. `login` is what macOS runs first,
/// before it hands over to the shell.
const SHELLS: &[&str] = &[
    "bash", "csh", "dash", "elvish", "fish", "ksh", "login", "nu", "pwsh", "sh", "tcsh", "xonsh",
    "zsh",
];

#[derive(Debug, Default)]
pub struct Activity {
    last_input: Option<Instant>,
    /// When the current stretch of output began, and when it last printed.
    burst: Option<(Instant, Instant)>,
    /// When something other than the shell took the terminal.
    command_since: Option<Instant>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Status {
    /// The program is producing output on its own right now.
    pub working: bool,
    /// A long enough run has just ended. True for one poll only.
    pub finished: bool,
}

impl Activity {
    pub fn input(&mut self, now: Instant) {
        self.last_input = Some(now);
    }

    pub fn output(&mut self, now: Instant) {
        if self
            .last_input
            .is_some_and(|at| now.duration_since(at) < ECHO_WINDOW)
        {
            return;
        }
        self.burst = match self.burst {
            Some((start, last)) if now.duration_since(last) < QUIET_AFTER => Some((start, now)),
            _ => Some((now, now)),
        };
    }

    /// `at_shell` is whether the shell has the terminal, which is to say
    /// nothing is running in it.
    pub fn poll(&mut self, now: Instant, at_shell: bool) -> Status {
        let mut status = Status::default();

        if at_shell {
            if let Some(since) = self.command_since.take() {
                status.finished = now.duration_since(since) >= WORTH_REPORTING;
            }
            // Prompts redraw and keystrokes echo; none of it is work.
            self.burst = None;
            return status;
        }
        self.command_since.get_or_insert(now);

        if let Some((start, last)) = self.burst {
            if now.duration_since(last) >= QUIET_AFTER {
                self.burst = None;
                status.finished = last.duration_since(start) >= WORTH_REPORTING;
            } else {
                status.working = now.duration_since(start) >= WORKING_AFTER;
            }
        }
        status
    }
}

/// Whether a process name is a shell, which means whoever owns the terminal
/// is sitting at a prompt.
pub fn is_shell(name: &str) -> bool {
    // Login shells are started as `-fish`, `-zsh` and so on.
    SHELLS.contains(&name.trim_start_matches('-'))
}

/// The short name of a running process, as `ps` shows it.
pub fn process_name(pid: u32) -> Option<String> {
    platform::process_name(pid)
}

#[cfg(target_os = "macos")]
mod platform {
    pub fn process_name(pid: u32) -> Option<String> {
        let mut buffer = [0u8; 256];
        // SAFETY: the buffer is ours and its length is passed alongside it.
        let length = unsafe {
            libc::proc_name(
                pid as libc::c_int,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
            )
        };
        (length > 0).then(|| String::from_utf8_lossy(&buffer[..length as usize]).into_owned())
    }
}

#[cfg(target_os = "linux")]
mod platform {
    pub fn process_name(pid: u32) -> Option<String> {
        let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        Some(name.trim_end().to_string())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    pub fn process_name(_pid: u32) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    #[test]
    fn a_long_command_finishing_is_reported_once() {
        let start = Instant::now();
        let mut activity = Activity::default();
        assert_eq!(activity.poll(start, false), Status::default());
        assert!(!activity.poll(at(start, 2_000), false).finished);
        assert!(activity.poll(at(start, 5_000), true).finished);
        assert!(!activity.poll(at(start, 5_500), true).finished);
    }

    #[test]
    fn a_quick_command_is_not_worth_reporting() {
        let start = Instant::now();
        let mut activity = Activity::default();
        activity.poll(start, false);
        assert!(!activity.poll(at(start, 1_000), true).finished);
    }

    #[test]
    fn a_program_going_quiet_after_working_has_finished() {
        let start = Instant::now();
        let mut activity = Activity::default();
        for millis in (0..=4_000).step_by(100) {
            activity.output(at(start, millis));
        }
        assert!(activity.poll(at(start, 4_000), false).working);
        let quiet = activity.poll(at(start, 7_000), false);
        assert!(quiet.finished && !quiet.working);
    }

    #[test]
    fn echoed_typing_is_not_work() {
        let start = Instant::now();
        let mut activity = Activity::default();
        for millis in (0..=5_000).step_by(100) {
            activity.input(at(start, millis));
            activity.output(at(start, millis + 20));
        }
        assert!(!activity.poll(at(start, 5_100), false).working);
        assert!(!activity.poll(at(start, 9_000), false).finished);
    }

    #[test]
    fn login_shells_are_shells() {
        assert!(is_shell("-fish"));
        assert!(is_shell("zsh"));
        assert!(!is_shell("claude"));
        assert!(process_name(std::process::id()).is_some());
    }
}
