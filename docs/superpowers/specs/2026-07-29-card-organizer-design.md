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

Skip only directories **directly under `<root>`** whose name is a game folder name
(`Koikatu`, `KoikatsuSunshine`, `HoneyCome`, `SVC`, `Aicomi`, `EmotionCreators`). Comparison is
of the whole path component against the whole name — never a substring — and is
**case-insensitive**.

Case-insensitive because the target filesystem is: on Windows, the folder this program
created as `Koikatu` is the same folder Explorer will happily show as `koikatu`, and a
case-sensitive comparison would fail to recognise the program's own output and re-file it into
itself. It is also the safe direction for the error that remains: on a case-sensitive filesystem
a user folder named `svc/` or `aicomi/` directly under the root is skipped, so the cards under
it are never scanned — they stay exactly where the user put them, untouched. The opposite error
loses the idempotence guarantee and starts moving already-filed cards around, which is the
failure this program was written to end.

`Unknown` is not in that set. It is an enum sentinel, not a folder this program creates at the
top level, and treating it as one is the original bug.

Deeper directories that happen to share a game's name are not output folders and are not
skipped — a `pack/Koikatu/Female/` inside a downloaded archive is somebody else's layout, and
its cards are consolidated into this root's own folders like any others.

The exclusion carries idempotence on its own for every ordinary run. What was rejected is
*pre-emptive* destination-equals-source checking — re-parsing an already-filed collection
(~160k cards) on every run to find out whether each card is where it belongs is too slow to be
worth it. A card that reaches the move step is checked at that point, after its parse has
already happened, and skipped if its destination is the folder it is already in; that costs
nothing and stops the one failure it guards against, a card renamed onto its own successor
(`x.png` → `x(1).png`) and again on the next run.

One case that check cannot catch, accepted: run inside an already-organised `…/Koikatu`
folder — that folder is then the root, so a card in `Female/` computes a destination of
`…/Koikatu/Koikatu/Female`, which is not its parent. The collection is re-filed one level
deeper. Nothing is lost, duplicated or suffixed, and the second run is a no-op because
`Koikatu` is a first-level component again. Catching it would need an "ends with
`{Game}/{leaf}`" rule, which is exactly the consolidation behaviour of the paragraph above.

### Per-file decision order

| condition | action |
|---|---|
| not a PNG, or no payload after the first IEND | count only; not listed per file (there are thousands of textures) |
| marker not in the table, and the payload read from offset 0 as a length-prefixed string matches `^\d+(\.\d+)+$` (e.g. `1.0.4.2`) | KStudio scene card: count, leave in place |
| marker not in the table, and not a version string | **print file name and the marker**, leave in place |
| in table, not a Character card | move to `{Game}/{CardType}` |
| in table, Character | read `sex` from the `Parameter` block → `{Game}/{Female,Male}`; undeterminable → `{Game}/Unknown` |
| structure broken part-way through | **print file name and the error**, leave in place, continue |
| the file cannot be read at all | **print file name and the I/O error**, leave in place, continue |

**Undeterminable `sex` versus a broken card.** The marker has already told us which
game's folder the card belongs in, so a card whose structure parses but whose `sex`
cannot be established is *placeable* and goes to `{Game}/Unknown`. That covers all
three ways it can be undeterminable: no `Parameter` entry in `lstInfo`, a `Parameter`
block that does not decode, and a missing or non-integer `sex` field. None of them is
a reason to leave a card where it lies, and none of them may fail the run's exit code
— one odd card in a 2000-card batch must not make the whole run report failure.

`malformed` is reserved for a card whose *structure* is broken: a truncated payload,
an undecodable block table, or a `Parameter` entry whose `pos`/`size` do not describe
a region of the payload. Those are reported per file and counted as errors.

**Unreadable is not "not a card".** A file that cannot be read — permission denied, a
Windows sharing violation because the card is open in the character maker, a network
share that blinked, a file deleted between the walk and the read — is reported and
counted as an error. It is never folded into the silent `non-card image` count: doing
so is how the C# version came to call a real card a texture and still exit 0.

Recognizing scene cards is what keeps the "unrecognized marker" line meaningful: one real batch
held 278 of them, which would otherwise bury a genuine miss in noise. They are counted, never
moved — consistent with scene handling being out of scope.

**A recognized marker always wins.** The scene probe runs only after the marker lookup misses.
Probing first would mean reading a real card's `ProductNo` low byte (`0x64`) as a 100-byte string
length and asking whether 100 bytes of unrelated payload happen to spell a version number — it
practically never would, but nothing structural stops it, and a card misfiled that way leaves no
diagnostic behind. Checking the marker first makes the guarantee absolute rather than
probabilistic. Scene cards still classify correctly: their payload opens with the version string
where a chara card keeps its marker, so the lookup misses and the probe matches.

One consequence, accepted: the marker read now happens before the probe on every file, so a
payload too short to survive that read reports `malformed` rather than `scene`. A real scene card
carries the whole scene blob and is never that small; only a synthetic near-empty fixture is.

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
              already in place             0
              symlinked .png files         0
              errors                       0
```

Unrecognized markers and errors have already been printed per file; the summary exists so the
counts do not require scrolling back.

`already in place` and `symlinked .png files` are the two ways a card-shaped file can be
skipped without being an error: one already sits in the folder it would be moved to, the other
is a symlink or reparse point the walk refuses to follow. Both are counted rather than passed
over in silence — a file the run met and did nothing about is exactly what a summary must not
hide, and hiding it is the defect this rewrite exists to end.

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
