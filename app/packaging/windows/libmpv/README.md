# libmpv for the Windows build

Haven's player needs libmpv. Nothing is kept here in git; put these two
files in this folder to get a Windows build **with** the player:

- `mpv.lib`: the import library the build links against.
- `libmpv-2.dll`: the library itself, which the installer puts beside
  `haven.exe` (plus any other `.dll` it needs, in this same folder).

They must come from an **LGPL** build of mpv (built with `-Dgpl=false`,
on an FFmpeg configured without `--enable-gpl` and `--enable-nonfree`).
Haven is proprietary, so a GPL build must never be shipped with it. Most
ready-made Windows builds of mpv are GPL: check before using one, or
build it (for example with mpv-winbuild-cmake and its LGPL option).

If a download only has `libmpv-2.dll` and `mpv.def`, make the import
library with:

    lib /def:mpv.def /name:libmpv-2.dll /out:mpv.lib /machine:x64

Without these files CI still builds Haven, but with no player: streams
and channels are listed and cannot be played.
