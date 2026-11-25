#!/bin/bash
# Makes the Apple TV app's App Store IPA (no add-ons, Local Area Network
# servers): iphone/build/tv-ipa/Haven-<v>-tvos-store.ipa, signed when
# HAVEN_IOS_SIGN_IDENTITY and HAVEN_TV_PROFILE (a tvOS App Store profile)
# are set, else unsigned. Numbered by the time it was made (UTC), as the
# iPhone store build is.
#
#   iphone/scripts/package-tv.sh
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"

scripts/build-tv.sh device
built="build/Build/Products/Store-appletvos/Haven.app"
out="build/tv-ipa"
rm -rf "$out" && mkdir -p "$out"
app="$out/Haven.app"
cp -R "$built" "$app"
version="$(plutil -extract CFBundleShortVersionString raw "$app/Info.plist")"
plutil -replace CFBundleVersion -string "$(date -u +%Y%m%d%H%M)" "$app/Info.plist"

# Haven's licence and those of the parts it ships with.
cp ../LICENSE "$app/LICENSE.txt"
cp ../THIRD-PARTY.md "$app/"
mkdir -p "$app/licenses"
cp ../licenses/*.txt "$app/licenses/"

scripts/store-check.sh "$app"
ipa="$out/Haven-$version-tvos-store.ipa"
if [ -n "${HAVEN_IOS_SIGN_IDENTITY:-}" ] && [ -n "${HAVEN_TV_PROFILE:-}" ]; then
  scripts/sign-store.sh "$app" "$HAVEN_IOS_SIGN_IDENTITY" "$HAVEN_TV_PROFILE"
  signed="signed for the App Store"
else
  signed="NOT signed: set HAVEN_IOS_SIGN_IDENTITY and HAVEN_TV_PROFILE to sign it"
fi

mkdir -p "$out/Payload" && mv "$app" "$out/Payload/"
(cd "$out" && zip -qry "$(basename "$ipa")" Payload)
rm -rf "$out/Payload"
echo "Made $here/$ipa ($signed)"
