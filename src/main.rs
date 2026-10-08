mod analysis;
mod cli;
mod export;
mod image;
mod tui;

fn main() -> std::io::Result<()> {
    cli::run()
}
