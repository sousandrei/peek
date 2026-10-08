# Peek

Peek shows file additions, modifications, and deletions between container image layers. Read the results in your terminal or export them as JSON.

## Install

With Rust and Cargo installed, run from this directory:

```sh
cargo install --path . --locked
```

## Usage

Use Docker or Podman. Peek pulls missing images and platform variants into the selected engine’s image store. Untagged image names use `:latest`.

```sh
peek analyze ubuntu:latest
peek analyze redis:latest --source podman
peek analyze ubuntu:latest --platform linux/arm64
```

### JSON output

```sh
# Write layer changes to stdout
peek analyze ubuntu:latest --json

# Save layer changes to a new file
peek analyze ubuntu:latest --json -o changes.json

# Save the complete filesystem listing after each layer
peek analyze ubuntu:latest --json --layers full -o files.json
```

Existing output files are not overwritten. Set `NO_COLOR=1` to disable terminal colors.

Use `peek analyze --help` for options. `peek a` is shorthand for `peek analyze`; `peek v` prints the version.

## Contributing

From this directory:

```sh
cargo run -- analyze ubuntu:latest
cargo fmt --check
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
```
