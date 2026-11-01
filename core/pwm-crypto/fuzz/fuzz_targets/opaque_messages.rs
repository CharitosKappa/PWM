//! Untrusted input the server parses at login (D26): OPAQUE messages from
//! clients, stored records and login state, server secrets and session
//! tokens. Never panics; garbage is rejected, never accepted as a login.
#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use pwm_crypto::{ServerSecrets, TokenKind, opaque_record, token_hash};

fn server() -> &'static ServerSecrets {
    static SERVER: OnceLock<ServerSecrets> = OnceLock::new();
    SERVER.get_or_init(|| ServerSecrets::generate().unwrap())
}

fuzz_target!(|data: &[u8]| {
    let id = [0x42; 16];
    let _ = server().register_respond(&id, data);
    let _ = opaque_record(data);
    let _ = server().login_respond(None, &id, data);
    let _ = server().login_respond(Some(data), &id, data);
    let (state, finalization) = data.split_at(data.len() / 2);
    // The state is sealed under the server's key: garbage never verifies.
    assert!(server().login_finish(state, finalization, &id).is_err());
    let _ = ServerSecrets::from_bytes(data);
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = token_hash(TokenKind::Access, text);
        let _ = token_hash(TokenKind::Refresh, text);
    }
});
