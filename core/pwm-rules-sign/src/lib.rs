//! The offline side of the signed autofill rules (decisions D33, D34). The
//! release owner runs this on an offline machine: `keygen` makes a signing
//! key and prints its public half for `RULES_KEYS`; `sign` turns a rules
//! JSON into the signed file the extension downloads. The seed never leaves
//! that machine (or its hardware token / offline backup).
#![forbid(unsafe_code)]

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use pwm_autofill::verify_rules;
use pwm_crypto::{Secret, SignatureContext, public_key_from_seed, random_seed, sign_with_seed};

/// File format of a seed: 64 lowercase hex characters and a newline.
fn encode_seed(seed: &Secret<[u8; 32]>) -> Secret<String> {
    Secret::new(format!("{}\n", hex(seed.expose_secret())))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_seed(text: &str) -> Result<Secret<[u8; 32]>, String> {
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("the seed file is not 64 hex characters".to_owned());
    }
    let mut seed = [0u8; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&text[2 * i..2 * i + 2], 16).map_err(|_| "bad hex".to_owned())?;
    }
    Ok(Secret::new(seed))
}

/// Writes a new seed to `path` (which must not exist; owner-only on Unix)
/// and returns the public key as hex.
pub fn keygen(path: &Path) -> Result<String, String> {
    let seed = random_seed().map_err(|e| e.to_string())?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| {
        format!(
            "cannot create {}: {e} (an existing key is never overwritten)",
            path.display()
        )
    })?;
    file.write_all(encode_seed(&seed).expose_secret().as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(hex(&public_key_from_seed(&seed)))
}

/// The public key of the seed at `path`, as hex.
pub fn public_key(path: &Path) -> Result<String, String> {
    let text = Secret::new(std::fs::read_to_string(path).map_err(|e| e.to_string())?);
    Ok(hex(&public_key_from_seed(&decode_seed(
        text.expose_secret(),
    )?)))
}

/// Signs `json` with the seed at `path` as key `key_id`. The result is
/// checked with the same code the apps run, as of `now_ms`, so a file that
/// would be refused is never produced.
pub fn sign(path: &Path, key_id: u8, json: &[u8], now_ms: i64) -> Result<Vec<u8>, String> {
    let text = Secret::new(std::fs::read_to_string(path).map_err(|e| e.to_string())?);
    let seed = decode_seed(text.expose_secret())?;
    let message = [&[key_id][..], json].concat();
    let signature = sign_with_seed(&seed, SignatureContext::AutofillRules, &message);
    let signed = [&[1u8, key_id][..], &signature, json].concat();
    verify_rules(&signed, &[(key_id, public_key_from_seed(&seed))], now_ms, 0)
        .map_err(|e| format!("the signed file would be refused: {e}"))?;
    Ok(signed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pwm-rules-sign-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        dir
    }

    const NOW: i64 = 1_767_225_600_000;

    fn rules(expires: i64) -> Vec<u8> {
        format!(r#"{{"v":1,"version":3,"issued_at":{NOW},"expires_at":{expires},"sites":[{{"domain":"example.com","never_fill":true}}]}}"#).into_bytes()
    }

    #[test]
    fn keygen_sign_and_verify() {
        let path = temp("seed");
        let public = keygen(&path).unwrap();
        assert_eq!(public.len(), 64);
        assert_eq!(public_key(&path).unwrap(), public);
        // Never overwrites a key.
        assert!(keygen(&path).is_err());

        let signed = sign(&path, 1, &rules(NOW + 86_400_000), NOW).unwrap();
        let key: [u8; 32] = (0..32)
            .map(|i| u8::from_str_radix(&public[2 * i..2 * i + 2], 16).unwrap())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_eq!(
            verify_rules(&signed, &[(1, key)], NOW, 0)
                .unwrap()
                .version(),
            3
        );
        // A file the apps would refuse is not produced.
        assert!(
            sign(&path, 1, &rules(NOW - 1), NOW)
                .unwrap_err()
                .contains("refused")
        );
        assert!(sign(&path, 1, b"{}", NOW).is_err());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn bad_seed_files_are_refused() {
        let path = temp("bad");
        std::fs::write(&path, "not hex").unwrap();
        assert!(public_key(&path).is_err());
        assert!(sign(&path, 1, b"{}", NOW).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
