# Never Silently Skip — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the three paths by which `koikatsu-hamster-rs` can pass over a file without it appearing anywhere in the run's output.

**Architecture:** Three vertical slices. Each adds one fact to `walk::Scan`, carries it into `Report` the same way `symlinked_pngs` already travels, prints it, and tests it end to end. No new module, no new dependency, and no change to which files get moved.

**Tech Stack:** Rust 2021, std only, `cargo test`. Tests build real directories under the repo's own `tempdir::Dir`; there is no mocking layer.

**Spec:** [docs/superpowers/specs/2026-07-30-never-silently-skip-design.md](../specs/2026-07-30-never-silently-skip-design.md), amending [2026-07-29-card-organizer-design.md](../specs/2026-07-29-card-organizer-design.md).

## Global Constraints

- **Zero external crates.** Nothing is added to `Cargo.toml`. Everything here is `std`.
- **Behaviour is unchanged by default.** No run may move, skip, or rename a file it would not have before. The exclusion rule in `plan.rs::is_in_dest_folder` is not modified by any task.
- **`--any-extension` is off by default**, so the default candidate set stays exactly `.png`.
- **Every skipped thing gets a line.** A count kept in `Report` but never printed is a plan failure, not a detail.
- **A test that cannot build its own precondition prints a `note:` to stderr and returns.** It never weakens its assertions instead. This follows the existing symlink tests in `walk.rs`.
- **Commit style:** `type(scope): imperative lowercase subject`, matching `git log` (`fix(walk): …`, `feat(cli): …`, `docs(readme): …`).
- Run the whole suite with `cargo test` from the repo root before every commit.

## File Structure

| file | responsibility | change |
|---|---|---|
| `src/walk.rs` | directory traversal; decides what is a candidate | Modify: two new `Scan` fields, `any_ext` parameter, new tests |
| `src/main.rs` | `Args`, `Report`, `run`, `print_summary` | Modify: two new `Report` fields, one new `Args` field, two summary lines, `run` signature, new tests |
| `src/plan.rs` | the exclusion rule itself | **Unchanged.** `walk` records what this already decides |
| `README.md` | user-facing flag list and behaviour | Modify: document `--any-extension` and the two new summary lines |

---

### Task 1: Name the folders the exclusion rule skipped

**Files:**
- Modify: `src/walk.rs` (`Scan` struct ~line 7, `candidates` ~line 31, tests module)
- Modify: `src/main.rs` (`Report` struct ~line 98, `run` ~line 175, `print_summary` ~line 325, tests module)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `walk::Scan::excluded_dirs: Vec<String>` (sorted, deduplicated, first-level folder names only) and `Report::excluded_dirs: Vec<String>`. Task 2 and Task 3 both add further fields to these same two structs.

- [ ] **Step 1: Write the failing walk test**

Add to the `tests` module in `src/walk.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib walk::tests::an_excluded_first_level_folder`
Expected: FAIL to compile — `no field 'excluded_dirs' on type 'Scan'`.

- [ ] **Step 3: Add the field and record it**

In `src/walk.rs`, add to `Scan` (after `symlinked_pngs`):

```rust
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
```

In `candidates`, add the accumulator next to the others:

```rust
    let mut excluded_dirs = Vec::new();
```

Replace the directory branch:

```rust
            if ft.is_dir() {
                if !is_in_dest_folder(root, &path) {
                    stack.push(path);
                }
            } else if ft.is_file() && has_png_extension(&path) {
```

with:

```rust
            if ft.is_dir() {
                if is_in_dest_folder(root, &path) {
                    if let Some(name) = path.file_name() {
                        excluded_dirs.push(name.to_string_lossy().into_owned());
                    }
                } else {
                    stack.push(path);
                }
            } else if ft.is_file() && has_png_extension(&path) {
```

And before the return:

```rust
    out.sort();
    excluded_dirs.sort();
    excluded_dirs.dedup();
    Scan { files: out, symlinked_pngs, excluded_dirs }
```

- [ ] **Step 4: Run the walk tests to verify they pass**

Run: `cargo test --lib walk::`
Expected: PASS, all `walk::tests::*` green.

