use std::io::{Cursor, Write};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{export, oci};

struct Entry<'a> {
    path: &'a str,
    content: &'a [u8],
    kind: tar::EntryType,
    link: Option<&'a str>,
    mode: u32,
}

fn file<'a>(path: &'a str, content: &'a [u8]) -> Entry<'a> {
    Entry {
        path,
        content,
        kind: tar::EntryType::Regular,
        link: None,
        mode: 0o644,
    }
}

fn directory(path: &str) -> Entry<'_> {
    Entry {
        kind: tar::EntryType::Directory,
        mode: 0o755,
        ..file(path, b"")
    }
}

fn link<'a>(path: &'a str, target: &'a str, kind: tar::EntryType) -> Entry<'a> {
    Entry {
        kind,
        link: Some(target),
        ..file(path, b"")
    }
}

fn layer(entries: &[Entry<'_>]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(entry.kind);
        header.set_size(entry.content.len() as u64);
        header.set_mode(entry.mode);
        header.set_uid(1000);
        header.set_gid(1000);
        header.set_mtime(0);
        if let Some(target) = entry.link {
            header.set_link_name(target).unwrap();
        }
        header.set_cksum();
        builder
            .append_data(&mut header, entry.path, entry.content)
            .unwrap();
    }
    builder.into_inner().unwrap()
}

fn outer(files: &[(&str, &[u8])]) -> Vec<u8> {
    layer(
        &files
            .iter()
            .map(|(path, content)| file(path, content))
            .collect::<Vec<_>>(),
    )
}

fn docker_archive(layers: &[Vec<u8>]) -> Vec<u8> {
    let config = serde_json::to_vec(&json!({
        "architecture": "amd64", "os": "linux",
        "history": [{"created_by": "metadata", "empty_layer": true}, {"created_by": "base"}, {"created_by": "update"}]
    })).unwrap();
    let names: Vec<_> = (0..layers.len())
        .map(|index| format!("{index}/layer.tar"))
        .collect();
    let manifest = serde_json::to_vec(
        &json!([{"Config":"config.json", "RepoTags":["fixture:test"], "Layers":names}]),
    )
    .unwrap();
    let mut entries: Vec<_> = names
        .iter()
        .zip(layers)
        .rev()
        .map(|(path, content)| (path.as_str(), content.as_slice()))
        .collect();
    entries.extend([
        ("manifest.json", manifest.as_slice()),
        ("config.json", config.as_slice()),
    ]);
    outer(&entries)
}

fn analyze(bytes: &[u8]) -> oci::Analysis {
    oci::analyze(oci::archive_for_test(bytes).unwrap()).unwrap()
}

