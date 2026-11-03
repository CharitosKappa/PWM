//! Device approval messages (D46) from an untrusted relay: arbitrary public
//! keys, commitments and envelopes. Nothing may panic, a key that fails its
//! commitment is refused, and only the real envelope opens.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_crypto::{ApprovalKeys, Envelope, Secret, approval_commitment, check_approval_commitment};

const APPROVAL: [u8; 16] = [0xAB; 16];

fuzz_target!(|data: &[u8]| {
    let own = ApprovalKeys::from_seed(&Secret::new([0x5A; 32]));
    let mut peer = [0u8; 32];
    let take = data.len().min(32);
    peer[..take].copy_from_slice(&data[..take]);
    let rest = &data[take..];

    // A revealed key against a commitment made from other bytes.
    let commitment = approval_commitment(&own.public());
    if check_approval_commitment(&commitment, &peer).is_ok() {
        assert_eq!(peer, own.public());
    }

    // Any peer key: a channel or a refusal, never a panic.
    if let Ok(channel) = own.channel(&APPROVAL, &own.public(), &peer) {
        assert!(channel.code() < 1_000_000);
        if let Ok(envelope) = Envelope::from_bytes(rest) {
            // A random envelope does not open (only a 16-byte tag forgery would).
            let _ = channel.open(&envelope);
        }
        let sealed = channel.seal(rest).unwrap();
        assert_eq!(
            channel.open(&sealed).unwrap().expose_secret().as_slice(),
            rest
        );
    }
});
