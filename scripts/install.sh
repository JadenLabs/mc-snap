#!/usr/bin/env bash
set -euo pipefail

PROFILE="release"
TARGET="user"
PREFIX="/usr/local/bin"

usage() {
    cat <<EOF
Usage: $0 [--debug] [--system] [--prefix DIR]

  --debug          build the debug profile (faster iteration, slower runtime)
  --system         install to \$PREFIX instead of ~/.cargo/bin (uses sudo)
  --prefix DIR     install dir for --system (default: /usr/local/bin)
  -h, --help       show this help

Default: cargo install --path crates/mc-snap-cli (release into ~/.cargo/bin).
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug) PROFILE="debug"; shift ;;
        --system) TARGET="system"; shift ;;
        --prefix) PREFIX="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown arg: $1" >&2; usage; exit 1 ;;
    esac
done

cd "$(dirname "$0")/.."

if [[ "$TARGET" == "user" ]]; then
    if [[ "$PROFILE" == "debug" ]]; then
        cargo install --debug --path crates/mc-snap-cli --force
    else
        cargo install --path crates/mc-snap-cli --force
    fi
    DEST="${CARGO_HOME:-$HOME/.cargo}/bin/mc-snap"
else
    if [[ "$PROFILE" == "debug" ]]; then
        cargo build
        SRC="target/debug/mc-snap"
    else
        cargo build --release
        SRC="target/release/mc-snap"
    fi
    sudo install -m 755 "$SRC" "$PREFIX/mc-snap"
    DEST="$PREFIX/mc-snap"
fi

echo
echo "installed: $DEST"
"$DEST" --version