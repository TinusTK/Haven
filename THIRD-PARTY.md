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
- **FFmpeg** (inside libmpv), LGPL-2.1-or-later when configured without
  `--enable-gpl` and `--enable-nonfree`.
- **libmpv2** (Rust bindings to libmpv), LGPL-2.1.
- **SQLite** (public domain), and the Rust crates listed in
  `app/Cargo.lock`, each under its own permissive licence (MIT, Apache-2.0,
  BSD, ISC, Zlib, MPL-2.0 and similar).

Before a public release: regenerate the full list of crate licences (for
example with `cargo about`) and have the LGPL points above checked.
