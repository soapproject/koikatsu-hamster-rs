//! Where a card belongs, and which paths are this program's own output.

use crate::card::{CardMeta, Route, DEST_FOLDERS};
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
pub fn free_name(dir: &Path, file_name: &str) -> PathBuf {
    let direct = dir.join(file_name);
    if !direct.exists() {
        return direct;
    }
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (file_name.to_string(), String::new()),
    };
    let mut n = 1u32;
    loop {
        let candidate = dir.join(format!("{stem}({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
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
        assert_eq!(free_name(d.path(), "x.png"), d.path().join("x.png"));
    }

    #[test]
    fn a_taken_name_gains_a_counter() {
        let d = Dir::new();
        std::fs::write(d.path().join("x.png"), b"1").unwrap();
        assert_eq!(free_name(d.path(), "x.png"), d.path().join("x(1).png"));
        std::fs::write(d.path().join("x(1).png"), b"1").unwrap();
        assert_eq!(free_name(d.path(), "x.png"), d.path().join("x(2).png"));
    }

    #[test]
    fn a_name_without_an_extension_still_gets_a_counter() {
        let d = Dir::new();
        std::fs::write(d.path().join("x"), b"1").unwrap();
        assert_eq!(free_name(d.path(), "x"), d.path().join("x(1)"));
    }
}
