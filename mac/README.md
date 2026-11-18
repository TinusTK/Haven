# Haven for macOS

- `fetch-libmpv.sh`: fills `libmpv/` with the LGPL video library and the
  libraries it loads, for Intel and Apple silicon.
- `bundle.sh [universal]`: builds Haven, makes `Haven.app` (the libraries
  inside it) and `Haven-<version>-macos.dmg` in `app/target/macos`.
- `Info.plist`: the app's details; its identifier is `net.haven.app`.

With `HAVEN_SIGN_IDENTITY` set to a Developer ID certificate in the
keychain, `bundle.sh` signs the app with it; with `HAVEN_NOTARY_PROFILE`
set as well, it has Apple notarise the disk image (see the top of the
script). Without them the app is signed "ad hoc" and macOS warns the first
time it is opened (right-click Haven > Open). The CI build is ad hoc.
