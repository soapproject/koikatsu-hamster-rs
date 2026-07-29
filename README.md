# koikatsu-hamster-rs

A Rust rewrite of [koikatsu-hamster](https://github.com/soapproject/koikatsu-hamster) — it sorts
Koikatsu (and related IllGames/Illusion titles) card PNGs into `[Game]/[Female|Male|Coordinate]`
folders by reading the card data appended after the PNG's `IEND` chunk.

**Status: design complete, implementation not started.** See
[the design document](docs/superpowers/specs/2026-07-29-card-organizer-design.md).

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
