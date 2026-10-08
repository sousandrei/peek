//! Normalize Docker and OCI archives into ordered OCI filesystem layers.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use super::digest::hash_reader;
use super::layer::read_layer;
use super::model::{Image, ImageMetadata, Layer};

#[derive(Clone)]
struct Blob {
    offset: u64,
    size: u64,
    link: Option<String>,
}

#[derive(Deserialize)]
struct Descriptor {
    digest: String,
    size: u64,
    #[serde(rename = "mediaType")]
    media_type: String,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Index {
    manifests: Vec<Descriptor>,
}

#[derive(Deserialize)]
struct Manifest {
    config: Descriptor,
    layers: Vec<Descriptor>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerManifest {
    config: String,
    layers: Vec<String>,
    #[serde(default)]
    repo_tags: Option<Vec<String>>,
}

#[derive(Default, Deserialize)]
struct Config {
    #[serde(default)]
    architecture: String,
    #[serde(default)]
    os: String,
    #[serde(default)]
    history: Vec<History>,
    #[serde(default)]
    rootfs: Rootfs,
}

#[derive(Default, Deserialize)]
struct Rootfs {
    #[serde(default)]
    diff_ids: Vec<String>,
}

#[derive(Deserialize)]
struct History {
    #[serde(default)]
    created_by: String,
    #[serde(default)]
    empty_layer: bool,
}

struct LayerSource {
    path: String,
    descriptor: Option<Descriptor>,
}

pub fn read<R: Read + Seek>(reader: &mut R, reference: &str) -> Result<Image> {
    let blobs = index_archive(reader)?;
    let ManifestData {
        config,
        layer_sources,
        tags,
    } = if blobs.contains_key("index.json") {
        read_oci_manifest(reader, &blobs)?
    } else {
        read_docker_manifest(reader, &blobs)?
    };

    ensure!(
        config.rootfs.diff_ids.is_empty() || config.rootfs.diff_ids.len() == layer_sources.len(),
        "config diff_ids count does not match manifest layer count"
    );

    let mut history = config.history.iter().filter(|entry| !entry.empty_layer);
    let mut layers = Vec::with_capacity(layer_sources.len());

    for (index, source) in layer_sources.into_iter().enumerate() {
        if let Some(descriptor) = &source.descriptor {
            verify_blob(reader, &blobs, &source.path, descriptor)?;
        }

        let blob = resolve_blob(&blobs, &source.path)?;
        reader.seek(SeekFrom::Start(blob.offset))?;
        let parsed = read_layer(
            reader.take(blob.size),
            index,
            source.descriptor.as_ref().map(|d| d.media_type.as_str()),
        )
        .with_context(|| format!("cannot parse layer {index} ({})", source.path))?;

        if let Some(expected) = config.rootfs.diff_ids.get(index) {
            ensure!(
                parsed.diff_id == *expected,
                "uncompressed layer digest mismatch at layer {index}"
            );
        }

        layers.push(Layer {
            index,
            id: source
                .descriptor
                .as_ref()
                .map(|d| d.digest.clone())
                .unwrap_or(source.path),
            diff_id: Some(parsed.diff_id),
            command: history
                .next()
                .map(|entry| entry.created_by.clone())
                .unwrap_or_else(|| "(missing)".into()),
            blob_size_bytes: blob.size,
            entries: parsed.entries,
        });
    }

    Ok(Image {
        metadata: ImageMetadata {
            reference: reference.into(),
            tags,
            architecture: config.architecture,
            os: config.os,
        },
        layers,
    })
}

struct ManifestData {
    config: Config,
    layer_sources: Vec<LayerSource>,
    tags: Vec<String>,
}

fn read_oci_manifest<R: Read + Seek>(
    reader: &mut R,
    blobs: &BTreeMap<String, Blob>,
) -> Result<ManifestData> {
    let index: Index = read_json(reader, blobs, "index.json")?;
    ensure!(
        index.manifests.len() == 1,
        "OCI archive must select exactly one image; multi-image/platform selection is not supported yet"
    );

    let mut descriptor = index
        .manifests
        .into_iter()
        .next()
        .context("empty OCI index")?;
    let tags = descriptor
        .annotations
        .get("io.containerd.image.name")
        .or_else(|| {
            descriptor
                .annotations
                .get("org.opencontainers.image.ref.name")
        })
        .into_iter()
        .cloned()
        .collect();

    let mut visited = BTreeSet::new();
    let manifest = loop {
        ensure!(
            visited.insert(descriptor.digest.clone()),
            "cyclic OCI index"
        );
        let path = descriptor_path(&descriptor)?;
        verify_blob(reader, blobs, &path, &descriptor)?;
        if descriptor.media_type.ends_with("index.v1+json")
            || descriptor.media_type.ends_with("manifest.list.v2+json")
        {
            let index: Index = read_json(reader, blobs, &path)?;
            ensure!(
                index.manifests.len() == 1,
                "OCI index requires image/platform selection"
            );
            descriptor = index
                .manifests
                .into_iter()
                .next()
                .context("empty OCI index")?;
        } else {
            let manifest: Manifest = read_json(reader, blobs, &path)?;
            break manifest;
        }
    };

    let path = descriptor_path(&manifest.config)?;
    verify_blob(reader, blobs, &path, &manifest.config)?;
    let config: Config = read_json(reader, blobs, &path)?;

    let sources = manifest
        .layers
        .into_iter()
        .map(|descriptor| {
            Ok(LayerSource {
                path: descriptor_path(&descriptor)?,
                descriptor: Some(descriptor),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ManifestData {
        config,
        layer_sources: sources,
        tags,
    })
}

fn read_docker_manifest<R: Read + Seek>(
    reader: &mut R,
    blobs: &BTreeMap<String, Blob>,
) -> Result<ManifestData> {
    let manifests: Vec<DockerManifest> = read_json(reader, blobs, "manifest.json")
        .context("archive contains neither an OCI index nor a Docker manifest")?;
    ensure!(
        manifests.len() == 1,
        "Docker archive contains multiple images; selection is not supported yet"
    );
    let manifest = manifests
        .into_iter()
        .next()
        .context("empty Docker manifest")?;
    let config: Config = read_json(reader, blobs, &manifest.config)?;

    let sources = manifest
        .layers
        .into_iter()
        .map(|path| LayerSource {
            path,
            descriptor: None,
        })
        .collect();

    Ok(ManifestData {
        config,
        layer_sources: sources,
        tags: manifest.repo_tags.unwrap_or_default(),
    })
}

fn index_archive<R: Read + Seek>(reader: &mut R) -> Result<BTreeMap<String, Blob>> {
    let mut archive = tar::Archive::new(reader);
    let mut blobs = BTreeMap::new();
    for entry in archive
        .entries_with_seek()
        .context("invalid outer tar archive")?
    {
        let entry = entry.context("invalid archive entry")?;
        let path = normalize_path(
            std::str::from_utf8(&entry.path_bytes()).context("archive path is not UTF-8")?,
        )?;
        let path = path.trim_start_matches('/').to_owned();
        if entry.header().entry_type().is_dir() {
            continue;
        }
        let kind = entry.header().entry_type();
        ensure!(
            kind.is_file() || kind.is_symlink() || kind.is_hard_link(),
            "unsupported archive entry type at {path}"
        );
        let link = entry
            .link_name_bytes()
            .map(|bytes| -> Result<String> {
                let raw = std::str::from_utf8(&bytes).context("archive link is not UTF-8")?;
                let target = if kind.is_symlink() {
                    let parent = path
                        .rsplit_once('/')
                        .map(|(parent, _)| parent)
                        .unwrap_or("");
                    format!("{parent}/{raw}")
                } else {
                    raw.to_owned()
                };
                Ok(normalize_path(&target)?.trim_start_matches('/').to_owned())
            })
            .transpose()?;
        ensure!(
            blobs
                .insert(
                    path.clone(),
                    Blob {
                        offset: entry.raw_file_position(),
                        size: entry.size(),
                        link
                    }
                )
                .is_none(),
            "duplicate archive path {path}"
        );
    }
    Ok(blobs)
}

fn resolve_blob<'a>(blobs: &'a BTreeMap<String, Blob>, path: &str) -> Result<&'a Blob> {
    let mut path = normalize_path(path)?.trim_start_matches('/').to_owned();

    let mut visited = BTreeSet::new();
    loop {
        ensure!(
            visited.insert(path.clone()),
            "cyclic archive link at {path}"
        );
        let blob = blobs
            .get(&path)
            .with_context(|| format!("missing archive blob {path}"))?;
        match &blob.link {
            Some(target) => path = target.clone(),
            None => return Ok(blob),
        }
    }
}

fn read_json<R: Read + Seek, T: serde::de::DeserializeOwned>(
    reader: &mut R,
    blobs: &BTreeMap<String, Blob>,
    path: &str,
) -> Result<T> {
    let blob = resolve_blob(blobs, path)?;
    ensure!(
        blob.size <= 16 * 1024 * 1024,
        "JSON metadata exceeds 16 MiB at {path}"
    );
    reader.seek(SeekFrom::Start(blob.offset))?;
    serde_json::from_reader(reader.take(blob.size))
        .with_context(|| format!("invalid JSON in {path}"))
}

fn descriptor_path(descriptor: &Descriptor) -> Result<String> {
    let (algorithm, digest) = descriptor
        .digest
        .split_once(':')
        .context("invalid OCI descriptor digest")?;
    ensure!(
        algorithm == "sha256"
            && digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "unsupported or malformed OCI digest {}",
        descriptor.digest
    );
    Ok(format!("blobs/{algorithm}/{digest}"))
}

fn verify_blob<R: Read + Seek>(
    reader: &mut R,
    blobs: &BTreeMap<String, Blob>,
    path: &str,
    descriptor: &Descriptor,
) -> Result<()> {
    let blob = resolve_blob(blobs, path)?;
    ensure!(
        blob.size == descriptor.size,
        "OCI size mismatch for {}",
        descriptor.digest
    );
    reader.seek(SeekFrom::Start(blob.offset))?;
    let digest = hash_reader(&mut reader.take(blob.size))?;
    ensure!(
        digest == descriptor.digest,
        "OCI digest mismatch for {}",
        descriptor.digest
    );
    Ok(())
}

pub(super) fn normalize_path(path: &str) -> Result<String> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => bail!("path traversal is not allowed: {path:?}"),
            part => parts.push(part),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}
