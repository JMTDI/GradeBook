#!/usr/bin/env bash
# Builds a universal (Apple Silicon + Intel) GradeBook.app and a drag-to-install .dmg
# in dist/. Run on macOS, from anywhere, after building both targets:
#
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
#   cargo build --release --target aarch64-apple-darwin
#   cargo build --release --target x86_64-apple-darwin
#   packaging/macos/build-dmg.sh
#
# The app is signed ad hoc (no Apple Developer ID), so Gatekeeper asks for a
# right-click > Open the first time. See the README for signing and notarizing.
set -euo pipefail
cd "$(dirname "$0")/../.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arm=${ARM_BINARY:-target/aarch64-apple-darwin/release/gradebook}
intel=${INTEL_BINARY:-target/x86_64-apple-darwin/release/gradebook}
out=dist/macos
app=$out/GradeBook.app
dmg=dist/GradeBook-$version-macos-universal.dmg

rm -rf "$out" "$dmg"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

lipo -create -output "$app/Contents/MacOS/gradebook" "$arm" "$intel"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
cp assets/icons/icon.icns "$app/Contents/Resources/AppIcon.icns"
cp ui/fonts/Inter-OFL.txt "$app/Contents/Resources/Inter-OFL.txt"

# Apple Silicon refuses to run unsigned code, and lipo invalidates the signature the
# linker added, so sign again (ad hoc).
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

# A disk image with the app next to an Applications shortcut, ready to drag.
stage=$out/dmg-root
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname "GradeBook" -srcfolder "$stage" -ov -format UDZO "$dmg"

echo "built $dmg"
