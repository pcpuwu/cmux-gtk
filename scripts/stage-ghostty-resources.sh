#!/usr/bin/env bash
set -euo pipefail

# stage-ghostty-resources.sh -- Copy Ghostty themes, shell integration and terminfo
# into <share>/cmux/{ghostty,terminfo}, the layout src/ghostty/resources.rs resolves.
# Usage: ./stage-ghostty-resources.sh <destination-share-dir> [ghostty-zig-out-share]

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:?usage: stage-ghostty-resources.sh <destination-share-dir> [source-share-dir]}"
SOURCE="${2:-$REPO_ROOT/ghostty/zig-out/share}"

for required in ghostty/themes ghostty/shell-integration terminfo/x/xterm-ghostty; do
    if [[ ! -e "$SOURCE/$required" ]]; then
        echo "ERROR: Ghostty resource missing: $SOURCE/$required (build ghostty first)" >&2
        exit 1
    fi
done

mkdir -p "$DEST/cmux/ghostty" "$DEST/cmux/terminfo"
rm -rf "$DEST/cmux/ghostty/themes" "$DEST/cmux/ghostty/shell-integration"
cp -R "$SOURCE/ghostty/themes" "$SOURCE/ghostty/shell-integration" "$DEST/cmux/ghostty/"
cp -R "$SOURCE/terminfo/." "$DEST/cmux/terminfo/"
chmod -R u+rwX,go+rX,go-w "$DEST/cmux/ghostty" "$DEST/cmux/terminfo"
