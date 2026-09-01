plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "dev.capture"
    compileSdk = 34

    defaultConfig {
        applicationId = "dev.capture"
        // 30 rather than lower: the Quick Settings tile is the surface that has
        // to be faster than opening anything, and its lock-screen behaviour
        // settled around this level. Supporting older devices would mean a
        // second capture path for the one that matters most.
        minSdk = 30
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        ndk {
            // Only what can actually be run: the emulator here is x86_64 and a
            // physical phone is arm64. Building the other two ABIs would triple
            // the link time for binaries nothing on this project's bench can
            // execute.
            abiFilters += listOf("x86_64", "arm64-v8a")
        }
    }

    // Built by `make android-kotlin` / `make android-build`, not by Gradle.
    //
    // Gradle invoking cargo would mean two build systems each believing they
    // own the artifact, and the one that loses caches a stale copy. The
    // Makefile is the single place that knows how to cross-compile, and this
    // only consumes what it produced.
    sourceSets {
        getByName("main") {
            jniLibs.srcDir("src/main/jniLibs")
        }
    }

    buildTypes {
        release {
            // Shrunk and stripped, because the debug build is not the thing
            // whose speed is worth knowing. §2.1 makes capture's cost the
            // point, and a measurement taken against an unstripped 74 MB
            // library per ABI is a measurement of the wrong program.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )

            // Signed with the debug key, deliberately and only so it can be
            // installed on a bench. This is not a distribution build and there
            // is no release key in this repository — putting one here is how
            // signing keys end up in version control.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    packaging {
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.activity.compose)
    // Named explicitly to raise it above the 1.1.0 that arrives transitively.
    // `registerForActivityResult` — how this asks for the microphone and for
    // location — needs 1.3.0 or newer, and lint is right that the older one on
    // the classpath is a runtime hazard rather than a warning to silence.
    implementation(libs.androidx.fragment)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.material3)
    implementation(libs.androidx.lifecycle.viewmodel.compose)

    // Sync runs here. Android has no resident process to put a daemon in, so
    // the scheduling the daemon does with a timer is done by the platform,
    // under constraints it enforces rather than ones an application asks for.
    implementation(libs.androidx.work)

    // Geofencing. §2.4.1 puts location cues on the phone because the phone is
    // the only device that can observe one; this is how it observes without
    // holding the radio open.
    implementation(libs.play.services.location)

    // uniffi's generated Kotlin calls the native library through JNA.
    implementation(libs.jna) { artifact { type = "aar" } }

    androidTestImplementation(libs.androidx.test.junit)
    androidTestImplementation(libs.androidx.test.runner)
}
