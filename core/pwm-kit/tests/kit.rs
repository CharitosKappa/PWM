//! The Emergency Kit PDF and its QR code (D42 §5).

use std::path::PathBuf;

use pwm_crypto::{Secret, SecretKey};
use pwm_kit::{Kit, KitError, KitQr, Paper, kit_pdf, parse_kit_qr};

const ACCOUNT_ID: &str = "0f8e2f4c-3a1b-4d5e-9f60-7a8b9c0d1e2f";

fn secret_key() -> SecretKey {
    SecretKey::from_bytes(Secret::new(
        *b"\x01\x23\x45\x67\x89\xab\xcd\xef\xfe\xdc\xba\x98\x76\x54\x32\x10",
    ))
}

fn kit<'a>(key: &'a SecretKey, server: Option<&'a str>) -> Kit<'a> {
    Kit {
        email: "ada@example.com",
        secret_key: key,
        server,
        account_id: ACCOUNT_ID,
        created: "2026-11-02",
    }
}

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

/// Compares with the committed file; `PWM_UPDATE_GOLDEN=1` rewrites it.
fn check_golden(name: &str, bytes: &[u8]) {
    let path = golden(name);
    if std::env::var_os("PWM_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, bytes).unwrap();
    }
    let expected = std::fs::read(&path)
        .unwrap_or_else(|_| panic!("{name} is missing; run with PWM_UPDATE_GOLDEN=1"));
    assert!(
        expected == bytes,
        "{name} changed; run with PWM_UPDATE_GOLDEN=1 and review the PDF"
    );
}

#[test]
fn a4_and_letter_match_the_golden_files() {
    let key = secret_key();
    let a4 = kit_pdf(&kit(&key, None), Paper::A4).unwrap();
    check_golden("kit-a4.pdf", a4.expose_secret());
    let letter = kit_pdf(&kit(&key, Some("https://vault.example.org")), Paper::Letter).unwrap();
    check_golden("kit-letter-self-hosted.pdf", letter.expose_secret());
}

#[test]
fn the_same_input_gives_the_same_bytes() {
    let key = secret_key();
    let a = kit_pdf(&kit(&key, None), Paper::A4).unwrap();
    let b = kit_pdf(&kit(&key, None), Paper::A4).unwrap();
    assert_eq!(a.expose_secret(), b.expose_secret());
}

#[test]
fn the_pdf_is_well_formed() {
    let key = secret_key();
    let pdf = kit_pdf(&kit(&key, Some("https://vault.example.org")), Paper::A4).unwrap();
    let bytes = pdf.expose_secret();
    assert!(bytes.starts_with(b"%PDF-1.4\n"));
    assert!(bytes.ends_with(b"%%EOF\n"));
    let text = ascii(bytes);

    // startxref points at the xref table, and every offset in it at its object.
    let start: usize = text
        .rsplit("startxref\n")
        .next()
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(text[start..].starts_with("xref\n0 "));
    for (n, line) in text[start..]
        .lines()
        .skip(3)
        .take_while(|l| l.ends_with(" n "))
        .enumerate()
    {
        let offset: usize = line[..10].parse().unwrap();
        assert!(
            text[offset..].starts_with(&format!("{} 0 obj", n + 1)),
            "object {}",
            n + 1
        );
    }
    // The stream's /Length is its exact size.
    let at = text.find("/Length ").unwrap() + 8;
    let length: usize = text[at..]
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let body = text.find("stream\n").unwrap() + 7;
    assert_eq!(&text[body + length..body + length + 10], "\nendstream");
}

#[test]
fn the_page_holds_what_signing_in_needs() {
    let key = secret_key();
    let pdf = kit_pdf(&kit(&key, Some("https://vault.example.org")), Paper::A4).unwrap();
    let text = ascii(pdf.expose_secret());
    for needle in [
        "(Emergency Kit)",
        "(ada@example.com)",
        &format!("({})", key.to_text().expose_secret()),
        "(https://vault.example.org)",
        &format!("({ACCOUNT_ID})"),
        "2026-11-02",
        "(Master password)",
    ] {
        assert!(text.contains(needle), "missing {needle}");
    }
    // The hosted server is the default and is not printed.
    let hosted = kit_pdf(&kit(&key, None), Paper::A4).unwrap();
    assert!(!ascii(hosted.expose_secret()).contains("(Server)"));
}

