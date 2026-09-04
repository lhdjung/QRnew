// SPDX-License-Identifier: MPL-2.0

//! Saved themes: the two colours, and the picture in the middle of the code
//! and how big it should be drawn. A theme is applied in two clicks and leaves
//! the text alone, which is the point — the same house style over a different
//! code.
//!
//! Not to be confused with [`crate::ui::Appearance`], which is whether the
//! *window* is light or dark. That is a setting; this is a set of answers
//! somebody named, and there are as many as they care to make.
//!
//! # One folder per theme
//!
//! ```text
//! themes/qrnew-theme-uni-bern/settings.toml         name, and what it changes
//! themes/qrnew-theme-uni-bern/uni-bern-logo.png     the picture, under its own name
//! ```
//!
//! **A folder rather than one file among many**, because the picture is a
//! second file and two themes are entitled to a picture called `logo.png`. The
//! folder is named after the theme, flattened by [`slug`]; the name a person
//! sees lives inside the file, so it can hold anything.
//!
//! **The folder is the theme, and the two names say so.** A theme is imported,
//! exported and shared as a folder, and the folder is named to survive that: a
//! `qrnew-theme-` prefix means the thing sitting in somebody's Downloads says
//! what it is and what opens it, so copying one out needs no renaming. The
//! file inside is `settings.toml` rather than `theme.toml` for the other half
//! of the same point — a file called `theme.toml` reads as *the* theme, and
//! then the picture beside it looks like something else.
//!
//! # The file is TOML
//!
//! Real TOML — eight keys, `margin` a bare integer and the other seven quoted
//! basic strings — written and read here by hand rather than by a crate. The
//! point of the format is that a person or a language model can write one, and
//! that anything that reads TOML reads these; the point of not taking the
//! dependency is that QRnew's pitch is a dependency list you can read in a
//! minute. `the_file_is_valid_toml` holds the first half by parsing what
//! [`save`] writes with a real parser, from a dev-dependency that ships in
//! nothing.
//!
//! What is *not* claimed is the other direction: this reads the subset it
//! writes. One key per line, `key = "value"` — or `margin = 4`. A hand-written
//! file that spreads a value over three lines is TOML this module will not
//! understand, and the theme is skipped rather than half-read.
//!
//! Everything here is best-effort and silent, like [`crate::settings`]: an
//! unwritable home is an app with no themes, and the controls that make them
//! say so by being absent.

use std::path::{Path, PathBuf};

use qrnew_core::Rgb;

use crate::settings;

/// The file inside a theme's folder that holds everything but the picture.
const FILE: &str = "settings.toml";

/// What every theme folder's name starts with, so that one out on its own —
/// downloaded, mailed, dropped on a desktop — says what it is.
const FOLDER: &str = "qrnew-theme-";

/// The longest a folder name is allowed to get, in characters. A theme named
/// with a paragraph is still a theme; a path component of a thousand
/// characters is a write that fails on most filesystems.
const SLUG_MAX: usize = 64;

