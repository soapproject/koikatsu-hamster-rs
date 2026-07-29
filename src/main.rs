mod card;
mod msgpack;
mod plan;
mod png;
mod walk;
#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tempdir;

use crate::card::{read_card, CardError};
use crate::plan::{destination_dir, free_name, is_already_filed};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

const VERSION_BANNER: &str = "koikatsu-hamster 0.1.0 (rust)";

const USAGE: &str = "\
usage: koikatsu-hamster [--root <dir>] [--dry-run] [search term]

  --root <dir>   directory to organise (default: the current directory)
  --dry-run      report what would move, change nothing
  search term    cards whose full name contains it are filed one level deeper
";

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub root: Option<PathBuf>,
    pub dry_run: bool,
    pub search: Option<String>,
}

impl Args {
    pub fn parse(argv: &[String]) -> Result<Args, String> {
        let mut a = Args::default();
        let mut i = 0;
        while i < argv.len() {
            match argv[i].as_str() {
                "--dry-run" => a.dry_run = true,
                "--root" => {
                    i += 1;
                    // A value that itself looks like a flag (e.g. `--root --dry-run`)
                    // is rejected the same way a missing value is: silently treating
                    // it as the root would send the walk into a directory that
                    // doesn't exist and no-op the whole run without a word.
                    let v = argv
                        .get(i)
                        .filter(|v| !v.starts_with("--"))
                        .ok_or("--root needs a directory")?;
                    a.root = Some(PathBuf::from(v));
                }
                other if other.starts_with("--") => {
                    return Err(format!("unknown option {other}"));
                }
                other => {
                    if a.search.is_some() {
                        return Err("only one search term is accepted".into());
                    }
                    a.search = Some(other.to_string());
                }
            }
            i += 1;
        }
        Ok(a)
    }
}

#[derive(Debug, Default)]
pub struct Report {
    /// (destination relative to root, count), in first-seen order.
    pub moved: Vec<(String, u64)>,
    pub scenes: u64,
    pub non_cards: u64,
    pub unrecognized: u64,
    /// Cards found already sitting in the folder they would be moved to. Counted
    /// rather than passed over in silence: a file the run touched and did nothing
    /// about is exactly what the summary must not hide.
    pub already_filed: u64,
    pub errors: u64,
}

impl Report {
    fn record(&mut self, dest: String) {
        if let Some(e) = self.moved.iter_mut().find(|(d, _)| *d == dest) {
            e.1 += 1;
        } else {
            self.moved.push((dest, 1));
        }
    }
}

