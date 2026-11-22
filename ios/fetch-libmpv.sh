#!/bin/bash
# Puts what the iPhone and iPad player needs into ios/libmpv: libmpv and
# the libraries it loads (FFmpeg, libass and so on) as frameworks, one set
# for devices and one for the simulator, shipped in Haven.app/Frameworks.
#
#   ios/fetch-libmpv.sh                 # download (needs gh, signed in)
#   ios/fetch-libmpv.sh archive.tar.gz  # use an archive already here
#
# The archive is libmpv-xcframeworks_v0.7.3_ios-universal-video-full.tar.gz
# from https://github.com/media-kit/libmpv-darwin-build, the same project
# and version as the Mac copy. Its simulator build has no sound output.
# A copy is kept as a pre-release of Haven's own repository, so a build
# does not depend on theirs staying up. To move to a newer one: upload it
# as a new pre-release and change the three values below.
set -euo pipefail

TAG="libmpv-ios-0.7.3"
ARCHIVE="libmpv-xcframeworks_v0.7.3_ios-universal-video-full.tar.gz"
SHA256="8d9068dd553b39f07ed9efe67f9acd212e7f7cebaba1caa39320e99ef85757a9"

here="$(cd "$(dirname "$0")" && pwd)"
dest="$here/libmpv"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ $# -ge 1 ]; then
  cp "$1" "$work/$ARCHIVE"
else
  gh release download "$TAG" --repo TinusTK/Haven --pattern "$ARCHIVE" --dir "$work"
fi
echo "$SHA256  $work/$ARCHIVE" | shasum -a 256 -c - >/dev/null \
  || { echo "The libmpv archive does not match its checksum." >&2; exit 1; }

mkdir -p "$work/unpacked"
tar -xzf "$work/$ARCHIVE" -C "$work/unpacked"
rm -rf "$dest"
mkdir -p "$dest/device" "$dest/simulator"
# Each .xcframework holds one .framework per platform: ios-arm64 for
# devices, ios-arm64_x86_64-simulator for the simulator.
while IFS= read -r xc; do
  cp -R "$xc"/ios-arm64/*.framework "$dest/device/"
  cp -R "$xc"/ios-arm64*-simulator/*.framework "$dest/simulator/"
done < <(find "$work/unpacked" -name '*.xcframework' -type d)

for slice in device simulator; do
  [ -f "$dest/$slice/Mpv.framework/Mpv" ] || { echo "No Mpv.framework in the archive." >&2; exit 1; }
  # Only for the linker, which looks for libmpv by that name; the program
  # records the framework's own name (@rpath/Mpv.framework/Mpv) and loads
  # that. This file is never copied into the app.
  ln -sf Mpv.framework/Mpv "$dest/$slice/libmpv.dylib"
done
echo "Ready in $dest: $(ls -d "$dest"/device/*.framework | wc -l | tr -d ' ') frameworks for devices and the simulator"