#[test]
fn paper_sizes() {
    let key = secret_key();
    let a4 = kit_pdf(&kit(&key, None), Paper::A4).unwrap();
    assert!(ascii(a4.expose_secret()).contains("/MediaBox [0 0 595 842]"));
    let letter = kit_pdf(&kit(&key, None), Paper::Letter).unwrap();
    assert!(ascii(letter.expose_secret()).contains("/MediaBox [0 0 612 792]"));
}

#[test]
fn text_outside_the_standard_fonts_prints_as_a_question_mark() {
    let key = secret_key();
    let mut input = kit(&key, None);
    input.email = "zoë(ελ)\\@example.com";
    let pdf = kit_pdf(&input, Paper::A4).unwrap();
    let text = ascii(pdf.expose_secret());
    // ë is in Latin-1 (octal escape), Greek is not, and PDF string syntax is escaped.
    assert!(text.contains(r"(zo\353\(??\)\\@example.com)"), "{text}");
}

#[test]
fn long_values_wrap_and_stay_above_the_footer() {
    let key = secret_key();
    let email = format!("{}@example.com", "a".repeat(240));
    let server = format!("https://{}.example.org", "b".repeat(180));
    let mut input = kit(&key, Some(&server));
    input.email = &email;
    for paper in [Paper::A4, Paper::Letter] {
        kit_pdf(&input, paper).unwrap();
    }
}

/// Changes one field of a valid kit.
type Change = fn(&mut Kit<'_>);

#[test]
fn invalid_inputs_are_refused() {
    let key = secret_key();
    let cases: [(Change, KitError); 7] = [
        (|k| k.email = "", KitError::InvalidEmail),
        (|k| k.email = "ada\n@example.com", KitError::InvalidEmail),
        (
            |k| k.server = Some("ftp://vault.example.org"),
            KitError::InvalidServer,
        ),
        (
            |k| k.server = Some("https://vault.example.org/\u{7}"),
            KitError::InvalidServer,
        ),
        (|k| k.account_id = "not-an-id", KitError::InvalidAccountId),
        (|k| k.created = "2 November 2026", KitError::InvalidDate),
        (|k| k.created = "2026-13-02", KitError::InvalidDate),
    ];
    for (change, expected) in cases {
        let mut input = kit(&key, None);
        change(&mut input);
        assert_eq!(kit_pdf(&input, Paper::A4).err(), Some(expected));
    }
    let long = "a".repeat(255);
    let mut input = kit(&key, None);
    input.email = &long;
    assert_eq!(
        kit_pdf(&input, Paper::A4).err(),
        Some(KitError::InvalidEmail)
    );
}

#[test]
fn the_qr_payload_round_trips() {
    let key = secret_key();
    for server in [None, Some("http://localhost:8080")] {
        let mut input = kit(&key, server);
        input.email = "ada+kit@example.com";
        let text = input.qr_payload().unwrap();
        assert!(
            text.expose_secret()
                .starts_with("pwm-kit:1?email=ada%2Bkit%40example.com&secret-key=A1-")
        );
        let back = parse_kit_qr(text.expose_secret()).unwrap();
        assert_eq!(back.email, "ada+kit@example.com");
        assert_eq!(
            back.secret_key.to_text().expose_secret(),
            key.to_text().expose_secret()
        );
        assert_eq!(back.server.as_deref(), server);
    }
}

#[test]
fn the_qr_payload_never_holds_a_password() {
    let key = secret_key();
    let text = kit(&key, None).qr_payload().unwrap();
    assert!(!text.expose_secret().contains("password"));
}

#[test]
fn qr_text_that_is_not_a_kit_is_refused() {
    let key = key_text();
    let ok = format!("pwm-kit:1?email=ada%40example.com&secret-key={key}");
    assert!(parse_kit_qr(&ok).is_ok());
    // Unknown fields from a later version-1 kit are ignored.
    assert!(parse_kit_qr(&format!("{ok}&colour=blue")).is_ok());

    let cases = [
        ("https://example.com", KitError::NotAKit),
        (
            "pwm-kit:2?email=ada%40example.com",
            KitError::UnsupportedVersion,
        ),
        ("pwm-kit:1", KitError::NotAKit),
        ("pwm-kit:1?email=ada%40example.com", KitError::NotAKit),
        (
            &format!("pwm-kit:1?secret-key={key}") as &str,
            KitError::NotAKit,
        ),
        (&format!("{ok}&email=eve%40example.com"), KitError::NotAKit),
        (
            &format!("pwm-kit:1?email=ada%4&secret-key={key}"),
            KitError::NotAKit,
        ),
        (
            &format!("pwm-kit:1?email=%FF&secret-key={key}"),
            KitError::NotAKit,
        ),
        (
            "pwm-kit:1?email=ada%40example.com&secret-key=A1-NOPE",
            KitError::InvalidSecretKey,
        ),
        (
            &format!("{ok}&server=ftp%3A%2F%2Fx"),
            KitError::InvalidServer,
        ),
        (
            &format!("pwm-kit:1?email=&secret-key={key}"),
            KitError::InvalidEmail,
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(parse_kit_qr(text).err(), Some(expected), "{text}");
    }
}

#[test]
fn scanned_kits_never_print_their_secret_key() {
    let key = key_text();
    let scanned: KitQr = parse_kit_qr(&format!(
        "pwm-kit:1?email=ada%40example.com&secret-key={key}"
    ))
    .unwrap();
    let debug = format!("{scanned:?}");
    assert!(!debug.contains(&key[3..9]), "{debug}");
    assert!(debug.contains("ada@example.com"));
}

/// The file as text, one char per byte (its only non-ASCII bytes are in the
/// binary marker comment), so string offsets are byte offsets.
fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if b.is_ascii() { char::from(b) } else { '?' })
        .collect()
}