pub fn run(root: &Path, dry_run: bool, search: Option<&str>, out: &mut dyn Write) -> Report {
    let mut rep = Report::default();
    for file in walk::candidates(root) {
        let meta = match read_card(&file) {
            Ok(m) => m,
            Err(CardError::NotCard) => {
                rep.non_cards += 1;
                continue;
            }
            Err(CardError::Scene(_)) => {
                rep.scenes += 1;
                continue;
            }
            Err(e @ CardError::Unrecognized(_)) => {
                rep.unrecognized += 1;
                let _ = writeln!(out, "Skipped {}: {}", file.display(), e.reason());
                continue;
            }
            // Io and Malformed. An unreadable file is reported and counted as an
            // error — never folded into `non_cards`, which would call a card the
            // user cannot open a texture and still exit 0.
            Err(e) => {
                rep.errors += 1;
                let _ = writeln!(out, "Failed to handle {}: {}", file.display(), e.reason());
                continue;
            }
        };

        let dir = destination_dir(root, &meta, search);

        // Never move a card onto itself. The exclusion rule is meant to make this
        // unreachable, but it is a rule about folder names and can miss — the two
        // lists in card.rs drifting apart, or a root chosen inside an already
        // organised tree. When it misses, free_name sees the card occupying its
        // own destination name and returns x(1).png; the next run makes that
        // x(1)(1).png. The parse has already happened by this point, so the check
        // the spec rejected on re-parsing cost is here just one path comparison.
        if is_already_filed(&dir, &file) {
            rep.already_filed += 1;
            continue;
        }

        // The name stays an OsStr all the way to `rename`: a card out of a
        // Shift-JIS archive can have a file name that is not valid UTF-8, and
        // moving it under its `to_string_lossy` spelling would rename it to a
        // row of U+FFFD with no way back. `shown` is for the printed line only.
        let name = file.file_name().unwrap_or_default();
        let shown = name.to_string_lossy();
        let rel = dir
            .strip_prefix(root)
            .unwrap_or(&dir)
            .to_string_lossy()
            .replace('\\', "/");

        if dry_run {
            // Preview the same free_name collision check a real move would use, so
            // the dry run reports the name a real run would actually produce
            // rather than always the bare name. Honest limitation: nothing is
            // written to disk during a dry run, so free_name can only see files
            // that already existed before this run started — two source files
            // that would collide with EACH OTHER within the same dry run both
            // preview the same free name, because neither one's previewed move
            // ever "happens" on disk for the other to see.
            let preview = free_name(&dir, name);
            let _ = writeln!(out, "Move file: {} to {}", shown, preview.display());
            rep.record(rel);
            continue;
        }

        if let Err(e) = std::fs::create_dir_all(&dir) {
            rep.errors += 1;
            let _ = writeln!(out, "Failed to handle {}: {}", file.display(), e);
            continue;
        }
        let target = free_name(&dir, name);
        if let Err(e) = std::fs::rename(&file, &target) {
            rep.errors += 1;
            let _ = writeln!(out, "Failed to handle {}: {}", file.display(), e);
            continue;
        }
        let _ = writeln!(out, "Move file: {} to {}", shown, target.display());
        rep.record(rel);
    }
    rep
}

/// Checked separately from the run loop so a typoed or unreadable `--root`
/// produces a clear diagnostic and a non-zero exit instead of a banner, an empty
/// summary, and an exit code that tells the user everything succeeded.
fn check_root(root: &Path) -> Result<(), String> {
    match std::fs::metadata(root) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(format!("--root {} is not a directory", root.display())),
        Err(e) => Err(format!("--root {} is not accessible: {e}", root.display())),
    }
}

