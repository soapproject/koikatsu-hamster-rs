//! A `Read + Seek` wrapper that counts bytes actually read, so a test can pin
//! how much of a stream a parser touches. Guards the point of the whole
//! streaming rewrite: a seek costs nothing here, only a `read` adds to the
//! count, because the C# tool this replaces earns its speed by seeking past
//! data it does not need instead of reading past it.
#![allow(dead_code)]

use std::io::{self, Read, Seek, SeekFrom};

pub struct CountingReader<R> {
    inner: R,
    read: u64,
}

impl<R> CountingReader<R> {
    pub fn new(inner: R) -> Self {
        CountingReader { inner, read: 0 }
    }

    /// Total bytes handed back across every `read` call. Excludes seeking
    /// entirely — jumping over a stretch of the stream must not count against
    /// this the same way actually reading it would.
    pub fn bytes_read(&self) -> u64 {
        self.read
    }
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        Ok(n)
    }
}

impl<R: Seek> Seek for CountingReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}
