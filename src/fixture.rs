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