fn print_summary(rep: &Report, out: &mut dyn Write) {
    let _ = writeln!(out, "--- summary ---");
    if rep.moved.is_empty() {
        let _ = writeln!(out, "  moved       (nothing)");
    }
    for (i, (dest, n)) in rep.moved.iter().enumerate() {
        let label = if i == 0 { "moved" } else { "" };
        let _ = writeln!(out, "  {label:<11} {dest:<26} {n:>5}");
    }
    let _ = writeln!(out, "  left alone  {:<26} {:>5}", "scene cards", rep.scenes);
    let _ = writeln!(out, "  {:<11} {:<26} {:>5}", "", "non-card images", rep.non_cards);
    let _ = writeln!(out, "  {:<11} {:<26} {:>5}", "", "unrecognized markers", rep.unrecognized);
    let _ = writeln!(out, "  {:<11} {:<26} {:>5}", "", "already in place", rep.already_filed);
    let _ = writeln!(out, "  {:<11} {:<26} {:>5}", "", "errors", rep.errors);
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "{VERSION_BANNER}");

    let root = args
        .root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("current directory"));
    if let Err(e) = check_root(&root) {
        let _ = out.flush();
        eprintln!("{e}");
        std::process::exit(1);
    }
    let rep = run(&root, args.dry_run, args.search.as_deref(), &mut out);
    print_summary(&rep, &mut out);
    if args.dry_run {
        let _ = writeln!(out, "(dry run — nothing was moved)");
    }
    let _ = out.flush();

    // Only pause when a human is watching. hamster called ReadKey unconditionally,
    // which throws under redirected stdin and hangs under a hidden window — that is
    // why it could never be scripted.
    if std::io::stdin().is_terminal() {
        println!("All jobs done, press Enter to exit...");
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
    }
    std::process::exit(if rep.errors == 0 { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempdir::Dir;
    use std::path::Path;

    fn args(v: &[&str]) -> Result<Args, String> {
        Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn no_arguments_means_current_directory_and_a_real_move() {
        let a = args(&[]).unwrap();
        assert_eq!(a.root, None);
        assert!(!a.dry_run);
        assert_eq!(a.search, None);
    }

    #[test]
    fn flags_and_a_positional_search_term_parse() {
        let a = args(&["--root", "/tmp/x", "--dry-run", "asuna"]).unwrap();
        assert_eq!(a.root.as_deref(), Some(Path::new("/tmp/x")));
        assert!(a.dry_run);
        assert_eq!(a.search.as_deref(), Some("asuna"));
    }

    #[test]
    fn root_without_a_value_is_an_error_rather_than_a_silent_default() {
        assert!(args(&["--root"]).is_err());
    }

    #[test]
    fn a_root_value_that_looks_like_a_flag_is_rejected_rather_than_swallowed() {
        assert!(args(&["--root", "--dry-run"]).is_err());
    }

    #[test]
    fn a_nonexistent_root_is_reported_rather_than_silently_producing_an_empty_run() {
        let d = Dir::new();
        let missing = d.path().join("does-not-exist");
        let err = check_root(&missing).unwrap_err();
        assert!(err.contains(&missing.display().to_string()), "{err}");
    }

    #[test]
    fn an_unknown_flag_is_an_error() {
        assert!(args(&["--recursive"]).is_err());
    }

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    #[test]
    fn a_run_files_each_card_and_counts_everything_else() {
        let d = Dir::new();
        let r = d.path();
        write(r, "girl.png", &fixture::card("【KoiKatuChara】", 1, "a", "b"));
        write(r, "boy.png", &fixture::card("【KoiKatuChara】", 0, "a", "b"));
        write(r, "outfit.png", &fixture::card("【KoiKatuClothes】", 1, "", ""));
        write(r, "ec.png", &fixture::card("【EroMakeChara】", 1, "", ""));
        write(r, "scene.png", &fixture::scene("1.0.4.2"));
        write(r, "texture.png", &fixture::plain_png());
        write(r, "future.png", &fixture::card("【SomeFutureGame】", 1, "", ""));

        let mut out = Vec::new();
        let rep = run(r, false, None, &mut out);

        assert!(r.join("Koikatu/Female/girl.png").exists());
        assert!(r.join("Koikatu/Male/boy.png").exists());
        assert!(r.join("Koikatu/Coordinate/outfit.png").exists());
        assert!(r.join("EmotionCreators/Character/ec.png").exists());
        assert!(r.join("scene.png").exists(), "scene cards are never moved");
        assert!(r.join("texture.png").exists());
        assert!(r.join("future.png").exists());

        assert_eq!(rep.scenes, 1);
        assert_eq!(rep.non_cards, 1);
        assert_eq!(rep.unrecognized, 1);
        assert_eq!(rep.errors, 0);
        assert_eq!(rep.moved.iter().map(|(_, n)| n).sum::<u64>(), 4);

        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Move file: girl.png to"), "{text}");
        assert!(text.contains("【SomeFutureGame】"), "unknown markers are named: {text}");
    }

    #[test]
    fn a_dry_run_reports_the_same_thing_and_moves_nothing() {
        let d = Dir::new();
        let r = d.path();
        write(r, "girl.png", &fixture::card("【KoiKatuChara】", 1, "a", "b"));

        let mut out = Vec::new();
        let rep = run(r, true, None, &mut out);

        assert_eq!(rep.moved.iter().map(|(_, n)| n).sum::<u64>(), 1);
        assert!(r.join("girl.png").exists(), "dry run must not move");
        assert!(!r.join("Koikatu/Female/girl.png").exists());
    }

    /// A dry run must preview the name a real run would actually produce, not
    /// always the bare name — so it has to run the same free_name collision check
    /// a real move uses. Two cards share a file name ("dup.png") from different
    /// source folders, one Female and one Male, so they land in different
    /// destination folders and don't collide with each other; only the Male one
    /// collides, with a file already sitting on disk at its destination (as if
    /// left over from an earlier run). The preview must show the plain name for
    /// the Female card and the counter-suffixed name for the Male one.
    #[test]
    fn a_dry_run_previews_the_free_name_against_files_already_on_disk() {
        let d = Dir::new();
        let r = d.path();
        write(r, "one/dup.png", &fixture::card("【KoiKatuChara】", 1, "a", "b")); // Female
        write(r, "two/dup.png", &fixture::card("【KoiKatuChara】", 0, "c", "d")); // Male
        std::fs::create_dir_all(r.join("Koikatu").join("Male")).unwrap();
        std::fs::write(r.join("Koikatu").join("Male").join("dup.png"), b"already here").unwrap();

        let mut out = Vec::new();
        let rep = run(r, true, None, &mut out);

        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains(&format!("to {}", r.join("Koikatu").join("Female").join("dup.png").display())),
            "expected the plain name for the non-colliding card: {text}"
        );
        assert!(
            text.contains(&format!("to {}", r.join("Koikatu").join("Male").join("dup(1).png").display())),
            "expected the counter-suffixed name for the colliding card: {text}"
        );
        assert_eq!(rep.moved.iter().map(|(_, n)| n).sum::<u64>(), 2);
        assert!(r.join("one").join("dup.png").exists(), "dry run must not move");
        assert!(r.join("two").join("dup.png").exists(), "dry run must not move");
    }

    #[test]
    fn a_second_run_moves_nothing_because_the_output_folder_is_skipped() {
        let d = Dir::new();
        let r = d.path();
        write(r, "girl.png", &fixture::card("【KoiKatuChara】", 1, "a", "b"));

        let mut out = Vec::new();
        run(r, false, None, &mut out);
        let mut out2 = Vec::new();
        let rep = run(r, false, None, &mut out2);
        assert_eq!(rep.moved.iter().map(|(_, n)| n).sum::<u64>(), 0);
        assert_eq!(rep.non_cards, 0);
    }

    /// Every `.png` under `root`, relative and slash-separated — including the
    /// output folders `walk` skips, because this is about what is on disk.
    fn all_pngs(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().map(|x| x == "png").unwrap_or(false) {
                    out.push(
                        p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"),
                    );
                }
            }
        }
        out.sort();
        out
    }

    /// Double-clicking the exe inside an already-organised `…/Koikatu` folder
    /// makes that folder the scan root, so `Female/` is not a first-level
    /// component and the exclusion rule does not fire — the collection is walked
    /// and parsed again. Whatever else that does, the one thing that must never
    /// happen is a card being renamed onto its own successor: `girl.png` becoming
    /// `girl(1).png`, then `girl(1)(1).png`, growing a suffix on every run while
    /// the summary claims a successful move. Two runs from inside the game folder
    /// and the card is still a single file that never gained a counter.
    #[test]
    fn a_double_click_inside_an_organised_game_folder_never_renames_a_card_onto_itself() {
        let d = Dir::new();
        let root = d.path().join("Koikatu"); // the organised game folder itself
        write(&root, "Female/girl.png", &fixture::card("【KoiKatuChara】", 1, "a", "b"));

        let mut out = Vec::new();
        let rep1 = run(&root, false, None, &mut out);
        let after_first = all_pngs(&root);
        let mut out2 = Vec::new();
        let rep2 = run(&root, false, None, &mut out2);
        let after_second = all_pngs(&root);

        assert_eq!(rep1.errors, 0);
        assert_eq!(rep2.errors, 0);
        assert_eq!(after_first.len(), 1, "no copy was made: {after_first:?}");
        assert_eq!(after_second.len(), 1, "and none the second time: {after_second:?}");
        assert_eq!(after_first, after_second, "the second run is a no-op");
        for p in &after_second {
            assert!(
                p.ends_with("/girl.png"),
                "a card must never be renamed onto its own successor: {p}"
            );
        }
        // Documented, not endorsed: with the game folder itself as the root, the
        // card's destination is `<root>/Koikatu/Female`, which is NOT its parent
        // `<root>/Female`, so the destination-equals-parent guard does not fire
        // and the card is re-filed one level deeper. The second run then skips it
        // (the nested `Koikatu/` IS a first-level output folder), so the layout is
        // stable and nothing is lost or duplicated — but a user who double-clicks
        // inside a game folder does get a `Koikatu/Koikatu/Female`.
        assert_eq!(after_second, ["Koikatu/Female/girl.png"]);
    }

    /// The self-move guard's verdict, asserted directly, on a card sitting in the
    /// folder it would be moved to. `run` cannot be made to reach the guard while
    /// `Game::folder()` and `DEST_FOLDERS` agree — `walk` excludes the whole
    /// first-level `Koikatu/` before the parse — which is exactly why the guard
    /// exists: it is the backstop for the day those two lists drift apart. The
    /// `run` half below pins the other side of that: with the lists in agreement,
    /// the card is left strictly alone.
    #[test]
    fn a_card_already_in_its_destination_is_counted_and_not_moved() {
        let d = Dir::new();
        let r = d.path();
        let dest = r.join("Koikatu").join("Female");
        std::fs::create_dir_all(&dest).unwrap();
        let card = dest.join("girl.png");
        std::fs::write(&card, fixture::card("【KoiKatuChara】", 1, "a", "b")).unwrap();

        let meta = read_card(&card).expect("card");
        let dir = destination_dir(r, &meta, None);
        assert!(
            is_already_filed(&dir, &card),
            "the card is already where it belongs, so nothing may be renamed"
        );

        let mut out = Vec::new();
        let rep = run(r, false, None, &mut out);
        assert!(card.exists(), "still there under its own name");
        assert!(!dest.join("girl(1).png").exists(), "never renamed onto its successor");
        assert_eq!(rep.moved.iter().map(|(_, n)| n).sum::<u64>(), 0);
    }

    #[test]
    fn a_name_collision_at_the_destination_gains_a_counter() {
        let d = Dir::new();
        let r = d.path();
        write(r, "one/dup.png", &fixture::card("【KoiKatuChara】", 1, "a", "b"));
        write(r, "two/dup.png", &fixture::card("【KoiKatuChara】", 1, "c", "d"));

        let mut out = Vec::new();
        run(r, false, None, &mut out);
        assert!(r.join("Koikatu/Female/dup.png").exists());
        assert!(r.join("Koikatu/Female/dup(1).png").exists());
    }

    /// A character card whose `sex` cannot be read is still placeable — the
    /// marker named the game — so it is filed under `{Game}/Unknown` and the run
    /// still exits 0. Before the fix a missing `Parameter` entry was `Malformed`:
    /// the card was left where it lay and the whole run's exit code failed over
    /// it, which is how one odd card made a 2000-card run look like a failure.
    #[test]
    fn a_character_card_whose_sex_cannot_be_read_is_filed_under_unknown_not_failed() {
        let d = Dir::new();
        let r = d.path();
        let mut bytes = fixture::card("【KoiKatuChara】", 1, "a", "b");
        let at = bytes.windows(9).position(|w| w == b"Parameter").expect("entry");
        bytes[at..at + 9].copy_from_slice(b"Xarameter");
        write(r, "odd.png", &bytes);

        let mut out = Vec::new();
        let rep = run(r, false, None, &mut out);

        assert_eq!(rep.errors, 0, "a placeable card must not fail the run");
        assert!(r.join("Koikatu").join("Unknown").join("odd.png").exists());
    }

    /// End to end: a card whose file name is not valid UTF-8 must arrive at its
    /// destination under exactly the name it had. Before the fix the move target
    /// was built from `to_string_lossy`, so the card was renamed to a row of
    /// U+FFFD and the original spelling was unrecoverable.
    #[test]
    fn a_card_with_a_non_utf8_name_is_moved_under_that_exact_name() {
        use std::ffi::OsString;
        let d = Dir::new();
        let r = d.path();

        #[cfg(windows)]
        let name: OsString = {
            use std::os::windows::ffi::OsStringExt;
            let mut w: Vec<u16> = "card".encode_utf16().collect();
            w.push(0xD800); // unpaired high surrogate
            w.extend(".png".encode_utf16());
            OsString::from_wide(&w)
        };
        #[cfg(unix)]
        let name: OsString = {
            use std::os::unix::ffi::OsStringExt;
            let mut b = b"card".to_vec();
            b.push(0xFF);
            b.extend_from_slice(b".png");
            OsString::from_vec(b)
        };
        #[cfg(not(any(windows, unix)))]
        let name: OsString = OsString::new();

        if name.is_empty() || std::fs::write(r.join(&name), fixture::card("【KoiKatuChara】", 1, "a", "b")).is_err() {
            eprintln!(
                "note: skipping a_card_with_a_non_utf8_name_is_moved_under_that_exact_name — \
                 this platform/filesystem refused to create a file with a non-UTF-8 name"
            );
            return;
        }

        let mut out = Vec::new();
        let rep = run(r, false, None, &mut out);

        assert_eq!(rep.errors, 0);
        assert!(
            r.join("Koikatu").join("Female").join(&name).exists(),
            "the card must land under its original name, not a lossy one"
        );
    }

    #[test]
    fn a_search_term_sorts_matches_into_a_subfolder_without_filtering_the_rest() {
        let d = Dir::new();
        let r = d.path();
        write(r, "hit.png", &fixture::card("【KoiKatuChara】", 1, "Asuna", "Yuuki"));
        write(r, "miss.png", &fixture::card("【KoiKatuChara】", 1, "Rika", "Shinozaki"));

        let mut out = Vec::new();
        run(r, false, Some("asuna"), &mut out);
        assert!(r.join("Koikatu/Female/asuna/hit.png").exists());
        assert!(r.join("Koikatu/Female/miss.png").exists());
    }

    /// A card that cannot be opened must be reported and counted as an error, not
    /// silently added to `non_cards`. `share_mode(0)` reproduces the routine real
    /// case — the card is open in the character maker — and before the fix this
    /// run reported `non_cards: 1`, `errors: 0`, printed nothing about the file
    /// and exited 0.
    #[cfg(windows)]
    #[test]
    fn a_card_that_cannot_be_read_is_reported_as_an_error_not_counted_as_a_texture() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = Dir::new();
        let r = d.path();
        write(r, "locked.png", &fixture::card("【KoiKatuChara】", 1, "a", "b"));
        let _lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(r.join("locked.png"))
            .expect("take an exclusive handle");

        let mut out = Vec::new();
        let rep = run(r, false, None, &mut out);

        assert_eq!(rep.errors, 1, "an unreadable card is an error");
        assert_eq!(rep.non_cards, 0, "and must not be laundered into non-card images");
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("locked.png"), "the file must be named: {text}");
    }

    #[test]
    fn a_malformed_card_is_counted_as_an_error_and_left_alone() {
        let d = Dir::new();
        let r = d.path();
        let mut bytes = fixture::card("【KoiKatuChara】", 1, "a", "b");
        bytes.truncate(bytes.len() - 4);
        write(r, "broken.png", &bytes);

        let mut out = Vec::new();
        let rep = run(r, false, None, &mut out);
        assert_eq!(rep.errors, 1);
        assert!(r.join("broken.png").exists());
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("broken.png"), "{text}");
    }
}
