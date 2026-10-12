use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use self_update::{Download, backends::github::ReleaseList, update::Releases};
use sha2::{Digest, Sha256};

const REPOSITORY_OWNER: &str = "sousandrei";
const REPOSITORY_NAME: &str = "peek";

pub struct AvailableUpdate {
    pub version: String,
    url: String,
    digest: String,
    asset_name: &'static str,
}

pub fn check_for_update() -> Result<Option<AvailableUpdate>> {
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

    select_update(&releases, asset_name)
}

fn select_update(releases: &Releases, asset_name: &'static str) -> Result<Option<AvailableUpdate>> {
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
    let digest = asset
        .digest()
        .context("release asset has no SHA-256 digest")?;
    sha256_digest(digest)?;

    Ok(Some(AvailableUpdate {
        version: release.version().to_owned(),
        url: asset.download_url().to_owned(),
        digest: digest.to_owned(),
        asset_name,
    }))
}

pub fn install(update: &AvailableUpdate) -> Result<()> {
    replace_from_asset(&update.url, &update.digest, update.asset_name)
}

fn replace_from_asset(url: &str, digest: &str, asset_name: &str) -> Result<()> {
    sha256_digest(digest)?;

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

fn sha256_digest(expected: &str) -> Result<&str> {
    let (algorithm, expected_hex) = expected
        .split_once(':')
        .context("release asset has an invalid digest")?;
    if algorithm != "sha256" {
        bail!("release asset uses unsupported digest algorithm {algorithm}");
    }
    ensure!(
        expected_hex.len() == 64 && expected_hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "release asset has an invalid SHA-256 digest; expected 64 hexadecimal digits"
    );
    Ok(expected_hex)
}

fn verify_digest(path: &Path, expected: &str) -> Result<()> {
    let expected_hex = sha256_digest(expected)?;

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
    if !actual.eq_ignore_ascii_case(expected_hex) {
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

    use super::*;
    use self_update::update::{Release, ReleaseAsset};

    const ASSET: &str = "peek-linux-x86_64";
    const SHA256: &str = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn releases(version: &str, assets: Vec<ReleaseAsset>) -> Releases {
        let release = Release::builder()
            .version(version)
            .assets(assets)
            .build()
            .unwrap();
        Releases::from_releases(vec![release], env!("CARGO_PKG_VERSION"))
    }

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

        assert!(verify_digest(&binary, SHA256).is_ok());
        assert!(
            verify_digest(
                &binary,
                &format!("sha256:{}", SHA256[7..].to_ascii_uppercase())
            )
            .is_ok()
        );
        let wrong = format!("sha256:{}", "0".repeat(64));
        assert!(
            verify_digest(&binary, &wrong)
                .unwrap_err()
                .to_string()
                .contains("failed its SHA-256 digest check")
        );
    }

    #[test]
    fn no_release_or_no_newer_version_has_no_update() {
        let empty = Releases::from_releases(Vec::new(), env!("CARGO_PKG_VERSION"));
        assert!(select_update(&empty, ASSET).unwrap().is_none());
        for version in ["0.0.0", env!("CARGO_PKG_VERSION")] {
            assert!(
                select_update(&releases(version, Vec::new()), ASSET)
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn newer_release_selects_the_exact_host_asset_with_a_verified_digest() {
        let url = "https://example.invalid/peek";
        let assets = vec![
            ReleaseAsset::new("peek-linux-aarch64", "https://example.invalid/arm64")
                .with_digest(SHA256),
            ReleaseAsset::new(ASSET, url).with_digest(SHA256),
        ];
        let update = select_update(&releases("9999.0.0", assets), ASSET)
            .unwrap()
            .unwrap();
        assert_eq!(update.version, "9999.0.0");
        assert_eq!(update.url, url);
        assert_eq!(update.asset_name, ASSET);
        assert_eq!(update.digest, SHA256);
    }

    #[test]
    fn missing_host_asset_or_missing_digest_returns_a_clear_error() {
        let error = select_update(&releases("9999.0.0", Vec::new()), ASSET)
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("has no asset named peek-linux-x86_64")
        );
        let asset = ReleaseAsset::new(ASSET, "https://example.invalid/peek");
        let error = select_update(&releases("9999.0.0", vec![asset]), ASSET)
            .err()
            .unwrap();
        assert!(error.to_string().contains("has no SHA-256 digest"));
    }

    #[test]
    fn malformed_or_unsupported_digests_are_rejected_before_download() {
        let too_long = format!("sha256:{}", "0".repeat(65));
        let non_hex = format!("sha256:{}", "g".repeat(64));
        let unsupported = format!("sha512:{}", "0".repeat(64));
        for digest in [
            "",
            "sha256:",
            "sha256:00",
            &too_long,
            &non_hex,
            &unsupported,
        ] {
            let asset =
                ReleaseAsset::new(ASSET, "https://example.invalid/peek").with_digest(digest);
            assert!(
                select_update(&releases("9999.0.0", vec![asset]), ASSET).is_err(),
                "{digest}"
            );
            let error =
                replace_from_asset("https://example.invalid/peek", digest, ASSET).unwrap_err();
            assert!(error.to_string().contains("digest"), "{error}");
            assert!(!error.to_string().contains("download"), "{error}");
        }
    }
}
