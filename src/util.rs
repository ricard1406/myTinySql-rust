//! The [`SqlError`] type and small string helpers used everywhere.

use std::fmt;

/// A problem with one statement: bad syntax, unknown table, value too long...
///
/// C++ throws it as an exception; Rust returns it in an `Err`, and the `?` operator passes it on.
/// [`Session::execute`](crate::session::Session::execute) turns it into a failed result and carries on
/// with the next statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlError(pub String);

impl SqlError {
    /// Makes an error from a message: `SqlError::new("unknown table")` or `SqlError::new(format!(...))`.
    pub fn new(message: impl Into<String>) -> Self {
        SqlError(message.into())
    }
}

/// `{}` on an SqlError prints its message, as `e.what()` does in C++.
impl fmt::Display for SqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SqlError {}

/// What a function that can fail with an [`SqlError`] returns: `Ok(value)` or `Err(error)`.
pub type SqlResult<T> = Result<T, SqlError>;

/// The characters [`trim`] and [`trim_right`] remove: space, tab, carriage return and line feed.
///
/// Rust's own `str::trim()` removes every Unicode space, so the C++ list is spelled out.
const SPACES: [char; 4] = [' ', '\t', '\r', '\n'];

/// Removes trailing spaces, tabs and line ends.
///
/// Returns a slice of `text`, like `std::string_view` in C++: nothing is copied.
pub fn trim_right(text: &str) -> &str {
    text.trim_end_matches(SPACES)
}

/// Removes leading and trailing spaces, tabs and line ends; nothing is copied.
pub fn trim(text: &str) -> &str {
    text.trim_matches(SPACES)
}

/// Tells whether a line holds only spaces, tabs and line ends (or nothing).
pub fn is_blank(line: &[u8]) -> bool {
    line.iter().all(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n'))
}

/// Turns bytes read from a file into text. Invalid UTF-8 becomes `�` instead of stopping the program.
pub fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Matches text against an SQL `LIKE` pattern, ignoring the case of ASCII letters.
///
/// In the pattern, `%` stands for any text (also none) and `_` for exactly one character;
/// every other character must match itself. For example `%fran%` matches "Francesco" and "FRANCO",
/// and `_uigi` matches "Luigi". Characters are counted in bytes, so `_` does not match an accented
/// letter such as `ò`, which takes 2 bytes in UTF-8; use `%` there.
pub fn matches_like(text: &str, pattern: &str) -> bool {
    let (text, pattern) = (text.as_bytes(), pattern.as_bytes());
    let (mut t, mut p) = (0, 0);
    // Where the last `%` was, and where in the text it started matching: on a mismatch we come
    // back there and let that `%` swallow one more character.
    let mut last_percent: Option<usize> = None;
    let mut resume_text = 0;

    while t < text.len() {
        match pattern.get(p) {
            Some(b'%') => {
                last_percent = Some(p);
                p += 1;
                resume_text = t;
            }
            Some(&c) if c == b'_' || c.eq_ignore_ascii_case(&text[t]) => {
                p += 1;
                t += 1;
            }
            _ => match last_percent {
                Some(percent) => {
                    p = percent + 1;
                    resume_text += 1;
                    t = resume_text;
                }
                None => return false,
            },
        }
    }
    while pattern.get(p) == Some(&b'%') {
        p += 1; // a trailing `%` also matches the empty rest
    }
    p == pattern.len()
}

/// Parses a whole string as a base-10 integer.
///
/// Surrounding whitespace and one leading `+` are allowed. Returns `None` if `text` is not exactly
/// one integer (`std::nullopt` in C++).
pub fn parse_integer(text: &str) -> Option<i64> {
    let text = trim(text);
    let text = text.strip_prefix('+').unwrap_or(text);
    if text.starts_with('+') {
        return None; // `str::parse` would accept "+5", so "++5" must be refused here
    }
    text.parse().ok()
}

// Unit tests live next to the code they test, and run with `cargo test`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_only_ascii_whitespace() {
        assert_eq!(trim("  ab c \t\r\n"), "ab c");
        assert_eq!(trim_right("  ab  "), "  ab");
        assert_eq!(trim("\u{a0}x\u{a0}"), "\u{a0}x\u{a0}"); // a no-break space stays, as in C++
    }

    #[test]
    fn like_patterns() {
        assert!(matches_like("Francesco", "%fran%"));
        assert!(matches_like("FRANCO", "%fran%"));
        assert!(matches_like("Luigi", "_uigi"));
        assert!(matches_like("Torino", "tor%"));
        assert!(!matches_like("Casella", "%a%a%a%"));
        assert!(matches_like("Banana", "%a%a%a%"));
        assert!(matches_like("", "%"));
        assert!(!matches_like("Roma", "Rom"));
    }

    #[test]
    fn integers() {
        assert_eq!(parse_integer(" 42 "), Some(42));
        assert_eq!(parse_integer("+7"), Some(7));
        assert_eq!(parse_integer("-3"), Some(-3));
        assert_eq!(parse_integer("++7"), None);
        assert_eq!(parse_integer("4 2"), None);
        assert_eq!(parse_integer(""), None);
        assert_eq!(parse_integer("99999999999999999999"), None);
    }
}
