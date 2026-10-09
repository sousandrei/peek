//! Load local engine images, pulling missing images or platform variants once.

use std::io::{self, Cursor, Read};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};

use super::{LoadProgress, Platform, archive, model::Image, progress};

pub(super) fn load(
    engine: &str,
    reference: &str,
    platform: Option<&Platform>,
    progress: Option<&LoadProgress>,
) -> Result<Image> {
    progress::report(progress, "Checking local image")?;
    let saved_reference = engine_reference(reference);
    let target = platform.cloned().unwrap_or_else(Platform::host);
    let mut pulled = false;

    if !image_exists(engine, &saved_reference, progress)? {
        pull(engine, &saved_reference, &target, progress)?;
        pulled = true;
    }

    match save(engine, &saved_reference, reference, &target, progress) {
        Err(error) if !pulled && error.is::<archive::MissingPlatform>() => {
            pull(engine, &saved_reference, &target, progress)?;
            save(engine, &saved_reference, reference, &target, progress)
        }
        result => result,
    }
}

fn image_exists(engine: &str, reference: &str, progress: Option<&LoadProgress>) -> Result<bool> {
    let operation = if engine == "podman" {
        "exists"
    } else {
        "inspect"
    };
    let mut command = Command::new(engine);
    command.args(["image", operation, "--", reference]);
    let output = command_output(&mut command, progress)
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

fn pull(
    engine: &str,
    reference: &str,
    platform: &Platform,
    progress: Option<&LoadProgress>,
) -> Result<()> {
    progress::report(progress, "Pulling image")?;
    ensure!(
        !is_image_id(reference),
        "local image ID {reference:?} cannot be pulled; use a registry image name or digest reference"
    );
    if progress.is_none() {
        eprintln!(
            "Pulling {} for {platform} with {engine}...",
            reference.escape_debug()
        );
    }

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

    command.args(["--", reference]);
    if progress.is_some() {
        let output = command_output(&mut command, progress)
            .with_context(|| format!("cannot run {engine} image pull"))?;
        ensure!(
            output.status.success(),
            "{engine} image pull for {reference:?} failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    } else {
        // Keep native terminal progress for headless analysis.
        let status = command
            .stdin(Stdio::inherit())
            .stdout(Stdio::from(io::stderr()))
            .stderr(Stdio::inherit())
            .status()
            .with_context(|| format!("cannot run {engine} image pull"))?;
        ensure!(
            status.success(),
            "{engine} image pull for {reference:?} failed ({status})"
        );
    }
    Ok(())
}

fn save(
    engine: &str,
    saved_reference: &str,
    reference: &str,
    platform: &Platform,
    progress: Option<&LoadProgress>,
) -> Result<Image> {
    progress::report(progress, "Exporting image archive")?;
    let mut command = Command::new(engine);
    command.args(["image", "save"]);
    if engine == "podman" {
        command.args(["--format", "oci-archive"]);
    }

    // The complete archive is captured in RAM, without a destination or temp file.
    command.args(["--", saved_reference]);
    let output = command_output(&mut command, progress)
        .with_context(|| format!("cannot run {engine} image save"))?;
    ensure!(
        output.status.success(),
        "{engine} image save for {saved_reference:?} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );

    progress::report(progress, "Decoding image layers")?;
    archive::read(&mut Cursor::new(output.stdout), reference, Some(platform))
        .with_context(|| format!("cannot read archive returned by {engine}"))
}

// Drain both pipes concurrently so large exports cannot block the child. Poll
// cancellation while it runs, then always reap it before releasing the pipes.
fn command_output(command: &mut Command, progress: Option<&LoadProgress>) -> Result<Output> {
    let Some(progress) = progress else {
        return command
            .output()
            .context("cannot execute image engine command");
    };
    ensure!(!progress.is_cancelled(), "image loading cancelled");
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot start image engine command")?;
    let stdout = child.stdout.take().context("missing engine stdout pipe")?;
    let stderr = child.stderr.take().context("missing engine stderr pipe")?;

    thread::scope(|scope| {
        let stdout = scope.spawn(|| read_pipe(stdout));
        let stderr = scope.spawn(|| read_pipe(stderr));
        let status = loop {
            if progress.is_cancelled() {
                let _ = child.kill();
                break child.wait().context("cannot reap cancelled engine command");
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => thread::sleep(Duration::from_millis(25)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(error).context("cannot wait for image engine command");
                }
            }
        };
        let stdout = stdout
            .join()
            .map_err(|_| anyhow::anyhow!("engine stdout reader panicked"))??;
        let stderr = stderr
            .join()
            .map_err(|_| anyhow::anyhow!("engine stderr reader panicked"))??;
        ensure!(!progress.is_cancelled(), "image loading cancelled");
        Ok(Output {
            status: status?,
            stdout,
            stderr,
        })
    })
}

fn read_pipe(mut pipe: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes)?;
    Ok(bytes)
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    #[test]
    fn interactive_commands_capture_both_pipes_and_failure_status() {
        let (sender, _) = mpsc::channel();
        let progress = LoadProgress::new(sender, Arc::new(AtomicBool::new(false)));
        let mut command = Command::new("sh");
        command.args(["-c", "printf archive; printf failure >&2; exit 7"]);
        let output = command_output(&mut command, Some(&progress)).unwrap();
        assert_eq!(output.stdout, b"archive");
        assert_eq!(output.stderr, b"failure");
        assert_eq!(output.status.code(), Some(7));
    }

    #[test]
    fn cancelling_an_engine_command_kills_and_reaps_it() {
        let (sender, _) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let progress = LoadProgress::new(sender, Arc::clone(&cancelled));
        thread::scope(|scope| {
            scope.spawn(|| {
                thread::sleep(Duration::from_millis(50));
                cancelled.store(true, Ordering::Relaxed);
            });
            let mut command = Command::new("sleep");
            command.arg("10");
            let started = std::time::Instant::now();
            let error = command_output(&mut command, Some(&progress)).unwrap_err();
            assert!(error.to_string().contains("cancelled"));
            assert!(started.elapsed() < Duration::from_secs(5));
        });
    }
}
