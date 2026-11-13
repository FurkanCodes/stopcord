#!/bin/bash
# Builds Stopcord.app for release and packs it as dist/Stopcord-<version>.zip
# with a .sha256 file next to it, the two files a release carries.
#   scripts/package.sh
# The app is signed ad hoc (no Apple Developer account yet), so it isn't notarized.
set -euo pipefail

cd "$(dirname "$0")/.."
version="$(sed -n 's/^  "version": "\(.*\)",$/\1/p' app/src-tauri/tauri.conf.json)"
[ -n "$version" ] || { echo "error: no version in app/src-tauri/tauri.conf.json" >&2; exit 1; }

npm ci
(cd app && npx tauri build --bundles app)

app="target/release/bundle/macos/Stopcord.app"
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

mkdir -p dist
name="Stopcord-$version.zip"
rm -f "dist/$name" "dist/$name.sha256"
ditto -c -k --keepParent "$app" "dist/$name"
(cd dist && shasum -a 256 "$name" > "$name.sha256")
echo "dist/$name"
cat "dist/$name.sha256"
