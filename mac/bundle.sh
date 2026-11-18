#!/bin/bash
# Makes Haven.app and Haven-<version>-macos.dmg in app/target/macos from
# the release builds. Run after fetch-libmpv.sh, from anywhere:
#
#   mac/bundle.sh            # this Mac's kind of processor only
#   mac/bundle.sh universal  # Intel and Apple silicon in one
#
# Signing:
#   HAVEN_SIGN_IDENTITY="Developer ID Application: <name> (<team>)"
#       signs the app with that certificate from the keychain (the first
#       time, macOS asks to let codesign use the key: choose Always Allow).
#   HAVEN_NOTARY_PROFILE=<name>
#       also sends the disk image to Apple to be notarised and staples
#       the answer to it. The profile is made once with
#       `xcrun notarytool store-credentials <name> --apple-id <Apple ID>
#       --team-id <team>`, which asks for an app-specific password.
#   HAVEN_NOTARY_KEY=<AuthKey .p8 file>, HAVEN_NOTARY_KEY_ID, HAVEN_NOTARY_ISSUER
#       the same with an App Store Connect API key instead of a stored
#       profile (how the CI build does it, from the repository's secrets).
#   HAVEN_NOTARY_APPLE_ID, HAVEN_NOTARY_PASSWORD, HAVEN_NOTARY_TEAM_ID
#       or with an Apple ID and an app-specific password.
# Without them the app is signed "ad hoc": macOS warns the first time it
# is opened, and the user allows it with right-click > Open.
set -euo pipefail

cd "$(dirname "$0")/../app"
libs="../mac/libmpv"
[ -f "$libs/libmpv.dylib" ] || { echo "Run mac/fetch-libmpv.sh first." >&2; exit 1; }
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
sed "s/@VERSION@/$version/g" ../mac/Info.plist > "$app/Contents/Info.plist"
cp ../LICENSE "$app/Contents/Resources/LICENSE.txt"
cp ../THIRD-PARTY.md "$app/Contents/Resources/"
cp ../licenses/*.txt "$app/Contents/Resources/licenses/"

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
if [ -n "${HAVEN_SIGN_IDENTITY:-}" ]; then
  # Apple only notarises code signed with the hardened runtime and a
  # secure timestamp; the libraries get the same signature, so the
  # program may load them.
  codesign --force --options runtime --timestamp --sign "$HAVEN_SIGN_IDENTITY" "$app/Contents/Frameworks/"*.dylib
  codesign --force --options runtime --timestamp --sign "$HAVEN_SIGN_IDENTITY" "$app"
  codesign --verify --deep --strict "$app"
else
  codesign --force --sign - "$app/Contents/Frameworks/"*.dylib
  codesign --force --sign - "$app"
fi

dmg="$out/Haven-$version-macos.dmg"
stage="$out/dmg"
mkdir -p "$stage" && cp -R "$app" "$stage/" && ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "Haven $version" -srcfolder "$stage" -format UDZO "$dmg"
rm -rf "$stage"
if [ -n "${HAVEN_SIGN_IDENTITY:-}" ]; then
  codesign --force --timestamp --sign "$HAVEN_SIGN_IDENTITY" "$dmg"
  if [ -n "${HAVEN_NOTARY_PROFILE:-}" ]; then
    xcrun notarytool submit "$dmg" --keychain-profile "$HAVEN_NOTARY_PROFILE" --wait
    xcrun stapler staple "$dmg"
  elif [ -n "${HAVEN_NOTARY_KEY:-}" ] && [ -n "${HAVEN_NOTARY_KEY_ID:-}" ]; then
    xcrun notarytool submit "$dmg" --key "$HAVEN_NOTARY_KEY" --key-id "$HAVEN_NOTARY_KEY_ID" \
      --issuer "${HAVEN_NOTARY_ISSUER:-}" --wait
    xcrun stapler staple "$dmg"
  elif [ -n "${HAVEN_NOTARY_APPLE_ID:-}" ] && [ -n "${HAVEN_NOTARY_PASSWORD:-}" ]; then
    xcrun notarytool submit "$dmg" --apple-id "$HAVEN_NOTARY_APPLE_ID" \
      --password "$HAVEN_NOTARY_PASSWORD" --team-id "${HAVEN_NOTARY_TEAM_ID:-}" --wait
    xcrun stapler staple "$dmg"
  else
    echo "Signed, not notarised: set HAVEN_NOTARY_PROFILE to have Apple check it."
  fi
fi
echo "Made $app and $dmg"
