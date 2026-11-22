#!/bin/bash
# Makes Haven.app for iPhone and iPad in app/target/ios, from anywhere:
#
#   ios/bundle.sh                  # for a device, and Haven-<version>-ios-unsigned.ipa
#   ios/bundle.sh simulator        # for the iOS Simulator on this Mac
#   ios/bundle.sh store            # the App Store build: Haven-<version>-ios-store.ipa
#   ios/bundle.sh simulator store  # the App Store build, for the Simulator
#
# Both builds have Haven's own player: libmpv and FFmpeg go into
# Haven.app/Frameworks, fetched once with ios/fetch-libmpv.sh.
#
# The GitHub IPA is not signed. To put it on a device it has to be signed
# by whoever installs it, with a tool such as AltStore or Sideloadly (they
# sign it with that person's own Apple ID).
#
# The store build has no add-on code at all and reaches media servers only
# on the home network or Tailscale. It is signed for the App Store when
# HAVEN_IOS_SIGN_IDENTITY (an "Apple Distribution" identity in the
# keychain) and HAVEN_IOS_PROFILE (its .mobileprovision file) are set;
# otherwise it is left unsigned, for testing.
set -euo pipefail

cd "$(dirname "$0")/../app"
version="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="haven-app"))')"
simulator=false; store=false
for arg in "$@"; do
  case "$arg" in
    simulator) simulator=true ;;
    store) store=true ;;
    *) echo "unknown: $arg (use simulator and/or store)"; exit 2 ;;
  esac
done
if $simulator; then
  target="aarch64-apple-ios-sim"; platform="iPhoneSimulator"; out="target/ios-simulator"
else
  target="aarch64-apple-ios"; platform="iPhoneOS"; out="target/ios"
fi
features=(--features player)
slice="device"; $simulator && slice="simulator"
if $store; then
  out="$out-store"
  features=(--no-default-features --features store,player)
fi
libmpv="../ios/libmpv/$slice"
[ -f "$libmpv/Mpv.framework/Mpv" ] || ../ios/fetch-libmpv.sh
# The oldest iOS Haven runs on; the linker needs to be told.
export IPHONEOS_DEPLOYMENT_TARGET=15.0
cargo build --release -p haven-app --target "$target" ${features[@]+"${features[@]}"}

app="$out/Haven.app"
rm -rf "$out" && mkdir -p "$app/licenses"
cp "target/$target/release/haven" "$app/haven"
sed -e "s/@VERSION@/$version/g" -e "s/@PLATFORM@/$platform/g" ../ios/Info.plist > "$app/Info.plist"
# What Xcode writes about the build; the App Store reads the SDK from it.
sdk="$(echo "$platform" | tr '[:upper:]' '[:lower:]')"
sdk_version="$(xcrun --sdk "$sdk" --show-sdk-version)"
xcode_version="$(xcodebuild -version | awk 'NR==1 {print $2}')"
xcode_major="${xcode_version%%.*}"; xcode_rest="${xcode_version#*.}"; [ "$xcode_rest" = "$xcode_version" ] && xcode_rest=0
xcode_minor="${xcode_rest%%.*}"; xcode_patch="${xcode_rest#*.}"; [ "$xcode_patch" = "$xcode_rest" ] && xcode_patch=0
plutil -replace DTPlatformName -string "$sdk" "$app/Info.plist"
plutil -replace DTPlatformVersion -string "$sdk_version" "$app/Info.plist"
plutil -replace DTSDKName -string "$sdk$sdk_version" "$app/Info.plist"
plutil -replace DTSDKBuild -string "$(xcrun --sdk "$sdk" --show-sdk-build-version)" "$app/Info.plist"
plutil -replace DTPlatformBuild -string "$(xcrun --sdk "$sdk" --show-sdk-build-version)" "$app/Info.plist"
plutil -replace DTXcode -string "$(printf '%02d%d%d' "$xcode_major" "$xcode_minor" "$xcode_patch")" "$app/Info.plist"
plutil -replace DTXcodeBuild -string "$(xcodebuild -version | awk 'NR==2 {print $3}')" "$app/Info.plist"
plutil -replace DTCompiler -string com.apple.compilers.llvm.clang.1_0 "$app/Info.plist"
plutil -replace BuildMachineOSBuild -string "$(sw_vers -buildVersion)" "$app/Info.plist"
# The App Store takes each build number once per version: store builds
# are numbered by the time they were made (UTC), e.g. 202611230130.
if $store; then
  plutil -replace CFBundleVersion -string "$(date -u +%Y%m%d%H%M)" "$app/Info.plist"
fi
plutil -convert binary1 "$app/Info.plist"
printf 'APPL????' > "$app/PkgInfo"
cp ../LICENSE "$app/LICENSE.txt"
cp ../THIRD-PARTY.md "$app/"
cp ../licenses/OFL-*.txt "$app/licenses/"
cp ../licenses/LGPL-*.txt "$app/licenses/"