/// One saved theme.
///
/// **Every setting is a `None` the theme has no opinion about**, which is the
/// app's own default and an absent key in the file — so a theme holds what it
/// changes and nothing else. Writing `background = "#ffffff"` into a file says
/// exactly as much as leaving it out, and costs a line somebody has to read.
///
/// Applying one still sets all eight: `None` means *the default*, not *leave it
/// alone*. Two themes in a row cannot leave a mark from the first inside the
/// second, which is the same rule that makes a theme with no picture take the
/// picture away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// What it is called, as it was typed — the folder is a flattened copy.
    /// The one thing a theme cannot leave out, and so the one thing that
    /// decides whether a folder holds a theme at all.
    pub name: String,
    /// The code's own marks. Not "dark": [`crate::ui`] calls these two
    /// Foreground and Background, and a code drawn light on dark is a thing
    /// this app makes on purpose and reads back.
    pub foreground: Option<Rgb>,
    pub background: Option<Rgb>,
    /// The picture, as the name its file goes by and the bytes in it.
    pub image_file: Option<(String, Vec<u8>)>,
    /// How big the picture should be drawn, as `InsetSize::slug`'s own word.
    ///
    /// **A preference rather than an instruction.** The largest size does not
    /// fit a short code — a logo has to clear the finder patterns, which are a
    /// fixed number of modules in from each edge — so the app asks for this and
    /// draws the largest that fits. See `Drawn::capped`.
    ///
    /// A string rather than the enum because that type is the interface's, and
    /// an unknown word here is a file somebody edited rather than an error
    /// worth a type.
    pub image_size: Option<String>,
    /// The rest of the code's settings, each as the word the control files it
    /// under: `ErrorCorrection`'s level and `Look`'s own name.
    ///
    /// Strings for [`Self::image_size`]'s reason — the types are the
    /// interface's — and because an unknown word is a hand-edited file rather
    /// than an error worth a type. [`crate::ui`] reads each one and falls back
    /// to the default the app would have used anyway.
    ///
    /// **Error correction is a preference and nothing more.** A picture in the
    /// middle of a code needs 30%, so a theme that carries both gets the
    /// picture's answer; the row in the window says so.
    pub error_correction: Option<String>,
    pub shape: Option<String>,
    /// How wide the quiet zone is, in modules. The one value in the file that
    /// is a number, and written as one.
    pub margin: Option<u32>,
}

impl Theme {
    /// The code's own two colours as this theme draws them: its own where it
    /// has one, the app's own where it has none. A mark on its own ground,
    /// which is what a code is.
    ///
    /// [`crate::ui`] both applies a theme and draws a swatch of it, and the two
    /// have to agree about a theme that says nothing about either.
    pub fn mark(&self) -> Rgb {
        self.foreground.unwrap_or(Rgb::BLACK)
    }

    pub fn ground(&self) -> Rgb {
        self.background.unwrap_or(Rgb::WHITE)
    }
}

/// Why a folder somebody picked is not a theme.
///
/// Two rather than one, because they are two different things to do about it:
/// the first is the wrong folder, the second is a file to fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotATheme {
    /// Nothing in the folder called [`FILE`], or nothing readable.
    NoFile,
    /// A settings file that never says what the theme is called.
    NoName,
}

/// Where themes live, or `None` on a machine with nowhere to put them.
pub fn dir() -> Option<PathBuf> {
    Some(settings::dir()?.join("themes"))
}

/// Every theme in `dir`, by name. A folder that will not parse is skipped
/// rather than reported: the rest of them still work.
pub fn list(dir: &Path) -> Vec<Theme> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut themes: Vec<Theme> = entries
        .flatten()
        .filter_map(|entry| load(&entry.path()).ok())
        .collect();
    themes.sort_by_key(|theme| theme.name.to_lowercase());
    themes
}

