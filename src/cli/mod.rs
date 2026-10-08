mod commands;

use std::io;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Container image explorer (skeleton; operations are placeholders)",
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

    /// Continue despite image parsing errors
    #[arg(short = 'i', long)]
    pub ignore_errors: bool,
}

#[derive(Debug, Args)]
pub struct AnalyzeOptions {
    #[command(flatten)]
    pub image: ImageOptions,

    /// Export analysis to this JSON file
    #[arg(short = 'j', long, value_name = "PATH")]
    pub json: Option<PathBuf>,

    /// Minimum efficiency ratio, or disabled
    #[arg(long = "lowestEfficiency", value_name = "VALUE")]
    pub lowest_efficiency: Option<String>,

    /// Maximum wasted bytes with optional units, or disabled
    #[arg(long = "highestWastedBytes", value_name = "VALUE")]
    pub highest_wasted_bytes: Option<String>,

    /// Maximum waste/user-size ratio, or disabled
    #[arg(long = "highestUserWastedPercent", value_name = "VALUE")]
    pub highest_user_wasted_percent: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Analyze an image and evaluate supplied rules without opening the TUI (placeholder)
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

pub fn run() -> io::Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Analyze(options)) => commands::analyze(options),
        Some(Command::Version) => commands::version(),
        Some(Command::Completion { shell }) => commands::completion(*shell),
        None => commands::inspect(&cli.options),
    }
}
