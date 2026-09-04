// SPDX-License-Identifier: MPL-2.0

//! One line per setting, and at the time of writing there is one: the
//! appearance — whether the window is light or dark.
//!
//! Not a settings system, and it should not become one. Everything in the
//! window belongs to the *code being made* — text, colours, margin, inset — and
//! the next code is a different code. The appearance is the only one of them
//! that is about the person rather than the picture.
//!
//! Saved *themes* are the other thing on disk, and they are [`crate::themes`]'
//! rather than a key here: a theme is a set of answers somebody named, not a
//! setting, and there are as many of them as they care to make.
//!
//! Every operation is best-effort and silent. A read-only home, a sandbox, an
//! unset `$HOME`, a file edited into nonsense: the fallback is the default the
//! app would have used anyway, and the control that sets it is on screen.
//!
//! `read` and `write`, and the same pair in [`crate::themes`], are the only
//! things in QRnew that touch a path the user did not choose in a dialog.
//! Nothing is sent anywhere.

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
/// same key at once leave one of the two answers, and both are an answer
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
///
/// Shared with [`crate::themes`], which files each saved look in the same
/// `key = value` lines for the same reason: it is a format a person can read
/// and edit, and it needs no crate to parse.
pub(crate) fn value_of<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(name, _)| name.trim() == key)
        .map(|(_, value)| value.trim())
        .next_back()
}

/// Where the settings file lives.
fn file() -> Option<PathBuf> {
    Some(dir()?.join(FILE))
}

/// The directory the app keeps its own files in, by the convention of the
/// platform. [`crate::themes`] hangs its own folder off this one.
///
/// Hand-rolled rather than `dirs` or `directories`: QRnew's pitch is that you
/// can read its dependency list and see nothing in it can talk to a network,
/// and three crates to join two strings is a poor trade against that.
pub(crate) fn dir() -> Option<PathBuf> {
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

    Some(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_read_back_off_its_own_line() {
        let file = "appearance = dark\n";
        assert_eq!(value_of(file, "appearance"), Some("dark"));
        assert_eq!(value_of(file, "colour"), None);
    }

    /// Whitespace, blank lines and unknown keys are all things a file may
    /// contain, and none of them is a reason to give up on the rest of it.
    #[test]
    fn a_file_with_other_things_in_it_still_reads() {
        let file = "\n  window  =  wide  \n\nappearance=light\nnonsense\n";
        assert_eq!(value_of(file, "appearance"), Some("light"));
        assert_eq!(value_of(file, "window"), Some("wide"));
        assert_eq!(value_of(file, "missing"), None);
    }

    /// The rule [`write`] leans on: it appends, so the file it produces must
    /// read as the value it appended.
    #[test]
    fn the_last_line_wins() {
        assert_eq!(value_of("appearance = light\nappearance = dark\n", "appearance"), Some("dark"));
    }

    #[test]
    fn a_line_says_which_key_it_sets() {
        assert_eq!(key_of("appearance = dark"), Some("appearance"));
        assert_eq!(key_of("  appearance=dark"), Some("appearance"));
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
