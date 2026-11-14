# libmpv for the Windows build

Haven's player needs libmpv. Nothing is kept here in git; the build needs
these two files in this folder:

- `mpv.lib`: the import library the build links against.
- `libmpv-2.dll`: the library itself, which the installer puts beside
  `haven.exe` (plus any other `.dll` put in this same folder).

## Getting them

Run `..\fetch-libmpv.ps1` (CI does this before the release build). It
downloads the archive named in the script from a release of Haven's own
repository, checks its SHA-256, copies `libmpv-2.dll` here and makes
`mpv.lib` from the functions the DLL exports. It needs `gh` (signed in),
7-Zip and the Visual Studio C++ build tools.

## Where the build comes from

The archive is `mpv-dev-lgpl-x86_64-*.7z` from
https://github.com/zhongfly/mpv-winbuild: mpv built with `-Dgpl=false` on
an FFmpeg configured without `--enable-gpl` and `--enable-nonfree` (FFmpeg
is built with `--enable-version3`, so it is LGPL version 3). That project
keeps releases for 30 days only, which is why a copy is kept as a release
of Haven's repository.

Haven is proprietary, so a GPL build must never be shipped with it. Most
ready-made Windows builds of mpv are GPL (the archives without `lgpl` in
the name are): only ever use an LGPL one.

## Moving to a newer libmpv

1. Download the new `mpv-dev-lgpl-x86_64-*.7z` and note its SHA-256.
2. Upload it as a new release of Haven's repository (tag
   `libmpv-win-<date>`).
3. Change `$Tag`, `$File` and `$Sha256` in `fetch-libmpv.ps1`, and the
   commits named in `THIRD-PARTY.md`.
