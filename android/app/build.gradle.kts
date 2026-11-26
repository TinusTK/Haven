import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

val havenVersion = providers.gradleProperty("havenVersion").get()
val havenBuild = providers.gradleProperty("havenBuild").get().toInt()

/** 0.2.6 build 3 -> 20020603: above the Rust app's codes (16777734 for
 *  0.2.6) and rising with every build and version, so each one can go to
 *  Google Play. Kept in step with VersionCode.kt (tested there). */
fun versionCodeOf(version: String, build: Int): Int {
    val (major, minor, patch) = version.split(".").map { it.toInt() }
    return 20_000_000 + major * 1_000_000 + minor * 10_000 + patch * 100 + build
}

/** Build keys (Trakt, Simkl, OMDb; never a TMDB key): from the environment
 *  (CI secrets, or `set -a; . Local/build-keys.env; set +a` here). */
fun key(name: String): String = (System.getenv(name) ?: "").replace("\"", "")

android {
    namespace = "net.haven.app"
    compileSdk = 36
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "net.haven.app"
        minSdk = 26
        targetSdk = 36
        versionName = havenVersion
        versionCode = versionCodeOf(havenVersion, havenBuild)
        buildConfigField("int", "HAVEN_BUILD", "$havenBuild")
        buildConfigField("String", "TRAKT_CLIENT_ID", "\"${key("HAVEN_TRAKT_CLIENT_ID")}\"")
        buildConfigField("String", "TRAKT_CLIENT_SECRET", "\"${key("HAVEN_TRAKT_CLIENT_SECRET")}\"")
        buildConfigField("String", "SIMKL_CLIENT_ID", "\"${key("HAVEN_SIMKL_CLIENT_ID")}\"")
        buildConfigField("String", "OMDB_API_KEY", "\"${key("HAVEN_OMDB_API_KEY")}\"")
        // 64-bit phones and TVs only, as the Rust build (libmpv is arm64).
        ndk { abiFilters += "arm64-v8a" }
        externalNativeBuild { cmake { arguments += "-DANDROID_STL=none" } }
    }

    // Two builds: the GitHub APK has everything; the Google Play one has
    // the Stremio sign-in (which brings the account's add-ons in) but no
    // Add-ons page and no installing by address.
    flavorDimensions += "store"
    productFlavors {
        create("github") {
            dimension = "store"
            // The Add-ons page and installing an add-on by its address.
            buildConfigField("boolean", "ADDON_SCREENS", "true")
        }
        create("play") {
            dimension = "store"
            // Google Play: add-ons only through the Stremio sign-in.
            buildConfigField("boolean", "ADDON_SCREENS", "false")
        }
    }

    // Haven's Android key (the same one the Rust APK is signed with, so
    // the Kotlin app updates it) for the GitHub APK; the Play upload key
    // for the Play build. From the environment (CI) or
    // ~/.config/haven/android/<name>.properties here; without them a
    // release build is left unsigned.
    fun signing(name: String, envFile: String, envPassword: String) {
        val file = System.getenv(envFile)
        val password = System.getenv(envPassword)
        val local = File(System.getProperty("user.home"), ".config/haven/android/$name.properties")
        val props = Properties().apply { if (local.isFile) local.inputStream().use { load(it) } }
        val store = file ?: props.getProperty("storeFile")
        val pass = password ?: props.getProperty("storePassword")
        if (store != null && pass != null) {
            signingConfigs.create(name) {
                storeFile = File(store)
                storePassword = pass
                keyAlias = props.getProperty("keyAlias") ?: System.getenv("${envPassword}_ALIAS") ?: "haven"
                keyPassword = props.getProperty("keyPassword") ?: pass
            }
        }
    }
    signing("github", "HAVEN_ANDROID_KEYSTORE", "HAVEN_ANDROID_KEYSTORE_PASSWORD")
    signing("play", "HAVEN_PLAY_KEYSTORE", "HAVEN_PLAY_KEYSTORE_PASSWORD")
    productFlavors.getByName("github").signingConfig = signingConfigs.findByName("github")
    productFlavors.getByName("play").signingConfig = signingConfigs.findByName("play")

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    sourceSets["main"].apply {
        // Haven's icons, shared with the Rust build until it goes.
        res.srcDirs("../res", "src/main/res")
        // libmpv.so, from ../fetch-libmpv.sh (arm64-v8a/).
        jniLibs.srcDirs("../libmpv-jni")
        // What's New, from the Rust build's file (one copy for every build).
        assets.srcDirs(layout.buildDirectory.dir("generated/haven-assets"))
    }

    externalNativeBuild {
        cmake { path = file("src/main/cpp/CMakeLists.txt") }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures { compose = true; buildConfig = true }
    testOptions { unitTests.isReturnDefaultValues = true }
}

// Compiled for Java 17 by whichever JDK runs Gradle (21 here and in CI).
kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }

// jniLibs wants ABI folders only: libmpv's arm64-v8a, without the Rust
// build's link stand-in (../libmpv/stub).
val copyLibmpv by tasks.registering(Copy::class) {
    from("../libmpv/arm64-v8a") { include("libmpv.so") }
    into("../libmpv-jni/arm64-v8a")
}
val copyWhatsNew by tasks.registering(Copy::class) {
    from("../../app/crates/haven-app/whats-new.json")
    into(layout.buildDirectory.dir("generated/haven-assets"))
}
tasks.named("preBuild") { dependsOn(copyLibmpv, copyWhatsNew) }

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2025.05.00")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.0")
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.8.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    // Chromecast: Google's Cast SDK, and MediaRouter to find the devices
    implementation("com.google.android.gms:play-services-cast-framework:22.1.0")
    implementation("androidx.mediarouter:mediarouter:1.7.0")
    // SQLite with the app: the shared database's tables are STRICT, which
    // Android's own SQLite reads only from Android 14 on.
    implementation("androidx.sqlite:sqlite:2.5.1")
    implementation("androidx.sqlite:sqlite-bundled:2.5.1")
    debugImplementation("androidx.compose.ui:ui-tooling")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
    testImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
    // Unit tests run on the computer: the bundled SQLite's desktop build.
    testImplementation("androidx.sqlite:sqlite-bundled-jvm:2.5.1")
}

// The Android build of the bundled SQLite cannot load on a computer.
configurations.matching { it.name.endsWith("UnitTestRuntimeClasspath") }.configureEach {
    exclude(group = "androidx.sqlite", module = "sqlite-bundled-android")
}
