#!/bin/bash
# Makes an IPA of the Apple TV app, as package.sh does for the iPhone:
#
#   iphone/scripts/package-tv.sh github   iphone/build/tv-ipa/Haven-<v>-tvos-unsigned.ipa
#                                         (add-ons; whoever installs it signs
#                                         it, with Sideloadly or Xcode)
#   iphone/scripts/package-tv.sh [store]  iphone/build/tv-ipa/Haven-<v>-tvos-store.ipa
#                                         (no add-ons, Local Area Network
#                                         servers; signed when
#                                         HAVEN_IOS_SIGN_IDENTITY and
#                                         HAVEN_TV_PROFILE, a tvOS App Store
#                                         profile, are set, else unsigned)
#
# The store build is numbered by the time it was made (UTC), as the iPhone
# store build is.
set -euo pipefail

kind="${1:-store}"
here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"

case "$kind" in
  github) release=Release ;;
  store) release=Store ;;
  *) echo "Kind is github or store" >&2; exit 2 ;;
esac

scripts/build-tv.sh "$kind" device
built="build/Build/Products/$release-appletvos/Haven.app"
out="build/tv-ipa"
rm -rf "$out" && mkdir -p "$out"
app="$out/Haven.app"
cp -R "$built" "$app"
version="$(plutil -extract CFBundleShortVersionString raw "$app/Info.plist")"

# Haven's licence and those of the parts it ships with.
cp ../LICENSE "$app/LICENSE.txt"
cp ../THIRD-PARTY.md "$app/"
mkdir -p "$app/licenses"
cp ../licenses/*.txt "$app/licenses/"

if [ "$kind" = store ]; then
  plutil -replace CFBundleVersion -string "$(date -u +%Y%m%d%H%M)" "$app/Info.plist"
  scripts/store-check.sh "$app"
  ipa="$out/Haven-$version-tvos-store.ipa"
  if [ -n "${HAVEN_IOS_SIGN_IDENTITY:-}" ] && [ -n "${HAVEN_TV_PROFILE:-}" ]; then
    scripts/sign-store.sh "$app" "$HAVEN_IOS_SIGN_IDENTITY" "$HAVEN_TV_PROFILE"
    signed="signed for the App Store"
  else
    signed="NOT signed: set HAVEN_IOS_SIGN_IDENTITY and HAVEN_TV_PROFILE to sign it"
  fi
else
  ipa="$out/Haven-$version-tvos-unsigned.ipa"
  signed="unsigned"
fi

mkdir -p "$out/Payload" && mv "$app" "$out/Payload/"
(cd "$out" && zip -qry "$(basename "$ipa")" Payload)
rm -rf "$out/Payload"
echo "Made $here/$ipa ($signed)"
