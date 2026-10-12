mod cli;
mod colors;
mod export;
mod oci;
mod tui;
mod updater;

fn main() -> anyhow::Result<()> {
    cli::run()
}

#[cfg(test)]
mod tests;