# The player's libraries: libmpv, FFmpeg and what they load. Their headers
# are left out, and their Info.plist gets what the App Store asks of a
# framework (the oldest iOS is Haven's own).
mkdir -p "$app/Frameworks"
for framework in "$libmpv"/*.framework; do
  name="$(basename "$framework")"
  cp -R "$framework" "$app/Frameworks/"
  rm -rf "$app/Frameworks/$name/Headers" "$app/Frameworks/$name/Modules" "$app/Frameworks/$name/_CodeSignature"
  plist="$app/Frameworks/$name/Info.plist"
  plutil -replace MinimumOSVersion -string "$IPHONEOS_DEPLOYMENT_TARGET" "$plist"
  plutil -replace CFBundlePackageType -string FMWK "$plist"
  plutil -replace CFBundleSupportedPlatforms -json "[\"$platform\"]" "$plist"
done

# The icon: the halo on its dark tile, square (iOS rounds the corners).
for entry in 60x60@2x:120 60x60@3x:180 76x76@2x~ipad:152; do
  name="${entry%%:*}"; size="${entry##*:}"
  sips -z "$size" "$size" assets/icon/haven-tile.png --out "$app/AppIcon$name.png" >/dev/null
done
# The App Store takes the icon only from an asset catalog (Assets.car),
# made from one 1024-pixel picture with no transparency:
# haven-store-1024.png is the tile with its see-through edge cropped off
# and its rounded corners filled (iOS rounds them away again).
icons="$out/icons" && rm -rf "$icons" && mkdir -p "$icons/Assets.xcassets/AppIcon.appiconset"
cp assets/icon/haven-store-1024.png "$icons/Assets.xcassets/AppIcon.appiconset/icon.png"
printf '%s\n' '{"images":[{"filename":"icon.png","idiom":"universal","platform":"ios","size":"1024x1024"}],"info":{"author":"xcode","version":1}}' \
  > "$icons/Assets.xcassets/AppIcon.appiconset/Contents.json"
printf '%s\n' '{"info":{"author":"xcode","version":1}}' > "$icons/Assets.xcassets/Contents.json"
xcrun actool "$icons/Assets.xcassets" --compile "$app" --platform "$(echo "$platform" | tr '[:upper:]' '[:lower:]')" \
  --minimum-deployment-target "$IPHONEOS_DEPLOYMENT_TARGET" --app-icon AppIcon \
  --target-device iphone --target-device ipad --output-format human-readable-text \
  --output-partial-info-plist "$icons/partial.plist" >/dev/null
for key in CFBundleIcons CFBundleIcons~ipad CFBundleIconName; do
  /usr/libexec/PlistBuddy -c "Delete :$key" "$app/Info.plist" 2>/dev/null || true
done
/usr/libexec/PlistBuddy -c "Merge $icons/partial.plist" "$app/Info.plist"
rm -rf "$icons"

if $store; then
  ../app/store-check.sh "$app/haven" "$target"
fi

if [ "$platform" = "iPhoneSimulator" ]; then
  # The simulator wants a signature of some kind; an empty one will do.
  for framework in "$app"/Frameworks/*.framework; do codesign --force --sign - "$framework"; done
  codesign --force --sign - "$app"
  echo "Made $app (install it with: xcrun simctl install booted $PWD/$app)"
elif $store; then
  ipa="$out/Haven-$version-ios-store.ipa"
  if [ -n "${HAVEN_IOS_SIGN_IDENTITY:-}" ] && [ -n "${HAVEN_IOS_PROFILE:-}" ]; then
    cp "$HAVEN_IOS_PROFILE" "$app/embedded.mobileprovision"
    # The entitlements the profile grants, as the signature must carry them.
    security cms -D -i "$HAVEN_IOS_PROFILE" > "$out/profile.plist"
    /usr/libexec/PlistBuddy -x -c 'Print :Entitlements' "$out/profile.plist" > "$out/entitlements.plist"
    # A profile lists iCloud's services as "*"; the signature must name the
    # one Haven uses (iCloud Drive, for the backup).
    if /usr/libexec/PlistBuddy -c 'Print :com.apple.developer.icloud-services' "$out/entitlements.plist" >/dev/null 2>&1; then
      /usr/libexec/PlistBuddy -c 'Delete :com.apple.developer.icloud-services' "$out/entitlements.plist"
      /usr/libexec/PlistBuddy -c 'Add :com.apple.developer.icloud-services array' \
        -c 'Add :com.apple.developer.icloud-services:0 string CloudDocuments' "$out/entitlements.plist"
    fi
    # What the App Store refuses in a signature, as Xcode would leave it:
    # no development containers, no wildcards (Haven uses neither the
    # key-value store nor a shared keychain), and the production iCloud.
    for key in com.apple.developer.icloud-container-development-container-identifiers \
      com.apple.developer.ubiquity-kvstore-identifier keychain-access-groups \
      com.apple.developer.icloud-container-environment; do
      /usr/libexec/PlistBuddy -c "Delete :$key" "$out/entitlements.plist" 2>/dev/null || true
    done
    if /usr/libexec/PlistBuddy -c 'Print :com.apple.developer.icloud-container-identifiers' "$out/entitlements.plist" >/dev/null 2>&1; then
      /usr/libexec/PlistBuddy -c 'Add :com.apple.developer.icloud-container-environment string Production' "$out/entitlements.plist"
    fi
    for framework in "$app"/Frameworks/*.framework; do
      codesign --force --sign "$HAVEN_IOS_SIGN_IDENTITY" --timestamp "$framework"
    done
    codesign --force --sign "$HAVEN_IOS_SIGN_IDENTITY" --entitlements "$out/entitlements.plist" --timestamp "$app"
    rm -f "$out/profile.plist" "$out/entitlements.plist"
    signed="signed for the App Store"
  else
    signed="NOT signed: set HAVEN_IOS_SIGN_IDENTITY and HAVEN_IOS_PROFILE to sign it"
  fi
  mkdir -p "$out/Payload" && cp -R "$app" "$out/Payload/"
  (cd "$out" && zip -qry "$(basename "$ipa")" Payload)
  rm -rf "$out/Payload"
  echo "Made $app and $ipa ($signed)"
else
  ipa="$out/Haven-$version-ios-unsigned.ipa"
  mkdir -p "$out/Payload" && cp -R "$app" "$out/Payload/"
  (cd "$out" && zip -qry "$(basename "$ipa")" Payload)
  rm -rf "$out/Payload"
  echo "Made $app and $ipa (unsigned)"
fi
