use std::{
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};

use crate::oci::{self, ChangeKind, FileKind};

struct TestBuilder {
    name: String,
    removed: bool,
}

impl TestBuilder {
    fn create() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let builder = Self {
            name: format!("peek-test-{}-{timestamp}", std::process::id()),
            removed: false,
        };
        let output = Command::new("docker")
            .args([
                "buildx",
                "create",
                "--name",
                &builder.name,
                "--driver",
                "docker-container",
                "--bootstrap",
            ])
            .output()
            .expect("Docker Buildx must be installed for image integration tests");

        assert!(
            output.status.success(),
            "cannot create Docker Buildx test builder: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        builder
    }

    fn remove(mut self) {
        let output = Command::new("docker")
            .args(["buildx", "rm", "--force", &self.name])
            .output()
            .expect("Docker Buildx must be available to remove the test builder");

        assert!(
            output.status.success(),
            "cannot remove Docker Buildx test builder {}: {}",
            self.name,
            String::from_utf8_lossy(&output.stderr)
        );

        self.removed = true;
    }
}

impl Drop for TestBuilder {
    fn drop(&mut self) {
        if self.removed {
            return;
        }

        let cleanup = Command::new("docker")
            .args(["buildx", "rm", "--force", &self.name])
            .output();

        match cleanup {
            Ok(output) if output.status.success() => {}
            Ok(output) => eprintln!(
                "could not clean up Docker Buildx test builder {}: {}",
                self.name,
                String::from_utf8_lossy(&output.stderr)
            ),
            Err(error) => eprintln!(
                "could not run Docker Buildx cleanup for {}: {error}",
                self.name
            ),
        }
    }
}

fn build_fixture_analysis(builder: &TestBuilder, name: &str, compression: &str) -> oci::Analysis {
    let context = format!(
        "{}/tests/fixtures/docker-images/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let dockerfile = format!("{context}/Dockerfile");
    let output = Command::new("docker")
        .args([
            "buildx",
            "build",
            "--builder",
            &builder.name,
            "--file",
            &dockerfile,
            "--output",
            &format!("type=oci,dest=-,compression={compression}"),
            "--provenance=false",
            "--progress=quiet",
            "--platform",
            "linux/amd64",
            &context,
        ])
        .output()
        .expect("Docker Buildx must be installed for image integration tests");

    assert!(
        output.status.success(),
        "Docker Buildx failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let image = oci::archive_for_test(&output.stdout).unwrap_or_else(|error| {
        panic!("Buildx emitted an invalid OCI archive for {name}: {error:#}")
    });

    oci::analyze(image)
        .unwrap_or_else(|error| panic!("cannot analyze OCI fixture {name}: {error:#}"))
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

fn assert_layer_diffs(builder: &TestBuilder) {
    let analysis = build_fixture_analysis(builder, "layer-diffs", "gzip");
    let changes: Vec<_> = analysis
        .layers
        .iter()
        .flat_map(|layer| &layer.changes)
        .collect();

    assert!(changes.iter().any(|change| {
        change.path == "/opt/peek/modified.txt" && change.kind == ChangeKind::Modified
    }));
    assert!(changes.iter().any(|change| {
        change.path == "/opt/peek/removed.txt" && change.kind == ChangeKind::Removed
    }));
    assert!(changes.iter().any(|change| {
        change.path == "/opt/peek/added.txt" && change.kind == ChangeKind::Added
    }));
    assert!(changes.iter().any(|change| {
        change.path == "/opt/peek/replaced" && change.kind == ChangeKind::Modified
    }));

    assert_eq!(
        analysis.filesystem["/opt/peek/modified.txt"].content_digest,
        Some(digest(b"updated\n"))
    );
    assert_eq!(
        analysis.filesystem["/opt/peek/added.txt"].content_digest,
        Some(digest(b"added\n"))
    );
    assert!(!analysis.filesystem.contains_key("/opt/peek/removed.txt"));
    assert_eq!(
        analysis.filesystem["/opt/peek/replaced"].kind,
        FileKind::File
    );
}

fn assert_file_types(builder: &TestBuilder) {
    let analysis = build_fixture_analysis(builder, "file-types", "gzip");
    let target = &analysis.filesystem["/opt/peek/target.txt"];
    let symlink = &analysis.filesystem["/opt/peek/symlink"];
    let hardlink = &analysis.filesystem["/opt/peek/hardlink"];

    assert_eq!(target.mode, 0o600);
    assert_eq!(symlink.kind, FileKind::Symlink);
    assert_eq!(symlink.link_target.as_deref(), Some("target.txt"));
    assert_eq!(hardlink.kind, FileKind::File);
    assert_eq!(hardlink.content_digest, target.content_digest);
    assert_eq!(hardlink.size_bytes, target.size_bytes);
}

fn assert_zstd_archive(builder: &TestBuilder) {
    let analysis = build_fixture_analysis(builder, "layer-diffs", "zstd");

    assert_eq!(
        analysis.filesystem["/opt/peek/modified.txt"].content_digest,
        Some(digest(b"updated\n"))
    );
    assert!(analysis.layers.len() > 1);
}

#[test]
fn real_dockerfile_fixtures_are_exported_and_analyzed() {
    let builder = TestBuilder::create();

    assert_layer_diffs(&builder);
    assert_file_types(&builder);
    assert_zstd_archive(&builder);

    builder.remove();
}
