#!/usr/bin/env bash
# Build the release tarball: dist/tdm-<arch>-linux.tar.gz (+ .sha256)
# usage: scripts/package.sh [target-dir]
set -euo pipefail
cd "$(dirname "$0")/.."
T="${1:-${CARGO_TARGET_DIR:-target}}/release"
ARCH=$(uname -m)
NAME="tdm-$ARCH-linux"
rm -rf "dist/$NAME" && mkdir -p "dist/$NAME"
cp "$T/tdm" "$T/tdm-greeter" "dist/$NAME/"
cp -r data LICENSE README.md "dist/$NAME/"
mkdir -p "dist/$NAME/docs" && cp docs/*.md "dist/$NAME/docs/"
tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"
(cd dist && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
ls -la "dist/$NAME.tar.gz"; cat "dist/$NAME.tar.gz.sha256"
