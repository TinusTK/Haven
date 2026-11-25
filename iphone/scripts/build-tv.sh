#!/bin/bash
# Builds Haven for Apple TV (App Store kind only: no add-ons, Local Area
# Network servers) in the same fixed place as the iPhone app
# (iphone/build), so every build overwrites the last.
#
#   iphone/scripts/build-tv.sh [simulator|device]
#
# Build keys: set -a; . Local/build-keys.env; set +a  before this.
# The simulator is the project's one Apple TV simulator, "Apple TV 4K (4th generation)".
# libmpv for Apple TV comes from the MPVKit Swift package (fetched by Xcode).
set -euo pipefail

what="${1:-simulator}"
here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"

[ -f ../ios/libmpv/device/Mpv.framework/Mpv ] || ../ios/fetch-libmpv.sh
scripts/build-keys.sh Haven/Generated/BuildKeys.swift
scripts/build-keys.sh HavenTV/Generated/BuildKeys.swift
xcodegen generate --quiet

simulator="platform=tvOS Simulator,name=${HAVEN_TV_SIMULATOR:-Apple TV 4K (4th generation)}"
common=(-project Haven.xcodeproj -scheme HavenTV -derivedDataPath build -quiet)

case "$what" in
  simulator)
    dest="$simulator"
    xcrun simctl list devices available | grep -q "${HAVEN_TV_SIMULATOR:-Apple TV 4K (4th generation)}" \
      || dest="generic/platform=tvOS Simulator"
    xcodebuild "${common[@]}" -configuration StoreDebug -destination "$dest" build
    app="build/Build/Products/StoreDebug-appletvsimulator/Haven.app"
    ;;
  device)
    xcodebuild "${common[@]}" -configuration Store -destination "generic/platform=tvOS" \
      CODE_SIGNING_ALLOWED=NO build
    app="build/Build/Products/Store-appletvos/Haven.app"
    ;;
  *) echo "What is simulator or device" >&2; exit 2 ;;
esac

scripts/store-check.sh "$app"
echo "Built $here/$app"
