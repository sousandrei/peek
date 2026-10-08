//! Load local engine images, pulling missing images or platform variants once.

use std::io::{self, Cursor};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail, ensure};

use super::{Platform, archive, model::Image};

pub(super) fn load(engine: &str, reference: &str, platform: Option<&Platform>) -> Result<Image> {
    let saved_reference = engine_reference(reference);
    let target = platform.cloned().unwrap_or_else(Platform::host);
    let mut pulled = false;

    if !image_exists(engine, &saved_reference)? {
        pull(engine, &saved_reference, &target)?;
        pulled = true;
    }

    match save(engine, &saved_reference, reference, &target) {
        Err(error) if !pulled && error.is::<archive::MissingPlatform>() => {
            pull(engine, &saved_reference, &target)?;
            save(engine, &saved_reference, reference, &target)
        }
        result => result,
    }
}

fn image_exists(engine: &str, reference: &str) -> Result<bool> {
    let operation = if engine == "podman" {
        "exists"
    } else {
        "inspect"
    };
    let output = Command::new(engine)
        .args(["image", operation, "--", reference])
        .output()
        .with_context(|| format!("cannot run {engine} image {operation}"))?;

    if output.status.success() {
        return Ok(true);
    }

    let message = String::from_utf8_lossy(&output.stderr);
    let missing = if engine == "podman" {
        output.status.code() == Some(1)
    } else {
        // Docker inspect has no distinct missing-image exit code. Accept only its
        // specific daemon response; connection, permission, and other errors fail.
        message.lines().any(|line| {
            line.trim()
                .starts_with("Error response from daemon: No such image:")
        })
    };
    if missing {
        return Ok(false);
    }

    bail!(
        "{engine} image {operation} for {reference:?} failed ({}): {}",
        output.status,
        message.trim()
    )
}

fn pull(engine: &str, reference: &str, platform: &Platform) -> Result<()> {
    ensure!(
        !is_image_id(reference),
        "local image ID {reference:?} cannot be pulled; use a registry image name or digest reference"
    );
    eprintln!(
        "Pulling {} for {platform} with {engine}...",
        reference.escape_debug()
    );

    let mut command = Command::new(engine);
    command.args(["image", "pull", "--platform"]);
    if engine == "podman" {
        command.arg(format!("{}/{}", platform.os, platform.architecture));
        if let Some(variant) = &platform.variant {
            command.args(["--variant", variant]);
        }
    } else {
        command.arg(platform.to_string());
    }

    // Give the engine the actual stderr handle as stdout. A pipe disables its
    // terminal progress renderer; forwarding the bytes later cannot restore it.
    let status = command
        .args(["--", reference])
        .stdin(Stdio::inherit())
        .stdout(Stdio::from(io::stderr()))
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("cannot run {engine} image pull"))?;
    ensure!(
        status.success(),
        "{engine} image pull for {reference:?} failed ({status})"
    );
    Ok(())
}

fn save(
    engine: &str,
    saved_reference: &str,
    reference: &str,
    platform: &Platform,
) -> Result<Image> {
    let mut command = Command::new(engine);
    command.args(["image", "save"]);
    if engine == "podman" {
        command.args(["--format", "oci-archive"]);
    }

    // The complete archive is captured in RAM, without a destination or temp file.
    let output = command
        .args(["--", saved_reference])
        .output()
        .with_context(|| format!("cannot run {engine} image save"))?;
    ensure!(
        output.status.success(),
        "{engine} image save for {saved_reference:?} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );

    archive::read(&mut Cursor::new(output.stdout), reference, Some(platform))
        .with_context(|| format!("cannot read archive returned by {engine}"))
}

fn is_image_id(reference: &str) -> bool {
    let digest = reference.strip_prefix("sha256:").unwrap_or(reference);
    (12..=64).contains(&digest.len()) && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn engine_reference(reference: &str) -> String {
    let name = reference.rsplit('/').next().unwrap_or(reference);
    if reference.contains('@') || name.contains(':') || is_image_id(reference) {
        reference.into()
    } else {
        format!("{reference}:latest")
    }
}
