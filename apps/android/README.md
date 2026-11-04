# apps/android

The Android app (D52): Kotlin and Jetpack Compose over the Rust core.

| Module | What it holds |
| --- | --- |
| `:core` | The UniFFI Kotlin bindings, generated from the host build of `pwm-ffi`. Never edited. |
| `:kit` | The Kotlin layer the screens use: `Session` (the step loop over HTTP), `Item`, `PwmError`, `Redacted`, `KeyStore`, the design tokens and the error messages, and the screens' view models in `pwm.kit.ui` (D53). Plain JVM, so its tests run without Android. |
| `:app` | The Compose screens, the Activity and the platform pieces: `KeystoreKeys` (the device's keys sealed under an Android Keystore key), the kit scanner (CameraX), saving and printing the kit, and the clipboard. The core is built for `arm64-v8a`, `armeabi-v7a` and `x86_64` with `cargo-ndk` before every build. |

## Building

Needs Android Studio's JDK (or any JDK 21), the Android SDK with NDK 28, the Rust targets and `cargo-ndk`:

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install cargo-ndk --locked
```

Point Gradle at the SDK in `apps/android/local.properties` (not committed), with forward slashes:

```properties
sdk.dir=C:/Users/you/AppData/Local/Android/Sdk
```

Then, from the repository root:

```bash
just android          # JVM tests of :kit, a debug build, the licence check
just android-device   # the app's tests on a running emulator or a device
```

`:kit`'s server test runs when `PWM_TEST_SERVER` is set (see `docs/DEVELOPMENT.md`).

## Generated files

`kit/src/main/kotlin/pwm/kit/design/Tokens.kt` (from `design/tokens.json`) and `kit/src/main/kotlin/pwm/kit/Messages.kt` (from `design/messages.json`) are committed. `just android-generated` writes the tokens and `just messages` the messages, for both apps; `just check` fails when any is stale.

The messages are shared with the Apple app. Change them in `design/messages.json`, never in a generated file; `platforms` there holds one app's own wording where it names its own system (Face ID, the keychain).
