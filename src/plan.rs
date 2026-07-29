//! Where a card belongs, and which paths are this program's own output.

use crate::card::{CardMeta, Route, DEST_FOLDERS};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Where a card belongs. `search`, when the card's full name contains it, adds one
/// more level so matches collect together; non-matching cards are unaffected.
pub fn destination_dir(root: &Path, meta: &CardMeta, search: Option<&str>) -> PathBuf {
    let (leaf, named) = match meta.route {
        Route::BySex => (meta.sex.folder().to_string(), true),
        Route::Fixed(f) => (f.to_string(), false),
    };
    let dir = root.join(meta.game.folder()).join(leaf);
    match search {
        Some(term) if named && contains_ignore_case(&meta.fullname(), term) => dir.join(term),
        _ => dir,
    }
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// Whether `file` sits inside a folder this program itself creates — the only
/// thing the scan skips.
///
/// The comparison is on the FIRST path component relative to the scan root, and it
/// is an equality test. The C# version instead asked whether the absolute path
/// CONTAINED any game name, with the `Unknown` sentinel in the list; that dropped
/// `…/Koikatu_F_20240626232405280_x/` and `…/Unknown god/card/` without a word.
pub fn is_in_dest_folder(root: &Path, file: &Path) -> bool {
    let Ok(rel) = file.strip_prefix(root) else {
        return false;
    };
    let Some(first) = rel.components().next() else {
        return false;
    };
    let first = first.as_os_str().to_string_lossy();
    DEST_FOLDERS.iter().any(|d| d.eq_ignore_ascii_case(&first))
}

/// `dir/file_name`, or `dir/stem(1).ext` etc. when that is taken.
///
/// The name is an `OsStr` from end to end. A card extracted from a Shift-JIS
/// archive can carry a file name that is not valid UTF-8 (the card data itself
/// isn't guaranteed to be either — see `msgpack.rs`), and running it through
/// `to_string_lossy` first would make a string full of U+FFFD the *actual*
/// rename target: the file would be renamed to a mangled name and the original
/// would be unrecoverable. Lossy conversion is for the printed line only.
pub fn free_name(dir: &Path, file_name: &OsStr) -> PathBuf {
    let direct = dir.join(file_name);
    if !direct.exists() {
        return direct;
    }
    let (stem, ext) = split_at_last_dot(file_name);
    let mut n = 1u32;
    loop {
        let mut name = stem.clone();
        name.push(format!("({n})"));
        name.push(&ext);
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Split at the LAST `.`, the dot staying with the extension; a name with no dot
/// is all stem. This is the same split the `&str` version did with
/// `rsplit_once('.')`, done on the platform's own encoding so no byte is lost —
/// not `Path::extension`, whose treatment of leading and trailing dots differs.
#[cfg(windows)]
fn split_at_last_dot(name: &OsStr) -> (OsString, OsString) {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let w: Vec<u16> = name.encode_wide().collect();
    match w.iter().rposition(|&c| c == '.' as u16) {
        Some(i) => (OsString::from_wide(&w[..i]), OsString::from_wide(&w[i..])),
        None => (name.to_os_string(), OsString::new()),
    }
}

#[cfg(unix)]
fn split_at_last_dot(name: &OsStr) -> (OsString, OsString) {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let b = name.as_bytes();
    match b.iter().rposition(|&c| c == b'.') {
        Some(i) => (
            OsString::from_vec(b[..i].to_vec()),
            OsString::from_vec(b[i..].to_vec()),
        ),
        None => (name.to_os_string(), OsString::new()),
    }
}

/// Neither Windows nor Unix: there is no lossless way to inspect the bytes, and
/// guessing with `to_string_lossy` is precisely the corruption this function
/// exists to avoid. Treat the whole name as the stem — `x.png(1)` is ugly but it
/// keeps every byte of the original.
#[cfg(not(any(windows, unix)))]
fn split_at_last_dot(name: &OsStr) -> (OsString, OsString) {
    (name.to_os_string(), OsString::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{CardMeta, Game, Route, Sex};
    use crate::tempdir::Dir;
    use std::path::Path;

    fn meta(game: Game, route: Route, sex: Sex, last: &str, first: &str) -> CardMeta {
        CardMeta {
            game,
            route,
            sex,
            lastname: last.into(),
            firstname: first.into(),
        }
    }

    #[test]
    fn a_female_character_card_goes_to_game_female() {
        let m = meta(Game::Koikatu, Route::BySex, Sex::Female, "a", "b");
        assert_eq!(
            destination_dir(Path::new("/r"), &m, None),
            Path::new("/r").join("Koikatu").join("Female")
        );
    }

    #[test]
    fn an_unknown_sex_still_gets_a_folder_rather_than_being_dropped() {
        let m = meta(Game::Koikatu, Route::BySex, Sex::Unknown, "a", "b");
        assert_eq!(
            destination_dir(Path::new("/r"), &m, None),
            Path::new("/r").join("Koikatu").join("Unknown")
        );
    }

    #[test]
    fn a_fixed_route_uses_its_own_leaf() {
        let m = meta(Game::Svc, Route::Fixed("Coordinate"), Sex::Unknown, "", "");
        assert_eq!(
            destination_dir(Path::new("/r"), &m, None),
            Path::new("/r").join("SVC").join("Coordinate")
        );
        let e = meta(Game::EmotionCreators, Route::Fixed("Character"), Sex::Unknown, "", "");
        assert_eq!(
            destination_dir(Path::new("/r"), &e, None),
            Path::new("/r").join("EmotionCreators").join("Character")
        );
    }

    /// The search term sorts matches into a subfolder. It does not filter the run:
    /// a card that does not match still goes to its ordinary folder.
    #[test]
    fn a_matching_search_term_adds_a_subfolder_and_a_non_match_does_not() {
        let m = meta(Game::Koikatu, Route::BySex, Sex::Female, "Asuna", "Yuuki");
        assert_eq!(
            destination_dir(Path::new("/r"), &m, Some("asuna")),
            Path::new("/r").join("Koikatu").join("Female").join("asuna")
        );
        assert_eq!(
            destination_dir(Path::new("/r"), &m, Some("kirito")),
            Path::new("/r").join("Koikatu").join("Female")
        );
    }

    #[test]
    fn a_fixed_route_ignores_the_search_term_because_it_has_no_name() {
        let m = meta(Game::Koikatu, Route::Fixed("Coordinate"), Sex::Unknown, "", "");
        assert_eq!(
            destination_dir(Path::new("/r"), &m, Some("asuna")),
            Path::new("/r").join("Koikatu").join("Coordinate")
        );
    }

    #[test]
    fn output_folders_directly_under_the_root_are_excluded() {
        let r = Path::new("/r");
        assert!(is_in_dest_folder(r, &r.join("Koikatu").join("Female").join("x.png")));
        assert!(is_in_dest_folder(r, &r.join("EmotionCreators").join("Character").join("x.png")));
    }

    /// THE regression. The C# version asked whether the ABSOLUTE path contained a
    /// game name, so a card pack named after a character — which is exactly how
    /// Koikatsu names its own exports — was silently skipped.
    #[test]
    fn a_card_pack_folder_named_like_a_koikatsu_export_is_not_excluded() {
        let r = Path::new("/r");
        let p = r
            .join("pack")
            .join("Koikatu_F_20240626232405280_x")
            .join("x.png");
        assert!(!is_in_dest_folder(r, &p));
    }

    /// The other half of the same bug: `Unknown` is a `Sex` value, not a top-level
    /// folder, so a folder merely named "Unknown god" must not be excluded.
    #[test]
    fn a_folder_named_unknown_something_is_not_excluded() {
        let r = Path::new("/r");
        let p = r.join("ISEEU").join("Genshin").join("Unknown god").join("card").join("x.png");
        assert!(!is_in_dest_folder(r, &p));
    }

    #[test]
    fn a_game_named_folder_below_the_first_level_is_not_an_output_folder() {
        let r = Path::new("/r");
        assert!(!is_in_dest_folder(r, &r.join("somewhere").join("Koikatu").join("x.png")));
    }

    #[test]
    fn a_path_outside_the_root_is_not_excluded() {
        assert!(!is_in_dest_folder(Path::new("/r"), Path::new("/other/Koikatu/x.png")));
    }

    #[test]
    fn a_free_name_is_returned_unchanged_when_nothing_is_there() {
        let d = Dir::new();
        assert_eq!(free_name(d.path(), OsStr::new("x.png")), d.path().join("x.png"));
    }

    #[test]
    fn a_taken_name_gains_a_counter() {
        let d = Dir::new();
        std::fs::write(d.path().join("x.png"), b"1").unwrap();
        assert_eq!(free_name(d.path(), OsStr::new("x.png")), d.path().join("x(1).png"));
        std::fs::write(d.path().join("x(1).png"), b"1").unwrap();
        assert_eq!(free_name(d.path(), OsStr::new("x.png")), d.path().join("x(2).png"));
    }

    #[test]
    fn a_name_without_an_extension_still_gets_a_counter() {
        let d = Dir::new();
        std::fs::write(d.path().join("x"), b"1").unwrap();
        assert_eq!(free_name(d.path(), OsStr::new("x")), d.path().join("x(1)"));
    }

    /// Build `<prefix>` + one byte/unit that cannot be UTF-8 + `<suffix>`, the
    /// shape of a card name that came out of a Shift-JIS archive. On Windows that
    /// is an unpaired surrogate (NTFS stores UTF-16 and does not require it to be
    /// well-formed); on Unix a lone 0xFF byte.
    #[cfg(windows)]
    fn non_utf8_name(prefix: &str, suffix: &str) -> OsString {
        use std::os::windows::ffi::OsStringExt;
        let mut w: Vec<u16> = prefix.encode_utf16().collect();
        w.push(0xD800); // unpaired high surrogate
        w.extend(suffix.encode_utf16());
        OsString::from_wide(&w)
    }

    #[cfg(unix)]
    fn non_utf8_name(prefix: &str, suffix: &str) -> OsString {
        use std::os::unix::ffi::OsStringExt;
        let mut b = prefix.as_bytes().to_vec();
        b.push(0xFF); // never valid in UTF-8
        b.extend_from_slice(suffix.as_bytes());
        OsString::from_vec(b)
    }

    #[cfg(not(any(windows, unix)))]
    fn non_utf8_name(_prefix: &str, _suffix: &str) -> OsString {
        OsString::new()
    }

    /// A name that is not valid UTF-8 must survive `free_name` byte for byte. The
    /// `&str` version took `to_string_lossy(&name)` and joined THAT onto the
    /// destination, so the U+FFFD replacement characters became the real target
    /// name and the original spelling was gone for good.
    #[test]
    fn a_non_utf8_file_name_keeps_its_exact_bytes_through_the_collision_path() {
        let d = Dir::new();
        let name = non_utf8_name("card", ".png");
        if name.is_empty() {
            eprintln!("note: skipping — no non-UTF-8 name construction on this platform");
            return;
        }
        assert!(
            name.to_string_lossy().contains('\u{FFFD}'),
            "the fixture must really be un-representable as UTF-8"
        );

        // Occupy the plain name so the counter path (the one that rebuilds the
        // name from pieces, where the lossy conversion used to happen) is taken.
        if std::fs::write(d.path().join(&name), b"1").is_err() {
            eprintln!(
                "note: skipping a_non_utf8_file_name_keeps_its_exact_bytes_through_the_collision_path \
                 — this filesystem refused to create a file with a non-UTF-8 name; free_name still \
                 takes an OsStr, just unverified by this run"
            );
            return;
        }

        let free = free_name(d.path(), &name);
        assert_eq!(
            free.file_name().expect("a file name"),
            non_utf8_name("card", "(1).png").as_os_str(),
            "the counter goes before the extension and every other unit is untouched"
        );
    }
}
