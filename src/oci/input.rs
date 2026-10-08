//! Independent archive ranges for parallel reads without copying layer payloads.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek};

use anyhow::{Context, Result};

pub(super) trait ArchiveInput: Read + Seek + Sync {
    fn range(&self, offset: u64, size: u64) -> Result<Box<dyn Read + Send + '_>>;
}

impl<T: AsRef<[u8]> + Sync> ArchiveInput for Cursor<T> {
    fn range(&self, offset: u64, size: u64) -> Result<Box<dyn Read + Send + '_>> {
        let start = usize::try_from(offset).context("archive offset exceeds address space")?;
        let end = offset.checked_add(size).context("archive range overflow")?;
        let end = usize::try_from(end).context("archive range exceeds address space")?;
        let bytes = self
            .get_ref()
            .as_ref()
            .get(start..end)
            .context("truncated archive range")?;
        Ok(Box::new(bytes))
    }
}

impl ArchiveInput for File {
    fn range(&self, offset: u64, size: u64) -> Result<Box<dyn Read + Send + '_>> {
        Ok(Box::new(FileRange {
            file: self,
            offset,
            remaining: size,
        }))
    }
}

struct FileRange<'a> {
    file: &'a File,
    offset: u64,
    remaining: u64,
}

impl Read for FileRange<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = buffer
            .len()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        if length == 0 {
            return Ok(0);
        }

        #[cfg(unix)]
        let count =
            std::os::unix::fs::FileExt::read_at(self.file, &mut buffer[..length], self.offset)?;
        #[cfg(windows)]
        let count = std::os::windows::fs::FileExt::seek_read(
            self.file,
            &mut buffer[..length],
            self.offset,
        )?;

        self.offset += count as u64;
        self.remaining -= count as u64;
        Ok(count)
    }
}
