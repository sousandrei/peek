//! Decode an OCI layer stream into file entries and a verified uncompressed digest.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, BufReader, Read};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};

use super::LoadProgress;
use super::archive::normalize_path;
use super::digest::{HashingReader, hash_reader};
use super::input::Cancellable;
use super::model::{FileEntry, FileKind};

pub struct ParsedLayer {
    pub entries: Vec<Arc<FileEntry>>,
    pub diff_id: String,
}

pub fn read_layer<R: Read>(
    reader: R,
    layer_index: usize,
    media_type: Option<&str>,
    progress: Option<&LoadProgress>,
) -> Result<ParsedLayer> {
    let mut reader = BufReader::new(Cancellable::new(reader, progress));
    let prefix = reader.fill_buf()?;
    let gzip = prefix.starts_with(&[0x1f, 0x8b]);
    let zstd = prefix.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]);

    if let Some(media_type) = media_type {
        ensure!(
            media_type.contains(".tar"),
            "unsupported OCI layer media type {media_type}"
        );
        ensure!(
            !media_type.ends_with("+gzip") || gzip,
            "layer media type requires gzip"
        );
        ensure!(
            !media_type.ends_with("+zstd") || zstd,
            "layer media type requires zstd"
        );
    }

    let decoded: Box<dyn Read + '_> = if gzip {
        Box::new(flate2::read::MultiGzDecoder::new(reader))
    } else if zstd {
        Box::new(zstd::stream::read::Decoder::new(reader)?)
    } else {
        Box::new(reader)
    };

    let mut decoded = HashingReader::new(Cancellable::new(decoded, progress));
    let entries = read_entries(&mut decoded, layer_index)?;

    // Read trailing bytes too: compression checksums and OCI DiffID cover the complete stream.
    io::copy(&mut decoded, &mut io::sink())?;

    Ok(ParsedLayer {
        entries,
        diff_id: decoded.digest(),
    })
}

fn read_entries<R: Read>(reader: &mut R, layer_index: usize) -> Result<Vec<Arc<FileEntry>>> {
    let mut archive = tar::Archive::new(reader);
    let mut files = Vec::new();
    let mut paths = BTreeSet::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let file = read_entry(&mut entry, layer_index)?;
        ensure!(
            paths.insert(file.path.clone()),
            "duplicate layer path {}",
            file.path
        );
        files.push(file);
    }

    Ok(files)
}

fn read_entry<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    layer_index: usize,
) -> Result<Arc<FileEntry>> {
    let path = normalize_path(
        std::str::from_utf8(&entry.path_bytes()).context("layer path is not UTF-8")?,
    )?;
    let header = entry.header().clone();
    let name = path.rsplit('/').next().unwrap_or("");
    let kind = if name == ".wh..wh..opq" {
        FileKind::OpaqueWhiteout
    } else if let Some(target) = name.strip_prefix(".wh.") {
        ensure!(!target.is_empty(), "invalid whiteout at {path}");
        FileKind::Whiteout
    } else {
        match header.entry_type().as_byte() {
            0 | b'0' => FileKind::File,
            b'1' => FileKind::Hardlink,
            b'2' => FileKind::Symlink,
            b'3' => FileKind::CharacterDevice,
            b'4' => FileKind::BlockDevice,
            b'5' => FileKind::Directory,
            b'6' => FileKind::Fifo,
            other => bail!("unsupported tar entry type {other} at {path}"),
        }
    };

    if matches!(kind, FileKind::Whiteout | FileKind::OpaqueWhiteout) {
        ensure!(
            header.entry_type().is_file() && entry.size() == 0,
            "whiteout must be an empty regular file at {path}"
        );
    }

    let link_target = entry
        .link_name_bytes()
        .map(|bytes| -> Result<String> {
            let raw = std::str::from_utf8(&bytes).context("link target is not UTF-8")?;
            if kind == FileKind::Hardlink {
                normalize_path(raw)
            } else {
                Ok(raw.to_owned())
            }
        })
        .transpose()?;

    let mut mtime = header.mtime()?.to_string();
    let mut xattrs = BTreeMap::new();
    if let Some(extensions) = entry.pax_extensions()? {
        for extension in extensions {
            let extension = extension?;
            if extension.key_bytes() == b"mtime" {
                mtime = std::str::from_utf8(extension.value_bytes())?.to_owned();
            } else if let Some(name) = extension.key()?.strip_prefix("SCHILY.xattr.") {
                let value = extension
                    .value_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                xattrs.insert(name.to_owned(), value);
            }
        }
    }

    let content_digest = if kind == FileKind::File {
        Some(hash_reader(entry)?)
    } else {
        None
    };

    Ok(Arc::new(FileEntry {
        path,
        kind,
        size_bytes: entry.size(),
        mode: header.mode()?,
        uid: header.uid()?,
        gid: header.gid()?,
        mtime,
        link_target,
        content_digest,
        device_major: if matches!(kind, FileKind::CharacterDevice | FileKind::BlockDevice) {
            header.device_major()?
        } else {
            None
        },
        device_minor: if matches!(kind, FileKind::CharacterDevice | FileKind::BlockDevice) {
            header.device_minor()?
        } else {
            None
        },
        xattrs,
        layer_index,
        content_layer_index: (kind == FileKind::File).then_some(layer_index),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::mpsc;

    struct CancelOnRead<'a> {
        bytes: Cursor<Vec<u8>>,
        progress: &'a LoadProgress,
        cancel_at: u64,
    }

    impl Read for CancelOnRead<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.bytes.position() < self.cancel_at {
                let length = buffer
                    .len()
                    .min((self.cancel_at - self.bytes.position()) as usize);
                return self.bytes.read(&mut buffer[..length]);
            }
            let count = self.bytes.read(buffer)?;
            self.progress.cancel();
            Ok(count)
        }
    }

    #[test]
    fn cancellation_interrupts_buffered_decompression_and_trailing_data() {
        for trailing in [false, true] {
            let (sender, _) = mpsc::channel();
            let progress = LoadProgress::new(sender);
            let mut encoded = Vec::new();
            let mut first =
                flate2::read::GzEncoder::new(&[0_u8; 1024][..], flate2::Compression::default());
            first.read_to_end(&mut encoded).unwrap();
            let boundary = encoded.len() as u64;
            let mut second =
                flate2::read::GzEncoder::new(&b"trailing data"[..], flate2::Compression::default());
            second.read_to_end(&mut encoded).unwrap();
            let reader = CancelOnRead {
                bytes: Cursor::new(encoded),
                progress: &progress,
                // Either cancel with compressed bytes already buffered, or
                // when draining the member after the tar terminator.
                cancel_at: if trailing { boundary } else { 0 },
            };
            let error = read_layer(
                reader,
                0,
                Some("application/vnd.oci.image.layer.v1.tar+gzip"),
                Some(&progress),
            )
            .err()
            .unwrap();
            assert!(format!("{error:#}").contains("cancelled"));
        }
    }
}
