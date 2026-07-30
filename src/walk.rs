//! Recursive scan for candidate PNGs.

use crate::plan::is_in_dest_folder;
use std::path::{Path, PathBuf};

/// What a scan of `root` turned up.
#[derive(Debug, Default)]
pub struct Scan {
    /// Candidate files, in sorted order.
    pub files: Vec<PathBuf>,
    /// `.png` entries that were skipped because they are symlinks or, on
    /// Windows, some other reparse point. Skipping them is deliberate; leaving
    /// them out of every counter is not — a file in a place the summary does not
    /// report is the failure mode this tool exists to prevent.
    pub symlinked_pngs: u64,
    /// Names of directories directly under the root that the exclusion rule
    /// rejected, sorted and deduplicated. Recorded where the rule already runs,
    /// so it costs one push and no filesystem call — and it can only ever hold
    /// first-level names, because that is all the rule looks at.
    ///
    /// Skipping them is deliberate; skipping them without a word is not. A
    /// downloaded pack that unpacks to `SVC/` is indistinguishable from this
    /// program's own output by name alone, and the name is exactly what lets the
    /// user tell the difference.
    pub excluded_dirs: Vec<String>,
}

/// Every `.png` under `root`, excluding this program's own output folders, in
/// sorted order.
///
/// Iterative rather than recursive: a pathological directory tree must not be able
/// to overflow the stack, for the same reason the msgpack decoder caps its depth.
/// Directories that cannot be read are skipped — one unreadable folder is not a
/// reason to abandon the scan.
///
/// Symlinks and, on Windows, directory junctions are never descended into.
/// `DirEntry::file_type` does not follow the link, so `is_symlink()` is true for
/// both without touching the target — a junction pointing at an ancestor
/// (common in real Windows user profiles) would otherwise make the scan re-read
/// the same subtree forever, since nothing else here tracks visited paths.
pub fn candidates(root: &Path) -> Scan {
    let mut out = Vec::new();
    let mut symlinked_pngs = 0u64;
    let mut excluded_dirs = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                // Reparse point (symlink or, on Windows, a junction/mount
                // point) — never descend, and never treat it as a candidate
                // file either. Cheap: no extra filesystem call, and it is
                // exactly what stops a cycle back to an ancestor.
                //
                // One that looks like a card is counted on the way past, so the
                // summary can say it was skipped. The extension is all that is
                // consulted: asking what the link points at would mean following
                // the link this guard exists to refuse.
                if has_png_extension(&path) {
                    symlinked_pngs += 1;
                }
                continue;
            }
            if ft.is_dir() {
                if is_in_dest_folder(root, &path) {
                    if let Some(name) = path.file_name() {
                        excluded_dirs.push(name.to_string_lossy().into_owned());
                    }
                } else {
                    stack.push(path);
                }
            } else if ft.is_file() && has_png_extension(&path) {
                out.push(path);
            }
        }
    }
    // The traversal order (stack pop order, filesystem enumeration order
    // within a directory) is not meaningful on its own — pin down the
    // documented "deterministic order" by sorting the whole result once.
    out.sort();
    excluded_dirs.sort();
    excluded_dirs.dedup();
    Scan { files: out, symlinked_pngs, excluded_dirs }
}

