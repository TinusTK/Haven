# Haven for macOS

- `fetch-libmpv.sh`: fills `libmpv/` with the LGPL video library and the
  libraries it loads, for Intel and Apple silicon.
- `bundle.sh [universal]`: builds Haven, makes `Haven.app` (the libraries
  inside it) and `Haven-<version>-macos.dmg` in `app/target/macos`.
- `Info.plist`: the app's details; its identifier is `net.haven.app`.

The app is signed "ad hoc", without an Apple Developer ID: macOS warns the
first time it is opened (right-click Haven > Open).
