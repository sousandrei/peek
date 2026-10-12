mod cli;
mod colors;
mod export;
mod oci;
mod tui;

fn main() -> anyhow::Result<()> {
    cli::run()
}

#[cfg(test)]
mod tests;
