// SPDX-License-Identifier: MPL-2.0

//! The one thing QRnew remembers between runs: the theme.
//!
//! Not a settings system, and it should not become one. Everything else in the
//! window belongs to the *code being made* — text, colours, margin, inset — and
//! the next code is a different code. The theme is the only setting about the
//! person rather than the picture.
//!
//! Every operation is best-effort and silent. A read-only home, a sandbox, an
//! unset `$HOME`, a file edited into nonsense: the fallback is the default the
//! app would have used anyway, and the control that sets it is on screen.
//!
//! `read` and `write` are the only two things in QRnew that touch a path the
//! user did not choose in a dialog. Nothing is sent anywhere.

use std::path::PathBuf;

/// The file, and the format: `key = value`, one per line.
///
/// A line-based format for a single key so a second setting can be added
/// without older versions misreading the file: an unknown key is skipped by
/// [`read`] and a missing one is `None`.
const FILE: &str = "settings";

/// The reverse-DNS name the packaging already uses.
///
/// Taken from `resources/app.metainfo.xml` rather than invented here.
const APP_ID: &str = "dev.lhdjung.QRnew";

/// The short name, for the platforms whose convention is a plain directory.
const APP_DIR: &str = "qrnew";

/// The value stored under `key`, if there is one.
pub fn read(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(file()?).ok()?;
    value_of(&text, key).map(str::to_string)
}

/// Stores `value` under `key`, keeping whatever else the file holds.
///
/// The read-modify-write is not a transaction: two copies of QRnew writing the
/// same key at once leave one of the two answers, and both are a theme
/// somebody just asked for.
pub fn write(key: &str, value: &str) {
    let Some(path) = file() else { return };
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let mut lines: Vec<String> = existing
        .lines()
        .filter(|line| key_of(line) != Some(key))
        .map(str::to_string)
        .collect();
    lines.push(format!("{key} = {value}"));

    if let Some(directory) = path.parent()
        && std::fs::create_dir_all(directory).is_ok()
    {
        let _ = std::fs::write(&path, lines.join("\n") + "\n");
    }
}

/// The key a line sets, if it sets one.
fn key_of(line: &str) -> Option<&str> {
    let (key, _) = line.split_once('=')?;
    Some(key.trim())
}

/// The value `key` is given in `text`. The last one wins, which is what makes
/// [`write`]'s append safe.
fn value_of<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(name, _)| name.trim() == key)
        .map(|(_, value)| value.trim())
        .next_back()
}

/// Where the settings file lives, by the convention of the platform.
///
/// Hand-rolled rather than `dirs` or `directories`: QRnew's pitch is that you
/// can read its dependency list and see nothing in it can talk to a network,
/// and three crates to join two strings is a poor trade against that.
fn file() -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").filter(|home| !home.is_empty());

    let directory = if cfg!(target_os = "macos") {
        PathBuf::from(home()?)
            .join("Library/Application Support")
            .join(APP_ID)
    } else if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var_os("APPDATA").filter(|it| !it.is_empty())?).join(APP_ID)
    } else {
        // XDG, and its own fallback: the spec says an unset or relative
        // `XDG_CONFIG_HOME` is to be treated as absent.
        match std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
        {
            Some(config) => config.join(APP_DIR),
            None => PathBuf::from(home()?).join(".config").join(APP_DIR),
        }
    };

    Some(directory.join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_read_back_off_its_own_line() {
        let file = "theme = dark\n";
        assert_eq!(value_of(file, "theme"), Some("dark"));
        assert_eq!(value_of(file, "colour"), None);
    }

    /// Whitespace, blank lines and unknown keys are all things a file may
    /// contain, and none of them is a reason to give up on the rest of it.
    #[test]
    fn a_file_with_other_things_in_it_still_reads() {
        let file = "\n  window  =  wide  \n\ntheme=light\nnonsense\n";
        assert_eq!(value_of(file, "theme"), Some("light"));
        assert_eq!(value_of(file, "window"), Some("wide"));
        assert_eq!(value_of(file, "missing"), None);
    }

    /// The rule [`write`] leans on: it appends, so the file it produces must
    /// read as the value it appended.
    #[test]
    fn the_last_line_wins() {
        assert_eq!(value_of("theme = light\ntheme = dark\n", "theme"), Some("dark"));
    }

    #[test]
    fn a_line_says_which_key_it_sets() {
        assert_eq!(key_of("theme = dark"), Some("theme"));
        assert_eq!(key_of("  theme=dark"), Some("theme"));
        assert_eq!(key_of("nonsense"), None);
    }

    /// It is three different paths and only one exists on the machine running
    /// this, so the path is not asserted — but it has to *be* one, and it has
    /// to end at the file rather than the directory holding it.
    #[test]
    fn the_settings_file_is_somewhere_absolute() {
        let Some(path) = file() else {
            // A test runner with no `HOME` is a machine where the app would
            // simply not remember anything, which is allowed.
            return;
        };
        assert!(path.is_absolute(), "{path:?}");
        assert_eq!(path.file_name().and_then(|name| name.to_str()), Some(FILE));
        assert!(path.parent().is_some_and(|it| it.parent().is_some()), "{path:?}");
    }
}
