// The generated UniFFI Kotlin over `pwm-ffi` (D52 §3): never edited. The
// host build of the library is made first, since the bindings are read from
// it; the Android libraries are built by `:app`.
import org.gradle.internal.os.OperatingSystem
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.kotlin.jvm)
}

val repo: File = rootDir.parentFile.parentFile
val os: OperatingSystem = OperatingSystem.current()
val hostLibrary = when {
    os.isWindows -> "pwm_ffi.dll"
    os.isMacOsX -> "libpwm_ffi.dylib"
    else -> "libpwm_ffi.so"
}
val generated = layout.buildDirectory.dir("generated/uniffi")

val cargoHost = tasks.register<Exec>("cargoHost") {
    description = "Builds pwm-ffi for this computer, for the bindings and the JVM tests."
    workingDir = repo
    commandLine("cargo", "build", "--locked", "-p", "pwm-ffi")
}

val uniffiKotlin = tasks.register<Exec>("uniffiKotlin") {
    description = "Generates the Kotlin bindings."
    dependsOn(cargoHost)
    workingDir = repo
    val out = generated.get().asFile
    outputs.dir(out)
    commandLine(
        "cargo", "run", "--locked", "-q", "-p", "pwm-ffi-bindgen", "--",
        "generate", "--library", "target/debug/$hostLibrary",
        "--language", "kotlin", "--out-dir", out.path, "--no-format",
    )
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
    sourceSets.main { kotlin.srcDir(uniffiKotlin) }
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

dependencies {
    // The app ships JNA's Android AAR; the JVM tests add the jar.
    compileOnly(libs.jna)
}
