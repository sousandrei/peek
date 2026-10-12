use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use self_update::{Download, backends::github::ReleaseList};
use sha2::{Digest, Sha256};

const REPOSITORY_OWNER: &str = "sousandrei";
const REPOSITORY_NAME: &str = "peek";

pub fn check_and_update() -> Result<Option<String>> {
    if cfg!(debug_assertions) {
        return Ok(None);
    }

    let Some(asset_name) = asset_name_for_host(std::env::consts::OS, std::env::consts::ARCH) else {
        return Ok(None);
    };

    let releases = ReleaseList::configure()
        .repo_owner(REPOSITORY_OWNER)
        .repo_name(REPOSITORY_NAME)
        .timeout(Duration::from_secs(5))
        .build()?
        .fetch()?;
    let Some(release) = releases.latest() else {
        return Ok(None);
    };

    if !self_update::version::bump_is_greater(env!("CARGO_PKG_VERSION"), release.version())? {
        return Ok(None);
    }

    let asset = release
        .assets()
        .iter()
        .find(|asset| asset.name() == asset_name)
        .with_context(|| {
            format!(
                "release {} has no asset named {asset_name}",
                release.version()
            )
        })?;

    replace_from_asset(asset.download_url(), asset.digest(), asset_name)?;

    Ok(Some(release.version().to_owned()))
}

fn replace_from_asset(url: &str, digest: Option<&str>, asset_name: &str) -> Result<()> {
    let executable =
        std::env::current_exe().context("cannot locate the running Peek executable")?;
    let executable_dir = executable
        .parent()
        .context("running executable has no parent directory")?;
    let staging_dir = tempfile::Builder::new()
        .prefix("peek-update-")
        .tempdir_in(executable_dir)
        .context("cannot create an update staging directory next to the executable")?;
    let staged_executable = staging_dir.path().join(asset_name);
    let mut file =
        File::create(&staged_executable).context("cannot create staged update binary")?;

    Download::from_url(url)
        .timeout(Duration::from_secs(60))
        .download_to(&mut file)
        .context("cannot download the Peek update")?;
    file.flush()
        .context("cannot finish writing the update binary")?;
    verify_digest(&staged_executable, digest)?;
    set_executable_permissions(&staged_executable)?;

    self_replace::self_replace(&staged_executable)
        .context("cannot replace the running Peek executable")?;
    Ok(())
}

fn verify_digest(path: &Path, expected: Option<&str>) -> Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };

    let (algorithm, expected_hex) = expected
        .split_once(':')
        .context("release asset has an invalid digest")?;
    if algorithm != "sha256" {
        bail!("release asset uses unsupported digest algorithm {algorithm}");
    }

    let mut file = File::open(path).context("cannot read downloaded update for verification")?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 16 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .context("cannot hash downloaded update")?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    let actual = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual != expected_hex {
        bail!("downloaded update failed its SHA-256 digest check");
    }

    Ok(())
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .context("cannot make the downloaded update executable")
}

#[cfg(not(unix))]
fn set_executable_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn asset_name_for_host(os: &str, architecture: &str) -> Option<&'static str> {
    match (os, architecture) {
        ("linux", "x86_64") => Some("peek-linux-x86_64"),
        ("linux", "aarch64") => Some("peek-linux-aarch64"),
        ("macos", "aarch64") => Some("peek-macos-aarch64"),
        ("macos", "x86_64") => Some("peek-macos-x86_64"),
        ("windows", "x86_64") => Some("peek-windows-x86_64.exe"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{asset_name_for_host, verify_digest};

    #[test]
    fn maps_supported_hosts_to_release_assets() {
        assert_eq!(
            asset_name_for_host("linux", "x86_64"),
            Some("peek-linux-x86_64")
        );
        assert_eq!(
            asset_name_for_host("linux", "aarch64"),
            Some("peek-linux-aarch64")
        );
        assert_eq!(
            asset_name_for_host("macos", "aarch64"),
            Some("peek-macos-aarch64")
        );
        assert_eq!(
            asset_name_for_host("macos", "x86_64"),
            Some("peek-macos-x86_64")
        );
        assert_eq!(
            asset_name_for_host("windows", "x86_64"),
            Some("peek-windows-x86_64.exe")
        );
    }

    #[test]
    fn leaves_unsupported_hosts_unchanged() {
        assert_eq!(asset_name_for_host("windows", "aarch64"), None);
        assert_eq!(asset_name_for_host("linux", "x86"), None);
    }

    #[test]
    fn verifies_sha256_release_digest() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("peek");
        fs::write(&binary, b"abc").unwrap();

        assert!(
            verify_digest(
                &binary,
                Some("sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
            )
            .is_ok()
        );
        assert!(verify_digest(&binary, Some("sha256:00")).is_err());
    }
}
