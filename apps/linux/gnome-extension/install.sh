#!/usr/bin/env bash
# Installs the extension into the user's extension directory by symlink, so an
# edit in the working tree is picked up on the next Shell start.
set -euo pipefail

UUID="claude-dashboard@haiz.github.io"
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST="${HOME}/.local/share/gnome-shell/extensions/${UUID}"

mkdir -p "$(dirname "$DEST")"
rm -rf "$DEST"
ln -s "$SRC" "$DEST"

if [ -d "${SRC}/schemas" ]; then
    glib-compile-schemas "${SRC}/schemas"
fi

echo "Installed ${UUID} -> ${SRC}"
echo "Enable with: gnome-extensions enable ${UUID}"
