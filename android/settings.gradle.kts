// The Android application, over the same core the desktop runs.
//
// One module. A phone application that needs a module graph before it has a
// screen is one where the graph was the interesting part, and here the
// interesting part is entirely in Rust.

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "capture"
include(":app")
