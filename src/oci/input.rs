//! Independent archive ranges for parallel reads without copying layer payloads.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use anyhow::{Context, Result};

use super::{LoadProgress, progress};

pub(super) trait ArchiveInput: Read + Seek + Sync {
    fn range(&self, offset: u64, size: u64) -> Result<Box<dyn Read + Send + '_>>;
}

// Check both stored-byte reads and decoded reads. Bound each read so hashing and
// decompression return to the cancellation check even for very large payloads.
pub(super) struct Cancellable<'a, R> {
    reader: R,
    progress: Option<&'a LoadProgress>,
}

impl<'a, R> Cancellable<'a, R> {
    pub fn new(reader: R, progress: Option<&'a LoadProgress>) -> Self {
        Self { reader, progress }
    }
}

impl<R: Read> Read for Cancellable<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        progress::check(self.progress).map_err(io::Error::other)?;
        let length = buffer.len().min(64 * 1024);
        self.reader.read(&mut buffer[..length])
    }
}

impl<R: Seek> Seek for Cancellable<'_, R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        progress::check(self.progress).map_err(io::Error::other)?;
        self.reader.seek(position)
    }
}

impl<R: ArchiveInput> ArchiveInput for Cancellable<'_, &mut R> {
    fn range(&self, offset: u64, size: u64) -> Result<Box<dyn Read + Send + '_>> {
        progress::check(self.progress)?;
        Ok(Box::new(Cancellable::new(
            self.reader.range(offset, size)?,
            self.progress,
        )))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn cancellation_stops_reads_seeks_and_independent_ranges() {
        let (sender, _) = mpsc::channel();
        let progress = LoadProgress::new(sender);
        let mut bytes = Cursor::new(vec![0; 128 * 1024]);
        let mut input = Cancellable::new(&mut bytes, Some(&progress));
        let mut buffer = vec![0; 128 * 1024];
        assert_eq!(input.read(&mut buffer).unwrap(), 64 * 1024);
        let mut range = input.range(0, 100).unwrap();
        progress.cancel();
        let error = range.read(&mut buffer).unwrap_err();
        // Interrupted would be retried by io::copy, preventing cancellation.
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("cancelled"));
        drop(range);
        assert!(input.read(&mut buffer).is_err());
        assert!(input.rewind().is_err());
        assert!(input.range(0, 100).is_err());
    }
}
