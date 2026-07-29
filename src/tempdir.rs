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
