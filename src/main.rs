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
use crate::plan::{destination_dir, free_name};
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
                    let v = argv.get(i).ok_or("--root needs a directory")?;
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
            Err(e) => {
                rep.errors += 1;
                let _ = writeln!(out, "Failed to handle {}: {}", file.display(), e.reason());
                continue;
            }
        };

        let dir = destination_dir(root, &meta, search);
        let name = file.file_name().unwrap_or_default().to_string_lossy().to_string();
        let rel = dir
            .strip_prefix(root)
            .unwrap_or(&dir)
            .to_string_lossy()
            .replace('\\', "/");

        if dry_run {
            let _ = writeln!(out, "Move file: {} to {}", name, dir.join(&name).display());
            rep.record(rel);
            continue;
        }

        if let Err(e) = std::fs::create_dir_all(&dir) {
            rep.errors += 1;
            let _ = writeln!(out, "Failed to handle {}: {}", file.display(), e);
            continue;
        }
        let target = free_name(&dir, &name);
        if let Err(e) = std::fs::rename(&file, &target) {
            rep.errors += 1;
            let _ = writeln!(out, "Failed to handle {}: {}", file.display(), e);
            continue;
        }
        let _ = writeln!(out, "Move file: {} to {}", name, target.display());
        rep.record(rel);
    }
    rep
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
        // fixture::scene's minimal payload isn't long enough for the marker-first
        // read to fail without truncating and fall through to the scene probe (see
        // card::tests::a_scene_card_is_recognized_by_its_version_string); pad it
        // out here for the same reason.
        let mut scene_bytes = fixture::scene("1.0.4.2");
        scene_bytes.extend(std::iter::repeat(0u8).take(128));
        write(r, "scene.png", &scene_bytes);
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
