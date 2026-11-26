# Haven for Android

**The Kotlin app (on its way in):** a Gradle project here, written in
Kotlin with Jetpack Compose for phones, tablets and Android TV. Same
package (`net.haven.app`), same data, and it takes over from the Rust
build below once it does everything that one does.

    ./fetch-libmpv.sh                      # libmpv for the player
    ./gradlew assembleGithubDebug          # or assemblePlayDebug
    ./gradlew testGithubDebugUnitTest

Needs JDK 21 (`JAVA_HOME`), the Android SDK (`local.properties`, not in
git) and NDK 27.2.12479018. The version and build number are in
`gradle.properties`; the version code comes from both (0.2.6 build 3 is
20020603), so every build can go to Google Play. `scripts/make-banner.swift`
draws the Android TV banner.

**The Rust app (until the Kotlin one replaces it):**

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
