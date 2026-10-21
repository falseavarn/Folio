#!/bin/sh
# Builds Folio's Linux release packages into dist/: .deb, .rpm and AppImage, plus SHA256SUMS.
# Needs cargo-deb, cargo-generate-rpm and appimagetool (on PATH, or set APPIMAGETOOL).
set -eu
cd "$(dirname "$0")/.."

pkgid=$(cargo pkgid)
version=${pkgid##*[#@]}
appimagetool=${APPIMAGETOOL:-appimagetool}

rm -rf dist target/Folio.AppDir
mkdir -p dist
cargo build --release --locked

cargo deb --no-build --output dist/
cargo generate-rpm --output dist/

appdir=target/Folio.AppDir
mkdir -p "$appdir/usr/bin"
cp target/release/folio "$appdir/usr/bin/"
cp packaging/folio.desktop "$appdir/"
cp icons/128x128@2x.png "$appdir/folio.png"
ln -s usr/bin/folio "$appdir/AppRun"
# The CI runner has no FUSE, so let appimagetool unpack itself instead of mounting.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$appimagetool" --no-appstream "$appdir" "dist/Folio-$version-x86_64.AppImage"

(cd dist && sha256sum -- * > SHA256SUMS)
ls -l dist
