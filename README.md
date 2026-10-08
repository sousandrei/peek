# Peek

Peek inspects container image layers and shows which files were added, modified, or deleted. View the results in the terminal or export them as JSON.

Interactive exploration is not available yet.

## Install

With Rust and Cargo installed, run from the project directory:

```sh
cargo install --path . --locked
```

## Usage

Docker must be running. Use an image and tag already available locally; these examples use Ubuntu and Redis. Untagged names select `:latest`.

```sh
docker image ls
peek analyze ubuntu:latest
peek analyze redis:latest
```

For multi-platform images, Peek selects the host platform. The requested variant must already be available locally; Peek does not pull images. Select another locally available platform with `--platform`:

```sh
peek analyze ubuntu:latest --platform linux/arm64
```

Terminal output groups file changes by layer and formats image commands for readability. Color is enabled automatically where supported; set `NO_COLOR=1` to disable it.

### JSON export

```sh
# Print JSON to stdout
peek analyze ubuntu:latest --json

# Save JSON to a new file
peek analyze ubuntu:latest --json -o changes.json

# Export the complete filesystem listing after each layer
peek analyze ubuntu:latest --json --layers full -o files.json
```

JSON shows per-layer changes by default. `-o`/`--output` selects a file destination; existing files are not overwritten.

Run `peek --help` or `peek analyze --help` for options. `peek a` aliases `peek analyze`, and `peek v` prints the version.

## Development

With Rust, Cargo, and Docker available, run from the project directory:

```sh
cargo run -- analyze ubuntu:latest
cargo fmt --check
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
```

Build and run an optimized executable:

```sh
cargo build --release --locked
./target/release/peek analyze ubuntu:latest
```