fn has_png_extension(p: &Path) -> bool {
    p.extension()
        .map(|e| e.eq_ignore_ascii_case("png"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempdir::Dir;
    use std::path::Path;

    fn touch(root: &Path, rel: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
    }

    fn names(root: &Path) -> Vec<String> {
        let mut v: Vec<String> = candidates(root)
            .files
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn finds_pngs_at_any_depth_and_ignores_other_extensions() {
        let d = Dir::new();
        touch(d.path(), "a.png");
        touch(d.path(), "sub/b.PNG");
        touch(d.path(), "sub/deeper/c.png");
        touch(d.path(), "sub/notes.txt");
        touch(d.path(), "sub/d.zipmod");
        assert_eq!(names(d.path()), ["a.png", "sub/b.PNG", "sub/deeper/c.png"]);
    }

    #[test]
    fn skips_this_programs_own_output_folders() {
        let d = Dir::new();
        touch(d.path(), "a.png");
        touch(d.path(), "Koikatu/Female/already.png");
        touch(d.path(), "EmotionCreators/Character/already.png");
        assert_eq!(names(d.path()), ["a.png"]);
    }

    /// The two shapes that the C# substring filter silently dropped.
    #[test]
    fn does_not_skip_card_pack_folders_that_merely_contain_a_game_name() {
        let d = Dir::new();
        touch(d.path(), "ISEEU/Genshin/Unknown god/card/god.png");
        touch(d.path(), "pack/Koikatu_F_20240626232405280_x/x.png");
        touch(d.path(), "somewhere/Koikatu/deep.png");
        assert_eq!(
            names(d.path()),
            [
                "ISEEU/Genshin/Unknown god/card/god.png",
                "pack/Koikatu_F_20240626232405280_x/x.png",
                "somewhere/Koikatu/deep.png",
            ]
        );
    }

    #[test]
    fn an_empty_root_yields_nothing() {
        let d = Dir::new();
        let scan = candidates(d.path());
        assert!(scan.files.is_empty());
        assert_eq!(scan.symlinked_pngs, 0);
    }

    /// A symlinked `.png` is not descended into or moved — following links is
    /// what the reparse-point guard refuses to do — but it must not vanish from
    /// the accounting. Being skipped silently puts a card-shaped file in a place
    /// no counter reports, which is the failure this whole tool is about.
    #[test]
    fn a_symlinked_png_is_skipped_but_counted() {
        let d = Dir::new();
        touch(d.path(), "real.png");
        touch(d.path(), "target/actual.png");
        let link = d.path().join("linked.png");

        if !make_png_link(&link, &d.path().join("target").join("actual.png"), &d.path().join("target")) {
            eprintln!(
                "note: skipping a_symlinked_png_is_skipped_but_counted — this environment \
                 allowed neither a file symlink nor a junction; the counter in `candidates` \
                 is still in place, just unverified by this run"
            );
            return;
        }

        let scan = candidates(d.path());
        let mut got: Vec<String> = scan
            .files
            .iter()
            .map(|p| p.strip_prefix(d.path()).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        got.sort();

        // Unlink before asserting, so a failure still leaves no reparse point
        // behind for `Dir`'s recursive delete to walk through.
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir(&link);

        assert_eq!(got, ["real.png", "target/actual.png"], "the link itself is not a candidate");
        assert_eq!(scan.symlinked_pngs, 1, "but it is counted");
    }

    /// Create a reparse point at `link`, whose name ends in `.png`.
    ///
    /// A real file symlink is what this is about, but on Windows that needs
    /// Developer Mode or elevation, which routinely is not available — so fall
    /// back to a directory junction, which needs neither. The guard cannot tell
    /// them apart and is not meant to: `FileType::is_symlink` is true for
    /// `IO_REPARSE_TAG_SYMLINK` and `IO_REPARSE_TAG_MOUNT_POINT` alike, and the
    /// counter keys off that plus the `.png` name. Either way the branch under
    /// test is the one that really runs.
    #[cfg(windows)]
    fn make_png_link(link: &Path, file_target: &Path, dir_target: &Path) -> bool {
        if std::os::windows::fs::symlink_file(file_target, link).is_ok() {
            return true;
        }
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(dir_target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(unix)]
    fn make_png_link(link: &Path, file_target: &Path, _dir_target: &Path) -> bool {
        std::os::unix::fs::symlink(file_target, link).is_ok()
    }

    #[cfg(not(any(windows, unix)))]
    fn make_png_link(_link: &Path, _file_target: &Path, _dir_target: &Path) -> bool {
        false
    }

    /// `is_in_dest_folder` uses `strip_prefix`, which fails outright (returning
    /// `false` for everything) if `root` and the file path disagree about being
    /// absolute vs relative. `candidates` never triggers that: every path it
    /// returns is built by joining onto the very `root` it was given, so the
    /// prefix always matches — even when `root` itself is relative. This pins
    /// that invariant down without touching the process-wide current directory
    /// (which other tests running in parallel also rely on).
    #[test]
    fn a_relative_root_still_skips_its_own_output_folders() {
        let d = Dir::new();
        touch(d.path(), "a.png");
        touch(d.path(), "Koikatu/Female/already.png");

        let cwd = std::env::current_dir().unwrap();
        let Some(rel_root) = pathdiff(d.path(), &cwd) else {
            eprintln!(
                "note: skipping a_relative_root_still_skips_its_own_output_folders — \
                 the temp directory and the current directory are on different \
                 Windows drives, so no relative path between them exists"
            );
            return;
        };

        assert_eq!(names(&rel_root), ["a.png"]);
    }

    /// Build a relative path from `from` to `to` by counting shared prefix
    /// components. Both inputs are expected to be absolute (temp dirs and
    /// `current_dir()` both are), so no `..`-walking symlink resolution is
    /// needed — just component comparison.
    ///
    /// Returns `None` when `to` and `from` disagree on Windows drive (or,
    /// more generally, path-prefix) — there is no relative path across
    /// drives, and silently building one by counting `..`s anyway would
    /// produce a nonsense path that quietly checks the wrong thing instead
    /// of failing loudly.
    fn pathdiff(to: &Path, from: &Path) -> Option<std::path::PathBuf> {
        let to_comps: Vec<_> = to.components().collect();
        let from_comps: Vec<_> = from.components().collect();

        if let (Some(std::path::Component::Prefix(tp)), Some(std::path::Component::Prefix(fp))) =
            (to_comps.first(), from_comps.first())
        {
            if tp.as_os_str() != fp.as_os_str() {
                return None;
            }
        }

        let common = to_comps
            .iter()
            .zip(from_comps.iter())
            .take_while(|(a, b)| a == b)
            .count();
        let mut result = std::path::PathBuf::new();
        for _ in common..from_comps.len() {
            result.push("..");
        }
        for comp in &to_comps[common..] {
            result.push(comp.as_os_str());
        }
        if result.as_os_str().is_empty() {
            result.push(".");
        }
        Some(result)
    }

    /// Pins the Critical fix: a directory junction (Windows) or symlink
    /// (elsewhere) pointing back at an ancestor must not send the scan into
    /// an infinite loop. `entry.file_type()` never follows the link, so
    /// `is_symlink()` is true for the link itself without ever resolving the
    /// target — exactly the cheap check `candidates` needs in order to
    /// refuse to descend, with no visited-set and no `canonicalize` calls.
    ///
    /// On Windows this uses `mklink /J`, a directory **junction**, not
    /// `symlink_dir` — a plain symlink needs elevation or Developer Mode,
    /// which routinely is not available, but a junction needs neither and
    /// still trips `FILE_ATTRIBUTE_REPARSE_POINT` alongside
    /// `FILE_ATTRIBUTE_DIRECTORY`, which is exactly the shape the guard has
    /// to handle: `FileType::is_symlink` on Windows is true for
    /// `IO_REPARSE_TAG_MOUNT_POINT` (junctions) as well as
    /// `IO_REPARSE_TAG_SYMLINK` — unlike e.g. Python's `os.path.islink`,
    /// which reports `False` for a junction.
    ///
    /// Without the `is_symlink` guard this test would not merely fail an
    /// assertion, it would hang: `sub/loop_back_to_root` resolves back to
    /// `root`, whose own listing contains `sub` again, so the walk would
    /// keep re-descending through the junction forever, appending a fresh
    /// duplicate of `a.png`/`b.png` to the result and a fresh nested
    /// `.../loop_back_to_root/sub/loop_back_to_root/...` path onto the stack
    /// on every pass — unbounded, not merely slow.
    #[test]
    fn a_directory_junction_pointing_at_an_ancestor_does_not_loop_forever() {
        let d = Dir::new();
        touch(d.path(), "a.png");
        touch(d.path(), "sub/b.png");
        let link = d.path().join("sub").join("loop_back_to_root");

        if !make_ancestor_link(&link, d.path()) {
            eprintln!(
                "note: skipping a_directory_junction_pointing_at_an_ancestor_does_not_loop_forever \
                 — could not create a directory junction/symlink in this environment; the \
                 reparse-point guard in `candidates` is still in place, just unverified by this run"
            );
            return;
        }
        eprintln!(
            "a_directory_junction_pointing_at_an_ancestor_does_not_loop_forever: link created, \
             calling candidates() through the cycle for real"
        );

        // The call returning at all (rather than hanging) is half of what
        // this test is pinning down; the other half is that the result is
        // still exactly the two real files, each counted once, with nothing
        // pulled in by walking through the junction.
        let result = names(d.path());

        remove_ancestor_link(&link);
        assert!(
            d.path().exists() && d.path().join("a.png").exists(),
            "cleanup must remove only the junction, not the tree it points at"
        );

        assert_eq!(result, ["a.png", "sub/b.png"]);
    }

    /// Create `link` pointing at `target` (an ancestor of `link`), using
    /// whichever mechanism the platform allows without elevation. Returns
    /// `false` if creation failed for any reason (missing tool, permissions,
    /// unsupported platform) — the caller treats that as "skip, don't fail".
    #[cfg(windows)]
    fn make_ancestor_link(link: &Path, target: &Path) -> bool {
        // Directory junctions need no elevation and no Developer Mode, unlike
        // `std::os::windows::fs::symlink_dir`. `mklink` is a `cmd.exe`
        // builtin, not a standalone executable, so it has to be invoked
        // through `cmd /C`.
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(unix)]
    fn make_ancestor_link(link: &Path, target: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    #[cfg(not(any(windows, unix)))]
    fn make_ancestor_link(_link: &Path, _target: &Path) -> bool {
        false
    }

    /// Unlink `link` itself without touching what it points at.
    ///
    /// `std::fs::remove_dir_all` on a path containing a junction can, on
    /// some Windows versions, follow the junction and delete through it —
    /// exactly the wrong thing when the junction points at an ancestor.
    /// `remove_dir` on a junction unlinks the reparse point only; the target
    /// directory and its contents are untouched. On Unix, the link is a
    /// symlink and is removed the same way any file entry is.
    #[cfg(windows)]
    fn remove_ancestor_link(link: &Path) {
        let _ = std::fs::remove_dir(link);
    }

    #[cfg(unix)]
    fn remove_ancestor_link(link: &Path) {
        let _ = std::fs::remove_file(link);
    }

    #[cfg(not(any(windows, unix)))]
    fn remove_ancestor_link(_link: &Path) {}

    /// The Important fix: `candidates` must return a fully, deterministically
    /// sorted `Vec`, not just sorted within each directory. Unlike every
    /// other test in this file, this asserts the RAW return value —
    /// `names()` sorts before comparing, which is exactly what let the
    /// unsorted-across-directories bug through unnoticed. The tree below is
    /// built so sorted order does not match the depth-first / LIFO order the
    /// old code produced (root's subdirectories are visited stack-LIFO, so
    /// "z" would surface before "a" without the final sort).
    #[test]
    fn returns_the_full_result_sorted_not_just_sorted_within_each_directory() {
        let d = Dir::new();
        touch(d.path(), "z/first.png");
        touch(d.path(), "a/second.png");
        touch(d.path(), "a/deep/inner.png");
        touch(d.path(), "m.png");

        let raw: Vec<String> = candidates(d.path())
            .files
            .iter()
            .map(|p| {
                p.strip_prefix(d.path())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();

        assert_eq!(
            raw,
            ["a/deep/inner.png", "a/second.png", "m.png", "z/first.png"]
        );
    }

    /// The exclusion rule is a deliberate trade-off (see the 2026-07-29 design,
    /// §Exclusion rule): a first-level folder named after a game is skipped
    /// whole, even when it is a downloaded pack rather than this program's own
    /// output. That is defensible only while the run SAYS so — a pack that
    /// unpacks to `SVC/` must not vanish from the summary as well as from the
    /// walk. The deeper namesake is asserted in the same test because widening
    /// the rule while adding the recording is the obvious way to break it.
    #[test]
    fn an_excluded_first_level_folder_is_named_and_a_deeper_namesake_is_not() {
        let d = Dir::new();
        touch(d.path(), "SVC/x.png");
        touch(d.path(), "pack/SVC/y.png");

        let scan = candidates(d.path());

        assert_eq!(scan.excluded_dirs, ["SVC"], "the first-level SVC is recorded by name");
        let files: Vec<String> = scan
            .files
            .iter()
            .map(|p| p.strip_prefix(d.path()).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(files, ["pack/SVC/y.png"], "and only the deeper namesake is still scanned");
    }

    /// Nothing excluded means nothing recorded — the list is evidence, so it must
    /// not carry a name on an ordinary run over a freshly unpacked folder.
    #[test]
    fn an_ordinary_tree_records_no_excluded_folders() {
        let d = Dir::new();
        touch(d.path(), "a.png");
        touch(d.path(), "sub/b.png");
        assert!(candidates(d.path()).excluded_dirs.is_empty());
    }
}