/// Writes `theme` into `dir`, replacing any theme of the same name.
pub fn save(dir: &Path, theme: &Theme) {
    let folder = dir.join(folder_for(&theme.name));
    if std::fs::create_dir_all(&folder).is_err() {
        return;
    }
    // A replaced theme must not keep the old picture beside the new one:
    // nothing would point at it, and it would sit there forever.
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            if entry.file_name() != FILE {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    let mut file = String::new();
    // The value arrives rendered, because one of the eight is not a string.
    let mut put = |key: &str, value: &str| {
        file.push_str(key);
        file.push_str(" = ");
        file.push_str(value);
        file.push('\n');
    };
    put("name", &quote(&theme.name));
    // Everything below is written only when the theme has something to say
    // about it — see the note on [`Theme`].
    if let Some(colour) = theme.foreground {
        put("foreground", &quote(&colour.to_hex()));
    }
    if let Some(colour) = theme.background {
        put("background", &quote(&colour.to_hex()));
    }
    // Named only once the file is actually there, so a theme never points at a
    // picture that failed to write.
    if let Some((name, bytes)) = &theme.image_file
        && bare(name)
        && std::fs::write(folder.join(name), bytes).is_ok()
    {
        put("image_file", &quote(name));
    }
    if let Some(size) = &theme.image_size {
        put("image_size", &quote(size));
    }
    if let Some(level) = &theme.error_correction {
        put("error_correction", &quote(level));
    }
    if let Some(margin) = theme.margin {
        put("margin", &margin.to_string());
    }
    if let Some(shape) = &theme.shape {
        put("shape", &quote(shape));
    }

    let _ = std::fs::write(folder.join(FILE), file);
}

/// Takes the theme called `name` away, picture and all.
pub fn remove(dir: &Path, name: &str) {
    // The only recursive delete in the app, and what makes it safe is that the
    // path is built rather than accepted: it starts with [`FOLDER`] and [`slug`]
    // cannot return a separator or a dot, so this can only ever name a folder
    // directly inside `dir`.
    let _ = std::fs::remove_dir_all(dir.join(folder_for(name)));
}

/// Takes a theme folder somebody else made and files it with the rest.
///
/// The same folder [`save`] writes — a [`FILE`] and the picture beside it — so
/// importing is reading one and writing it back where the app keeps its own.
pub fn import(dir: &Path, folder: &Path) -> Result<(), NotATheme> {
    let theme = load(folder)?;
    save(dir, &theme);
    Ok(())
}

/// The theme in one folder, or why there is not one.
fn load(folder: &Path) -> Result<Theme, NotATheme> {
    let text = std::fs::read_to_string(folder.join(FILE)).map_err(|_| NotATheme::NoFile)?;
    // `settings`' own splitter, which takes the last line to set a key and
    // trims around the `=`. What is peculiar to TOML is the quoting, and that
    // is `unquote`'s.
    let field = |key: &str| settings::value_of(&text, key).and_then(unquote);

    Ok(Theme {
        name: field("name")
            .filter(|name| !name.trim().is_empty())
            .ok_or(NotATheme::NoName)?,
        foreground: field("foreground").and_then(|hex| crate::ui::parse_hex(&hex)),
        background: field("background").and_then(|hex| crate::ui::parse_hex(&hex)),
        // **The one value from the file that becomes a path.** A file somebody
        // edited could say `../../../etc/passwd`, so it has to be a bare name
        // before it is joined onto anything.
        image_file: field("image_file")
            .filter(|name| bare(name))
            .and_then(|name| Some((name.clone(), std::fs::read(folder.join(name)).ok()?))),
        // Absent — or a word this app does not know, from a file somebody
        // edited — is the app's own default, which is what [`crate::ui`] makes
        // of a `None` and of an unknown word alike.
        image_size: field("image_size"),
        error_correction: field("error_correction"),
        shape: field("shape"),
        // The one value that is not a string, so it does not go through
        // `unquote`.
        margin: settings::value_of(&text, "margin").and_then(|value| value.parse().ok()),
    })
}

/// The folder a theme called `name` lives in: [`FOLDER`] and a flattened copy
/// of the name.
fn folder_for(name: &str) -> String {
    format!("{FOLDER}{}", slug(name))
}

/// A string as TOML writes one: a basic string, in double quotes.
///
/// Backslash and quote are escaped, and so is every control character, because
/// TOML forbids a raw one inside a basic string and a name is whatever somebody
/// pasted into the field.
fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '\\' => out.push_str(r"\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            control if control.is_control() => {
                out.push_str(&format!("\\u{:04X}", control as u32));
            }
            plain => out.push(plain),
        }
    }
    out.push('"');
    out
}

/// The other direction, over the subset [`quote`] writes.
///
/// `None` for anything that is not one complete basic string, which is a line
/// this module did not write and will not guess at.
fn unquote(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            // An unescaped quote would have ended the string, so a line with
            // one in the middle is not one string.
            if character == '"' {
                return None;
            }
            out.push(character);
            continue;
        }
        match characters.next()? {
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'u' => out.push(scalar(&mut characters, 4)?),
            'U' => out.push(scalar(&mut characters, 8)?),
            _ => return None,
        }
    }
    Some(out)
}

/// The character `digits` hex digits off `characters` spell.
fn scalar(characters: &mut std::str::Chars<'_>, digits: usize) -> Option<char> {
    let mut value = 0u32;
    for _ in 0..digits {
        value = value * 16 + characters.next()?.to_digit(16)?;
    }
    char::from_u32(value)
}

