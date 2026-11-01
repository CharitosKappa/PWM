//! Vault share decoding and acceptance: never panics, re-encodes losslessly,
//! and arbitrary input is never accepted.
#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use pwm_crypto::{AccountKeyset, VaultShare};

static RECIPIENT: LazyLock<AccountKeyset> = LazyLock::new(|| AccountKeyset::generate().unwrap());
static SENDER: LazyLock<AccountKeyset> = LazyLock::new(|| AccountKeyset::generate().unwrap());

fuzz_target!(|data: &[u8]| {
    if let Ok(share) = VaultShare::from_bytes(data) {
        assert_eq!(share.to_bytes(), data);
        let accepted = RECIPIENT.accept_vault_share(&share, &[1; 16], &SENDER.ed25519_public());
        assert!(accepted.is_err());
    }
});
