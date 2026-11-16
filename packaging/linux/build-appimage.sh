#!/usr/bin/env bash
# Builds dist/GradeBook-<version>-<arch>.AppImage from the release binary.
#
# Run `cargo build --release` first. Needs `appimagetool` on PATH (or its location in
# $APPIMAGETOOL): https://github.com/AppImage/appimagetool/releases
#
# Nothing is bundled besides the binary: it loads the system's X11/Wayland libraries at
# run time, as every desktop Linux has them.
set -euo pipefail
cd "$(dirname "$0")/../.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)
id=io.github.JMTDI.GradeBook
appdir=target/appimage/GradeBook.AppDir
output=dist/GradeBook-$version-$arch.AppImage

rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" \
         "$appdir/usr/share/applications" \
         "$appdir/usr/share/icons/hicolor/256x256/apps" \
         "$appdir/usr/share/doc/gradebook" \
         dist

install -m 755 target/release/gradebook "$appdir/usr/bin/gradebook"
install -m 644 "packaging/linux/$id.desktop" "$appdir/usr/share/applications/$id.desktop"
install -m 644 assets/icons/icon-256.png "$appdir/usr/share/icons/hicolor/256x256/apps/$id.png"
install -m 644 ui/fonts/Inter-OFL.txt "$appdir/usr/share/doc/gradebook/Inter-OFL.txt"

# appimagetool wants the desktop file and the icon at the top of the AppDir too.
install -m 644 "packaging/linux/$id.desktop" "$appdir/$id.desktop"
install -m 644 assets/icons/icon-256.png "$appdir/$id.png"
ln -s "$id.png" "$appdir/.DirIcon"
ln -s usr/bin/gradebook "$appdir/AppRun"

# Run appimagetool without FUSE, so this also works in containers and on CI runners.
export APPIMAGE_EXTRACT_AND_RUN=1
ARCH=$arch "${APPIMAGETOOL:-appimagetool}" "$appdir" "$output"
chmod +x "$output"
echo "built $output"
