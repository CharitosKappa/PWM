//! Times `derive_password_key` with the spec params (D15).
//!
//! Native:  cargo run --release -p pwm-crypto --example kdf_bench
//! WASM results are recorded under decision D15 in docs/DECISIONS.md.

use std::time::Instant;

use pwm_crypto::{KdfParams, derive_password_key};

fn main() {
    for (name, params) in [
        ("default 64 MiB t=3 p=4", KdfParams::DEFAULT),
        ("mobile floor 32 MiB t=4 p=4", KdfParams::MOBILE_FLOOR),
    ] {
        let runs = 3;
        let start = Instant::now();
        for _ in 0..runs {
            derive_password_key(b"benchmark password", &[7u8; 16], &params).expect("derive");
        }
        let ms = start.elapsed().as_millis() / runs;
        println!("{name}: {ms} ms per derivation");
    }
}
