//! A `Read + Seek` wrapper that goes bad the second time it is rewound to a
//! chosen absolute position — modelling a network share or a locked handle
//! that blinks partway through a run, specifically during the rewind
//! `read_card_from`'s scene probe performs. Every real read up to that point
//! succeeds; only reads issued after the second seek to `trap_pos` fail.
#![allow(dead_code)]

use std::io::{self, Read, Seek, SeekFrom};

pub struct SeekTrap<R> {
    inner: R,
    trap_pos: u64,
    visits: u32,
    poisoned: bool,
}

impl<R> SeekTrap<R> {
    pub fn new(inner: R, trap_pos: u64) -> Self {
        SeekTrap {
            inner,
            trap_pos,
            visits: 0,
            poisoned: false,
        }
    }
}

impl<R: Read> Read for SeekTrap<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.poisoned {
            return Err(io::Error::new(io::ErrorKind::Other, "simulated I/O failure after rewind"));
        }
        self.inner.read(buf)
    }
}

impl<R: Seek> Seek for SeekTrap<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let at = self.inner.seek(pos)?;
        if at == self.trap_pos {
            self.visits += 1;
            if self.visits >= 2 {
                self.poisoned = true;
            }
        }
        Ok(at)
    }
}
