# Peek

[![CI](https://github.com/sousandrei/peek/actions/workflows/ci.yml/badge.svg)](https://github.com/sousandrei/peek/actions/workflows/ci.yml)

Peek lets you look inside a container image, one layer at a time. It’s a Rust project inspired by [Dive](https://github.com/wagoodman/dive).

## What it does

- Opens an image in a terminal UI where you can browse layers, their commands, and the files they leave behind.
- Marks files that were added, changed, or deleted. Select a file to see its metadata and which layer it came from.
- Lets you filter paths, fold directories, compare against the image base, and sort by name or size.
- Prints layer changes in the terminal or writes them as JSON. Use `--layers full` to export the filesystem after each layer.
- Works with Docker and Podman images, including platform selection. Peek asks the selected engine to pull an image if it isn’t available locally.
- Reads saved Docker image archives too, so you can inspect them without running an engine.

Peek looks at images you already build with Docker, Podman, or another tool. It doesn’t build images itself.

## Get started

Install Rust and Cargo, then install Peek from GitHub:

```sh
cargo install --git https://github.com/sousandrei/peek.git --locked
```

Open an image in the interactive UI:

```sh
peek ubuntu:latest
```

Use the arrow keys or mouse to move around. Tab switches between layers and files; Ctrl+Down or a click focuses the command pane. Press `?` for the shortcuts or `q` to quit.

To print or export layer changes, run:

```sh
peek analyze ubuntu:latest
peek analyze ubuntu:latest --json
peek analyze ubuntu:latest --json --output changes.json
```

You can pick an engine or platform, or export the complete filesystem after every layer:

```sh
peek --source podman ubuntu:latest
peek --platform linux/arm64 ubuntu:latest
peek analyze ubuntu:latest --json --layers full --output filesystem.json
```

Docker or Podman is needed for images from an engine. A saved Docker archive works on its own. Run `peek --help` or `peek analyze --help` to see all the options.

## Development

From the repository root, run the checks with:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked --all-targets
```

`cargo test` requires Docker and Buildx. Each test process creates a uniquely named temporary `docker-container` builder, reuses it for all image fixtures, then removes its container and cache volume on success or ordinary test failure. Concurrent test runs use separate builders. A forced process termination can bypass cleanup.

Thanks to [Alex Goodman](https://github.com/wagoodman) for making Dive and inspiring Peek.
