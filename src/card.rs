//! Card payload -> structured metadata.

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

/// Reads the appended block and classifies it. A recognised marker always wins:
/// `ProductNo` and the marker string are read first, and only when the marker is
/// NOT in the table does this fall back to probing offset 0 as a KStudio scene's
/// version string. That order means a card with a known marker can never be
/// misclassified as a scene, no matter what its `ProductNo` bytes would probe as
/// — the old code ran the scene probe first and relied on it almost never
/// matching real card bytes, which was true in practice but not guaranteed by
/// the structure of the format. A failure to even read the marker (truncated or
/// unreadable payload) is still `Malformed`, never silently treated as a scene.
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

    let mut c = Cur { b: &buf, p: 0 };
    c.i32v().map_err(CardError::Malformed)?; // ProductNo
    let marker = c.string().map_err(CardError::Malformed)?;

    // Only when the marker is not recognised do we consider this might be a
    // KStudio scene card: its payload has no ProductNo and opens directly with a
    // version string where a chara card keeps its marker, so re-read from offset
    // 0 as a length-prefixed string and check whether it looks like a dotted
    // version number.
    let (game, route) = match classify_marker(&marker) {
        Some(gr) => gr,
        None => {
            let mut probe = Cur { b: &buf, p: 0 };
            if let Ok(s) = probe.string() {
                if looks_like_version(&s) {
                    return Err(CardError::Scene(s));
                }
            }
            return Err(CardError::Unrecognized(marker));
        }
    };

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
    let start = blocks_at.saturating_add(pos as usize);
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
    ///
    /// Under the reordered read (marker read attempted first, scene probe only as
    /// fallback), the initial marker read starts 4 bytes in and misinterprets one
    /// of the version string's own bytes as a length prefix — it needs enough
    /// trailing bytes present to complete without truncating, so it can fail to
    /// recognise the garbage marker and fall through to the probe, rather than
    /// erroring out as `Malformed` before the probe ever runs. `fixture::scene`'s
    /// minimal payload is real card size; pad it out here so this stays a test of
    /// scene detection, not of the fixture's brevity.
    #[test]
    fn a_scene_card_is_recognized_by_its_version_string() {
        let d = Dir::new();
        let mut bytes = fixture::scene("1.0.4.2");
        bytes.extend(std::iter::repeat(0u8).take(128));
        let p = file(&d, "a.png", &bytes);
        match read_card(&p) {
            Err(CardError::Scene(v)) => assert_eq!(v, "1.0.4.2"),
            other => panic!("expected Scene, got {other:?}"),
        }
    }

    /// The reordered read makes a recognised marker win unconditionally: even
    /// though this card's `ProductNo` bytes (`100i32` little-endian, so byte 0 is
    /// `0x64` = 100) would happily feed the old first-run scene probe as a
    /// plausible length prefix, the marker is read and classified before the
    /// probe is ever consulted, so a real card can never be misfiled as a scene.
    #[test]
    fn a_recognized_marker_is_never_reclassified_as_a_scene() {
        let d = Dir::new();
        let p = file(&d, "a.png", &fixture::card("【KoiKatuChara】", 1, "a", "b"));
        let m = read_card(&p).expect("a recognised marker must classify as a card");
        assert_eq!(m.game, Game::Koikatu);
        assert_eq!(m.route, Route::BySex);
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
