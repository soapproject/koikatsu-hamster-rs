//! Recursive scan for candidate PNGs.

use crate::plan::is_in_dest_folder;
use std::path::{Path, PathBuf};

/// Every `.png` under `root`, excluding this program's own output folders.
///
/// Iterative rather than recursive: a pathological directory tree must not be able
/// to overflow the stack, for the same reason the msgpack decoder caps its depth.
/// Directories that cannot be read are skipped — one unreadable folder is not a
/// reason to abandon the scan.
pub fn candidates(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut found: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if !is_in_dest_folder(root, &path) {
                    stack.push(path);
                }
            } else if ft.is_file() && has_png_extension(&path) {
                found.push(path);
            }
        }
        found.sort();
        out.extend(found);
    }
    out
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
        assert!(candidates(d.path()).is_empty());
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
        let rel_root = pathdiff(d.path(), &cwd);

        assert_eq!(names(&rel_root), ["a.png"]);
    }

    /// Build a relative path from `from` to `to` by counting shared prefix
    /// components. Both inputs are expected to be absolute (temp dirs and
    /// `current_dir()` both are), so no `..`-walking symlink resolution is
    /// needed — just component comparison.
    fn pathdiff(to: &Path, from: &Path) -> std::path::PathBuf {
        let to_comps: Vec<_> = to.components().collect();
        let from_comps: Vec<_> = from.components().collect();
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
        result
    }
}
