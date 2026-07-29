# koikatsu-hamster-rs

A Rust rewrite of [koikatsu-hamster](https://github.com/soapproject/koikatsu-hamster) — it sorts
Koikatsu (and related IllGames/Illusion titles) card PNGs into
`[Game]/[Female|Male|Coordinate|Character]` folders by reading the card data appended after the
PNG's `IEND` chunk.

See [the design document](docs/superpowers/specs/2026-07-29-card-organizer-design.md) for why
this exists and how it decides.

## Usage

Drop `koikatsu-hamster.exe` in the folder where you keep your cards and double-click it, exactly
like the C# version. It walks that folder and everything under it, and files each card into
`[Game]/[Female|Male|Coordinate|Character]`.

```
koikatsu-hamster [--root <dir>] [--dry-run] [search term]

  --root <dir>   directory to organise (default: the current directory)
  --dry-run      report what would move, change nothing
  search term    cards whose full name contains it are filed one level deeper
```

The search term becomes a folder name, so it has to be one: a term containing a path separator
or `..` is a usage error (exit 2) rather than something that quietly files matched cards
somewhere else entirely.

The run ends with a summary of what moved and what was left alone. It pauses for a keypress only
when stdin is a terminal, so it can be called from a script; the exit code is 1 if any file
errored, otherwise 0. A `--root` that doesn't exist or can't be read is reported on stderr and
exits non-zero before anything is scanned, rather than printing an empty summary and exiting 0.

## Why a rewrite

The C# version excludes a file from the scan when any `GameType` enum name — including the
sentinel `Unknown` — appears as a *substring of its absolute directory path*, rather than as a
path component directly under the scan root. Ordinary card-pack folder names collide with that:

| card | folder | matched name |
|---|---|---|
| `[IseeU] [card] genshin_unkown_god.png` | `…/Genshin/Unknown god/card/` | `Unknown` |
| `Koikatu_F_20240626232405280_….png` | `…/Koikatu_F_20240626232405280_…/` | `Koikatu` |

Both cards were skipped without a message, in a batch of 2267. Placing either one alone in an
empty folder and re-running produces a correct move, so the parser is not at fault — only the
directory filter is.

## What changes

- Scan exclusion matches **path components directly under the root**, never substrings, and
  `Unknown` is not among them.
- Unrecognized markers and parse failures are **printed and counted** instead of silently
  skipped, with a summary at the end of the run.
- A file that cannot be **read** — permission denied, a sharing violation because the card is
  open in the character maker, a network share that blinked — is reported as an error, never
  quietly counted as an ordinary image.
- Every other way a card-shaped file can be passed over gets a summary line too: one already
  sitting in the folder it would be moved to (`already in place`), and one the walk refuses to
  follow because it is a symlink or reparse point (`symlinked .png files`).
- `【EroMakeChara】` (Emotion Creators) is recognized, so unconverted cards are visible rather
  than left behind without a word.
- KStudio scene cards are recognized and counted, so they no longer drown out that report. They
  are never moved.
- The end-of-run key wait happens **only when stdin is a terminal**, so the tool can be scripted.
  Added `--root` and `--dry-run`; exit code is non-zero when any file errored.

Double-clicking the executable in a folder still walks that folder and moves the cards, exactly
as before.

## License

MIT
