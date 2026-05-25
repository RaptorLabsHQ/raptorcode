#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACKAGE_NAME="raptor-code"
BINARIES=("raptor" "raptorcode")

cd "$ROOT_DIR"
umask 022

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "error: dpkg-deb is required to build a Debian package" >&2
  exit 1
fi

VERSION="${RAPTOR_VERSION:-$(awk -F' = ' '/^version = / { gsub(/"/, "", $2); print $2; exit }' Cargo.toml)}"
ARCH="${DEB_ARCH:-$(dpkg --print-architecture)}"
BUILD_ROOT="$ROOT_DIR/target/debian"
PACKAGE_ROOT="$BUILD_ROOT/${PACKAGE_NAME}_${VERSION}_${ARCH}"
OUTPUT_DIR="$ROOT_DIR/dist"
OUTPUT_FILE="$OUTPUT_DIR/${PACKAGE_NAME}_${VERSION}_${ARCH}.deb"

if [[ -z "$VERSION" ]]; then
  echo "error: could not determine package version from Cargo.toml" >&2
  exit 1
fi

cargo build --release -p raptor-cli --bin raptor --bin raptorcode

rm -rf "$PACKAGE_ROOT"
mkdir -p \
  "$PACKAGE_ROOT/DEBIAN" \
  "$PACKAGE_ROOT/usr/bin" \
  "$PACKAGE_ROOT/usr/share/doc/$PACKAGE_NAME" \
  "$OUTPUT_DIR"

for binary in "${BINARIES[@]}"; do
  install -m 0755 "$ROOT_DIR/target/release/$binary" "$PACKAGE_ROOT/usr/bin/$binary"
done

install -m 0644 "$ROOT_DIR/README.md" "$PACKAGE_ROOT/usr/share/doc/$PACKAGE_NAME/README.md"
install -m 0644 "$ROOT_DIR/CHANGELOG.md" "$PACKAGE_ROOT/usr/share/doc/$PACKAGE_NAME/CHANGELOG.md"
install -m 0644 "$ROOT_DIR/LICENSE" "$PACKAGE_ROOT/usr/share/doc/$PACKAGE_NAME/LICENSE"

cat >"$PACKAGE_ROOT/DEBIAN/control" <<CONTROL
Package: $PACKAGE_NAME
Version: $VERSION
Section: devel
Priority: optional
Architecture: $ARCH
Maintainer: Bob Vasic <17962121+bobvasic@users.noreply.github.com>
Depends: libc6 (>= 2.31), ca-certificates, git
Homepage: https://raptorlabs.dev
Vcs-Git: https://github.com/RaptorLabsHQ/raptor-code.git
Description: Rust-native autonomous AI coding agent CLI
 RaptorCode is a local-first supervised AI coding terminal from RaptorLabs.dev.
 It installs the raptor and raptorcode commands for Linux users.
CONTROL

dpkg-deb --build --root-owner-group "$PACKAGE_ROOT" "$OUTPUT_FILE"

echo "$OUTPUT_FILE"
