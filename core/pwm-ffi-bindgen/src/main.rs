//! `cargo run -p pwm-ffi-bindgen -- generate --library <libpwm_ffi> --language swift|kotlin --out-dir <dir>`
//! (see `just bindings`). Kept apart from `pwm-ffi` so the CLI's
//! dependencies never reach the library the apps link.
#![forbid(unsafe_code)]

fn main() {
    uniffi::uniffi_bindgen_main();
}