- [ ] **Step 5: Write the failing summary test**

Add to the `tests` module in `src/main.rs`:

```rust
    /// A folder the exclusion rule skipped is named in the summary. It is not an
    /// error — the rule is deliberate — but naming it is the one thing that tells
    /// the user a downloaded pack called `SVC` was passed over whole.
    ///
    /// The line is omitted when there is nothing to report, unlike the counts
    /// above it: an empty list is the normal case here, and a permanent `0` row
    /// trains the eye to skip the very row this exists to catch.
    #[test]
    fn the_summary_names_excluded_folders_and_omits_the_line_when_there_are_none() {
        let rep = Report {
            excluded_dirs: vec!["Koikatu".into(), "SVC".into()],
            ..Report::default()
        };
        let mut out = Vec::new();
        print_summary(&rep, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("skipped output folders"), "missing label:\n{text}");
        assert!(text.contains("Koikatu, SVC"), "missing names:\n{text}");

        let mut empty = Vec::new();
        print_summary(&Report::default(), &mut empty);
        let text = String::from_utf8(empty).unwrap();
        assert!(
            !text.contains("skipped output folders"),
            "an always-present empty row is the noise this line must not become:\n{text}"
        );
    }
```

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test --lib tests::the_summary_names_excluded_folders`
Expected: FAIL to compile — `no field 'excluded_dirs' on type 'Report'`.

- [ ] **Step 7: Carry the field into Report and print it**

In `src/main.rs`, add to `Report` (after `symlinked`):

```rust
    /// Names of first-level folders the exclusion rule skipped. Not an error —
    /// the rule is deliberate — but the same principle as `already_filed` and
    /// `symlinked`: passed over on purpose, never passed over in silence.
    pub excluded_dirs: Vec<String>,
```

In `run`, make the scan binding mutable and take the list before `scan.files` is consumed:

```rust
    let mut scan = walk::candidates(root);
    rep.symlinked = scan.symlinked_pngs;
    rep.excluded_dirs = std::mem::take(&mut scan.excluded_dirs);
```

In `print_summary`, after the `symlinked .png files` line:

```rust
    if !rep.excluded_dirs.is_empty() {
        let _ = writeln!(
            out,
            "  {:<11} {:<26} {}",
            "", "skipped output folders", rep.excluded_dirs.join(", ")
        );
    }
```

Fix the existing struct literal in `the_summary_prints_every_count_it_keeps` — it lists every field explicitly, so it stops compiling. Add as the last field:

```rust
            excluded_dirs: vec![],
```

- [ ] **Step 8: Run the whole suite**

Run: `cargo test`
Expected: PASS, no warnings about unused fields.

- [ ] **Step 9: Commit**

```bash
git add src/walk.rs src/main.rs
git commit -m "fix(walk): name the first-level folders the exclusion rule skipped

