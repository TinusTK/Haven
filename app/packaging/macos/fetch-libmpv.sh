#!/bin/bash
# Puts what the Mac player needs into packaging/macos/libmpv: libmpv.dylib
# and the libraries it loads (FFmpeg, libass and so on), an LGPL build for
# Intel and Apple silicon Macs. They are shipped inside Haven.app beside
# the program, never built into it.
#
#   packaging/macos/fetch-libmpv.sh                 # download (needs gh, signed in)
#   packaging/macos/fetch-libmpv.sh archive.tar.gz  # use an archive already here
#
# The archive is libmpv-libs_v0.7.3_macos-universal-video-full.tar.gz from
# https://github.com/media-kit/libmpv-darwin-build (mpv 0.36.0 built with
# -Dgpl=false, FFmpeg 6.0 built with --enable-version3 and without
# --enable-gpl). A copy is kept as a release of Haven's own repository, so
# a build does not depend on theirs staying up. To move to a newer one:
# upload it as a new pre-release and change the three values below.
set -euo pipefail

TAG="libmpv-mac-0.7.3"
ARCHIVE="libmpv-libs_v0.7.3_macos-universal-video-full.tar.gz"
SHA256="f570695d8ba8534835dd4cba0b6e275579c1c22c01a9da0442748ca37e2daac1"

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

mkdir -p "$work/unpacked" "$dest"
tar -xzf "$work/$ARCHIVE" -C "$work/unpacked"
rm -f "$dest"/*.dylib
find "$work/unpacked" -name '*.dylib' -exec cp {} "$dest/" \;
[ -f "$dest/libmpv.dylib" ] || { echo "No libmpv.dylib in the archive." >&2; exit 1; }
echo "Ready in $dest: $(ls "$dest"/*.dylib | wc -l | tr -d ' ') libraries"
