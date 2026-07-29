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
    let mut bytes = fixture::card("【KoiKatuChara】", 1, "a", "b");
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

/// `check_root`'s decision logic is unit-tested in `src/main.rs`, but only this
/// layer can observe a real process's exit code and stderr — the thing that
/// actually matters for a typoed `--root`: no banner promising a scan that never
/// ran, a clear message naming the bad path, and a non-zero exit instead of a
/// silent, empty success.
#[test]
fn a_nonexistent_root_is_reported_on_stderr_and_exits_nonzero() {
    let root = temp_root("missing");
    let missing = root.join("does-not-exist");

    let out = Command::new(EXE).arg("--root").arg(&missing).output().expect("run");
    assert!(!out.status.success(), "exit {:?}", out.status.code());
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains(&missing.display().to_string()), "{err}");

    let _ = std::fs::remove_dir_all(&root);
}
