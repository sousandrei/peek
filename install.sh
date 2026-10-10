#!/usr/bin/env bash

set -euo pipefail

case "$(uname -s)/$(uname -m)" in
  Linux/x86_64)
    asset="peek-linux-x86_64"
    ;;
  Linux/aarch64|Linux/arm64)
    asset="peek-linux-aarch64"
    ;;
  Darwin/arm64|Darwin/aarch64)
    asset="peek-macos-aarch64"
    ;;
  Darwin/x86_64)
    asset="peek-macos-x86_64"
    ;;
  *)
    printf 'Unsupported platform: %s/%s\n' "$(uname -s)" "$(uname -m)" >&2
    exit 1
    ;;
esac

install_dir="${PEEK_INSTALL_DIR:-$HOME/.local/bin}"
download_url="https://github.com/sousandrei/peek/releases/latest/download/$asset"
temporary_binary="$(mktemp)"
trap 'rm -f "$temporary_binary"' EXIT

curl --fail --location --silent --show-error "$download_url" --output "$temporary_binary"
mkdir -p "$install_dir"
install -m 0755 "$temporary_binary" "$install_dir/peek"

printf 'Installed peek to %s/peek\n' "$install_dir"
case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) printf 'Add %s to your PATH to run peek.\n' "$install_dir" ;;
esac
