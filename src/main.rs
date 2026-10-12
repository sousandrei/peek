mod cli;
mod colors;
mod export;
mod oci;
mod presentation;
mod tui;
mod updater;

fn main() -> anyhow::Result<()> {
    cli::run()
}

#[cfg(test)]
mod tests;
