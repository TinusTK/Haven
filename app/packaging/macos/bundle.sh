#!/bin/bash
# Makes Haven.app and Haven-<version>-macos.dmg in target/macos from the
# release builds. Run from app/ after fetch-libmpv.sh:
#
#   packaging/macos/bundle.sh            # this Mac's kind of processor only
#   packaging/macos/bundle.sh universal  # Intel and Apple silicon in one
#
# The app is signed "ad hoc" (no Apple Developer ID): macOS warns the
# first time it is opened, and the user allows it in System Settings >
# Privacy & Security, or with right-click > Open.
set -euo pipefail

cd "$(dirname "$0")/../.."
libs="packaging/macos/libmpv"
[ -f "$libs/libmpv.dylib" ] || { echo "Run packaging/macos/fetch-libmpv.sh first." >&2; exit 1; }
version="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="haven-app"))')"

out="target/macos"
app="$out/Haven.app"
rm -rf "$out" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources/licenses"

if [ "${1:-}" = "universal" ]; then
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release -p haven-app --features player --target "$target"
  done
  lipo -create -output "$app/Contents/MacOS/haven" \
    target/aarch64-apple-darwin/release/haven target/x86_64-apple-darwin/release/haven
else
  cargo build --release -p haven-app --features player
  cp target/release/haven "$app/Contents/MacOS/haven"
fi

# The video engine and its libraries, beside the program (LGPL: they
# stay separate files that can be replaced).
cp "$libs"/*.dylib "$app/Contents/Frameworks/"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
cp ../LICENSE "$app/Contents/Resources/LICENSE.txt"
cp ../THIRD-PARTY.md "$app/Contents/Resources/"
cp packaging/licenses/*.txt "$app/Contents/Resources/licenses/"

# The icon, from the halo on its dark tile.
set="$out/haven.iconset"
mkdir -p "$set"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" assets/icon/haven-tile.png --out "$set/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" assets/icon/haven-tile.png --out "$set/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set" -o "$app/Contents/Resources/haven.icns"
rm -rf "$set"

# The program must find its libraries inside the app, wherever it is put.
otool -l "$app/Contents/MacOS/haven" | grep -q "@executable_path/../Frameworks" \
  || install_name_tool -add_rpath "@executable_path/../Frameworks" "$app/Contents/MacOS/haven"
if otool -L "$app/Contents/MacOS/haven" | grep -q "/opt/homebrew\|/usr/local"; then
  echo "haven is linked against a libmpv outside the app (Homebrew's?)." >&2
  exit 1
fi
codesign --force --sign - "$app/Contents/Frameworks/"*.dylib
codesign --force --sign - "$app"

dmg="$out/Haven-$version-macos.dmg"
stage="$out/dmg"
mkdir -p "$stage" && cp -R "$app" "$stage/" && ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "Haven $version" -srcfolder "$stage" -format UDZO "$dmg"
rm -rf "$stage"
echo "Made $app and $dmg"