/// Whether `name` names a file in one directory and nothing else.
fn bare(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != ".."
}

/// A name, flattened into something that is certainly one path component.
///
/// Letters and digits survive, lowercased; everything else becomes a hyphen,
/// and runs of them collapse. A name with nothing alphanumeric in it at all
/// lands on `theme`.
///
/// ponytail: two names that flatten the same way are one theme, and saving the
/// second replaces the first. Give the folder a counted suffix if that ever
/// bites; nothing else here would have to change, because the name a person
/// sees is inside the file rather than in the folder's name.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for character in name.chars().take(SLUG_MAX) {
        if character.is_alphanumeric() {
            out.extend(character.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "theme".to_string()
    } else {
        out.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("qrnew-themes-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    fn uni_bern() -> Theme {
        Theme {
            name: "Uni Bern".to_string(),
            foreground: Some(Rgb::new(0xe4, 0x00, 0x46)),
            background: Some(Rgb::WHITE),
            image_file: Some(("logo.png".to_string(), vec![1, 2, 3, 4])),
            image_size: Some("large".to_string()),
            error_correction: Some("quartile".to_string()),
            shape: Some("dots".to_string()),
            margin: Some(4),
        }
    }

    /// The whole of what this module is for: a look goes out to disk, comes
    /// back the same, and can be taken away again.
    #[test]
    fn a_theme_survives_the_round_trip() {
        let dir = scratch("round-trip");
        assert_eq!(list(&dir), vec![], "nothing saved, nothing listed");

        save(&dir, &uni_bern());
        assert_eq!(list(&dir), vec![uni_bern()]);

        // The picture is a file of its own, under the name it arrived with,
        // in a folder named so it says what it is wherever it ends up.
        assert!(dir.join("qrnew-theme-uni-bern/logo.png").exists());

        remove(&dir, "Uni Bern");
        assert_eq!(list(&dir), vec![]);
    }

    /// **A theme holds what it changes, and nothing else.** Everything it has
    /// no opinion about is an absent key, which comes back as the `None` the
    /// interface reads as its own default — so the file is as short as the
    /// theme is small, and a line in it is a line worth reading.
    #[test]
    fn a_theme_with_no_opinions_is_one_line() {
        let dir = scratch("bare");
        let bare = Theme {
            name: "Bare".to_string(),
            foreground: None,
            background: None,
            image_file: None,
            image_size: None,
            error_correction: None,
            shape: None,
            margin: None,
        };
        save(&dir, &bare);

        assert_eq!(
            std::fs::read_to_string(dir.join("qrnew-theme-bare").join(FILE)).unwrap(),
            "name = \"Bare\"\n"
        );
        assert_eq!(list(&dir), vec![bare]);
    }

    /// **What this module writes is TOML**, checked by something that only
    /// knows TOML rather than by reading it back through [`unquote`] — which
    /// would agree with any private format the two of them made up together.
    ///
    /// The name is deliberately hostile: a quote, a backslash, a newline, a
    /// tab, a control character and something outside the Latin alphabet.
    #[test]
    fn the_file_is_valid_toml() {
        let dir = scratch("toml");
        let awkward = "He said \"hi\"\\\n\ttab\u{7}bell 日本語";
        save(
            &dir,
            &Theme {
                name: awkward.to_string(),
                ..uni_bern()
            },
        );

        let folder = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        let written = std::fs::read_to_string(folder.join(FILE)).unwrap();
        let parsed = toml::de::DeTable::parse(&written).expect("what we write is TOML");
        let field = |key: &str| {
            parsed.get_ref()[key]
                .get_ref()
                .as_str()
                .expect("every value but the margin is a string")
                .to_string()
        };
        assert_eq!(field("name"), awkward);
        assert_eq!(field("foreground"), "#e40046");
        assert_eq!(field("background"), "#ffffff");
        assert_eq!(field("image_file"), "logo.png");
        assert_eq!(field("image_size"), "large");
        assert_eq!(field("error_correction"), "quartile");
        assert_eq!(field("shape"), "dots");
        // A number rather than a string, which is the whole point of it being
        // the one key written without quotes.
        assert_eq!(
            parsed.get_ref()["margin"]
                .get_ref()
                .as_integer()
                .map(|margin| margin.as_str()),
            Some("4"),
            "the margin is a TOML integer"
        );

        // And the same name survives our own way back in.
        assert_eq!(list(&dir)[0].name, awkward);
    }

    /// Saving over a name replaces it rather than making a second one — and
    /// the picture that is no longer named goes with it.
    #[test]
    fn saving_the_same_name_twice_leaves_one_theme() {
        let dir = scratch("replace");
        save(&dir, &uni_bern());
        save(
            &dir,
            &Theme {
                image_file: Some(("mark.svg".to_string(), b"<svg/>".to_vec())),
                ..uni_bern()
            },
        );

        let saved = list(&dir);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].image_file.as_ref().unwrap().0, "mark.svg");
        assert!(
            !dir.join("qrnew-theme-uni-bern/logo.png").exists(),
            "the old picture is gone"
        );
    }

    /// A folder somebody else made — the same shape [`save`] writes — is read
    /// and filed with the rest, picture and all.
    #[test]
    fn a_theme_can_be_imported_from_a_folder() {
        let elsewhere = scratch("import-source");
        save(&elsewhere, &uni_bern());

        let dir = scratch("import");
        assert_eq!(import(&dir, &elsewhere.join("qrnew-theme-uni-bern")), Ok(()));
        assert_eq!(list(&dir), vec![uni_bern()]);
        assert!(dir.join("qrnew-theme-uni-bern/logo.png").exists());

        // A folder with no theme in it is refused rather than half-imported,
        // and says which of the two things is wrong with it.
        assert_eq!(import(&dir, &elsewhere), Err(NotATheme::NoFile));
        std::fs::write(elsewhere.join(FILE), "shape = \"dots\"\n").unwrap();
        assert_eq!(import(&dir, &elsewhere), Err(NotATheme::NoName));
        assert_eq!(list(&dir).len(), 1);
    }

    /// A hand-edited file pointing the picture out of its own folder is
    /// ignored, and the theme around it still loads.
    #[test]
    fn an_image_cannot_point_outside_its_folder() {
        let dir = scratch("escape");
        save(&dir, &uni_bern());
        std::fs::write(
            dir.join("qrnew-theme-uni-bern").join(FILE),
            "name = \"Uni Bern\"\nforeground = \"#e40046\"\nbackground = \"#ffffff\"\n\
             image_file = \"../../../etc/passwd\"\nimage_size = \"large\"\n",
        )
        .unwrap();

        let saved = list(&dir);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].image_file, None);
        assert!(!bare("../x"));
        assert!(!bare(".."));
        assert!(bare("logo.png"));
    }

    /// A value that is not one quoted string is a line this module did not
    /// write, and it is refused rather than half-read.
    #[test]
    fn only_a_complete_basic_string_is_a_value() {
        assert_eq!(unquote(r#""plain""#).as_deref(), Some("plain"));
        assert_eq!(unquote(r#""a \"b\" c""#).as_deref(), Some("a \"b\" c"));
        assert_eq!(unquote(r#""é""#).as_deref(), Some("é"));
        assert_eq!(unquote("\"\"").as_deref(), Some(""));
        assert_eq!(unquote("bare"), None);
        assert_eq!(unquote(r#""unclosed"#), None);
        assert_eq!(unquote(r#""a" and "b""#), None);
        assert_eq!(unquote(r#""bad \q escape""#), None);
    }

    #[test]
    fn a_name_becomes_one_path_component() {
        assert_eq!(folder_for("Uni Bern"), "qrnew-theme-uni-bern");
        assert_eq!(slug("Uni Bern"), "uni-bern");
        assert_eq!(slug("  ../Uni//Bern!  "), "uni-bern");
        assert_eq!(slug("日本語"), "日本語");
        assert_eq!(slug("***"), "theme");
        assert_eq!(slug(""), "theme");
        assert!(slug(&"x".repeat(500)).chars().count() <= SLUG_MAX);
    }
}
