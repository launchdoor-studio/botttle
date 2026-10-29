//! Choosing the user's shell, and turning things into shell input.

use std::ffi::CStr;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Points `SHELL` at the login shell from the user's account, so every pane
/// starts the shell the user chose.
///
/// The emulator takes `SHELL` over the account record, and an app started
/// from the Dock inherits `SHELL` from when the user logged in. After a
/// `chsh` that value stays stale until the next login, so without this new
/// panes would keep starting the old shell. Must run before any threads start.
pub fn use_login_shell() {
    if let Some(shell) = login_shell() {
        std::env::set_var("SHELL", shell);
    }
}

/// The shell set for this user in the account database, if it can be run.
fn login_shell() -> Option<String> {
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buffer = vec![0 as libc::c_char; 4096];
    let mut result: *mut libc::passwd = std::ptr::null_mut();

    // SAFETY: every pointer refers to memory owned by this frame, sized as
    // passed, and the strings in `entry` point into `buffer`, which outlives
    // the reads below.
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() || entry.pw_shell.is_null() {
        return None;
    }

    let shell = unsafe { CStr::from_ptr(entry.pw_shell) }
        .to_str()
        .ok()?
        .to_string();
    let executable = std::fs::metadata(&shell)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0);
    executable.then_some(shell)
}

/// Renders a path as a single shell word, so a directory with spaces or quotes
/// in it still arrives at the prompt in one piece.
pub fn quote(path: &Path) -> String {
    let path = path.to_string_lossy();
    let needs_quoting = path.is_empty()
        || path
            .chars()
            .any(|c| !matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | '.' | '_' | '-' | '~'));

    if needs_quoting {
        format!("'{}'", path.replace('\'', r"'\''"))
    } else {
        path.into_owned()
    }
}

/// Joins dropped or pasted paths into something a prompt can take, with a
/// trailing space so the next word does not run into the last path.
pub fn quote_all<'a>(paths: impl IntoIterator<Item = &'a Path>) -> String {
    let mut line = String::new();
    for path in paths {
        line.push_str(&quote(path));
        line.push(' ');
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn paths_are_quoted_only_when_they_need_it() {
        assert_eq!(quote(Path::new("/tmp/pasted-1.png")), "/tmp/pasted-1.png");
        assert_eq!(
            quote(Path::new("/tmp/my images/a.png")),
            "'/tmp/my images/a.png'"
        );
        assert_eq!(quote(Path::new("/tmp/it's.png")), r"'/tmp/it'\''s.png'");
    }

    #[test]
    fn several_paths_arrive_as_separate_words() {
        let paths = [
            PathBuf::from("/a/one.png"),
            PathBuf::from("/b/two three.png"),
        ];
        assert_eq!(
            quote_all(paths.iter().map(PathBuf::as_path)),
            "/a/one.png '/b/two three.png' "
        );
    }
}
