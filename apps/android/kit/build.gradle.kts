// The Kotlin layer the screens use (D52 §4): sessions, items, errors and the
// view models, in plain Kotlin so the tests run on the JVM against the host
// build of the core.
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.kotlin.jvm)
}

val repo: File = rootDir.parentFile.parentFile

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

dependencies {
    api(project(":core"))
    api(libs.coroutines.core)
    implementation(libs.serialization.json)
    compileOnly(libs.jna)
    testImplementation(libs.junit)
    testImplementation(libs.coroutines.test)
    testImplementation(libs.jna)
}

tasks.test {
    dependsOn(":core:cargoHost")
    // The host build of the core, found by JNA by name.
    systemProperty("jna.library.path", File(repo, "target/debug").path)
    // Against a real server when PWM_TEST_SERVER is set (as the Swift tests).
    environment("PWM_TEST_SERVER", System.getenv("PWM_TEST_SERVER") ?: "")
    testLogging { events("failed"); exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL }
}
