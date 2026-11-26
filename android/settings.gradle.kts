// Haven for Android, written in Kotlin with Jetpack Compose (phones,
// tablets and Android TV in one app). It replaces the Rust + Slint Android
// build (app/) with the same package and data; see HavenKotlinmemory.md.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}
rootProject.name = "Haven"
include(":app")
