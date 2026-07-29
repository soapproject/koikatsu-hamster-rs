# Koikatsu card organizer (Rust) — design

Date: 2026-07-29
Status: approved

## Why

`koikatsu-hamster` (C#, `ws/koikatsu-hamster`) files Koikatsu card PNGs into
`[Game]/[Female|Male|Coordinate]` folders. It works, but a real 2026-07-29 batch
(2267 cards) exposed a defect class that argues for a rewrite rather than a patch, and the
correct algorithm has already been written and verified in Rust elsewhere.

**The defect.** `Program.FindPngFiles` excludes a file when *any* `GameType` enum name occurs
as a **substring of the file's absolute directory path**:

```csharp
!gameTypeNames.Any(gameTypeName => s.Directory.FullName.Contains(gameTypeName))
```

The enum includes the sentinel `Unknown`, and the comparison is substring-anywhere rather than
path-component-under-the-scan-root. Two real cards were silently dropped by this in one batch:

| card | folder that triggered the exclusion | matched name |
|---|---|---|
| `[IseeU] [card] genshin_unkown_god.png` | `…/Genshin/Unknown god/card/` | `Unknown` |
| `Koikatu_F_20240626232405280_赛娜.png` | `…/Koikatu_F_20240626232405280_赛娜/` | `Koikatu` |

Both were verified by putting the card alone in an empty folder and re-running hamster: it files
them correctly. So parsing is not at fault — the IEND scan returns the same offset as a chunk
walk on both files, and `ParseCard` already consumes the `ProductNo` int before the marker, so
the version-`0.0.0` card layout is handled. Only the directory filter is wrong.

Card-pack folders named after a character (`Koikatu_F_…`) or containing words like `Unknown`
are common, so the real-world miss rate is not marginal — and nothing is printed when it
happens, because the `Console.WriteLine` on the unknown-game branch is commented out.

**Why a new program rather than a patch.** The parsing logic has already been rewritten in Rust
in `ws/deduplate` (`core.rs::png_char_block`, `card.rs`, `msgpack.rs`) and exercised against
real cards. Reusing that verified code, in a language with a faster cold run over ~10k files,
is worth more than another round of edits to the C# project, which is no longer being developed.

## Scope

Exactly what hamster does: walk a directory, identify card PNGs, move them into
`[Game]/[CardType-or-sex]` folders. Explicitly **out of scope**: extraction, Emotion Creators
conversion, scene-card cleanup, dl-pipeline orchestration.

Scene cards are *recognized* but never moved — see Behaviour, where the reason is that without
recognizing them the "unrecognized marker" report is drowned out.

## Code ownership

The new repository owns its copy of the parser. `deduplate` is not modified — not its
`Cargo.toml`, not its module boundaries. The cost is a second copy of the marker table and the
msgpack decoder; the benefit is that this tool ships without dragging in a Tauri GUI crate and
without restructuring a project that is in use by other people.

If `deduplate` is ever reworked, it can depend on this crate instead; the module layout below
keeps that a `lib.rs` away.

## Architecture

```
koikatsu-hamster-rs/
  src/
    main.rs      CLI entry: flags, walk, report, (interactive) wait for key
    walk.rs      directory traversal + the exclusion rule
    png.rs       PNG chunk walk -> (payload offset, payload len)
    card.rs      payload -> CardMeta { game, card_type, sex, name, personality }
    msgpack.rs   minimal msgpack decoder (ported from deduplate, keeps the depth cap)
    plan.rs      CardMeta + source path -> destination path, incl. name collisions
  tests/         integration tests over synthesised cards
```

Data flows one way: `walk` yields candidate files → `png` locates the payload → `card` decodes
metadata → `plan` computes a destination → `main` prints or moves. Each layer consumes only the
previous layer's output: `card.rs` knows nothing about the filesystem, `plan.rs` knows nothing
about the PNG format. All three are unit-testable in isolation.

Single binary crate, no external dependencies. Argument parsing (three flags) is hand-rolled;
pulling `clap` and `rmp-serde` would cost a dependency tree, compile time and binary size, and
swapping the hand-written msgpack decoder for `rmp-serde` would discard the very code this
rewrite exists to reuse — including its "a malformed card must not abort the process" fix.

## Behaviour

### Marker table

| marker | destination |
|---|---|
| `【KoiKatuChara】` `【KoiKatuCharaS】` `【KoiKatuCharaSP】` | `Koikatu/{Female,Male}` |
| `【KoiKatuClothes】` | `Koikatu/Coordinate` |
| `【KoiKatuCharaSun】` | `KoikatsuSunshine/{Female,Male}` |
| `【HCChara】` `【HCPChara】` | `HoneyCome/{Female,Male}` |
| `【SVChara】` | `SVC/{Female,Male}` |
| `【SVClothes】` | `SVC/Coordinate` |
| `【ACChara】` | `Aicomi/{Female,Male}` |
| `【ACClothes】` | `Aicomi/Coordinate` |
| `【EroMakeChara】` | `EmotionCreators/Character` |

`【EroMakeChara】` is commented out in hamster, which makes unconverted Emotion Creators cards
invisible: hamster leaves them where they are and says nothing, so getting the pipeline order
wrong loses a whole batch silently. Filing them under `EmotionCreators/` surfaces the count, and
`FromECtoKK` still finds them because it walks the working directory.

`AiSyoujyo` and `RoomGirl` markers stay unsupported: those games are not installed here, so the
branches could not be verified against a real card.

### Exclusion rule

Skip only directories **directly under `<root>`** whose name is **exactly** a game folder name
(`Koikatu`, `KoikatsuSunshine`, `HoneyCome`, `SVC`, `Aicomi`, `EmotionCreators`). Comparison is
per path component, never substring.

`Unknown` is not in that set. It is an enum sentinel, not a folder this program creates at the
top level, and treating it as one is the original bug.

Deeper directories that happen to share a game's name are not output folders and are not
skipped. Idempotence is achieved by the exclusion alone; destination-equals-source checking was
considered and rejected because re-parsing an already-filed collection (~160k cards) on every
run is too slow to be worth the extra safety net.

### Per-file decision order

| condition | action |
|---|---|
| not a PNG, or no payload after the first IEND | count only; not listed per file (there are thousands of textures) |
| the length-prefixed string at the marker position matches `^\d+(\.\d+)+$` (e.g. `1.0.4.2`) | KStudio scene card: count, leave in place |
| marker not in the table | **print file name and the marker**, leave in place |
| in table, not a Character card | move to `{Game}/{CardType}` |
| in table, Character | read `sex` from the `Parameter` block → `{Game}/{Female,Male}`; unreadable → `{Game}/Unknown` |
| parse fails part-way | **print file name and the error**, leave in place, continue |

Recognizing scene cards is what keeps the "unrecognized marker" line meaningful: one real batch
held 278 of them, which would otherwise bury a genuine miss in noise. They are counted, never
moved — consistent with scene handling being out of scope.

### Collisions and the optional search term

Destination name already taken → `name(1).png`, `name(2).png`, … (hamster's behaviour, kept).

The optional positional search term is kept: when the character's full name contains it
(case-insensitive), that card goes to `{Game}/{sex}/{searchTerm}/`. Cards that do not match go
to `{Game}/{sex}` as usual — the term sorts matches into a subfolder, it does not filter the
run. Non-Character cards ignore the term entirely, since they carry no name.

## Invocation

No arguments behaves as today — walk the current directory, move files — so the
drop-the-exe-in-the-folder-and-double-click habit is unchanged. What changes:

- **The end-of-run key wait only happens when stdin is a terminal** (`std::io::IsTerminal`).
  Redirected stdin exits immediately. hamster's unconditional `Console.ReadKey()` throws
  `InvalidOperationException` under a redirected stdin and hangs under
  `Start-Process -WindowStyle Hidden`, which is why it cannot be scripted.
- `--root <dir>` overrides the working directory.
- `--dry-run` prints the same output and moves nothing.
- A version banner is printed at startup (`koikatsu-hamster 0.1.0 (rust)`) so a same-named
  binary is still identifiable.
- Exit code 0 when the error count is 0, otherwise 1.

## Reporting

Per-file move lines keep hamster's format (`Move file: X to Y`), followed by a summary:

```
--- summary ---
  moved       Koikatu/Female            1789
              Koikatu/Coordinate         332
              Koikatu/Male                22
              KoikatsuSunshine/Female    124
              EmotionCreators/Character    0
  left alone  scene cards                278
              non-card images           1790
              unrecognized markers         0
              errors                       0
```

Unrecognized markers and errors have already been printed per file; the summary exists so the
counts do not require scrolling back.

## Testing

Three layers, all over synthesised cards — no local-only fixtures. `deduplate`'s
`tests/common/mod.rs` already has msgpack fixture writers (`mp_str`, `mp_map`, `mp_arr`,
`mp_uint`) that build byte-exact cards; those are ported.

**Unit**
- `png.rs`: well-formed card; chunk length running past EOF; IEND straddling a 4096-byte
  boundary; zero-length payload.
- `card.rs`: `ProductNo` consumed before the marker; version-`0.0.0` layout; marker absent from
  the table returns `Unrecognized(marker)` rather than a guess; `Parameter` block running past
  EOF; missing `sex` field.
- `msgpack.rs`: the seven ported tests, including the nesting-depth cap.
- `plan.rs`: destination per marker; unknown `sex`; search-term subfolder; collision renaming to
  `name(1)`, `name(2)`.

**Regression** — the two real misses become test names:
- `walk` must NOT exclude `<root>/ISEEU/Genshin/Unknown god/card/x.png`
- `walk` must NOT exclude `<root>/pack/Koikatu_F_20240626232405280_x/x.png`
- `walk` MUST exclude `<root>/Koikatu/Female/x.png`
- `walk` must NOT exclude `<root>/somewhere/Koikatu/x.png` (not a top-level output folder)

**Integration**: build a temp tree containing each of those path shapes, run the whole flow,
assert landing paths, summary counts and exit code; run a second time and assert 0 moved
(idempotence).

## Project

- New repository at `ws/koikatsu-hamster-rs`, alongside `koikatsu-hamster` and `deduplate`.
- Rust 2021, no external dependencies, `cargo build --release` produces one executable.
- The executable is named `koikatsu-hamster.exe` so it drops in over the existing one and no
  existing habit, script or note has to change; the version banner distinguishes them.
- MIT, matching the author's other repositories.
- The C# repository is left untouched.
