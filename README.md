# Peek

Peek analyzes existing Docker and OCI image archives using a layered filesystem model held in RAM. It reports incremental file changes or exports cumulative filesystem views as JSON.

**Current status:** archive loading, layer diffs, JSON export, help, and version work. Terminal rendering, efficiency metrics, rule evaluation, and completion generation are pending. The default image invocation loads the image but does not render an explorer yet. Docker and Podman adapters are present; live engine integration has not been verified.

## Usage

Docker must be installed and running. These examples use `ubuntu:latest` and `redis:latest`; use whichever image is already available locally. Run `docker image ls` to see your images.

```sh
docker image ls

# Analyze an image already present in Docker
peek analyze ubuntu:latest
peek a redis:latest

# Export incremental changes (default) or cumulative files at each layer
peek analyze ubuntu:latest --json changes.json
peek analyze ubuntu:latest --json files.json --layers full

# Help and version
peek --help
peek analyze --help
peek v
peek -v
```

`a` aliases `analyze`; `v` aliases `version`. `--source` accepts `docker` (default), `podman`, or `docker-archive`. Source URIs accept `docker://`, `podman://`, `docker-archive://`, and `docker-tar://`.

`--json PATH` writes a new file and refuses to overwrite an existing one. Its parent directory must exist. `--layers diff|full` is available only with JSON export; `diff` is the default. Each diff compares a layer against the filesystem immediately below it, with layer zero compared against an empty filesystem. Full exports contain the resulting visible files after each layer. The versioned schema includes metadata, content hashes, and layer provenance, without file payloads.

Archives are read directly, compressed layer payloads are streamed, and filesystem metadata stays in RAM. No filesystem extraction or temporary image files are created. Engine exports are captured in RAM, so large images require enough memory for the saved archive as well as metadata. Only single-image archives and SHA-256 OCI descriptors are supported currently.

All settings use CLI flags. Build images with an external container tool before using Peek. `--ignore-errors` and analysis threshold flags currently return an explicit unsupported-operation error. `peek completion SHELL` remains a placeholder.

Successful analysis exits with status `0`; loading/export errors exit with `1`; invalid arguments exit with `2`.

## Development

Install Rust and Cargo. With Docker running and `ubuntu:latest` available locally, run from the Peek project directory:

```sh
cargo run -- --help
cargo run -- analyze ubuntu:latest
cargo run -- analyze ubuntu:latest --json dev-changes.json
cargo run -- analyze ubuntu:latest --json dev-files.json --layers full

cargo fmt --check
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
```

For a manual check, confirm analysis reports the selected image's layers, the default JSON contains `diff` views, and `--layers full` contains `full` views. Use new output filenames for each run.

## Production

Build or install from the Peek project directory. The example uses a locally available `ubuntu:latest` image:

```sh
cargo build --release --locked
./target/release/peek analyze ubuntu:latest --json release-changes.json

cargo install --path . --locked
peek --help
```

Release builds have the same functionality and limitations described above.
