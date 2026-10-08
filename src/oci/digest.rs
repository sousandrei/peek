//! Hash streams without retaining their payloads.

use std::io::{self, Read};

use anyhow::Result;
use sha2::{Digest, Sha256};

pub fn hash_reader(reader: &mut impl Read) -> Result<String> {
    let mut reader = HashingReader::new(reader);
    io::copy(&mut reader, &mut io::sink())?;

    Ok(reader.digest())
}

pub struct HashingReader<R> {
    reader: R,
    hash: Sha256,
}

impl<R> HashingReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            hash: Sha256::new(),
        }
    }

    pub fn digest(self) -> String {
        let bytes = self.hash.finalize();
        let hex = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        format!("sha256:{hex}")
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.reader.read(buffer)?;
        self.hash.update(&buffer[..count]);

        Ok(count)
    }
}
