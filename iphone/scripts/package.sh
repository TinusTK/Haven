#!/bin/bash
# Makes an IPA of the Swift iPhone app, as the old Rust build's ios/bundle.sh
# did for the old one:
#
#   iphone/scripts/package.sh github   iphone/build/ipa/Haven-<v>-ios-unsigned.ipa
#                                      (add-ons; whoever installs it signs it,
#                                      as SideStore does)
#   iphone/scripts/package.sh store    iphone/build/ipa/Haven-<v>-ios-store.ipa
#                                      (no add-ons; signed for the App Store
#                                      when HAVEN_IOS_SIGN_IDENTITY and
#                                      HAVEN_IOS_PROFILE are set, else unsigned)
#
# The store build is numbered by the time it was made (UTC), e.g.
# 202611251830: the App Store takes each build number once per version.
set -euo pipefail

kind="${1:-github}"
here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"

case "$kind" in
  github) release=Release ;;
  store) release=Store ;;
  *) echo "Kind is github or store" >&2; exit 2 ;;
esac

scripts/build.sh "$kind" device
built="build/Build/Products/$release-iphoneos/Haven.app"
out="build/ipa"
rm -rf "$out" && mkdir -p "$out"
app="$out/Haven.app"
cp -R "$built" "$app"
version="$(plutil -extract CFBundleShortVersionString raw "$app/Info.plist")"
minimum="$(plutil -extract MinimumOSVersion raw "$app/Info.plist")"

# What the App Store asks of a framework: libmpv's Info.plists say iOS 9
# and name no platform.
for framework in "$app"/Frameworks/*.framework; do
  plist="$framework/Info.plist"
  plutil -replace MinimumOSVersion -string "$minimum" "$plist"
  plutil -replace CFBundlePackageType -string FMWK "$plist"
  plutil -replace CFBundleSupportedPlatforms -json '["iPhoneOS"]' "$plist"
  rm -rf "$framework/_CodeSignature"
done

# Haven's licence and those of the parts it ships with.
cp ../LICENSE "$app/LICENSE.txt"
cp ../THIRD-PARTY.md "$app/"
mkdir -p "$app/licenses"
cp ../licenses/*.txt "$app/licenses/"

if [ "$kind" = store ]; then
  plutil -replace CFBundleVersion -string "$(date -u +%Y%m%d%H%M)" "$app/Info.plist"
  scripts/store-check.sh "$app"
  ipa="$out/Haven-$version-ios-store.ipa"
  if [ -n "${HAVEN_IOS_SIGN_IDENTITY:-}" ] && [ -n "${HAVEN_IOS_PROFILE:-}" ]; then
    scripts/sign-store.sh "$app" "$HAVEN_IOS_SIGN_IDENTITY" "$HAVEN_IOS_PROFILE"
    signed="signed for the App Store"
  else
    signed="NOT signed: set HAVEN_IOS_SIGN_IDENTITY and HAVEN_IOS_PROFILE to sign it"
  fi
else
  ipa="$out/Haven-$version-ios-unsigned.ipa"
  signed="unsigned"
fi

mkdir -p "$out/Payload" && mv "$app" "$out/Payload/"
(cd "$out" && zip -qry "$(basename "$ipa")" Payload)
rm -rf "$out/Payload"
echo "Made $here/$ipa ($signed)"
