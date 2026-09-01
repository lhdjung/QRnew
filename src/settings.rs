// SPDX-License-Identifier: MPL-2.0

//! The one thing QRnew remembers between runs.
//!
//! A theme somebody picked and the app forgot on the way out is not a theme
//! they picked; it is a theme they will pick again tomorrow. So the choice
//! goes to a file, and the file is the whole of QRnew's state on disk.
//!
//! # What this is not
//!
//! It is not a settings system, and it should not grow into one by accident.
//! Everything else in the window is a property of the *code being made* — the
//! text, the colours, the margin, the inset — and none of that belongs to the
//! app between runs: the next code is a different code. The theme is the only
//! setting here that is about the person rather than the picture, which is why
//! it is the only one saved.
//!
//! It is also not a reason to re-read the privacy claim. Nothing is sent
//! anywhere; a word is written to the directory the platform keeps for exactly
//! this, and the app reads it back. `read` and `write` are the only two things
//! in QRnew that touch a path the user did not choose in a dialog.
//!
//! # Failure is not an error
//!
//! Every operation here is best-effort and silent. A read-only home, a
//! sandbox, a `$HOME` that is not set, a file somebody has edited into
//! nonsense — none of that is worth a message, because the fallback is the
//! default the app would have used anyway and the person is standing in front
//! of the control that sets it. An app that cannot start because it could not
//! remember a colour scheme has its priorities wrong.

use std::path::PathBuf;

/// The file, and the format: `key = value`, one per line.
///
/// A whole line-based format for a single key is not over-building so much as
/// declining to build twice. The alternative — a file whose entire contents
/// are the word `dark` — cannot gain a second setting without every older
/// version of the app misreading the newer file, and this can: an unknown key
/// is skipped by [`read`] and a missing one is `None`.
const FILE: &str = "settings";

/// The reverse-DNS name the packaging already uses.
///
/// Taken from `resources/app.metainfo.xml` rather than invented here, so that
/// the directory macOS makes for this app is the one the desktop entry, the
/// icon and the metainfo all agree it is called.
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
/// The read-modify-write is not a transaction and does not try to be. Two
/// copies of QRnew writing the same key at the same moment would leave one of
/// the two answers in the file, and both answers are a theme somebody just
/// asked for.
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

/// The value `key` is given in `text`.
///
/// The last one wins, which is the rule that makes [`write`] safe to be as
/// simple as it is: a file that somehow ends up with the key twice reads as
/// whatever was written most recently.
fn value_of<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(name, _)| name.trim() == key)
        .map(|(_, value)| value.trim())
        .next_back()
}

/// Where the settings file lives, by the convention of the platform.
///
/// Hand-rolled rather than taken from `dirs` or `directories`, and the reason
/// is the one that decides most of this app's dependency questions: QRnew's
/// pitch is that you can read its dependency list and see that nothing in it
/// can talk to a network. Three crates to join two strings is a poor trade
/// against that, and these are three rules that have not moved in a decade.
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

    /// Whitespace, blank lines and keys this version has never heard of are
    /// all things a file may contain, and none of them is a reason to give up
    /// on the rest of it.
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

    /// The path is not asserted — it is three different paths and only one of
    /// them exists on the machine running this — but it has to *be* one, and
    /// it has to end at the file rather than the directory holding it.
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