The rule itself is unchanged and stays deliberate. What changes is that a
downloaded pack unpacking to SVC/ or Koikatu/ directly under the root can
no longer be passed over without the summary saying which folder it was."
```

---

### Task 2: Report unreadable directories, and fail the run

**Files:**
- Modify: `src/walk.rs` (`Scan`, `candidates`, tests module)
- Modify: `src/main.rs` (`Report`, `run`, `print_summary`, tests module)

**Interfaces:**
- Consumes: `Scan`/`Report` as left by Task 1.
- Produces: `walk::Scan::unreadable_dirs: Vec<(PathBuf, String)>` (path, I/O error message; sorted) and `Report::unreadable_dirs: u64` — a count, because `Report` holds counts and `Scan` holds the detail. `run` prints one `Failed to scan <dir>: <error>` line per entry and adds each to `Report::errors`.

- [ ] **Step 1: Write the failing walk test and its two helpers**

Add to the `tests` module in `src/walk.rs`:

```rust
    /// A directory the walk cannot list must not disappear. The scan still
    /// continues past it — one unreadable folder is not a reason to abandon the
    /// walk — but it is recorded, because an unreadable folder may hide any
    /// number of cards and nothing else in the program can say so.
    #[test]
    fn an_unreadable_directory_is_recorded_and_the_rest_of_the_scan_continues() {
        let d = Dir::new();
        touch(d.path(), "readable/keep.png");
        touch(d.path(), "locked/hidden.png");
        let locked = d.path().join("locked");

        if !deny_read(&locked) {
            eprintln!(
                "note: skipping an_unreadable_directory_is_recorded_and_the_rest_of_the_scan_continues \
                 — this environment would not make a directory unreadable; the recording in \
                 `candidates` is still in place, just unverified by this run"
            );
            return;
        }

        let scan = candidates(d.path());

        // Restore before asserting, so a failure still leaves a tree `Dir` can delete.
        restore_read(&locked);

        assert_eq!(scan.unreadable_dirs.len(), 1, "the locked folder is recorded");
        assert_eq!(scan.unreadable_dirs[0].0, locked);
        let files: Vec<String> = scan
            .files
            .iter()
            .map(|p| p.strip_prefix(d.path()).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(files, ["readable/keep.png"], "the readable sibling is still scanned");
    }

    /// Make `dir` unlistable, or return false if this environment will not allow
    /// it. The precondition is VERIFIED, not assumed: an ACL that was applied but
    /// does not bite — an elevated process, a filesystem that ignores it — would
    /// otherwise let the test pass while testing nothing, which is the exact
    /// failure mode this whole change is about.
    #[cfg(windows)]
    fn deny_read(dir: &Path) -> bool {
        let Ok(user) = std::env::var("USERNAME") else { return false };
        let applied = std::process::Command::new("icacls")
            .arg(dir)
            .args(["/deny", &format!("{user}:(RX)")])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        applied && std::fs::read_dir(dir).is_err()
    }

    #[cfg(windows)]
    fn restore_read(dir: &Path) {
        let Ok(user) = std::env::var("USERNAME") else { return };
        let _ = std::process::Command::new("icacls")
            .arg(dir)
            .args(["/remove:d", &user])
            .output();
    }

    #[cfg(unix)]
    fn deny_read(dir: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o000)).is_err() {
            return false;
        }
        std::fs::read_dir(dir).is_err()
    }

    #[cfg(unix)]
    fn restore_read(dir: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
    }

    #[cfg(not(any(windows, unix)))]
    fn deny_read(_dir: &Path) -> bool {
        false
    }

    #[cfg(not(any(windows, unix)))]
    fn restore_read(_dir: &Path) {}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --lib walk::tests::an_unreadable_directory`
Expected: FAIL to compile — `no field 'unreadable_dirs' on type 'Scan'`.

- [ ] **Step 3: Record instead of swallowing**

In `src/walk.rs`, add to `Scan`:

```rust
    /// Directories whose contents could not be listed, with the I/O error. The
    /// walk continues past them on purpose; what it may not do is leave no trace,
    /// which contradicts the same spec's rule that an unreadable FILE is an error.
    pub unreadable_dirs: Vec<(PathBuf, String)>,
```

In `candidates`, add the accumulator:

```rust
    let mut unreadable_dirs = Vec::new();
```

Replace:

```rust
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
```

with:

```rust
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                unreadable_dirs.push((dir, e.to_string()));
                continue;
            }
        };
```

And before the return:

```rust
    unreadable_dirs.sort();
    Scan { files: out, symlinked_pngs, excluded_dirs, unreadable_dirs }
