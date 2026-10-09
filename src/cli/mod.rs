mod commands;
pub(crate) mod history;
mod output;

use anyhow::Result;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Container image filesystem explorer and analyzer",
    color = match output::color_choice() {
        anstream::ColorChoice::Never => clap::ColorChoice::Never,
        _ => clap::ColorChoice::Auto,
    },
    disable_version_flag = true,
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
pub struct Cli {
    #[command(flatten)]
    pub options: ImageOptions,

    #[command(subcommand)]
    pub command: Option<Command>,

    /// Print version
    #[arg(short = 'v', long, action = clap::ArgAction::Version)]
    pub version: Option<bool>,
}

#[derive(Debug, Args)]
pub struct ImageOptions {
    /// Image tag, ID, digest, source URI, or archive path
    #[arg(required = true, value_name = "IMAGE")]
    pub image: Option<String>,

    /// Image provider
    #[arg(long, value_enum, default_value = "docker")]
    pub source: Source,

    /// Container platform; multi-platform images default to the host platform
    #[arg(long, value_name = "OS/ARCH[/VARIANT]")]
    pub platform: Option<crate::oci::Platform>,

    /// Continue despite image parsing errors
    #[arg(short = 'i', long)]
    pub ignore_errors: bool,
}

#[derive(Debug, Args)]
pub struct AnalyzeOptions {
    #[command(flatten)]
    pub image: ImageOptions,

    /// Format layer changes as JSON (stdout unless --output is supplied)
    #[arg(short = 'j', long)]
    pub json: bool,

    /// Write JSON to a new file instead of stdout
    #[arg(short = 'o', long, value_name = "PATH", requires = "json")]
    pub output: Option<PathBuf>,

    /// JSON layer view: incremental changes or cumulative filesystem
    #[arg(long, value_enum, default_value = "diff", requires = "json")]
    pub layers: LayerFormat,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show file changes between image layers without opening the TUI
    #[command(visible_alias = "a")]
    Analyze(AnalyzeOptions),
    /// Print the application version
    #[command(visible_alias = "v")]
    Version,
    /// Generate shell completion scripts (placeholder)
    Completion {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LayerFormat {
    Diff,
    Full,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Source {
    Docker,
    DockerArchive,
    Podman,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Shell {
    Bash,
    Fish,
    Powershell,
    Zsh,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Analyze(options)) => commands::analyze(options),
        Some(Command::Version) => commands::version(),
        Some(Command::Completion { shell }) => commands::completion(*shell),
        None => commands::inspect(&cli.options),
    }
}
