#!/usr/bin/env bash
# Run the R-10 scratch suite inside WSL (or any Linux). Temporary files go to an ext4 directory
# under $HOME, never to /tmp (tmpfs: fsync is a no-op) and never to /mnt/c (9p).
set -euo pipefail
export PATH="$HOME/bin:$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/domovoi-target-r10}"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0   # the WSL root disk is small
export TMPDIR="$HOME/r10-tmp"
mkdir -p "$TMPDIR"
trap 'rm -rf "$TMPDIR"' EXIT
cd "$(dirname "$0")/.."
echo "uname -r: $(uname -r)"; rustc --version; cargo --version
echo "scratch fs: $(stat -f -c %T "$TMPDIR")  source fs: $(stat -f -c %T .)"
cargo test --offline "$@"
