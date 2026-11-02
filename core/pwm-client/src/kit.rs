//! The Emergency Kit (D42): its PDF, and reading the QR code printed on it.

use pwm_crypto::{Secret, SecretKey};
use pwm_kit::{Kit, Paper};

use crate::ClientError;

/// The kit as PDF bytes, to save or print. `secret_key` is its printed form;
/// `server` is a self-hosted address, `None` for the hosted service;
/// `created` is today's date as `YYYY-MM-DD` in the user's time zone.
pub fn emergency_kit(
    email: &str,
    secret_key: &str,
    server: Option<&str>,
    account_id: &str,
    created: &str,
    paper: Paper,
) -> Result<Secret<Vec<u8>>, ClientError> {
    let secret_key = SecretKey::parse(secret_key).map_err(|_| ClientError::InvalidSecretKey)?;
    let kit = Kit {
        email,
        secret_key: &secret_key,
        server,
        account_id,
        created,
    };
    pwm_kit::kit_pdf(&kit, paper).map_err(ClientError::Kit)
}

/// What a scanned kit fills in on the sign-in screen. Never a password.
#[derive(Debug)]
pub struct ScannedKit {
    pub email: String,
    /// The Secret Key in its printed form.
    pub secret_key: Secret<String>,
    pub server: Option<String>,
}

/// Reads the text of a scanned QR code.
pub fn scan_kit(text: &str) -> Result<ScannedKit, ClientError> {
    let kit = pwm_kit::parse_kit_qr(text).map_err(ClientError::Kit)?;
    Ok(ScannedKit {
        email: kit.email,
        secret_key: kit.secret_key.to_text(),
        server: kit.server,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA";
    const ACCOUNT: &str = "0f8e2f4c-3a1b-4d5e-9f60-7a8b9c0d1e2f";

    #[test]
    fn makes_a_kit_and_reads_its_code() {
        let pdf = emergency_kit(
            "ada@example.com",
            KEY,
            None,
            ACCOUNT,
            "2026-11-02",
            Paper::A4,
        )
        .unwrap();
        assert!(pdf.expose_secret().starts_with(b"%PDF-"));
        let scanned = scan_kit(
            "pwm-kit:1?email=ada%40example.com&secret-key=A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA",
        )
        .unwrap();
        assert_eq!(scanned.email, "ada@example.com");
        assert_eq!(scanned.secret_key.expose_secret(), KEY);
        assert_eq!(scanned.server, None);
    }

    #[test]
    fn errors_have_stable_codes() {
        let code = |r: Result<Secret<Vec<u8>>, ClientError>| r.err().map(|e| e.code());
        assert_eq!(
            code(emergency_kit(
                "ada@example.com",
                "A1-NOPE",
                None,
                ACCOUNT,
                "2026-11-02",
                Paper::A4
            )),
            Some("invalid_secret_key")
        );
        assert_eq!(
            code(emergency_kit(
                "",
                KEY,
                None,
                ACCOUNT,
                "2026-11-02",
                Paper::A4
            )),
            Some("invalid_email")
        );
        assert_eq!(
            code(emergency_kit(
                "ada@example.com",
                KEY,
                Some("ftp://x"),
                ACCOUNT,
                "2026-11-02",
                Paper::A4
            )),
            Some("invalid_server")
        );
        assert_eq!(
            code(emergency_kit(
                "ada@example.com",
                KEY,
                None,
                "x",
                "2026-11-02",
                Paper::A4
            )),
            Some("invalid_id")
        );
        assert_eq!(
            code(emergency_kit(
                "ada@example.com",
                KEY,
                None,
                ACCOUNT,
                "today",
                Paper::A4
            )),
            Some("invalid_date")
        );
        assert_eq!(scan_kit("hello").err().map(|e| e.code()), Some("not_a_kit"));
        assert_eq!(
            scan_kit("pwm-kit:2?x=1").err().map(|e| e.code()),
            Some("kit_too_new")
        );
    }
}
