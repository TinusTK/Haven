#!/bin/bash
# Makes Haven.app for iPhone and iPad in app/target/ios, from anywhere:
#
#   ios/bundle.sh            # for a device, and Haven-<version>-ios-unsigned.ipa
#   ios/bundle.sh simulator  # for the iOS Simulator on this Mac
#
# The IPA is not signed. To put it on a device it has to be signed by
# whoever installs it, with a tool such as AltStore or Sideloadly (they
# sign it with that person's own Apple ID).
set -euo pipefail

cd "$(dirname "$0")/../app"
version="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="haven-app"))')"
if [ "${1:-}" = "simulator" ]; then
  target="aarch64-apple-ios-sim"; platform="iPhoneSimulator"; out="target/ios-simulator"
else
  target="aarch64-apple-ios"; platform="iPhoneOS"; out="target/ios"
fi
# The oldest iOS Haven runs on; the linker needs to be told.
export IPHONEOS_DEPLOYMENT_TARGET=15.0
cargo build --release -p haven-app --target "$target"

app="$out/Haven.app"
rm -rf "$out" && mkdir -p "$app/licenses"
cp "target/$target/release/haven" "$app/haven"
sed -e "s/@VERSION@/$version/g" -e "s/@PLATFORM@/$platform/g" ../ios/Info.plist > "$app/Info.plist"
plutil -convert binary1 "$app/Info.plist"
printf 'APPL????' > "$app/PkgInfo"
cp ../LICENSE "$app/LICENSE.txt"
cp ../THIRD-PARTY.md "$app/"
cp ../licenses/OFL-*.txt "$app/licenses/"

# The icon: the halo on its dark tile, square (iOS rounds the corners).
for entry in 60x60@2x:120 60x60@3x:180 76x76@2x~ipad:152; do
  name="${entry%%:*}"; size="${entry##*:}"
  sips -z "$size" "$size" assets/icon/haven-tile.png --out "$app/AppIcon$name.png" >/dev/null
done

if [ "$platform" = "iPhoneSimulator" ]; then
  # The simulator wants a signature of some kind; an empty one will do.
  codesign --force --sign - "$app"
  echo "Made $app (install it with: xcrun simctl install booted $PWD/$app)"
else
  ipa="$out/Haven-$version-ios-unsigned.ipa"
  mkdir -p "$out/Payload" && cp -R "$app" "$out/Payload/"
  (cd "$out" && zip -qry "$(basename "$ipa")" Payload)
  rm -rf "$out/Payload"
  echo "Made $app and $ipa (unsigned)"
fi