fn key_text() -> String {
    secret_key().to_text().expose_secret().clone()
}

/// Redraws the PDF's filled rectangles (only the QR code uses them) as an
/// image and decodes it: the printed code must read back as the payload.
#[test]
fn the_printed_qr_code_scans() {
    let key = secret_key();
    for (paper, server) in [
        (Paper::A4, None),
        (Paper::Letter, Some("https://vault.example.org")),
    ] {
        let input = kit(&key, server);
        let pdf = kit_pdf(&input, paper).unwrap();
        let text = ascii(pdf.expose_secret());
        let rects: Vec<[f32; 4]> = text
            .lines()
            .filter(|l| l.ends_with(" re"))
            .map(|l| {
                let v: Vec<f32> = l
                    .split_whitespace()
                    .take(4)
                    .map(|n| n.parse().unwrap())
                    .collect();
                [v[0], v[1], v[2], v[3]]
            })
            .collect();
        assert!(!rects.is_empty());

        // Two pixels per point, over the QR area plus a white border.
        let scale = 2.0;
        let left = rects.iter().map(|r| r[0]).fold(f32::MAX, f32::min) - 16.0;
        let bottom = rects.iter().map(|r| r[1]).fold(f32::MAX, f32::min) - 16.0;
        let right = rects.iter().map(|r| r[0] + r[2]).fold(f32::MIN, f32::max) + 16.0;
        let top = rects.iter().map(|r| r[1] + r[3]).fold(f32::MIN, f32::max) + 16.0;
        let (w, h) = (
            ((right - left) * scale) as usize,
            ((top - bottom) * scale) as usize,
        );
        let mut pixels = vec![255u8; w * h];
        for r in &rects {
            let x0 = ((r[0] - left) * scale).round() as usize;
            let x1 = ((r[0] + r[2] - left) * scale).round() as usize;
            // Image rows go down; PDF y goes up.
            let y0 = ((top - r[1] - r[3]) * scale).round() as usize;
            let y1 = ((top - r[1]) * scale).round() as usize;
            for y in y0..y1.min(h) {
                pixels[y * w + x0..y * w + x1.min(w)].fill(0);
            }
        }
        let mut image = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| pixels[y * w + x]);
        let grids = image.detect_grids();
        assert_eq!(grids.len(), 1);
        let (_, decoded) = grids[0].decode().unwrap();
        assert_eq!(&decoded, input.qr_payload().unwrap().expose_secret());
    }
}
