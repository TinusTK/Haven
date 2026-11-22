#!/bin/bash
# Puts what the Android player needs into android/libmpv/arm64-v8a:
# libmpv.so for 64-bit Android phones. It is packed into the APK beside
# Haven's own library, never built into it.
#
#   android/fetch-libmpv.sh             # download (needs gh, signed in)
#   android/fetch-libmpv.sh archive.jar # use an archive already here
#
# The archive is full-arm64-v8a.jar from
# https://github.com/media-kit/libmpv-android-video-build (release v1.1.11,
# mpv 0.36 with FFmpeg 6.0 and MediaCodec). A copy is kept as a release of
# Haven's own repository, so a build does not depend on theirs staying up.
# To move to a newer one: upload it as a new pre-release and change the
# three values below.
set -euo pipefail

TAG="libmpv-android-1.1.11"
ARCHIVE="full-arm64-v8a.jar"
SHA256="cdb54c5cf24725623ca717bbbd6d991031d625a377460bd128f19c2dffe189bd"

here="$(cd "$(dirname "$0")" && pwd)"
dest="$here/libmpv/arm64-v8a"
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
unzip -q "$work/$ARCHIVE" -d "$work/unpacked"
rm -f "$dest"/*.so
# Only libmpv itself: the archive's other library is a helper for
# another project's Java code.
cp "$work/unpacked/lib/arm64-v8a/libmpv.so" "$dest/"

# Haven is linked against a stand-in that has only mpv's own functions
# (and the one that hands libmpv the Java machine). libmpv.so also carries
# FreeType and HarfBuzz, and linking against all of it would have Slint's
# text drawing pick up some of those instead of its own copies.
ndk="${ANDROID_NDK_ROOT:-${ANDROID_NDK_HOME:-}}"
if [ -z "$ndk" ] && [ -n "${ANDROID_HOME:-}" ]; then
  ndk="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
fi
bin="$(ls -d "$ndk"/toolchains/llvm/prebuilt/*/bin 2>/dev/null | head -1)"
[ -x "$bin/llvm-ifs" ] || { echo "No Android NDK found: set ANDROID_NDK_ROOT." >&2; exit 1; }
stub="$here/libmpv/stub"
mkdir -p "$stub"
{
  echo "--- !ifs-v1"
  echo "IfsVersion: 3.0"
  echo "SoName: libmpv.so"
  echo "Target: { ObjectFormat: ELF, Arch: AArch64, Endianness: little, BitWidth: 64 }"
  echo "Symbols:"
  "$bin/llvm-nm" -D --defined-only "$dest/libmpv.so" | awk '$2 == "T" && ($3 ~ /^mpv_/ || $3 == "av_jni_set_java_vm") { print "  - { Name: " $3 ", Type: Func }" }'
  echo "..."
} > "$work/libmpv.ifs"
"$bin/llvm-ifs" --output-elf="$stub/libmpv.so" "$work/libmpv.ifs"
echo "Ready in $dest (and the stand-in for linking in $stub)"
