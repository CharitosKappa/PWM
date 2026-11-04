// The Android app (D52 §2): Compose screens over `:kit`, the Keystore, and
// the core built for the device ABIs with cargo-ndk before every build.
import com.github.jk1.license.filter.LicenseBundleNormalizer
import com.github.jk1.license.render.ReportRenderer
import com.github.jk1.license.render.TextReportRenderer
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.license.report)
}

val repo: File = rootDir.parentFile.parentFile
val rustLibs = layout.buildDirectory.dir("rustJniLibs")
val noticeAssets = layout.buildDirectory.dir("noticeAssets")
val abis = listOf("arm64-v8a", "armeabi-v7a", "x86_64")

android {
    // Placeholders until D1 names the product.
    namespace = "pwm.app"
    compileSdk = 36
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "invalid.pwm.app"
        minSdk = 28
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk { abiFilters += abis }
    }

    buildFeatures { compose = true }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets.getByName("main").jniLibs.directories.add(rustLibs.get().asFile.path)
    sourceSets.getByName("main").assets.directories.add(noticeAssets.get().asFile.path)
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

val cargoNdk = tasks.register<Exec>("cargoNdk") {
    description = "Builds pwm-ffi for the device ABIs (release, stripped)."
    workingDir = repo
    val out = rustLibs.get().asFile
    outputs.dir(out)
    val ndk = androidComponents.sdkComponents.ndkDirectory
    doFirst { environment("ANDROID_NDK_HOME", ndk.get().asFile.path) }
    commandLine(
        listOf("cargo", "ndk") + abis.flatMap { listOf("-t", it) } +
            listOf("-P", "28", "-o", out.path, "build", "--locked", "--release", "-p", "pwm-ffi"),
    )
}

tasks.named("preBuild") { dependsOn(cargoNdk, notices) }

// Every library the app ships is on the allow-list (D52 §7, S6):
// `checkLicense` fails on anything else.
licenseReport {
    configurations = arrayOf("releaseRuntimeClasspath")
    filters = arrayOf(LicenseBundleNormalizer())
    allowedLicensesFile = rootProject.file("allowed-licenses.json")
    // The Android libraries' notices, bundled into the app (D54 §7).
    // Typed as the base class: the licence check adds its own renderer to this list.
    renderers = arrayOf<ReportRenderer>(TextReportRenderer("android-libraries.txt"))
}

// Third-party notices in every app (S6, D54 §7): the Android libraries'
// from the licence report, and the Rust crates' and data files' when
// `just notices` has made them. Settings shows them.
val notices = tasks.register<Copy>("noticeAssets") {
    description = "Bundles the third-party notices into the app's assets."
    dependsOn("generateLicenseReport")
    from(layout.buildDirectory.file("reports/dependency-license/android-libraries.txt"))
    from(File(repo, "THIRD_PARTY_NOTICES")) {
        include("rust.html", "data.md", "data-autofill.md")
    }
    into(noticeAssets.map { it.dir("notices") })
}

dependencies {
    implementation(project(":kit"))
    implementation("net.java.dev.jna:jna:${libs.versions.jna.get()}@aar")
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.activity.compose)
    implementation(libs.core.ktx)
    // Scanning the Emergency Kit (D53 §3).
    implementation(libs.camera.core)
    implementation(libs.camera.camera2)
    implementation(libs.camera.lifecycle)
    implementation(libs.camera.view)
    // The lock timeout (D54 §5).
    implementation(libs.lifecycle.process)

    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test.junit4)
    androidTestImplementation(libs.androidx.test.ext)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.espresso.core)
    debugImplementation(libs.compose.ui.test.manifest)
}
