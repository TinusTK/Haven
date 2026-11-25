#!/bin/bash
# Signs a built Haven.app (iPhone or Apple TV) for the App Store with an
# Apple Distribution identity and an App Store provisioning profile: the
# profile goes in, its frameworks are signed, then the app with the
# entitlements the profile grants, made fit for the App Store.
#
#   iphone/scripts/sign-store.sh <Haven.app> <identity> <profile.mobileprovision>
set -euo pipefail

app="$1" identity="$2" profile="$3"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cp "$profile" "$app/embedded.mobileprovision"
# The entitlements the profile grants, as the signature must carry them.
security cms -D -i "$profile" > "$work/profile.plist"
/usr/libexec/PlistBuddy -x -c 'Print :Entitlements' "$work/profile.plist" > "$work/entitlements.plist"
ent="$work/entitlements.plist"
team="$(/usr/libexec/PlistBuddy -c 'Print :com.apple.developer.team-identifier' "$ent")"
bundle="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist")"

# A profile lists iCloud's services as "*"; the signature names the one
# Haven uses (iCloud Drive, for the backup).
if /usr/libexec/PlistBuddy -c 'Print :com.apple.developer.icloud-services' "$ent" >/dev/null 2>&1; then
  /usr/libexec/PlistBuddy -c 'Delete :com.apple.developer.icloud-services' "$ent"
  /usr/libexec/PlistBuddy -c 'Add :com.apple.developer.icloud-services array' \
    -c 'Add :com.apple.developer.icloud-services:0 string CloudDocuments' "$ent"
fi
# What the App Store refuses in a signature: no development containers,
# no wildcards, and the production iCloud.
for key in com.apple.developer.icloud-container-development-container-identifiers \
  keychain-access-groups com.apple.developer.icloud-container-environment; do
  /usr/libexec/PlistBuddy -c "Delete :$key" "$ent" 2>/dev/null || true
done
# Apple TV has no iCloud Drive (the backup is iPhone and iPad only): it
# keeps just the key-value storage below.
if plutil -extract CFBundleSupportedPlatforms json -o - "$app/Info.plist" | grep -q AppleTVOS; then
  for key in com.apple.developer.icloud-services com.apple.developer.icloud-container-identifiers \
    com.apple.developer.ubiquity-container-identifiers; do
    /usr/libexec/PlistBuddy -c "Delete :$key" "$ent" 2>/dev/null || true
  done
fi
if /usr/libexec/PlistBuddy -c 'Print :com.apple.developer.icloud-container-identifiers' "$ent" >/dev/null 2>&1; then
  /usr/libexec/PlistBuddy -c 'Add :com.apple.developer.icloud-container-environment string Production' "$ent"
fi
# iCloud key-value storage (media servers, Live TV and accounts shared
# between devices): the profile grants "<team>.*"; the signature names
# Haven's own store, the same for the iPhone and Apple TV apps.
if /usr/libexec/PlistBuddy -c 'Print :com.apple.developer.ubiquity-kvstore-identifier' "$ent" >/dev/null 2>&1; then
  /usr/libexec/PlistBuddy -c "Set :com.apple.developer.ubiquity-kvstore-identifier $team.$bundle" "$ent"
fi

for framework in "$app"/Frameworks/*.framework; do
  [ -d "$framework" ] || continue
  codesign --force --sign "$identity" --timestamp "$framework"
done
codesign --force --sign "$identity" --entitlements "$ent" --timestamp "$app"
