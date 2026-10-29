//! Finding the URL or file path under the pointer.
//!
//! Terminal output is just characters, so "is there a link here" is a guess. The
//! rules here are deliberately conservative: a URL needs a scheme we recognise,
//! and a path has to actually exist on disk before it counts as one. Guessing
//! wrong costs the user a stray window, so it is better to do nothing.

use std::ops::Range;
use std::path::{Path, PathBuf};

/// Schemes we are willing to hand to the system opener. Anything else — custom
/// app schemes, `javascript:` — is left alone: the text came from a program's
/// output, which is not a trustworthy source of things to launch.
const SCHEMES: &[&str] = &["http://", "https://", "file://", "ssh://", "mailto:"];

/// Characters that always end a candidate, whichever side of it we are on.
const BOUNDARIES: &[char] = &[' ', '\t', '"', '\'', '`', '<', '>', '|', '\0'];

/// Trailing punctuation that is nearly always prose rather than part of the
/// target: "see http://example.com." or "(file.rs)".
const TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '>', '\''];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Url(String),
    Path(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detection {
    /// Character indices into the line, for underlining what will open.
    pub range: Range<usize>,
    pub target: Target,
}

/// The target of an explicit hyperlink, the kind programs attach to text with
/// OSC 8. The visible text can be anything, so only the URI is judged, by the
/// same rules as a link found in the text: a known scheme, and for `file://`
/// a path that exists.
pub fn from_uri(uri: &str) -> Option<Target> {
    let lowercase = uri.to_lowercase();
    if lowercase.starts_with("file://") {
        // `file://host/path`: the host is this machine (or empty), and what
        // follows it is the path, percent-encoded.
        let rest = &uri["file://".len()..];
        let path = &rest[rest.find('/')?..];
        let path = PathBuf::from(percent_decode(path)?);
        return path.exists().then_some(Target::Path(path));
    }
    SCHEMES
        .iter()
        .any(|scheme| lowercase.starts_with(scheme))
        .then(|| Target::Url(uri.to_string()))
}

/// Undoes `%XX` escapes. Malformed escapes or non-UTF-8 results give nothing
/// rather than a path that is subtly wrong.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// Looks for a target at `index` in a line of terminal text.
///
/// `home` and `working_directory` resolve `~` and relative paths; without them
/// only absolute paths can be recognised.
pub fn detect(
    line: &[char],
    index: usize,
    working_directory: Option<&Path>,
    home: Option<&Path>,
) -> Option<Detection> {
    if index >= line.len() || line[index].is_whitespace() || BOUNDARIES.contains(&line[index]) {
        return None;
    }

    let mut start = index;
    while start > 0 && !is_boundary(line[start - 1]) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < line.len() && !is_boundary(line[end]) {
        end += 1;
    }

    // Trim trailing punctuation, but keep a closing bracket that has an opener
    // inside the candidate, as in a URL with a parenthesised path.
    let word: String = line[start..end].iter().collect();
    let mut word = word.as_str();
    while let Some(last) = word.chars().last() {
        if !TRAILING.contains(&last) || balanced(word, last) {
            break;
        }
        word = &word[..word.len() - last.len_utf8()];
        end -= 1;
    }

    if word.is_empty() || end <= index {
        return None;
    }

    let target = classify(word, working_directory, home)?;
    Some(Detection {
        range: start..end,
        target,
    })
}

fn is_boundary(c: char) -> bool {
    c.is_whitespace() || c.is_control() || BOUNDARIES.contains(&c)
}

/// True when `closer` closes something opened inside `word`, e.g. the final `)`
/// of `https://en.wikipedia.org/wiki/Rust_(programming_language)`.
fn balanced(word: &str, closer: char) -> bool {
    let opener = match closer {
        ')' => '(',
        ']' => '[',
        '}' => '{',
        _ => return false,
    };
    word.matches(opener).count() >= word.matches(closer).count()
}

fn classify(word: &str, working_directory: Option<&Path>, home: Option<&Path>) -> Option<Target> {
    let lowercase = word.to_lowercase();
    if SCHEMES.iter().any(|scheme| lowercase.starts_with(scheme)) {
        return Some(Target::Url(word.to_string()));
    }

    // Compiler and test output points at a position in a file: strip it, so
    // `src/main.rs:42:9` opens `src/main.rs`.
    let without_position = strip_position(word);
    resolve(without_position, working_directory, home).map(Target::Path)
}

/// Removes a trailing `:line` or `:line:column`.
fn strip_position(word: &str) -> &str {
    let mut candidate = word;
    for _ in 0..2 {
        let Some((head, tail)) = candidate.rsplit_once(':') else {
            break;
        };
        if tail.is_empty() || !tail.chars().all(|c| c.is_ascii_digit()) {
            break;
        }
        candidate = head;
    }
    candidate
}

/// Turns a path-looking string into a path that exists, or nothing.
fn resolve(word: &str, working_directory: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    if word.is_empty() {
        return None;
    }

    let candidate = if let Some(rest) = word.strip_prefix("~/") {
        home?.join(rest)
    } else if word == "~" {
        home?.to_path_buf()
    } else {
        let path = Path::new(word);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            // Only treat bare words as paths when they carry a separator or an
            // extension; otherwise every word in a log line hits the filesystem.
            if !word.contains('/') && !word.contains('.') {
                return None;
            }
            working_directory?.join(path)
        }
    };

    candidate.exists().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(line: &str) -> Vec<char> {
        line.chars().collect()
    }

    fn find(line: &str, index: usize) -> Option<Detection> {
        detect(
            &chars(line),
            index,
            Some(Path::new("/tmp")),
            Some(Path::new("/tmp")),
        )
    }

    #[test]
    fn finds_a_url_anywhere_inside_it() {
        let line = "see https://example.com/docs for more";
        let expected = Target::Url("https://example.com/docs".to_string());
        assert_eq!(find(line, 4).unwrap().target, expected);
        assert_eq!(find(line, 12).unwrap().target, expected);
        assert_eq!(find(line, 27).unwrap().target, expected);
    }

    #[test]
    fn trailing_prose_punctuation_is_not_part_of_the_url() {
        let detection = find("read https://example.com.", 10).unwrap();
        assert_eq!(
            detection.target,
            Target::Url("https://example.com".to_string())
        );
    }

    #[test]
    fn a_bracket_that_belongs_to_the_url_is_kept() {
        let line = "https://en.wikipedia.org/wiki/Rust_(programming_language)";
        assert_eq!(find(line, 5).unwrap().target, Target::Url(line.to_string()));
    }

    #[test]
    fn unknown_schemes_are_left_alone() {
        assert!(find("javascript:alert(1)", 2).is_none());
        assert!(find("myapp://do-something", 2).is_none());
    }

    #[test]
    fn whitespace_has_no_target() {
        assert!(find("a  b", 1).is_none());
    }

    #[test]
    fn an_existing_absolute_path_is_a_target() {
        let detection = find("ls /usr/share", 3).unwrap();
        assert_eq!(detection.target, Target::Path(PathBuf::from("/usr/share")));
    }

    #[test]
    fn a_path_that_does_not_exist_is_not_a_target() {
        assert!(find("/definitely/not/here.txt", 3).is_none());
    }

    #[test]
    fn a_line_and_column_suffix_is_stripped() {
        let directory = std::env::temp_dir();
        let file = directory.join("botttle-hyperlink-test.rs");
        std::fs::write(&file, b"x").expect("write");

        let line = format!("{}:42:9", file.display());
        let detection = detect(&chars(&line), 3, Some(&directory), None).expect("a path");
        assert_eq!(detection.target, Target::Path(file.clone()));

        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn relative_paths_resolve_against_the_working_directory() {
        let directory = std::env::temp_dir().join("botttle-hyperlink-relative");
        std::fs::create_dir_all(&directory).expect("dir");
        let file = directory.join("notes.md");
        std::fs::write(&file, b"x").expect("write");

        let detection = detect(&chars("open notes.md now"), 6, Some(&directory), None);
        assert_eq!(detection.unwrap().target, Target::Path(file));

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn an_explicit_link_is_judged_by_its_uri() {
        assert_eq!(
            from_uri("https://example.com/a"),
            Some(Target::Url("https://example.com/a".into()))
        );
        assert_eq!(from_uri("vscode://open?x"), None);
    }

    #[test]
    fn a_file_uri_becomes_a_decoded_path() {
        let directory = std::env::temp_dir().join("botttle hyperlink uri");
        std::fs::create_dir_all(&directory).expect("dir");
        let uri = format!(
            "file://localhost{}",
            directory.display().to_string().replace(' ', "%20")
        );
        assert_eq!(from_uri(&uri), Some(Target::Path(directory.clone())));
        assert_eq!(from_uri("file:///botttle/does/not/exist"), None);
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_bare_word_is_not_probed_as_a_path() {
        assert!(detect(&chars("running tests"), 2, Some(Path::new("/")), None).is_none());
    }
}
