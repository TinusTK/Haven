# Haven for Android

- `res/`: the launcher icons.

The APK is built from `app/` with `cargo apk build -p haven-app --lib`
(Android SDK, an NDK and `cargo-apk` are needed; the settings are under
`[package.metadata.android]` in `app/crates/haven-app/Cargo.toml`). The
app's identifier is `net.haven.app`. On a phone the pages are laid out
for a narrow screen with tabs along the bottom, there is no Add-ons page,
and videos open in a player app on the device.