```

- [ ] **Step 4: Run the walk tests to verify they pass**

Run: `cargo test --lib walk:: -- --nocapture`
Expected: PASS. If the environment refuses the ACL change, the `note:` line appears and that one test returns early — that is the documented outcome, not a failure.

- [ ] **Step 5: Write the failing run/summary test**

Add to the `tests` module in `src/main.rs`:

```rust
    /// An unreadable directory is an error, on the same footing as an unreadable
    /// file (2026-07-29 design, "Unreadable is not 'not a card'"). A directory can
    /// hide any number of cards, so a script that trusts the exit code must not be
    /// told the run succeeded when part of the tree was never examined.
    #[test]
    fn an_unreadable_directory_is_reported_and_fails_the_run() {
        let d = Dir::new();
        let r = d.path();
        std::fs::create_dir_all(r.join("locked")).unwrap();
        std::fs::write(r.join("locked").join("hidden.png"), b"x").unwrap();
        let locked = r.join("locked");

        if !crate::walk::tests::deny_read(&locked) {
            eprintln!(
                "note: skipping an_unreadable_directory_is_reported_and_fails_the_run — this \
                 environment would not make a directory unreadable; the error accounting in \
                 `run` is still in place, just unverified by this run"
            );
            return;
        }

        let mut out = Vec::new();
        let rep = run(r, false, None, &mut out);
        crate::walk::tests::restore_read(&locked);

        assert_eq!(rep.unreadable_dirs, 1);
        assert!(rep.errors >= 1, "an unreadable directory is an error");
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Failed to scan"), "reported per directory:\n{text}");

        let mut summary = Vec::new();
        print_summary(&rep, &mut summary);
        let text = String::from_utf8(summary).unwrap();
        assert!(text.contains("unreadable folders"), "and in the summary:\n{text}");
    }
```

To let `main.rs` reach those two helpers, mark the `walk` test module and the two functions `pub(crate)` in `src/walk.rs`:

```rust
#[cfg(test)]
pub(crate) mod tests {
```

and change `fn deny_read` / `fn restore_read` to `pub(crate) fn` in **all** of their `#[cfg]` variants (windows, unix, and the fallback).

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test --lib tests::an_unreadable_directory_is_reported`
Expected: FAIL to compile — `no field 'unreadable_dirs' on type 'Report'`.

- [ ] **Step 7: Count it, print it, fail on it**

In `src/main.rs`, add to `Report`:

```rust
    /// Directories the scan could not list. Counted as errors as well, so the
    /// exit code reflects a tree that was only partly examined.
    pub unreadable_dirs: u64,
```

In `run`, right after the `excluded_dirs` line from Task 1:

```rust
    for (dir, err) in &scan.unreadable_dirs {
        rep.unreadable_dirs += 1;
        rep.errors += 1;
        let _ = writeln!(out, "Failed to scan {}: {}", dir.display(), err);
    }
```

In `print_summary`, after the `errors` line:

```rust
    if rep.unreadable_dirs > 0 {
        let _ = writeln!(
            out,
            "  {:<11} {:<26} {:>5}",
            "", "unreadable folders", rep.unreadable_dirs
        );
    }
```

Add to the struct literal in `the_summary_prints_every_count_it_keeps`:

```rust
            unreadable_dirs: 0,
```

- [ ] **Step 8: Run the whole suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/walk.rs src/main.rs
git commit -m "fix(walk): report the directories the scan could not read

An unreadable file was already an error; an unreadable directory was a
bare continue. It may hide any number of cards, so it is now reported per
directory, counted, and reflected in the exit code."
```

---

### Task 3: `--any-extension`

**Files:**
- Modify: `src/main.rs` (`Args`, `USAGE`, `run`, all 15 `run(` call sites, tests module)
- Modify: `src/walk.rs` (`candidates` signature, `is_candidate`, all 5 `candidates(` call sites, tests module)
- Modify: `README.md` (usage block, "What changes" list)

**Interfaces:**
- Consumes: `Scan`/`Report` as left by Tasks 1 and 2.
- Produces: `Args::any_ext: bool`; `walk::candidates(root: &Path, any_ext: bool) -> Scan`; `run(root: &Path, dry_run: bool, search: Option<&str>, any_ext: bool, out: &mut dyn Write) -> Report` — `any_ext` is the **last** parameter before `out`, so every existing call site gains `false, ` immediately before its `&mut out` argument.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/main.rs`:

```rust
    #[test]
    fn any_extension_parses_and_is_off_by_default() {
        assert!(!args(&[]).unwrap().any_ext);
        assert!(args(&["--any-extension"]).unwrap().any_ext);
    }
```

Add to the `tests` module in `src/walk.rs`:

```rust
    /// Off by default the candidate set is exactly `.png`, unchanged. On, every
    /// regular file is a candidate — for a batch where a card is suspected of
    /// having been renamed, at the cost of opening every texture in the tree.
    #[test]
    fn any_extension_widens_the_candidate_set_without_changing_the_default() {
        let d = Dir::new();
        touch(d.path(), "card.png");
        touch(d.path(), "renamed.jpg");
        touch(d.path(), "notes.txt");

        let mut off: Vec<String> = candidates(d.path(), false)
            .files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        off.sort();
        assert_eq!(off, ["card.png"]);

        let mut on: Vec<String> = candidates(d.path(), true)
            .files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        on.sort();
        assert_eq!(on, ["card.png", "notes.txt", "renamed.jpg"]);
    }