#[test]
fn incremental_diff_and_full_view_follow_manifest_order() {
    let archive = docker_archive(&[
        layer(&[
            directory("app"),
            file("app/main", b"old"),
            file("obsolete", b"gone"),
        ]),
        layer(&[
            file("app/main", b"new"),
            file(".wh.obsolete", b""),
            file("app/config", b"cfg"),
        ]),
    ]);
    let result = analyze(&archive);
    assert_eq!(result.layers[0].layer.command, "base");
    assert_eq!(result.layers[1].layer.command, "update");
    let diff = export::value(&result, export::LayerView::Diff);
    let changes = diff["layers"][1]["data"]["changes"].as_array().unwrap();
    assert_eq!(
        changes
            .iter()
            .map(|c| (c["path"].as_str().unwrap(), c["kind"].as_str().unwrap()))
            .collect::<Vec<_>>(),
        [
            ("/app/config", "added"),
            ("/app/main", "modified"),
            ("/obsolete", "removed")
        ]
    );
    assert_eq!(changes[1]["before"]["sizeBytes"], 3);
    assert_eq!(changes[2]["after"], serde_json::Value::Null);
    let full = export::value(&result, export::LayerView::Full);
    let files = full["layers"][1]["data"]["files"].as_array().unwrap();
    assert_eq!(
        files
            .iter()
            .map(|f| f["path"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["/app", "/app/config", "/app/main"]
    );
    assert_eq!(diff["schemaVersion"], 1);
    assert_eq!(diff["layers"][0]["data"]["view"], "diff");
    assert_eq!(full["layers"][0]["data"]["view"], "full");
}

#[test]
fn opaque_and_regular_whiteouts_preserve_same_layer_files() {
    let result = analyze(&docker_archive(&[
        layer(&[
            directory("etc"),
            file("etc/old", b"old"),
            file("replace", b"old"),
        ]),
        layer(&[
            file("etc/new", b"new"),
            file("replace", b"new"),
            file("etc/.wh..wh..opq", b""),
            file(".wh.replace", b""),
        ]),
    ]));
    assert!(result.filesystem.contains_key("/etc/new"));
    assert!(result.filesystem.contains_key("/replace"));
    assert!(!result.filesystem.contains_key("/etc/old"));
    assert!(!result.filesystem.keys().any(|path| path.contains(".wh.")));
    assert_eq!(
        result.layers[1]
            .changes
            .iter()
            .find(|c| c.path == "/replace")
            .unwrap()
            .kind,
        crate::oci::ChangeKind::Modified
    );
}

#[test]
fn directory_replacement_removes_descendants_but_preserves_siblings() {
    let result = analyze(&docker_archive(&[
        layer(&[directory("a"), file("a/child", b"old"), file("ab", b"keep")]),
        layer(&[file("a", b"new")]),
    ]));
    assert!(!result.filesystem.contains_key("/a/child"));
    assert!(result.filesystem.contains_key("/ab"));
    assert_eq!(result.filesystem["/a"].kind, crate::oci::FileKind::File);
}

#[test]
fn links_metadata_changes_and_content_provenance_are_explicit() {
    let mut changed_mode = file("data", b"same");
    changed_mode.mode = 0o600;
    let result = analyze(&docker_archive(&[
        layer(&[
            file("data", b"same"),
            link("sym", "old", tar::EntryType::Symlink),
            file("inherited", b"payload"),
        ]),
        layer(&[
            changed_mode,
            link("sym", "new", tar::EntryType::Symlink),
            link("second", "first", tar::EntryType::Link),
            link("first", "inherited", tar::EntryType::Link),
        ]),
    ]));
    assert_eq!(
        result.layers[1]
            .changes
            .iter()
            .find(|c| c.path == "/data")
            .unwrap()
            .kind,
        crate::oci::ChangeKind::Modified
    );
    assert_eq!(
        result.filesystem["/sym"].link_target.as_deref(),
        Some("new")
    );
    assert_eq!(
        result.filesystem["/second"].content_digest,
        result.filesystem["/inherited"].content_digest
    );
    assert_eq!(result.filesystem["/second"].size_bytes, 7);
    assert_eq!(result.filesystem["/second"].content_layer_index, Some(0));
}

#[test]
fn identical_entry_is_not_a_diff_but_updates_full_view_provenance() {
    let same = layer(&[file("same", b"same")]);
    let result = analyze(&docker_archive(&[same.clone(), same]));
    assert!(result.layers[1].changes.is_empty());
    let full = export::value(&result, export::LayerView::Full);
    assert_eq!(full["layers"][1]["data"]["files"][0]["layerIndex"], 1);
}

fn digest(bytes: &[u8]) -> String {
    format!(
        "sha256:{}",
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn oci_archive(layer_bytes: &[Vec<u8>], compression: &str) -> Vec<u8> {
    let config = serde_json::to_vec(&json!({"architecture":"amd64", "os":"linux"})).unwrap();
    let descriptor = |bytes: &[u8], media_type: &str| json!({"digest":digest(bytes), "size":bytes.len(), "mediaType":media_type});
    let config_descriptor = descriptor(&config, "application/vnd.oci.image.config.v1+json");
    let manifest = serde_json::to_vec(&json!({"schemaVersion":2,"config":config_descriptor,"layers":layer_bytes.iter().map(|l| descriptor(l, &format!("application/vnd.oci.image.layer.v1.tar{compression}"))).collect::<Vec<_>>()})).unwrap();
    let index = serde_json::to_vec(&json!({"schemaVersion":2,"manifests":[descriptor(&manifest,"application/vnd.oci.image.manifest.v1+json")]})).unwrap();
    let mut blobs: Vec<_> = layer_bytes
        .iter()
        .map(|bytes| {
            (
                format!(
                    "blobs/sha256/{}",
                    digest(bytes).strip_prefix("sha256:").unwrap()
                ),
                bytes.as_slice(),
            )
        })
        .collect();
    blobs.push((
        format!(
            "blobs/sha256/{}",
            digest(&config).strip_prefix("sha256:").unwrap()
        ),
        config.as_slice(),
    ));
    blobs.push((
        format!(
            "blobs/sha256/{}",
            digest(&manifest).strip_prefix("sha256:").unwrap()
        ),
        manifest.as_slice(),
    ));
    blobs.sort_by(|a, b| b.0.cmp(&a.0));
    let mut entries: Vec<_> = blobs
        .iter()
        .map(|(path, bytes)| (path.as_str(), *bytes))
        .collect();
    entries.push(("index.json", &index));
    outer(&entries)
}

#[test]
fn oci_descriptors_control_order_and_gzip_zstd_are_supported() {
    for compression in ["", "+gzip", "+zstd"] {
        let layers: Vec<_> = [b"old".as_slice(), b"new".as_slice()]
            .into_iter()
            .map(|content| {
                let bytes = layer(&[file("file", content)]);
                match compression {
                    "+gzip" => {
                        let mut encoder = flate2::write::GzEncoder::new(
                            Vec::new(),
                            flate2::Compression::default(),
                        );
                        encoder.write_all(&bytes).unwrap();
                        encoder.finish().unwrap()
                    }
                    "+zstd" => zstd::stream::encode_all(Cursor::new(bytes), 0).unwrap(),
                    _ => bytes,
                }
            })
            .collect();
        let result = analyze(&oci_archive(&layers, compression));
        assert_eq!(
            result.filesystem["/file"].content_digest,
            Some(digest(b"new"))
        );
        assert_eq!(result.layers[1].changes.len(), 1);
    }
}

#[test]
fn malformed_missing_and_cyclic_inputs_return_contextual_errors() {
    assert!(oci::archive_for_test(b"invalid").is_err());
    let missing = outer(&[("manifest.json", b"[{\"Config\":\"missing\",\"Layers\":[]}]")]);
    assert!(
        oci::archive_for_test(&missing)
            .unwrap_err()
            .to_string()
            .contains("missing archive blob")
    );
    let bytes = docker_archive(&[layer(&[
        link("a", "b", tar::EntryType::Link),
        link("b", "a", tar::EntryType::Link),
    ])]);
    assert!(
        oci::analyze(oci::archive_for_test(&bytes).unwrap())
            .unwrap_err()
            .to_string()
            .contains("cannot apply layer")
    );
}

#[test]
fn valid_reference_archives_load_offline() {
    for name in [
        "test-docker-image.tar",
        "test-kaniko-image.tar",
        "test-oci-uncompressed-image.tar",
        "test-oci-gzip-image.tar",
        "test-oci-zstd-image.tar",
        "test-oci-estargz-image.tar",
    ] {
        let path = format!("{}/../dive/.data/{name}", env!("CARGO_MANIFEST_DIR"));
        let image = oci::load(&path, oci::Source::DockerArchive)
            .unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let result = oci::analyze(image).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert!(!result.layers.is_empty());
    }
}

#[test]
fn reference_oci_archive_with_misordered_diff_ids_is_rejected() {
    let path = format!(
        "{}/../dive/.data/test-oci-docker-image.tar",
        env!("CARGO_MANIFEST_DIR")
    );
    let error = oci::load(&path, oci::Source::DockerArchive).unwrap_err();

    assert!(format!("{error:#}").contains("uncompressed layer digest mismatch"));
}
