#!/bin/bash
# Copies libmpv and the libraries it loads (FFmpeg, libass, …) into
# Haven.app/Frameworks and signs them: the device set for a phone, the
# simulator set for the simulator. They come from ../ios/libmpv, filled by
# ../ios/fetch-libmpv.sh (checksum-pinned, LGPL builds, kept as separate
# frameworks).
set -euo pipefail

case "$PLATFORM_NAME" in
  iphonesimulator) slice=simulator ;;
  *) slice=device ;;
esac
src="$SRCROOT/../ios/libmpv/$slice"
dest="$TARGET_BUILD_DIR/$FRAMEWORKS_FOLDER_PATH"

if [ ! -f "$src/Mpv.framework/Mpv" ]; then
  echo "error: libmpv is missing. Run ios/fetch-libmpv.sh first." >&2
  exit 1
fi

mkdir -p "$dest"
for framework in "$src"/*.framework; do
  name="$(basename "$framework")"
  rm -rf "$dest/$name"
  # No headers or module maps in the app.
  rsync -a --exclude Headers --exclude Modules "$framework/" "$dest/$name/"
  if [ "${CODE_SIGNING_ALLOWED:-YES}" = "YES" ] && [ -n "${EXPANDED_CODE_SIGN_IDENTITY:-}" ]; then
    codesign --force --sign "$EXPANDED_CODE_SIGN_IDENTITY" ${OTHER_CODE_SIGN_FLAGS:-} --preserve-metadata=identifier,entitlements "$dest/$name"
  fi
done
echo "Embedded $(ls -d "$src"/*.framework | wc -l | tr -d ' ') frameworks ($slice)"