    /// The reparse-point counter uses the SAME candidate test, not `.png`
    /// unconditionally: with `--any-extension` on, a symlinked `x.jpg` is a file
    /// the run would otherwise have examined, so it belongs in the count for the
    /// same reason a symlinked `.png` does.
    #[test]
    fn the_symlink_counter_follows_the_candidate_test() {
        let d = Dir::new();
        touch(d.path(), "target/actual.png");
        let link = d.path().join("linked.jpg");

        if !make_file_link(&link, &d.path().join("target").join("actual.png"), &d.path().join("target")) {
            eprintln!(
                "note: skipping the_symlink_counter_follows_the_candidate_test — this environment \
                 allowed neither a file symlink nor a junction; the shared candidate test in \
                 `candidates` is still in place, just unverified by this run"
            );
            return;
        }

        let off = candidates(d.path(), false).symlinked_pngs;
        let on = candidates(d.path(), true).symlinked_pngs;

        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir(&link);

        assert_eq!(off, 0, "a .jpg link is not a candidate by default");
        assert_eq!(on, 1, "but it is one under --any-extension, so it is counted");
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --lib any_extension`
Expected: FAIL to compile — `no field 'any_ext' on type 'Args'`, and `candidates` takes 1 argument.

- [ ] **Step 3: Add the flag**

In `src/main.rs`, add to `Args`:

```rust
    pub any_ext: bool,
```

Add the arm to `Args::parse`, next to `--dry-run`:

```rust
                "--any-extension" => a.any_ext = true,
```

Extend `USAGE`:

```rust
const USAGE: &str = "\
usage: koikatsu-hamster [--root <dir>] [--dry-run] [--any-extension] [search term]

  --root <dir>       directory to organise (default: the current directory)
  --dry-run          report what would move, change nothing
  --any-extension    examine every file, not just *.png — for a batch where a
                     card is suspected of having been renamed
  search term        cards whose full name contains it are filed one level deeper
";
```

- [ ] **Step 4: Thread it through the walk**

In `src/walk.rs`, change the signature and the two extension tests:

```rust
pub fn candidates(root: &Path, any_ext: bool) -> Scan {
```

Replace the symlink branch's test and the file branch's test with the shared helper:

```rust
                if is_candidate(&path, any_ext) {
                    symlinked_pngs += 1;
                }
```

```rust
            } else if ft.is_file() && is_candidate(&path, any_ext) {
```

Add below `has_png_extension`:

```rust
/// What the walk will pick up. One function so the file branch and the
/// reparse-point counter cannot drift apart: a link the run would have examined
/// has to be counted whichever setting made it a candidate.
fn is_candidate(p: &Path, any_ext: bool) -> bool {
    any_ext || has_png_extension(p)
}
```

Rename the existing symlink helper `make_png_link` to `make_file_link` in its three `#[cfg]` variants and at its one existing call site in `a_symlinked_png_is_skipped_but_counted` — the new test links a `.jpg`, and the name should stop claiming otherwise.

Update the other `candidates(` call sites inside `src/walk.rs`'s tests — `names()` (~line 92) and the two in `an_empty_root_yields_nothing` / `a_symlinked_png_is_skipped_but_counted` / `returns_the_full_result_sorted_…` — to pass `false`.

- [ ] **Step 5: Thread it through `run`**

In `src/main.rs`:

```rust
pub fn run(
    root: &Path,
    dry_run: bool,
    search: Option<&str>,
    any_ext: bool,
    out: &mut dyn Write,
) -> Report {
```

```rust
    let mut scan = walk::candidates(root, any_ext);
```

In `main`:

```rust
    let rep = run(&root, args.dry_run, args.search.as_deref(), args.any_ext, &mut out);
```

Every other `run(` call is in the tests module and passes `&mut out` (or `&mut out2`) last. Insert `false, ` before that final argument in each — there are 14:

```bash
# from the repo root; review the diff before staging
perl -0pi -e 's/\brun\(([^;]*?), (&mut out\d*)\)/run($1, false, $2)/g' src/main.rs
git diff src/main.rs | grep -c "^+.*run("
```

Expected: 15 changed `run(` lines (14 test call sites plus the one in `main`, which the perl also rewrites correctly because `args.search.as_deref()` precedes `&mut out` — verify that line reads `args.search.as_deref(), false, &mut out` and change it by hand to `args.any_ext` if the substitution produced `false`).

- [ ] **Step 6: Run the whole suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 7: Document the flag**

In `README.md`, replace the usage block:

```
koikatsu-hamster [--root <dir>] [--dry-run] [--any-extension] [search term]

  --root <dir>       directory to organise (default: the current directory)
  --dry-run          report what would move, change nothing
  --any-extension    examine every file, not just *.png — for a batch where a
                     card is suspected of having been renamed
  search term        cards whose full name contains it are filed one level deeper
```

And add to the "What changes" list, after the `already in place` / `symlinked` bullet:

```markdown
- A first-level folder skipped by the exclusion rule is **named** in the summary
  (`skipped output folders`). Skipping it is deliberate — that is how the tool stays
  idempotent over its own output — but a downloaded pack that unpacks to `SVC/` looks
  exactly like an output folder, and the name is what lets you tell them apart.
- A directory the scan **cannot read** is reported per directory and counted as an error,
  matching the rule for a file it cannot read. The walk still continues past it.
- `--any-extension` examines every file rather than just `*.png`, for a batch where a card
  is suspected of having been renamed. Off by default: with it on, every texture and readme
  in the tree is opened and probed.
```

- [ ] **Step 8: Commit**

```bash
git add src/main.rs src/walk.rs README.md
git commit -m "feat(cli): add --any-extension for cards that were renamed

Off by default, so the candidate set stays exactly *.png. The
reparse-point counter shares the same candidate test rather than
hard-coding .png, so the two cannot drift apart."
```

---

### Task 4: Build and deploy the executable

**Files:**
- Modify: `C:\Users\weiss\Desktop\scan\koikatsu-hamster.exe` (the deployed build)

**Interfaces:**
- Consumes: the finished binary from Tasks 1–3. Produces nothing further in code.

- [ ] **Step 1: Full test run**

Run: `cargo test`
Expected: PASS, zero failures. Note in the output whether the two ACL/symlink tests ran or printed their `note:` skip — say which in the handoff rather than reporting a clean run that quietly skipped two preconditions.

- [ ] **Step 2: Release build**

Run: `cargo build --release`
Expected: builds with no warnings; binary at `target/release/koikatsu-hamster.exe`.

- [ ] **Step 3: Smoke-test the new reporting against a real tree**

```bash
./target/release/koikatsu-hamster.exe --root "C:/Users/weiss/Desktop/scan" --dry-run
```

Expected: the banner, then a summary. Because `scan\` holds this program's own `Koikatu\` and `KoikatsuSunshine\` output folders, the summary MUST now carry a `skipped output folders` line naming them. That line's absence means Task 1 did not reach the deployed path.

- [ ] **Step 4: Deploy**

```bash
cp target/release/koikatsu-hamster.exe "C:/Users/weiss/Desktop/scan/koikatsu-hamster.exe"
"C:/Users/weiss/Desktop/scan/koikatsu-hamster.exe" --root "C:/Users/weiss/Desktop/scan" --dry-run | head -3
```

Expected: the banner reads `koikatsu-hamster 0.1.0 (rust)` from the freshly copied file.

- [ ] **Step 5: Commit**

Nothing to commit in the repo — the deployed `.exe` lives outside it. Confirm the tree is clean:

```bash
git status --short
```

Expected: empty.
