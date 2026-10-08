//! Container platform selection shared by the CLI and archive loader.

use std::{fmt, str::FromStr};

use anyhow::{Result, ensure};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Platform {
    pub os: String,
    pub architecture: String,
    #[serde(default)]
    pub variant: Option<String>,
}

impl Platform {
    pub fn host() -> Self {
        let architecture = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            "x86" => "386",
            other => other,
        };
        let os = match std::env::consts::OS {
            "macos" => "linux",
            other => other,
        };

        Self {
            os: os.into(),
            architecture: architecture.into(),
            variant: None,
        }
    }

    pub fn matches(&self, other: &Self) -> bool {
        self.os == other.os
            && self.architecture == other.architecture
            && self.variant.as_ref().is_none_or(|variant| {
                other.variant.as_ref() == Some(variant)
                    || (self.architecture == "arm64" && variant == "v8" && other.variant.is_none())
            })
    }
}

impl FromStr for Platform {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let parts: Vec<_> = value.split('/').collect();
        ensure!(
            (2..=3).contains(&parts.len())
                && parts.iter().all(|part| !part.is_empty()
                    && part.bytes().all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-' | b'.'))),
            "platform must be OS/ARCH or OS/ARCH/VARIANT (for example linux/arm64)"
        );

        Ok(Self {
            os: parts[0].into(),
            architecture: parts[1].into(),
            variant: parts.get(2).map(|variant| (*variant).into()),
        })
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.os, self.architecture)?;
        if let Some(variant) = &self.variant {
            write!(formatter, "/{variant}")?;
        }
        Ok(())
    }
}
