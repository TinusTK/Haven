# Haven for Android

A Gradle project, written in Kotlin with Jetpack Compose, for phones,
tablets and Android TV (package `net.haven.app`). It replaced the earlier
Rust build in 0.2.6 build 4 and installs over it, keeping its data.

    ./fetch-libmpv.sh                      # libmpv for the player
    ./gradlew assembleGithubDebug          # or assemblePlayDebug
    ./gradlew testGithubDebugUnitTest

Needs JDK 21 (`JAVA_HOME`), the Android SDK (`local.properties`, not in
git) and NDK 27.2.12479018. The version and build number are in
`gradle.properties`; the version code comes from both (0.2.6 build 4 is
20020604), so every build can go to Google Play. `scripts/make-banner.swift`
draws the Android TV banner.

Released APKs are signed with Haven's own key, so a newer one installs
over an older one; the Google Play bundle is signed with the upload key.
The keys are not in the repository: CI reads them from the repository's
secrets (`ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`,
`PLAY_UPLOAD_KEYSTORE_BASE64`, `PLAY_UPLOAD_KEYSTORE_PASSWORD`); for a build
by hand, point `HAVEN_ANDROID_KEYSTORE` / `HAVEN_ANDROID_KEYSTORE_PASSWORD`
(and `HAVEN_PLAY_KEYSTORE` / `HAVEN_PLAY_KEYSTORE_PASSWORD`) at them.
