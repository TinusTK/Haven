# Haven for Android

- `res/`: the launcher icons.

The APK is built from `app/` with `cargo apk build -p haven-app --lib --release`
(Android SDK, an NDK and `cargo-apk` are needed; the settings are under
`[package.metadata.android]` in `app/crates/haven-app/Cargo.toml`). The
app's identifier is `net.haven.app`. On a phone the pages are laid out
for a narrow screen with tabs along the bottom, and videos open in a player
app on the device. The APK on GitHub has add-ons; a build for Google Play
is made with the cargo feature `store`, which leaves them out.

Released APKs are signed with Haven's own key, so a newer one installs
over an older one. The key is not in the repository: CI reads it from
the repository's secrets (`ANDROID_KEYSTORE_BASE64`,
`ANDROID_KEYSTORE_PASSWORD`); for a build by hand, point
`CARGO_APK_RELEASE_KEYSTORE` and `CARGO_APK_RELEASE_KEYSTORE_PASSWORD`
at it.
