//! Symphonia reads through a `MediaSource`. In the browser the whole file is
//! already a `Vec<u8>`, so this adapts an in-memory buffer to that trait and
//! keeps every read seekable — which is what makes `Decoder::seek` cheap.

use std::io::{Cursor, Read, Seek, SeekFrom};
use symphonia::core::io::MediaSource;

pub struct MemorySource {
    cursor: Cursor<Vec<u8>>,
    length: u64,
}

impl MemorySource {
    pub fn new(bytes: Vec<u8>) -> Self {
        let length = bytes.len() as u64;
        Self {
            cursor: Cursor::new(bytes),
            length,
        }
    }
}

impl Read for MemorySource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.cursor.read(buf)
    }
}

impl Seek for MemorySource {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.cursor.seek(pos)
    }
}

impl MediaSource for MemorySource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.length)
    }
}
