# Koikatsu card organizer (Rust) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `koikatsu-hamster.exe` in Rust — it walks a directory, identifies Koikatsu-family card PNGs by the data appended after the PNG `IEND` chunk, and moves each into `[Game]/[Female|Male|Coordinate|Character]`.

**Architecture:** One binary crate, no external dependencies. Data flows one way: `walk` yields candidate files → `png` locates the appended payload → `card` decodes it into `CardMeta` → `plan` computes a destination → `main` prints or moves. Each layer consumes only the previous layer's output, so `card` never touches the filesystem beyond opening the file it was handed, and `plan` never parses bytes.

**Tech Stack:** Rust 2021, std only. Unit tests live in `#[cfg(test)] mod tests` inside each `src` module (a binary crate's modules are not reachable from `tests/`). The end-to-end test in `tests/cli.rs` drives the built executable through `std::process::Command` with `env!("CARGO_BIN_EXE_koikatsu-hamster")`.

**Source material:** `../deduplate/src-tauri/src/{msgpack.rs,card.rs,core.rs,organize.rs}` hold a working, card-verified implementation of the same parsing. This plan ports it. `deduplate` itself must not be modified.

## Global Constraints

- Rust edition 2021, `rust-version = "1.70"` (`std::io::IsTerminal` stabilised there).
- **Zero external dependencies.** `[dependencies]` stays empty. No `clap`, no `serde`, no `rmp-serde`.
- Package name and binary name are both `koikatsu-hamster`, producing `koikatsu-hamster.exe`.
- License MIT (`LICENSE` already present at the repo root).
- Version banner printed on every run, first line: `koikatsu-hamster 0.1.0 (rust)`.
- Move lines keep the C# format exactly: `Move file: {name} to {full destination path}`.
- Exit code 0 when the error count is 0, otherwise 1.
- Never abort the process over one bad file: every per-file failure is reported and the walk continues.
- `Unknown` is never a member of the destination-folder set used by the exclusion rule.

---

### Task 1: Crate scaffold and the MessagePack decoder

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs` (placeholder entry point, replaced in Task 6)
- Create: `src/msgpack.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `msgpack::Value` (enum with `Nil, Bool(bool), Int(i64), UInt(u64), F32(f32), F64(f64), Str(String), Bin(Vec<u8>), Array(Vec<Value>), Map(Vec<(Value, Value)>)`), its methods `get(&self, key: &str) -> Option<&Value>`, `as_str(&self) -> Option<&str>`, `as_i64(&self) -> Option<i64>`, `as_array(&self) -> Option<&[Value]>`, and `msgpack::decode(buf: &[u8]) -> Result<Value, String>`.

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "koikatsu-hamster"
version = "0.1.0"
description = "Sorts Koikatsu card PNGs into per-game folders"
authors = ["soapproject"]
license = "MIT"
repository = "https://github.com/soapproject/koikatsu-hamster-rs"
edition = "2021"
rust-version = "1.70"

[dependencies]

[profile.release]
strip = true
```

- [ ] **Step 2: Create a placeholder `src/main.rs` so the crate builds**

```rust
mod msgpack;

fn main() {
    println!("koikatsu-hamster 0.1.0 (rust)");
}
```

- [ ] **Step 3: Write `src/msgpack.rs` with the failing tests first**

Create the file containing ONLY the test module below, plus `pub enum Value {}` stubs sufficient to make the intent readable. Do not write the decoder yet.

```rust
//! Minimal MessagePack decoder. Decode-only, zero dependencies.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_string_keyed_map_like_a_card_parameter_block() {
        // fixmap(3) { "version": "0.0.5", "sex": 1, "personality": 19 }
        let buf = [
            0x83, 0xA7, b'v', b'e', b'r', b's', b'i', b'o', b'n', 0xA5, b'0', b'.', b'0', b'.',
            b'5', 0xA3, b's', b'e', b'x', 0x01, 0xAB, b'p', b'e', b'r', b's', b'o', b'n', b'a',
            b'l', b'i', b't', b'y', 0x13,
        ];
        let v = decode(&buf).expect("decode");
        assert_eq!(v.get("version").and_then(|x| x.as_str()), Some("0.0.5"));
        assert_eq!(v.get("sex").and_then(|x| x.as_i64()), Some(1));
        assert_eq!(v.get("personality").and_then(|x| x.as_i64()), Some(19));
    }

    #[test]
    fn decodes_nested_array_of_maps_like_a_block_table() {
        // fixmap(1) { "lstInfo": fixarray(1) [ fixmap(2) { "name":"Parameter", "size":5 } ] }
        let buf = [
            0x81, 0xA7, b'l', b's', b't', b'I', b'n', b'f', b'o', 0x91, 0x82, 0xA4, b'n', b'a',
            b'm', b'e', 0xA9, b'P', b'a', b'r', b'a', b'm', b'e', b't', b'e', b'r', 0xA4, b's',
            b'i', b'z', b'e', 0x05,
        ];
        let v = decode(&buf).expect("decode");
        let list = v.get("lstInfo").and_then(|x| x.as_array()).expect("lstInfo");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].get("name").and_then(|x| x.as_str()), Some("Parameter"));
        assert_eq!(list[0].get("size").and_then(|x| x.as_i64()), Some(5));
    }

    #[test]
    fn a_non_utf8_string_decodes_lossily_instead_of_failing() {
        let buf = [0xA2, 0xFF, b'a'];
        let v = decode(&buf).expect("must not error on invalid UTF-8");
        assert_eq!(v.as_str(), Some("\u{FFFD}a"));
    }

    #[test]
    fn truncated_input_is_an_error_not_a_panic() {
        let buf = [0xA5, b'0', b'.']; // fixstr(5) but only 2 bytes follow
        assert!(decode(&buf).is_err());
    }

    /// The decoder recurses per container, so deeply nested input would blow the
    /// stack — and a stack overflow in Rust ABORTS the process, which no malformed
    /// card may be allowed to do. It must be an ordinary `Err`.
    #[test]
    fn pathological_nesting_is_an_error_not_a_stack_overflow() {
        let mut buf = vec![0x91u8; 100_000];
        buf.push(0x00);
        let e = decode(&buf).expect_err("must refuse, not recurse");
        assert!(e.contains("nested deeper"), "{e}");

        let mut m = vec![0x81u8; 100_000];
        m.extend_from_slice(&[0xA1, b'k', 0x00]);
        assert!(decode(&m).is_err());
    }

    /// The cap must not reject the nesting real cards use: a block table is a map
    /// of an array of maps — three levels.
    #[test]
    fn nesting_within_the_cap_still_decodes() {
        let mut buf = vec![0x91u8; 60];
        buf.push(0x07);
        let v = decode(&buf).expect("60 levels is well inside the cap");
        let mut cur = &v;
        for _ in 0..60 {
            cur = &cur.as_array().expect("array")[0];
        }
        assert_eq!(cur.as_i64(), Some(7));
    }

    #[test]
    fn wide_types_decode() {
        assert_eq!(decode(&[0xCE, 0x00, 0x01, 0x00, 0x00]).unwrap().as_i64(), Some(65536));
        assert_eq!(decode(&[0xD0, 0xFD]).unwrap().as_i64(), Some(-3));
        assert_eq!(decode(&[0xD9, 0x02, b'h', b'i']).unwrap().as_str(), Some("hi"));
        assert!(matches!(decode(&[0xC4, 0x02, 0x01, 0x02]).unwrap(), Value::Bin(b) if b == vec![1, 2]));
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test msgpack`
Expected: FAIL — `cannot find function decode in this scope`.

- [ ] **Step 5: Write the decoder above the test module**

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    UInt(u64),
    F32(f32),
    F64(f64),
    Str(String),
    Bin(Vec<u8>),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

impl Value {
    /// Card blocks are string-keyed maps; look a key up by name.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(pairs) => pairs
                .iter()
                .find(|(k, _)| k.as_str() == Some(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::UInt(u) => i64::try_from(*u).ok(),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(v) => Some(v),
            _ => None,
        }
    }
}

/// How deep a container may nest before decoding is refused. Card blocks are maps
/// of arrays of maps — three or four levels — so a real card never comes close. The
/// cap exists because the decoder recurses: a hostile or corrupt block of
/// consecutive `0x91` bytes would otherwise recurse once per byte and overflow the
/// stack, which in Rust ABORTS the process rather than unwinding.
const MAX_DEPTH: usize = 64;

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    depth: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0, depth: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).ok_or("length overflow")?;
        let s = self.buf.get(self.pos..end).ok_or("truncated msgpack")?;
        self.pos = end;
        Ok(s)
    }
    fn u8v(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn be(&mut self, n: usize) -> Result<u64, String> {
        let s = self.take(n)?;
        Ok(s.iter().fold(0u64, |a, b| (a << 8) | *b as u64))
    }
    /// Card names are not guaranteed valid UTF-8 — decode lossily, never fail.
    fn string(&mut self, n: usize) -> Result<Value, String> {
        Ok(Value::Str(String::from_utf8_lossy(self.take(n)?).into_owned()))
    }
    /// Enters one container level, refusing to recurse past `MAX_DEPTH`. On the
    /// error path the depth counter is deliberately not restored: the whole decode
    /// is being abandoned, and every frame returns `Err`.
    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(format!("msgpack nested deeper than {MAX_DEPTH} levels"));
        }
        Ok(())
    }
    fn array(&mut self, n: usize) -> Result<Value, String> {
        self.enter()?;
        let mut v = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            v.push(self.read()?);
        }
        self.depth -= 1;
        Ok(Value::Array(v))
    }
    fn map(&mut self, n: usize) -> Result<Value, String> {
        self.enter()?;
        let mut v = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            let k = self.read()?;
            let val = self.read()?;
            v.push((k, val));
        }
        self.depth -= 1;
        Ok(Value::Map(v))
    }

    fn read(&mut self) -> Result<Value, String> {
        let c = self.u8v()?;
        Ok(match c {
            0x00..=0x7F => Value::Int(c as i64),
            0xE0..=0xFF => Value::Int(c as i8 as i64),
            0x80..=0x8F => self.map((c & 0x0F) as usize)?,
            0x90..=0x9F => self.array((c & 0x0F) as usize)?,
            0xA0..=0xBF => self.string((c & 0x1F) as usize)?,
            0xC0 => Value::Nil,
            0xC2 => Value::Bool(false),
            0xC3 => Value::Bool(true),
            0xC4 => { let n = self.be(1)? as usize; Value::Bin(self.take(n)?.to_vec()) }
            0xC5 => { let n = self.be(2)? as usize; Value::Bin(self.take(n)?.to_vec()) }
            0xC6 => { let n = self.be(4)? as usize; Value::Bin(self.take(n)?.to_vec()) }
            0xCA => Value::F32(f32::from_bits(self.be(4)? as u32)),
            0xCB => Value::F64(f64::from_bits(self.be(8)?)),
            0xCC => Value::UInt(self.be(1)?),
            0xCD => Value::UInt(self.be(2)?),
            0xCE => Value::UInt(self.be(4)?),
            0xCF => Value::UInt(self.be(8)?),
            0xD0 => Value::Int(self.be(1)? as u8 as i8 as i64),
            0xD1 => Value::Int(self.be(2)? as u16 as i16 as i64),
            0xD2 => Value::Int(self.be(4)? as u32 as i32 as i64),
            0xD3 => Value::Int(self.be(8)? as i64),
            0xD9 => { let n = self.be(1)? as usize; self.string(n)? }
            0xDA => { let n = self.be(2)? as usize; self.string(n)? }
            0xDB => { let n = self.be(4)? as usize; self.string(n)? }
            0xDC => { let n = self.be(2)? as usize; self.array(n)? }
            0xDD => { let n = self.be(4)? as usize; self.array(n)? }
            0xDE => { let n = self.be(2)? as usize; self.map(n)? }
            0xDF => { let n = self.be(4)? as usize; self.map(n)? }
            other => return Err(format!("unsupported msgpack byte 0x{other:02X}")),
        })
    }
}

pub fn decode(buf: &[u8]) -> Result<Value, String> {
    Reader::new(buf).read()
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test msgpack`
Expected: PASS, 7 tests. `cargo build` also emits `dead_code` warnings because nothing calls
`decode` yet — expected until Task 6 wires everything together, and not something to silence
with `#[allow]`.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/main.rs src/msgpack.rs
git commit -m "feat(msgpack): dependency-free decoder with a recursion cap"
```

---

### Task 2: PNG chunk walk

**Files:**
- Create: `src/png.rs`
- Modify: `src/main.rs` (add `mod png;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `png::payload_span(path: &Path) -> Option<(u64, u64)>` — byte offset just past the first `IEND` chunk, and the number of bytes from there to EOF. `None` when the file is not a PNG or the chunk chain is malformed. A `Some((_, 0))` result means a plain image with nothing appended.

- [ ] **Step 1: Write the failing tests**

Create `src/png.rs` with only this test module.

```rust
//! Locate the block Koikatsu appends after a PNG's first IEND chunk.

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A PNG whose IDAT payload is `filler`, followed by `appended` extra bytes.
    /// `filler` lets a test place IEND at a chosen absolute offset.
    fn png(filler: &[u8], appended: &[u8]) -> Vec<u8> {
        fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut v = (data.len() as u32).to_be_bytes().to_vec();
            v.extend_from_slice(kind);
            v.extend_from_slice(data);
            v.extend_from_slice(&[0, 0, 0, 0]); // CRC is never verified here
            v
        }
        let mut v = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend(chunk(b"IHDR", &[0; 13]));
        v.extend(chunk(b"IDAT", filler));
        v.extend(chunk(b"IEND", &[]));
        v.extend_from_slice(appended);
        v
    }

    fn write(bytes: &[u8]) -> (crate::tempdir::Dir, std::path::PathBuf) {
        let d = crate::tempdir::Dir::new();
        let p = d.path().join("a.png");
        std::fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        (d, p)
    }

    #[test]
    fn finds_the_payload_after_iend() {
        let (_d, p) = write(&png(&[7; 10], b"PAYLOAD"));
        let (off, len) = payload_span(&p).expect("span");
        assert_eq!(len, 7);
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(&bytes[off as usize..], b"PAYLOAD");
    }

    #[test]
    fn a_plain_image_has_a_zero_length_payload() {
        let (_d, p) = write(&png(&[7; 10], b""));
        assert_eq!(payload_span(&p).map(|(_, l)| l), Some(0));
    }

    /// hamster scanned for the 8 IEND bytes through a 4096-byte sliding window and
    /// mishandled a match that straddled the boundary. Walking the chunk chain has
    /// no window at all; this pins that the offset is right anyway.
    ///
    /// Fixture layout: signature 8B, IHDR chunk 25B, IDAT chunk 12+pad B, so the
    /// IEND chunk header starts at 45+pad and its type field occupies 49+pad..53+pad.
    /// The header straddles the 4095/4096 boundary for pad in 4044..=4050. The swept
    /// range brackets that window rather than naming one offset, so the test keeps
    /// covering it if the fixture's chunk layout changes.
    #[test]
    fn iend_straddling_a_4096_byte_boundary_is_still_found() {
        for pad in 4000..4200 {
            let (_d, p) = write(&png(&vec![7u8; pad], b"PAYLOAD"));
            let (off, len) = payload_span(&p).unwrap_or_else(|| panic!("pad {pad}"));
            assert_eq!(len, 7, "pad {pad}");
            let bytes = std::fs::read(&p).unwrap();
            assert_eq!(&bytes[off as usize..], b"PAYLOAD", "pad {pad}");
        }
    }

    /// The IEND signature can occur inside compressed image data. Walking chunks
    /// steps over IDAT by its declared length, so an embedded copy is never seen.
    #[test]
    fn an_iend_signature_inside_image_data_is_not_mistaken_for_the_real_one() {
        let mut filler = vec![0u8; 20];
        filler.extend_from_slice(&[0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82]);
        let (_d, p) = write(&png(&filler, b"PAYLOAD"));
        assert_eq!(payload_span(&p).map(|(_, l)| l), Some(7));
    }

    #[test]
    fn a_non_png_is_none() {
        let (_d, p) = write(b"not a png at all");
        assert_eq!(payload_span(&p), None);
    }

    #[test]
    fn a_chunk_length_running_past_eof_is_none() {
        let mut bytes = png(&[7; 10], b"");
        // Overwrite the IHDR length — the first chunk after the 8-byte signature —
        // with something enormous.
        bytes[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        let (_d, p) = write(&bytes);
        assert_eq!(payload_span(&p), None);
    }
}
```

- [ ] **Step 2: Add the throwaway temp-directory helper**

`tempdir` is not a dependency — write a 20-line one. Create `src/tempdir.rs`:

```rust
//! A self-deleting temp directory. Twenty lines beats a dependency.

use std::path::{Path, PathBuf};

pub struct Dir(PathBuf);

impl Dir {
    pub fn new() -> Self {
        // Process id plus a monotonic counter: unique within and across runs,
        // without pulling in a random-number crate.
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "kh-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).expect("create temp dir");
        Dir(p)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
```

Add these to `src/main.rs`. `tempdir` is gated so it does not ship in the release binary:

```rust
mod png;
#[cfg(test)]
mod tempdir;
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test png`
Expected: FAIL — `cannot find function payload_span in this scope`.

- [ ] **Step 4: Write the implementation above the test module in `src/png.rs`**

```rust
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const SIG: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Walk the PNG chunk chain to the FIRST `IEND` and return
/// `(offset just past it, bytes remaining to EOF)`.
///
/// Walking the chain — rather than scanning for the 8-byte IEND signature — is
/// what makes this reliable: the signature can occur inside compressed IDAT data,
/// and a scanner has to get buffer-boundary bookkeeping right. Stepping over each
/// chunk by its declared length has neither problem.
pub fn payload_span(path: &Path) -> Option<(u64, u64)> {
    let mut f = fs::File::open(path).ok()?;
    let file_len = f.metadata().ok()?.len();
    let mut sig = [0u8; 8];
    f.read_exact(&mut sig).ok()?;
    if sig != SIG {
        return None;
    }
    let mut off: u64 = 8;
    loop {
        let mut hdr = [0u8; 8];
        f.read_exact(&mut hdr).ok()?;
        let ln = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
        let is_iend = &hdr[4..8] == b"IEND";
        let next = off + 8 + ln + 4; // length + type + data + crc
        if next > file_len {
            return None; // malformed: chunk runs past EOF
        }
        if is_iend {
            return Some((next, file_len - next));
        }
        f.seek(SeekFrom::Start(next)).ok()?;
        off = next;
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test png`
Expected: PASS, 6 tests.

- [ ] **Step 6: Commit**

```bash
git add src/png.rs src/tempdir.rs src/main.rs
git commit -m "feat(png): locate the appended card block by walking the chunk chain"
```

---

### Task 3: Card metadata

**Files:**
- Create: `src/card.rs`
- Create: `src/fixture.rs`
- Modify: `src/main.rs` (add `mod card;` and `#[cfg(test)] mod fixture;`)

**Interfaces:**
- Consumes: `png::payload_span`, `msgpack::decode`.
- Produces:
  - `card::Game` — `Koikatu | KoikatsuSunshine | HoneyCome | Svc | Aicomi | EmotionCreators`, with `folder(&self) -> &'static str`.
  - `card::DEST_FOLDERS: [&str; 6]` — every folder name this program may create directly under the root.
  - `card::Route` — `BySex | Fixed(&'static str)`.
  - `card::Sex` — `Male | Female | Unknown`, with `folder(&self) -> &'static str`.
  - `card::CardMeta { game: Game, route: Route, sex: Sex, lastname: String, firstname: String }` with `fullname(&self) -> String`.
  - `card::CardError` — `NotCard | Scene(String) | Unrecognized(String) | Malformed(String)`, with `reason(&self) -> String`.
  - `card::read_card(path: &Path) -> Result<CardMeta, CardError>`.
  - `fixture::card(marker: &str, sex: u8, lastname: &str, firstname: &str) -> Vec<u8>` and `fixture::scene(version: &str) -> Vec<u8>` and `fixture::plain_png() -> Vec<u8>` (test-only).

- [ ] **Step 1: Write `src/fixture.rs`, the synthetic-card builder**

```rust
//! Builds byte-exact cards so the tests need no local files.
//!
//! Layout after the PNG's IEND, mirroring .NET `BinaryWriter`:
//!   i32 ProductNo | string marker | string loadVersion | i32 faceLen + face
//!   | i32 tableLen + msgpack block table | i64 total | blocks
//! Strings use the 7-bit encoded length prefix; every string here is under 128
//! bytes, so the prefix is a single byte.
//!
//! `tests/cli.rs` pulls this same file in with `#[path]`, so each consumer uses a
//! different subset of the builders.
#![allow(dead_code)]

fn dotnet_string(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    assert!(b.len() < 128, "fixture strings stay in the one-byte prefix range");
    let mut v = vec![b.len() as u8];
    v.extend_from_slice(b);
    v
}

fn mp_str(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    assert!(b.len() < 32, "fixture strings stay in fixstr range");
    let mut v = vec![0xA0 | b.len() as u8];
    v.extend_from_slice(b);
    v
}

fn mp_uint(n: u64) -> Vec<u8> {
    if n < 128 {
        vec![n as u8]
    } else if n <= u16::MAX as u64 {
        let mut v = vec![0xCD];
        v.extend_from_slice(&(n as u16).to_be_bytes());
        v
    } else {
        let mut v = vec![0xCE];
        v.extend_from_slice(&(n as u32).to_be_bytes());
        v
    }
}

fn mp_map(pairs: &[(&str, Vec<u8>)]) -> Vec<u8> {
    assert!(pairs.len() < 16);
    let mut v = vec![0x80 | pairs.len() as u8];
    for (k, val) in pairs {
        v.extend(mp_str(k));
        v.extend_from_slice(val);
    }
    v
}

fn mp_arr(items: &[Vec<u8>]) -> Vec<u8> {
    assert!(items.len() < 16);
    let mut v = vec![0x90 | items.len() as u8];
    for i in items {
        v.extend_from_slice(i);
    }
    v
}

/// A minimal PNG with `appended` bytes after IEND.
pub fn png_with(appended: &[u8]) -> Vec<u8> {
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = (data.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(data);
        v.extend_from_slice(&[0, 0, 0, 0]);
        v
    }
    let mut v = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend(chunk(b"IHDR", &[0; 13]));
    v.extend(chunk(b"IDAT", &[7; 16]));
    v.extend(chunk(b"IEND", &[]));
    v.extend_from_slice(appended);
    v
}

pub fn plain_png() -> Vec<u8> {
    png_with(b"")
}

/// A character or coordinate card. `sex` is written into the Parameter block.
pub fn card(marker: &str, sex: u8, lastname: &str, firstname: &str) -> Vec<u8> {
    let parameter = mp_map(&[
        ("version", mp_str("0.0.5")),
        ("sex", mp_uint(sex as u64)),
        ("lastname", mp_str(lastname)),
        ("firstname", mp_str(firstname)),
    ]);
    let table = mp_map(&[(
        "lstInfo",
        mp_arr(&[mp_map(&[
            ("name", mp_str("Parameter")),
            ("version", mp_str("0.0.5")),
            ("pos", mp_uint(0)),
            ("size", mp_uint(parameter.len() as u64)),
        ])]),
    )]);

    let mut p = Vec::new();
    p.extend_from_slice(&100i32.to_le_bytes()); // ProductNo
    p.extend(dotnet_string(marker));
    p.extend(dotnet_string("0.0.0")); // loadVersion — the old layout, still valid
    p.extend_from_slice(&0i32.to_le_bytes()); // faceLen
    p.extend_from_slice(&(table.len() as i32).to_le_bytes());
    p.extend_from_slice(&table);
    p.extend_from_slice(&(parameter.len() as i64).to_le_bytes()); // total
    p.extend_from_slice(&parameter);
    png_with(&p)
}

/// A KStudio scene card: where a chara card has a marker, a scene has a version
/// string. That difference is the only reliable way to tell them apart, because a
/// scene embeds the characters it uses and therefore contains the chara marker too.
pub fn scene(version: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend(dotnet_string(version));
    p.extend_from_slice(b"\x00\x01scene payload");
    png_with(&p)
}
```

- [ ] **Step 2: Write the failing tests in `src/card.rs`**

Create the file with only this test module.

```rust
//! Card payload -> structured metadata.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;
    use crate::tempdir::Dir;
    use std::io::Write;

    fn file(d: &Dir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = d.path().join(name);
        std::fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn reads_a_koikatu_female_character_card() {
        let d = Dir::new();
        let p = file(&d, "a.png", &fixture::card("【KoiKatuChara】", 1, "姬野", "夜王"));
        let m = read_card(&p).expect("card");
        assert_eq!(m.game, Game::Koikatu);
        assert_eq!(m.route, Route::BySex);
        assert_eq!(m.sex, Sex::Female);
        assert_eq!(m.fullname(), "姬野 夜王");
    }

    #[test]
    fn sex_zero_is_male() {
        let d = Dir::new();
        let p = file(&d, "a.png", &fixture::card("【KoiKatuChara】", 0, "a", "b"));
        assert_eq!(read_card(&p).unwrap().sex, Sex::Male);
    }

    #[test]
    fn a_coordinate_card_routes_to_a_fixed_folder_and_needs_no_parameter_block() {
        let d = Dir::new();
        let p = file(&d, "a.png", &fixture::card("【KoiKatuClothes】", 1, "a", "b"));
        let m = read_card(&p).expect("card");
        assert_eq!(m.game, Game::Koikatu);
        assert_eq!(m.route, Route::Fixed("Coordinate"));
    }

    /// hamster leaves unconverted Emotion Creators cards where they are and says
    /// nothing, so ordering the pipeline wrong loses a whole batch silently. They
    /// get their own folder, and their Parameter block is never parsed — the format
    /// differs and reading it would turn a recognized card into an error.
    #[test]
    fn an_emotion_creators_card_gets_its_own_folder() {
        let d = Dir::new();
        let p = file(&d, "a.png", &fixture::card("【EroMakeChara】", 1, "a", "b"));
        let m = read_card(&p).expect("card");
        assert_eq!(m.game, Game::EmotionCreators);
        assert_eq!(m.route, Route::Fixed("Character"));
    }

    /// The payload starts with a 4-byte ProductNo before the marker, and the
    /// fixture writes loadVersion `0.0.0` — an older but perfectly ordinary layout
    /// that a marker-first reader would misparse.
    #[test]
    fn the_product_no_prefix_and_version_0_0_0_are_handled() {
        let d = Dir::new();
        let bytes = fixture::card("【KoiKatuChara】", 1, "a", "b");
        assert!(
            bytes.windows(4).any(|w| w == 100i32.to_le_bytes()),
            "fixture must carry the ProductNo prefix this test is about"
        );
        let p = file(&d, "a.png", &bytes);
        assert_eq!(read_card(&p).unwrap().game, Game::Koikatu);
    }

    #[test]
    fn an_unknown_marker_is_reported_verbatim_never_guessed() {
        let d = Dir::new();
        let p = file(&d, "a.png", &fixture::card("【SomeFutureGame】", 1, "a", "b"));
        match read_card(&p) {
            Err(CardError::Unrecognized(m)) => assert_eq!(m, "【SomeFutureGame】"),
            other => panic!("expected Unrecognized, got {other:?}"),
        }
    }

    /// A scene card's payload starts with a version string where a chara card has a
    /// marker. Detecting it keeps the unrecognized-marker report readable: one real
    /// batch held 278 scenes.
    #[test]
    fn a_scene_card_is_recognized_by_its_version_string() {
        let d = Dir::new();
        let p = file(&d, "a.png", &fixture::scene("1.0.4.2"));
        match read_card(&p) {
            Err(CardError::Scene(v)) => assert_eq!(v, "1.0.4.2"),
            other => panic!("expected Scene, got {other:?}"),
        }
    }

    #[test]
    fn a_plain_image_is_not_a_card() {
        let d = Dir::new();
        let p = file(&d, "a.png", &fixture::plain_png());
        assert!(matches!(read_card(&p), Err(CardError::NotCard)));
    }

    #[test]
    fn a_non_png_is_not_a_card() {
        let d = Dir::new();
        let p = file(&d, "a.png", b"not a png");
        assert!(matches!(read_card(&p), Err(CardError::NotCard)));
    }

    #[test]
    fn a_parameter_block_running_past_the_end_is_malformed_not_a_panic() {
        let d = Dir::new();
        let mut bytes = fixture::card("【KoiKatuChara】", 1, "a", "b");
        bytes.truncate(bytes.len() - 4); // chop into the Parameter block
        let p = file(&d, "a.png", &bytes);
        assert!(matches!(read_card(&p), Err(CardError::Malformed(_))));
    }

    #[test]
    fn a_missing_sex_field_yields_unknown_rather_than_an_error() {
        // Build a card, then blank the "sex" key so the lookup misses.
        let d = Dir::new();
        let bytes = fixture::card("【KoiKatuChara】", 1, "a", "b");
        let mut bytes = bytes.clone();
        let at = bytes.windows(3).position(|w| w == b"sex").expect("key present");
        bytes[at..at + 3].copy_from_slice(b"zzz");
        let p = file(&d, "a.png", &bytes);
        assert_eq!(read_card(&p).unwrap().sex, Sex::Unknown);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test card`
Expected: FAIL — `cannot find function read_card in this scope`.

- [ ] **Step 4: Write the implementation above the test module**

```rust
use crate::msgpack::decode;
use crate::png::payload_span;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game {
    Koikatu,
    KoikatsuSunshine,
    HoneyCome,
    Svc,
    Aicomi,
    EmotionCreators,
}

impl Game {
    pub fn folder(&self) -> &'static str {
        match self {
            Game::Koikatu => "Koikatu",
            Game::KoikatsuSunshine => "KoikatsuSunshine",
            Game::HoneyCome => "HoneyCome",
            Game::Svc => "SVC",
            Game::Aicomi => "Aicomi",
            Game::EmotionCreators => "EmotionCreators",
        }
    }
}

/// Every folder this program may create directly under the scan root. The
/// exclusion rule compares a path's FIRST segment against exactly these — and
/// nothing else. There is deliberately no "Unknown" entry: that is a value of
/// `Sex`, never a top-level folder, and treating it as one is precisely the bug
/// that made the C# version skip a folder literally named "Unknown god".
pub const DEST_FOLDERS: [&str; 6] = [
    "Koikatu",
    "KoikatsuSunshine",
    "HoneyCome",
    "SVC",
    "Aicomi",
    "EmotionCreators",
];

/// How a card picks its leaf folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Split by the Parameter block's `sex` field.
    BySex,
    /// A fixed leaf; the Parameter block is not read at all.
    Fixed(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    Male,
    Female,
    Unknown,
}

impl Sex {
    pub fn folder(&self) -> &'static str {
        match self {
            Sex::Male => "Male",
            Sex::Female => "Female",
            Sex::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CardMeta {
    pub game: Game,
    pub route: Route,
    /// `Unknown` whenever `route` is `Fixed`, or the field could not be read.
    pub sex: Sex,
    pub lastname: String,
    pub firstname: String,
}

impl CardMeta {
    pub fn fullname(&self) -> String {
        format!("{} {}", self.lastname, self.firstname)
    }
}

#[derive(Debug)]
pub enum CardError {
    /// Not a PNG, or a PNG with nothing appended.
    NotCard,
    /// A KStudio scene card, carrying the version string found in place of a marker.
    Scene(String),
    /// A card of some kind, but its marker is not in the table. Never guessed.
    Unrecognized(String),
    /// Structurally broken past the marker.
    Malformed(String),
}

impl CardError {
    pub fn reason(&self) -> String {
        match self {
            CardError::NotCard => "not a card (nothing appended after IEND)".into(),
            CardError::Scene(v) => format!("KStudio scene card (version {v})"),
            CardError::Unrecognized(m) => format!("unrecognized marker {m:?}"),
            CardError::Malformed(m) => format!("malformed: {m}"),
        }
    }
}

/// Only markers seen on a real card are listed; anything else is reported
/// verbatim rather than guessed at.
fn classify_marker(marker: &str) -> Option<(Game, Route)> {
    Some(match marker {
        "【KoiKatuChara】" | "【KoiKatuCharaS】" | "【KoiKatuCharaSP】" => (Game::Koikatu, Route::BySex),
        "【KoiKatuClothes】" => (Game::Koikatu, Route::Fixed("Coordinate")),
        "【KoiKatuCharaSun】" => (Game::KoikatsuSunshine, Route::BySex),
        "【HCChara】" | "【HCPChara】" => (Game::HoneyCome, Route::BySex),
        "【SVChara】" => (Game::Svc, Route::BySex),
        "【SVClothes】" => (Game::Svc, Route::Fixed("Coordinate")),
        "【ACChara】" => (Game::Aicomi, Route::BySex),
        "【ACClothes】" => (Game::Aicomi, Route::Fixed("Coordinate")),
        "【EroMakeChara】" => (Game::EmotionCreators, Route::Fixed("Character")),
        _ => return None,
    })
}

/// True when a string is a dotted version number — what sits where a chara card
/// keeps its marker, in a KStudio scene card.
fn looks_like_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Cursor over the appended block, mirroring .NET `BinaryReader` primitives.
struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.p.checked_add(n).ok_or("length overflow")?;
        let s = self.b.get(self.p..end).ok_or("truncated card")?;
        self.p = end;
        Ok(s)
    }
    fn i32v(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i64v(&mut self) -> Result<i64, String> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    /// .NET `BinaryReader.ReadString`: 7-bit encoded length prefix, then UTF-8.
    fn string(&mut self) -> Result<String, String> {
        let mut n: usize = 0;
        let mut shift = 0;
        loop {
            let b = self.take(1)?[0];
            n |= ((b & 0x7F) as usize) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 28 {
                return Err("bad 7-bit length prefix".into());
            }
        }
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

pub fn read_card(path: &Path) -> Result<CardMeta, CardError> {
    let (off, len) = payload_span(path).ok_or(CardError::NotCard)?;
    if len == 0 {
        return Err(CardError::NotCard);
    }
    let mut f = fs::File::open(path).map_err(|e| CardError::Malformed(e.to_string()))?;
    f.seek(SeekFrom::Start(off))
        .map_err(|e| CardError::Malformed(e.to_string()))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| CardError::Malformed(e.to_string()))?;

    // A scene card has no ProductNo: its payload opens with the version string.
    // Try that reading first, and only if it yields a version number accept it.
    {
        let mut probe = Cur { b: &buf, p: 0 };
        if let Ok(s) = probe.string() {
            if looks_like_version(&s) {
                return Err(CardError::Scene(s));
            }
        }
    }

    let mut c = Cur { b: &buf, p: 0 };
    c.i32v().map_err(CardError::Malformed)?; // ProductNo
    let marker = c.string().map_err(CardError::Malformed)?;
    let (game, route) = classify_marker(&marker).ok_or(CardError::Unrecognized(marker))?;

    let mut meta = CardMeta {
        game,
        route,
        sex: Sex::Unknown,
        lastname: String::new(),
        firstname: String::new(),
    };
    if route != Route::BySex {
        return Ok(meta);
    }

    c.string().map_err(CardError::Malformed)?; // loadVersion
    let face = c.i32v().map_err(CardError::Malformed)?;
    if face > 0 {
        c.take(face as usize).map_err(CardError::Malformed)?;
    }
    let n = c.i32v().map_err(CardError::Malformed)?;
    if n < 0 {
        return Err(CardError::Malformed("negative block table length".into()));
    }
    let table_bytes = c.take(n as usize).map_err(CardError::Malformed)?;
    let table = decode(table_bytes).map_err(CardError::Malformed)?;
    c.i64v().map_err(CardError::Malformed)?; // total
    let blocks_at = c.p;

    let info = table
        .get("lstInfo")
        .and_then(|v| v.as_array())
        .and_then(|list| {
            list.iter()
                .find(|it| it.get("name").and_then(|v| v.as_str()) == Some("Parameter"))
        })
        .ok_or_else(|| CardError::Malformed("no Parameter block in the table".into()))?;
    let pos = info.get("pos").and_then(|v| v.as_i64()).unwrap_or(-1);
    let size = info.get("size").and_then(|v| v.as_i64()).unwrap_or(-1);
    if pos < 0 || size < 0 {
        return Err(CardError::Malformed("Parameter block has no pos/size".into()));
    }
    let start = blocks_at + pos as usize;
    let end = start.saturating_add(size as usize);
    let slice = buf
        .get(start..end)
        .ok_or_else(|| CardError::Malformed("Parameter block runs past end of card".into()))?;
    let p = decode(slice).map_err(CardError::Malformed)?;

    meta.sex = match p.get("sex").and_then(|v| v.as_i64()) {
        Some(0) => Sex::Male,
        Some(1) => Sex::Female,
        _ => Sex::Unknown,
    };
    meta.lastname = p.get("lastname").and_then(|v| v.as_str()).unwrap_or("").to_string();
    meta.firstname = p.get("firstname").and_then(|v| v.as_str()).unwrap_or("").to_string();
    Ok(meta)
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test card`
Expected: PASS, 11 tests.

- [ ] **Step 6: Commit**

```bash
git add src/card.rs src/fixture.rs src/main.rs
git commit -m "feat(card): marker table, sex routing, scene and unrecognized reporting"
```

---

### Task 4: Destination paths

**Files:**
- Create: `src/plan.rs`
- Modify: `src/main.rs` (add `mod plan;`)

**Interfaces:**
- Consumes: `card::{CardMeta, DEST_FOLDERS, Route}`.
- Produces:
  - `plan::destination_dir(root: &Path, meta: &CardMeta, search: Option<&str>) -> PathBuf`
  - `plan::is_in_dest_folder(root: &Path, file: &Path) -> bool`
  - `plan::free_name(dir: &Path, file_name: &str) -> PathBuf`

- [ ] **Step 1: Write the failing tests**

Create `src/plan.rs` with only this test module.

```rust
//! Where a card belongs, and which paths are this program's own output.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{CardMeta, Game, Route, Sex};
    use crate::tempdir::Dir;
    use std::path::Path;

    fn meta(game: Game, route: Route, sex: Sex, last: &str, first: &str) -> CardMeta {
        CardMeta {
            game,
            route,
            sex,
            lastname: last.into(),
            firstname: first.into(),
        }
    }

    #[test]
    fn a_female_character_card_goes_to_game_female() {
        let m = meta(Game::Koikatu, Route::BySex, Sex::Female, "a", "b");
        assert_eq!(
            destination_dir(Path::new("/r"), &m, None),
            Path::new("/r").join("Koikatu").join("Female")
        );
    }

    #[test]
    fn an_unknown_sex_still_gets_a_folder_rather_than_being_dropped() {
        let m = meta(Game::Koikatu, Route::BySex, Sex::Unknown, "a", "b");
        assert_eq!(
            destination_dir(Path::new("/r"), &m, None),
            Path::new("/r").join("Koikatu").join("Unknown")
        );
    }

    #[test]
    fn a_fixed_route_uses_its_own_leaf() {
        let m = meta(Game::Svc, Route::Fixed("Coordinate"), Sex::Unknown, "", "");
        assert_eq!(
            destination_dir(Path::new("/r"), &m, None),
            Path::new("/r").join("SVC").join("Coordinate")
        );
        let e = meta(Game::EmotionCreators, Route::Fixed("Character"), Sex::Unknown, "", "");
        assert_eq!(
            destination_dir(Path::new("/r"), &e, None),
            Path::new("/r").join("EmotionCreators").join("Character")
        );
    }

    /// The search term sorts matches into a subfolder. It does not filter the run:
    /// a card that does not match still goes to its ordinary folder.
    #[test]
    fn a_matching_search_term_adds_a_subfolder_and_a_non_match_does_not() {
        let m = meta(Game::Koikatu, Route::BySex, Sex::Female, "Asuna", "Yuuki");
        assert_eq!(
            destination_dir(Path::new("/r"), &m, Some("asuna")),
            Path::new("/r").join("Koikatu").join("Female").join("asuna")
        );
        assert_eq!(
            destination_dir(Path::new("/r"), &m, Some("kirito")),
            Path::new("/r").join("Koikatu").join("Female")
        );
    }

    #[test]
    fn a_fixed_route_ignores_the_search_term_because_it_has_no_name() {
        let m = meta(Game::Koikatu, Route::Fixed("Coordinate"), Sex::Unknown, "", "");
        assert_eq!(
            destination_dir(Path::new("/r"), &m, Some("asuna")),
            Path::new("/r").join("Koikatu").join("Coordinate")
        );
    }

    #[test]
    fn output_folders_directly_under_the_root_are_excluded() {
        let r = Path::new("/r");
        assert!(is_in_dest_folder(r, &r.join("Koikatu").join("Female").join("x.png")));
        assert!(is_in_dest_folder(r, &r.join("EmotionCreators").join("Character").join("x.png")));
    }

    /// THE regression. The C# version asked whether the ABSOLUTE path contained a
    /// game name, so a card pack named after a character — which is exactly how
    /// Koikatsu names its own exports — was silently skipped.
    #[test]
    fn a_card_pack_folder_named_like_a_koikatsu_export_is_not_excluded() {
        let r = Path::new("/r");
        let p = r
            .join("pack")
            .join("Koikatu_F_20240626232405280_x")
            .join("x.png");
        assert!(!is_in_dest_folder(r, &p));
    }

    /// The other half of the same bug: `Unknown` is a `Sex` value, not a top-level
    /// folder, so a folder merely named "Unknown god" must not be excluded.
    #[test]
    fn a_folder_named_unknown_something_is_not_excluded() {
        let r = Path::new("/r");
        let p = r.join("ISEEU").join("Genshin").join("Unknown god").join("card").join("x.png");
        assert!(!is_in_dest_folder(r, &p));
    }

    #[test]
    fn a_game_named_folder_below_the_first_level_is_not_an_output_folder() {
        let r = Path::new("/r");
        assert!(!is_in_dest_folder(r, &r.join("somewhere").join("Koikatu").join("x.png")));
    }

    #[test]
    fn a_path_outside_the_root_is_not_excluded() {
        assert!(!is_in_dest_folder(Path::new("/r"), Path::new("/other/Koikatu/x.png")));
    }

    #[test]
    fn a_free_name_is_returned_unchanged_when_nothing_is_there() {
        let d = Dir::new();
        assert_eq!(free_name(d.path(), "x.png"), d.path().join("x.png"));
    }

    #[test]
    fn a_taken_name_gains_a_counter() {
        let d = Dir::new();
        std::fs::write(d.path().join("x.png"), b"1").unwrap();
        assert_eq!(free_name(d.path(), "x.png"), d.path().join("x(1).png"));
        std::fs::write(d.path().join("x(1).png"), b"1").unwrap();
        assert_eq!(free_name(d.path(), "x.png"), d.path().join("x(2).png"));
    }

    #[test]
    fn a_name_without_an_extension_still_gets_a_counter() {
        let d = Dir::new();
        std::fs::write(d.path().join("x"), b"1").unwrap();
        assert_eq!(free_name(d.path(), "x"), d.path().join("x(1)"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test plan`
Expected: FAIL — `cannot find function destination_dir in this scope`.

- [ ] **Step 3: Write the implementation above the test module**

```rust
use crate::card::{CardMeta, Route, DEST_FOLDERS};
use std::path::{Path, PathBuf};

/// Where a card belongs. `search`, when the card's full name contains it, adds one
/// more level so matches collect together; non-matching cards are unaffected.
pub fn destination_dir(root: &Path, meta: &CardMeta, search: Option<&str>) -> PathBuf {
    let (leaf, named) = match meta.route {
        Route::BySex => (meta.sex.folder().to_string(), true),
        Route::Fixed(f) => (f.to_string(), false),
    };
    let dir = root.join(meta.game.folder()).join(leaf);
    match search {
        Some(term) if named && contains_ignore_case(&meta.fullname(), term) => dir.join(term),
        _ => dir,
    }
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// Whether `file` sits inside a folder this program itself creates — the only
/// thing the scan skips.
///
/// The comparison is on the FIRST path component relative to the scan root, and it
/// is an equality test. The C# version instead asked whether the absolute path
/// CONTAINED any game name, with the `Unknown` sentinel in the list; that dropped
/// `…/Koikatu_F_20240626232405280_x/` and `…/Unknown god/card/` without a word.
pub fn is_in_dest_folder(root: &Path, file: &Path) -> bool {
    let Ok(rel) = file.strip_prefix(root) else {
        return false;
    };
    let Some(first) = rel.components().next() else {
        return false;
    };
    let first = first.as_os_str().to_string_lossy();
    DEST_FOLDERS.iter().any(|d| d.eq_ignore_ascii_case(&first))
}

/// `dir/file_name`, or `dir/stem(1).ext` etc. when that is taken.
pub fn free_name(dir: &Path, file_name: &str) -> PathBuf {
    let direct = dir.join(file_name);
    if !direct.exists() {
        return direct;
    }
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (file_name.to_string(), String::new()),
    };
    let mut n = 1u32;
    loop {
        let candidate = dir.join(format!("{stem}({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test plan`
Expected: PASS, 13 tests.

- [ ] **Step 5: Commit**

```bash
git add src/plan.rs src/main.rs
git commit -m "feat(plan): destinations, output-folder exclusion, collision naming"
```

---

### Task 5: Directory walk

**Files:**
- Create: `src/walk.rs`
- Modify: `src/main.rs` (add `mod walk;`)

**Interfaces:**
- Consumes: `plan::is_in_dest_folder`.
- Produces: `walk::candidates(root: &Path) -> Vec<PathBuf>` — every `.png` (case-insensitive) under `root`, in a deterministic order, excluding this program's own output folders. Directories that cannot be read are skipped silently; unreadable *files* are the caller's problem, not the walker's.

- [ ] **Step 1: Write the failing tests**

Create `src/walk.rs` with only this test module.

```rust
//! Recursive scan for candidate PNGs.

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
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test walk`
Expected: FAIL — `cannot find function candidates in this scope`.

- [ ] **Step 3: Write the implementation above the test module**

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test walk`
Expected: PASS, 4 tests.

- [ ] **Step 5: Commit**

```bash
git add src/walk.rs src/main.rs
git commit -m "feat(walk): iterative scan that skips only real output folders"
```

---

### Task 6: CLI, reporting and moving

**Files:**
- Modify: `src/main.rs` (replace the placeholder entirely)

**Interfaces:**
- Consumes: `walk::candidates`, `card::{read_card, CardError}`, `plan::{destination_dir, free_name}`.
- Produces: the executable's behaviour. `main.rs` also exposes, for its own tests, `Args::parse(argv: &[String]) -> Result<Args, String>` where `Args { root: Option<PathBuf>, dry_run: bool, search: Option<String> }`, and `run(root: &Path, dry_run: bool, search: Option<&str>, out: &mut dyn Write) -> Report` where `Report { moved: Vec<(String, u64)>, scenes: u64, non_cards: u64, unrecognized: u64, errors: u64 }`.

- [ ] **Step 1: Write the failing tests**

Replace `src/main.rs` with the module declarations plus this test module only.

```rust
mod card;
mod msgpack;
mod plan;
mod png;
mod walk;
#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tempdir;

fn main() {}

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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bin koikatsu-hamster`
Expected: FAIL — `cannot find type Args in this scope`.

- [ ] **Step 3: Write the implementation**

Replace `fn main() {}` with everything below, keeping the module declarations at the top and the test module at the bottom.

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bin koikatsu-hamster`
Expected: PASS — all unit tests across every module, including the 10 new ones here.

- [ ] **Step 5: Verify the binary builds clean**

Run: `cargo clippy --all-targets -- -D warnings` (if clippy is installed) and `cargo build --release`
Expected: no warnings, `target/release/koikatsu-hamster.exe` exists.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat(cli): flags, per-file reporting, summary, terminal-only pause"
```

---

### Task 7: End-to-end test through the real executable

**Files:**
- Create: `tests/cli.rs`
- Modify: `README.md` (replace the "Status" line, add Usage)

**Interfaces:**
- Consumes: the built binary, via `env!("CARGO_BIN_EXE_koikatsu-hamster")`.
- Produces: nothing other tasks depend on.

- [ ] **Step 1: Write the failing test**

```rust
//! Drives the real executable. A binary crate's modules are unreachable from an
//! integration test through `use`, so the fixture builder is pulled in by path —
//! one source file, compiled into both targets. This is the layer where the exit
//! code, the banner and the redirected-stdin behaviour actually matter.

#[path = "../src/fixture.rs"]
mod fixture;

use std::path::Path;
use std::process::Command;

const EXE: &str = env!("CARGO_BIN_EXE_koikatsu-hamster");

fn temp_root(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("kh-cli-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

#[test]
fn organises_a_tree_prints_a_banner_and_exits_zero() {
    let root = temp_root("ok");
    write(&root, "ISEEU/Genshin/Unknown god/card/god.png", &fixture::card("【KoiKatuChara】", 1, "a", "b"));
    write(&root, "pack/Koikatu_F_20240626232405280_x/x.png", &fixture::card("【KoiKatuChara】", 0, "a", "b"));

    let out = Command::new(EXE)
        .arg("--root")
        .arg(&root)
        .output()
        .expect("run");

    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(text.starts_with("koikatsu-hamster 0.1.0 (rust)"), "{text}");
    assert!(out.status.success(), "exit {:?}\n{text}", out.status.code());
    assert!(root.join("Koikatu/Female/god.png").exists(), "{text}");
    assert!(root.join("Koikatu/Male/x.png").exists(), "{text}");
    assert!(text.contains("--- summary ---"), "{text}");

    // Second run: the output folders are skipped, so nothing is left to do.
    let out2 = Command::new(EXE).arg("--root").arg(&root).output().expect("run");
    let text2 = String::from_utf8_lossy(&out2.stdout).to_string();
    assert!(!text2.contains("Move file:"), "{text2}");

    let _ = std::fs::remove_dir_all(&root);
}

/// hamster's unconditional ReadKey throws under a redirected stdin. `Command`
/// gives the child a null stdin, so reaching this assertion at all proves the
/// process exits on its own.
#[test]
fn a_malformed_card_exits_one_without_waiting_for_input() {
    let root = temp_root("err");
    let mut bytes = card("【KoiKatuChara】", 1);
    bytes.truncate(bytes.len() - 4);
    write(&root, "broken.png", &bytes);

    let out = Command::new(EXE).arg("--root").arg(&root).output().expect("run");
    assert_eq!(out.status.code(), Some(1));
    assert!(root.join("broken.png").exists());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_unknown_flag_exits_two_with_usage() {
    let out = Command::new(EXE).arg("--recursive").output().expect("run");
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("usage:"), "{err}");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test cli`
Expected: FAIL if any behaviour from Task 6 is missing; if Task 6 is complete this may pass immediately, which is an acceptable outcome for an end-to-end test written last.

- [ ] **Step 3: Fix whatever the end-to-end test exposes**

Make only the changes the failures demand. Do not add features.

- [ ] **Step 4: Run the whole suite**

Run: `cargo test`
Expected: PASS — every unit test plus the 3 integration tests.

- [ ] **Step 5: Update the README**

Replace the line:

```markdown
**Status: design complete, implementation not started.** See
[the design document](docs/superpowers/specs/2026-07-29-card-organizer-design.md).
```

with:

```markdown
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

The run ends with a summary of what moved and what was left alone. It pauses for a keypress only
when stdin is a terminal, so it can be called from a script; the exit code is 1 if any file
errored, otherwise 0.
```

- [ ] **Step 6: Commit**

```bash
git add tests/cli.rs README.md
git commit -m "test(cli): end-to-end run, exit codes, no stdin wait; document usage"
```

---

## Self-Review

**Spec coverage.** Every section of the design maps to a task: marker table and EC handling →
Task 3; exclusion rule → Tasks 4 and 5; per-file decision order → Tasks 3 and 6; collision
naming and the search term → Tasks 4 and 6; invocation, banner, `IsTerminal`, exit code → Task
6; reporting format → Task 6; the four named regression cases → Tasks 4 and 5; the project
constraints → Task 1 and Global Constraints.

**Deviations from the spec, deliberate:** the spec's per-file table says an unreadable `sex`
sends a card to `{Game}/Unknown`; that is Task 4's
`an_unknown_sex_still_gets_a_folder_rather_than_being_dropped`. The spec describes card types as
`Character`/`Coordinate`; the implementation expresses that as `Route::BySex` /
`Route::Fixed(leaf)` because Emotion Creators cards are Character cards that must NOT be
sex-split, and encoding the destination in the marker table is what keeps their differently
shaped Parameter block from ever being parsed.

**Type consistency.** `CardMeta` is constructed in Task 3 and consumed in Tasks 4 and 6 with the
same five fields. `Route::Fixed(&'static str)` carries the leaf name used by
`destination_dir`. `Report`'s field names in Task 6 match every assertion in its tests and in
Task 7. `walk::candidates`, `plan::is_in_dest_folder`, `plan::free_name`, `png::payload_span` and
`card::read_card` keep one signature throughout.

**Fixture sharing.** `src/fixture.rs` is `#[cfg(test)]` inside a binary crate, so an integration
test cannot reach it through `use`. `tests/cli.rs` pulls the same source file in with
`#[path = "../src/fixture.rs"] mod fixture;` — one file, compiled into both targets, no
duplicated logic. The file is self-contained (std only), which is what makes that work, and it
carries `#![allow(dead_code)]` because each consumer uses a different subset of the builders.
