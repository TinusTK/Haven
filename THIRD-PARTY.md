# Third-party components

Haven is free software, copyright © 2026 TinusTK, licensed under the GNU
General Public License version 3 (see LICENSE). It is built with, and ships beside,
components that keep their own licences:

- **Slint** (user interface), used under the Slint Royalty-free Desktop,
  Mobile and Web Applications License. That licence asks for an
  attribution, which Haven shows on its Settings page ("Made with Slint").
  `app/vendor/i-slint-core` is a copy of Slint's core crate (1.18.1, same
  licence, its `LICENSES` folder kept) with two small changes to touch
  scrolling, listed in `app/vendor/README.md`.
- **mpv / libmpv** (video playback), LGPL-2.1-or-later when built without
  its GPL parts. Haven loads libmpv as a separate shared library
  (`libmpv-2.dll` on Windows, `libmpv.so.2` on Linux) that can be replaced
  by the user. Only an LGPL build may be shipped with Haven; never a GPL
  build (Homebrew's and most distributions' default builds are GPL once
  combined with a GPL FFmpeg).
- **FFmpeg** (inside libmpv), LGPL when configured without
  `--enable-gpl` and `--enable-nonfree`: version 2.1-or-later, or version
  3-or-later when built with `--enable-version3`.
- **The Windows `libmpv-2.dll`** shipped with Haven is the LGPL build
  `mpv-dev-lgpl-x86_64-20261113-git-fe753a7824` from
  https://github.com/zhongfly/mpv-winbuild (build scripts based on
  https://github.com/shinchiro/mpv-winbuild-cmake): mpv commit
  `fe753a7824` (https://github.com/mpv-player/mpv) built with
  `-Dgpl=false`, with FFmpeg commit `88c46fce2`
  (https://github.com/FFmpeg/FFmpeg) and the other libraries it uses
  linked inside the DLL. FFmpeg there is built with `--enable-version3`,
  so it is under LGPL version 3. The source code of each is at the
  addresses above.
- **mpv and FFmpeg on macOS** (`libmpv.dylib` and the libraries beside
  it in `Haven.app/Contents/Frameworks`), from
  `libmpv-libs_v0.7.3_macos-universal-video-full.tar.gz` of
  https://github.com/media-kit/libmpv-darwin-build: mpv 0.36.0 built
  with `-Dgpl=false` (LGPL-2.1 or later) and FFmpeg 6.0 built with
  `--enable-version3` and without `--enable-gpl` (LGPL version 3).
  With them, each a separate library under its own licence: libass
  (ISC), FreeType (FreeType Licence), HarfBuzz (MIT), FriBidi
  (LGPL-2.1), dav1d (BSD 2-Clause), Mbed TLS (Apache-2.0), libxml2
  (MIT), libpng (libpng licence) and uchardet (MPL-1.1). The source of
  each is with that project.
- **Vulkan loader** (`vulkan-1.dll`, Windows only), from the official
  Vulkan Runtime 1.4.363.0 components published by LunarG
  (https://vulkan.lunarg.com). Copyright The Khronos Group Inc., LunarG,
  Inc. and Valve Corporation; Apache-2.0 and MIT. Its licence file is
  installed beside it as `VulkanRT-License.txt`. libmpv needs it to load.
- **Figtree** (Copyright 2022 The Figtree Project Authors,
  https://github.com/erikdkennedy/figtree) and **Pixelify Sans**
  (Copyright 2021 The Pixelify Sans Project Authors,
  https://github.com/eifetx/Pixelify-Sans), the fonts built into Haven,
  both under the SIL Open Font License 1.1. The copies in
  `app/assets/fonts/` are modified: the digits 0 to 9 are taken out of
  their character maps (with `strip_digits.py` there), so numbers show
  in the system's font. Neither font declares a Reserved Font Name. The
  licence texts are `OFL-Figtree.txt` and `OFL-PixelifySans.txt`.
- **libmpv2** (Rust bindings to libmpv), LGPL-2.1.
- **SQLite** (public domain), and the Rust crates listed in
  `app/Cargo.lock`, each under its own permissive licence (MIT, Apache-2.0,
  BSD, ISC, Zlib, MPL-2.0 and similar).

The Windows installer puts the licence texts in the `licenses` folder
beside `haven.exe`: `LGPL-2.1.txt` (mpv), `LGPL-3.0.txt` with
`GPL-3.0.txt`, which it refers to (FFmpeg as built here), and
`VulkanRT-License.txt` beside `vulkan-1.dll`, and the two `OFL-…txt`
files (the fonts). `GPL-3.0.txt` is there only
because the LGPL version 3 text builds on it: nothing shipped with Haven
is under the GPL.

Before a public release: regenerate the full list of crate licences (for
example with `cargo about`) and have the LGPL points above checked.
