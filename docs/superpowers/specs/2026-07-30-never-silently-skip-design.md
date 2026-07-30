# Never silently skip — design

Date: 2026-07-30
Status: approved

Amends [2026-07-29-card-organizer-design.md](2026-07-29-card-organizer-design.md). That spec's
rules stand; this one closes three paths by which a file can be passed over without appearing
anywhere in the run's output.

## Why

The program's stated principle, written into `Report`'s own field comments, is that *a file the
run touched and did nothing about is exactly what the summary must not hide*. `already_filed`
and `symlinked` exist for that reason. Three paths still violate it.

A 2026-07-30 review found them while answering a different question: after a batch was
mishandled by hand, could the tool itself be trusted to have found every card? The answer has to
be readable off the summary, and today it is not.

| # | path | today | why it matters |
|---|---|---|---|
| 1 | a first-level directory whose name is a game folder name is excluded by the rule in `plan.rs::is_in_dest_folder` | not walked, not named, not counted | a downloaded pack that unpacks to `SVC/` or `Koikatu/` directly under the root is skipped whole, and nothing says so |
| 2 | only `.png` is a candidate (`walk.rs::has_png_extension`) | never a candidate | a card renamed `x.png.bak` or `x.jpg` is invisible |
| 3 | a directory that cannot be read (`read_dir` fails) | `continue`, no count, no message | contradicts the same spec's rule that an unreadable **file** is an error (design 2026-07-29, "Unreadable is not 'not a card'") |

Only #3 is a defect against the existing spec. #1 is a deliberate, documented trade-off and #2
is the tool's definition of a candidate — but *being skipped* and *being skipped silently* are
different things, and only the second is at issue here.

## Scope

Reporting, plus one opt-in flag. **Behaviour is otherwise unchanged**: the exclusion rule is not
touched, the default candidate set is not widened, and no run moves a file it would not have
moved before.

Explicitly out of scope:

- **Changing the exclusion rule.** The 2026-07-29 design (§Exclusion rule) accepts that a user
  folder named `svc/` directly under the root is skipped, because the opposite error loses
  idempotence and starts moving already-filed cards. That argument stands. Naming the skipped
  folder resolves the visibility problem without reopening it.
- **Counting the cards inside an excluded folder.** In the ordinary case the excluded folder is
  this program's own output — up to ~160k cards — and walking it on every run to produce a
  number contradicts the design's performance rules. The folder's *name* is what tells the user
  it was not an output folder at all; the count would not change that judgement.

## Behaviour

### Excluded first-level folders are named

`Scan` gains `excluded_dirs: Vec<String>`: the names of directories the exclusion rule rejected,
sorted, deduplicated. Recording happens where the rule already runs, so it costs one push and no
filesystem call. It can only ever contain first-level names, because that is all the rule looks
at.

Reported, not an error. Exit code is unaffected — this is the `already_filed`/`symlinked`
category: deliberately not processed, but said out loud.

### Unreadable directories are reported and counted as errors

`Scan` gains `unreadable_dirs: Vec<(PathBuf, String)>` — path and the I/O error's message. The
scan still continues to the next directory: one unreadable folder is not a reason to abandon the
walk, and that part of the current behaviour is deliberate.

Each entry prints `Failed to scan <dir>: <error>`, matching the existing `Failed to handle
<file>: <error>` line, and increments `Report::errors`. The run therefore exits 1 through the
existing `exit(if rep.errors == 0 { 0 } else { 1 })` — no change to the exit logic itself.

An unreadable directory may hide any number of cards, so it is at least as serious as one
unreadable file, which is already an error. A script that trusts the exit code must not be told
a run succeeded when part of the tree was never examined.

### `--any-extension`

`candidates(root, any_ext)` treats every regular file as a candidate when `any_ext` is true;
otherwise the `.png` test is unchanged. Off by default, so the default path is byte-identical to
today's.

For a batch where a card is suspected to have been renamed. It is not the everyday setting: with
it on, every texture, `.zipmod` and readme in the tree gets opened and probed, and they all land
in the existing silent `non_cards` count.

The reparse-point counter uses the **same** candidate test, not `.png` unconditionally: with
`--any-extension` on, a symlinked `x.jpg` is a file the run would otherwise have examined, so it
belongs in `symlinked` for the same reason a symlinked `.png` does. Keeping the two tests in one
place is also what stops them drifting apart.

## Design

Three changes, two files, no new module and no new cross-module dependency. The two new `Scan`
fields travel exactly the path `symlinked_pngs` already travels: filled by `walk`, copied into
`Report` at the top of `run`, printed by `print_summary`.

```
walk.rs    Scan { files, symlinked_pngs, excluded_dirs, unreadable_dirs }
           candidates(root, any_ext) -> Scan
main.rs    Report { …, excluded_dirs, unreadable_dirs }   run() copies them across
           print_summary prints the two new lines
```

`plan.rs::is_in_dest_folder` is unchanged: `walk` records what it already decides.

### Summary output

```
      skipped output folders    Koikatu, SVC
      unreadable folders        1
```

Each line is omitted when its collection is empty, so an ordinary run in a freshly unpacked
folder prints neither. This follows the existing summary, which always prints its counts; the
difference is deliberate — a zero here is the normal case and a permanent `0` line trains the
eye to skip the row where the whole point is to catch it.

## Testing

Following the existing style: real directories under `tempdir::Dir`, no mocking.

- `SVC/x.png` directly under the root is not a candidate **and** `excluded_dirs` contains `SVC`;
  `pack/SVC/x.png` is a candidate and does not appear in `excluded_dirs` — pins that the
  "first level only" rule was not widened while adding the recording.
- `--any-extension` off: `a.jpg` is not a candidate. On: it is, and `.png` handling is unchanged.
- An unreadable directory is recorded, and the walk still returns the cards from a sibling
  directory that is readable.
- A run over a tree containing an unreadable directory returns `rep.errors > 0`.
- The summary test (`the_summary_prints_every_count_it_keeps`) covers the two new lines,
  including that each is omitted when empty.

**On making a directory unreadable in a test:** Windows needs an ACL change (`icacls`), which
some environments refuse. Follow the precedent set by the symlink tests: if the setup cannot be
performed, print a `note:` to stderr and return, rather than asserting something weaker. A test
that quietly passes because it could not construct its own precondition is the failure mode this
whole spec is about.
