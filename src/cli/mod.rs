mod commands;
pub(crate) mod history;
mod output;

use anyhow::Result;
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Container image filesystem explorer and analyzer",
    version = env!("CARGO_PKG_VERSION"),
    help_template = "{before-help}{name} {version}\n{about-with-newline}{usage-heading} {usage}\n\n{all-args}{after-help}",
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

    /// Print version number
    #[arg(short = 'V', long, action = clap::ArgAction::SetTrue)]
    pub version: bool,
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
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();

    if arguments.is_empty() {
        print_usage_hint();
        return Ok(());
    }

    if arguments
        .iter()
        .any(|argument| argument == "--version" || argument == "-V")
    {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let cli = Cli::parse();

    if cli.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    run_command(
        &cli,
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        crate::updater::check_for_update,
    )
}

fn run_command(
    cli: &Cli,
    interactive: bool,
    check_update: impl FnOnce() -> Result<Option<crate::updater::AvailableUpdate>>,
) -> Result<()> {
    match &cli.command {
        Some(Command::Analyze(options)) => {
            commands::image_input(&options.image)?;
        }
        None => {
            commands::image_input(&cli.options)?;
        }
        Some(Command::Completion { .. }) => {}
    }

    if automatic_update_allowed(cli, interactive) && offer_update(check_update) {
        return Ok(());
    }

    match &cli.command {
        Some(Command::Analyze(options)) => commands::analyze(options),
        Some(Command::Completion { shell }) => commands::completion(*shell),
        None => commands::inspect(&cli.options),
    }
}

fn automatic_update_allowed(cli: &Cli, interactive: bool) -> bool {
    cli.command.is_none() && interactive
}

// Optional update failures are diagnostics; false continues the requested operation.
fn offer_update(
    check_update: impl FnOnce() -> Result<Option<crate::updater::AvailableUpdate>>,
) -> bool {
    match check_update() {
        Ok(Some(update)) => {
            let confirmed = match confirm_update(&update.version) {
                Ok(confirmed) => confirmed,
                Err(error) => {
                    eprintln!("peek: cannot read update confirmation: {error:#}");
                    false
                }
            };

            if confirmed {
                match crate::updater::install(&update) {
                    Ok(()) => {
                        eprintln!(
                            "Updated Peek to {}. Run the command again to start the new version.",
                            update.version
                        );
                        return true;
                    }
                    Err(error) => eprintln!("peek: update failed: {error:#}"),
                }
            }
        }
        Ok(None) => {}
        Err(error) => eprintln!("peek: update check failed: {error:#}"),
    }
    false
}

fn print_usage_hint() {
    println!(
        "Pass an image name to open the explorer, or use `peek analyze IMAGE` to print layer changes. Run `peek --help` for all options."
    );
}

fn confirm_update(version: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        eprintln!("peek: version {version} is available; run Peek in a terminal to update.");
        return Ok(false);
    }

    eprint!("Peek {version} is available. Install it now? [y/N] ");
    io::stderr().flush()?;

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;

    Ok(accepts_update(&answer))
}

fn accepts_update(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_prompt_only_accepts_explicit_yes() {
        assert!(accepts_update("y"));
        assert!(accepts_update(" YES \n"));
        assert!(!accepts_update(""));
        assert!(!accepts_update("n"));
        assert!(!accepts_update("no"));
    }

    #[test]
    fn automatic_updates_are_limited_to_interactive_inspection() {
        let inspect = Cli::try_parse_from(["peek", "fixture.tar"]).unwrap();
        assert!(automatic_update_allowed(&inspect, true));
        assert!(!automatic_update_allowed(&inspect, false));
        for arguments in [
            vec!["peek", "analyze", "fixture.tar"],
            vec!["peek", "analyze", "fixture.tar", "--json"],
            vec!["peek", "completion", "bash"],
        ] {
            let cli = Cli::try_parse_from(arguments).unwrap();
            assert!(!automatic_update_allowed(&cli, true));
            assert!(!automatic_update_allowed(&cli, false));
        }
    }

    #[test]
    fn headless_dispatch_never_invokes_the_update_lookup_even_with_a_terminal() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing.tar");
        for json in [false, true] {
            let mut arguments = vec![
                "peek",
                "analyze",
                "--source",
                "docker-archive",
                missing.to_str().unwrap(),
            ];
            if json {
                arguments.push("--json");
            }
            let cli = Cli::try_parse_from(arguments).unwrap();
            let error = run_command(&cli, true, || panic!("analysis must not check for updates"))
                .unwrap_err();
            assert!(format!("{error:#}").contains("cannot open image archive"));
        }
        let cli = Cli::try_parse_from(["peek", "completion", "bash"]).unwrap();
        run_command(&cli, true, || {
            panic!("completion must not check for updates")
        })
        .unwrap();
    }

    #[test]
    fn unsupported_options_fail_before_an_interactive_update_lookup() {
        let cli = Cli::try_parse_from(["peek", "fixture.tar", "--ignore-errors"]).unwrap();
        let error = run_command(&cli, true, || {
            panic!("invalid options must be rejected first")
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("best-effort archive parsing is not implemented")
        );
    }

    #[test]
    fn no_update_or_lookup_failure_continues_the_requested_operation() {
        assert!(!offer_update(|| Ok(None)));
        assert!(!offer_update(|| Err(anyhow::anyhow!(
            "network unavailable"
        ))));
    }
}
