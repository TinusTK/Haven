#!/bin/bash
# Builds the Swift iPhone app, or runs its tests, in one fixed place
# (iphone/build), so every build overwrites the last.
#
#   iphone/scripts/build.sh [github|store] [simulator|device|test]
#
#   github  the GitHub IPA's build: add-ons, no network limit (default)
#   store   the App Store / TestFlight build: no add-ons, Local Area
#           Network only; the store check runs on it
#
#   simulator  build for the simulator (default)
#   device     build for an iPhone, unsigned (signing comes in S18)
#   test       run the unit tests in the simulator
#
# Build keys: set -a; . Local/build-keys.env; set +a  before this.
# The simulator is the project's one simulator, "iPhone 18 Pro".
set -euo pipefail

kind="${1:-github}"
what="${2:-simulator}"
here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"

case "$kind" in
  github) scheme=Haven; debug=Debug ;;
  store) scheme=HavenStore; debug=StoreDebug ;;
  *) echo "Kind is github or store" >&2; exit 2 ;;
esac

[ -f ../ios/libmpv/device/Mpv.framework/Mpv ] || ../ios/fetch-libmpv.sh
scripts/build-keys.sh Haven/Generated/BuildKeys.swift
xcodegen generate --quiet

# HAVEN_SIMULATOR picks another simulator (CI has no "iPhone 18 Pro").
simulator="platform=iOS Simulator,name=${HAVEN_SIMULATOR:-iPhone 18 Pro}"
common=(-project Haven.xcodeproj -scheme "$scheme" -derivedDataPath build -quiet)

case "$what" in
  simulator)
    xcodebuild "${common[@]}" -configuration "$debug" -destination "$simulator" build
    app="build/Build/Products/$debug-iphonesimulator/Haven.app"
    ;;
  device)
    release=$([ "$kind" = store ] && echo Store || echo Release)
    xcodebuild "${common[@]}" -configuration "$release" -destination "generic/platform=iOS" \
      CODE_SIGNING_ALLOWED=NO build
    app="build/Build/Products/$release-iphoneos/Haven.app"
    ;;
  test)
    xcodebuild "${common[@]}" -destination "$simulator" test
    echo "Tests passed ($kind build)"
    exit 0
    ;;
  *) echo "What is simulator, device or test" >&2; exit 2 ;;
esac

if [ "$kind" = store ]; then
  scripts/store-check.sh "$app"
fi
echo "Built $here/$app"
