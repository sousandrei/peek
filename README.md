# Peek

Peek is a Rust CLI for exploring container images in a terminal and analyzing them without an interactive UI.

**Current status:** only command parsing, help, and version output work. Image loading, the TUI, analysis, rule evaluation, JSON export, and shell completion are placeholders that exit successfully without performing work.

## Usage

```sh
# Interactive explorer
peek nginx:latest
peek --source podman my-image:latest
peek --source docker-archive image.tar

# Headless analysis
peek analyze nginx:latest
peek a image.tar --source docker-archive

# Analysis rules and JSON output
peek analyze nginx:latest --lowestEfficiency 0.9
peek analyze nginx:latest --highestWastedBytes 10MB --highestUserWastedPercent 0.1
peek analyze nginx:latest --json analysis.json

# Help and version
peek --help
peek analyze --help
peek v
```

`a` is an alias for `analyze`; `v` is an alias for `version`. The default image invocation selects the TUI. Rule thresholds and `-j`/`--json` are available only with `analyze`. `--source` accepts `docker` (default), `podman`, or `docker-archive`. `-i`/`--ignore-errors` requests continued processing after image parsing errors.

All settings use CLI flags. Images must be created using an external container tool.

`peek completion bash` accepts `bash`, `fish`, `powershell`, or `zsh`, but does not generate scripts yet.

## Development

Install Rust and Cargo, then run these commands from the `peek/` directory:

```sh
cargo run -- --help
cargo run -- nginx:latest
cargo run -- analyze nginx:latest

cargo fmt --check
cargo check --locked
cargo clippy --locked -- -D warnings
```

## Production

Build a release binary from the `peek/` directory:

```sh
cargo build --release --locked
./target/release/peek --help
```

To install the executable into Cargo's binary directory:

```sh
cargo install --path . --locked
peek --help
```

Release builds currently have the same placeholder functionality described above. A successful exit does not indicate that an image was analyzed or any rules passed.
