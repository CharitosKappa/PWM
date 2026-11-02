//! `pwm-rules-sign`, run offline by the release owner (D34):
//!
//! ```text
//! pwm-rules-sign keygen <seed-file>                       # new key; prints its public key
//! pwm-rules-sign public <seed-file>                       # prints the public key again
//! pwm-rules-sign sign <seed-file> <key-id> <rules.json> <out.bin>
//! ```
//!
//! Only the public keys go into `RULES_KEYS` (by PR). Keep each seed file on
//! a hardware token or an offline backup; anyone with it can sign rules.
#![forbid(unsafe_code)]

use std::path::Path;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

fn run(args: &[String]) -> Result<String, String> {
    match args {
        [cmd, seed] if cmd == "keygen" => {
            let public = pwm_rules_sign::keygen(Path::new(seed))?;
            Ok(format!("public key: {public}\nKeep {seed} offline. Add the public key to RULES_KEYS by PR."))
        }
        [cmd, seed] if cmd == "public" => Ok(format!("public key: {}", pwm_rules_sign::public_key(Path::new(seed))?)),
        [cmd, seed, key_id, input, output] if cmd == "sign" => {
            let key_id: u8 = key_id.parse().map_err(|_| "the key id is a number from 0 to 255".to_owned())?;
            let json = std::fs::read(input).map_err(|e| format!("cannot read {input}: {e}"))?;
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis();
            let now_ms = i64::try_from(now_ms).map_err(|e| e.to_string())?;
            let signed = pwm_rules_sign::sign(Path::new(seed), key_id, &json, now_ms)?;
            std::fs::write(output, &signed).map_err(|e| format!("cannot write {output}: {e}"))?;
            Ok(format!("wrote {output} ({} bytes)", signed.len()))
        }
        _ => Err("usage: pwm-rules-sign keygen <seed> | public <seed> | sign <seed> <key-id> <rules.json> <out.bin>".to_owned()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
