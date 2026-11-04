// The Android app (D52): `:core` is the generated UniFFI Kotlin over the Rust
// core, `:kit` the Kotlin layer and view models (plain JVM, tested without
// Android), `:app` the Compose screens and the platform pieces.
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

rootProject.name = "pwm-android"
include(":core", ":kit", ":app")
