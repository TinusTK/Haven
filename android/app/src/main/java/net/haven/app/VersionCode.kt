package net.haven.app

/**
 * Android's version code for a Haven version and build number:
 * 0.2.6 build 3 -> 20020603. Above every code the Rust app used
 * (`(1 << 24) | major << 16 | minor << 8 | patch`, 16777734 for 0.2.6) and
 * rising with each build and version, so every "0.2.6 build N" can go to
 * Google Play. app/build.gradle.kts works it out the same way.
 */
object VersionCode {
    fun of(version: String, build: Int): Int {
        val (major, minor, patch) = version.split(".").map { it.toInt() }
        require(build in 0..99 && patch in 0..99 && minor in 0..99) { "out of range: $version build $build" }
        return 20_000_000 + major * 1_000_000 + minor * 10_000 + patch * 100 + build
    }

    /** What the Rust + Slint build (cargo-apk) gave a version. */
    fun rust(version: String): Int {
        val (major, minor, patch) = version.split(".").map { it.toInt() }
        return (1 shl 24) or (major shl 16) or (minor shl 8) or patch
    }
}
