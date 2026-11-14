# Third-party components

Haven is proprietary (see LICENSE). It is built with, and ships beside,
components that keep their own licences:

- **Slint** (user interface), used under the Slint Royalty-free Desktop,
  Mobile and Web Applications License. That licence asks for an
  attribution, which Haven shows on its Settings page ("Made with Slint").
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
- **libmpv2** (Rust bindings to libmpv), LGPL-2.1.
- **SQLite** (public domain), and the Rust crates listed in
  `app/Cargo.lock`, each under its own permissive licence (MIT, Apache-2.0,
  BSD, ISC, Zlib, MPL-2.0 and similar).

Before a public release: regenerate the full list of crate licences (for
example with `cargo about`) and have the LGPL points above checked.
