#!/bin/bash
# Builds Haven for Apple TV in the same fixed place as the iPhone app
# (iphone/build), so every build overwrites the last. Two kinds, as the
# iPhone app:
#
#   iphone/scripts/build-tv.sh [github|store] [simulator|device]
#
#   github  add-ons, media servers at any address (scheme HavenTVGitHub)
#   store   App Store: no add-ons, Local Area Network servers (scheme
#           HavenTV), with the store check. The default, so
#           `build-tv.sh simulator` / `build-tv.sh device` build it.
#
# Build keys: set -a; . Local/build-keys.env; set +a  before this.
# The simulator is the project's one Apple TV simulator, "Apple TV 4K (4th generation)".
# libmpv for Apple TV comes from the MPVKit Swift package (fetched by Xcode).
set -euo pipefail

kind=store
case "${1:-}" in github|store) kind="$1"; shift ;; esac
what="${1:-simulator}"
here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"

[ -f ../ios/libmpv/device/Mpv.framework/Mpv ] || ../ios/fetch-libmpv.sh
scripts/build-keys.sh Haven/Generated/BuildKeys.swift
scripts/build-keys.sh HavenTV/Generated/BuildKeys.swift
xcodegen generate --quiet

if [ "$kind" = github ]; then
  scheme=HavenTVGitHub debug=Debug release=Release
else
  scheme=HavenTV debug=StoreDebug release=Store
fi
simulator="platform=tvOS Simulator,name=${HAVEN_TV_SIMULATOR:-Apple TV 4K (4th generation)}"
common=(-project Haven.xcodeproj -scheme "$scheme" -derivedDataPath build -quiet)

case "$what" in
  simulator)
    dest="$simulator"
    xcrun simctl list devices available | grep -q "${HAVEN_TV_SIMULATOR:-Apple TV 4K (4th generation)}" \
      || dest="generic/platform=tvOS Simulator"
    xcodebuild "${common[@]}" -configuration "$debug" -destination "$dest" build
    app="build/Build/Products/$debug-appletvsimulator/Haven.app"
    ;;
  device)
    xcodebuild "${common[@]}" -configuration "$release" -destination "generic/platform=tvOS" \
      CODE_SIGNING_ALLOWED=NO build
    app="build/Build/Products/$release-appletvos/Haven.app"
    ;;
  *) echo "What is simulator or device" >&2; exit 2 ;;
esac

[ "$kind" = store ] && scripts/store-check.sh "$app"
echo "Built $here/$app ($kind)"
