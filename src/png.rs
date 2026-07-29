//! Locate the block Koikatsu appends after a PNG's first IEND chunk.

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
    /// In this fixture the IEND chunk's 8-byte header (4-byte zero length + "IEND"
    /// type) starts at offset `45 + pad` (signature 8 + IHDR chunk 25 + IDAT chunk
    /// overhead 12 = 45 bytes before the IDAT filler), so its type field lands at
    /// `49 + pad`. A single hardcoded `pad` would only prove the boundary case if
    /// that arithmetic is exactly right, so instead sweep `pad` across a range wide
    /// enough to bracket any 4096-byte boundary crossing regardless of exactly
    /// which bytes a sliding-window scanner treats as "the window", and regardless
    /// of small future changes to this fixture's chunk layout.
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
        // Overwrite the IHDR length (the first chunk after the 8-byte signature,
        // at offset 8..12) with something enormous.
        bytes[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        let (_d, p) = write(&bytes);
        assert_eq!(payload_span(&p), None);
    }
}
