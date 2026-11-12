#!/usr/bin/env bash
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "${1:-}" == "--install-system" ]]; then
  if [[ "$(id -u)" == 0 ]]; then
    apt-get update
    apt-get install -y build-essential pkg-config curl ca-certificates python3 libwayland-dev libxkbcommon-dev libdbus-1-dev xdg-utils dbus-user-session xdg-desktop-portal xdg-desktop-portal-kde
  else
    sudo apt-get update
    sudo apt-get install -y build-essential pkg-config curl ca-certificates python3 libwayland-dev libxkbcommon-dev libdbus-1-dev xdg-utils dbus-user-session xdg-desktop-portal xdg-desktop-portal-kde
  fi
elif [[ $# -gt 0 ]]; then
  printf '%s\n' 'Usage: bootstrap_linux.sh [--install-system]' >&2
  exit 2
fi
if ! command -v rustup >/dev/null 2>&1; then
  installer_file="$(mktemp)"
  trap 'rm -f -- "$installer_file"' EXIT
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$installer_file"
  sh "$installer_file" -y --profile minimal --default-toolchain none
  export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
fi
cd "$repo_dir"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
toolchain="$(python3 -c 'import pathlib,re; print(re.search(r"channel\s*=\s*\"([^\"]+)\"", pathlib.Path("rust-toolchain.toml").read_text()).group(1))')"
rustup toolchain install "$toolchain" --profile minimal --component rustfmt --component clippy
cargo build --locked -p ui4me-host
printf '%s\n' 'Build complete. Run python3 tools/run_example.py in your KDE Wayland desktop.'
