//! The few CBOR (RFC 8949) items a passkey needs, written by hand: the COSE
//! key and the attestation object are fixed structures, and nothing is ever
//! decoded. Lengths use the shortest form (deterministic encoding, §4.2).

const UNSIGNED: u8 = 0;
const NEGATIVE: u8 = 1;
const BYTES: u8 = 2;
const TEXT: u8 = 3;
const MAP: u8 = 5;

fn head(out: &mut Vec<u8>, major: u8, value: u64) {
    let major = major << 5;
    match value {
        0..=23 => out.push(major | value as u8),
        24..=0xff => out.extend_from_slice(&[major | 24, value as u8]),
        0x100..=0xffff => {
            out.push(major | 25);
            out.extend_from_slice(&(value as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(major | 26);
            out.extend_from_slice(&(value as u32).to_be_bytes());
        }
        _ => {
            out.push(major | 27);
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
}

pub(crate) fn int(out: &mut Vec<u8>, value: i64) {
    if value >= 0 {
        head(out, UNSIGNED, value as u64);
    } else {
        // -1 - n is encoded as n.
        head(out, NEGATIVE, (-1 - value) as u64);
    }
}

pub(crate) fn bytes(out: &mut Vec<u8>, value: &[u8]) {
    head(out, BYTES, value.len() as u64);
    out.extend_from_slice(value);
}

pub(crate) fn text(out: &mut Vec<u8>, value: &str) {
    head(out, TEXT, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

pub(crate) fn map(out: &mut Vec<u8>, entries: u64) {
    head(out, MAP, entries);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(f: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut out = Vec::new();
        f(&mut out);
        out
    }

    // RFC 8949 Appendix A.
    #[test]
    fn rfc_8949_examples() {
        assert_eq!(enc(|o| int(o, 0)), [0x00]);
        assert_eq!(enc(|o| int(o, 23)), [0x17]);
        assert_eq!(enc(|o| int(o, 24)), [0x18, 0x18]);
        assert_eq!(enc(|o| int(o, 1000)), [0x19, 0x03, 0xe8]);
        assert_eq!(enc(|o| int(o, 1_000_000)), [0x1a, 0x00, 0x0f, 0x42, 0x40]);
        assert_eq!(
            enc(|o| int(o, 1_000_000_000_000)),
            [0x1b, 0x00, 0x00, 0x00, 0xe8, 0xd4, 0xa5, 0x10, 0x00]
        );
        assert_eq!(enc(|o| int(o, -1)), [0x20]);
        assert_eq!(enc(|o| int(o, -7)), [0x26]);
        assert_eq!(enc(|o| int(o, -100)), [0x38, 0x63]);
        assert_eq!(enc(|o| int(o, -1000)), [0x39, 0x03, 0xe7]);
        assert_eq!(enc(|o| bytes(o, &[1, 2, 3, 4])), [0x44, 1, 2, 3, 4]);
        assert_eq!(enc(|o| text(o, "IETF")), [0x64, b'I', b'E', b'T', b'F']);
        assert_eq!(enc(|o| map(o, 0)), [0xa0]);
        assert_eq!(enc(|o| map(o, 3)), [0xa3]);
    }

    #[test]
    fn long_byte_strings_use_longer_lengths() {
        assert_eq!(&enc(|o| bytes(o, &[0; 24]))[..2], [0x58, 24]);
        assert_eq!(&enc(|o| bytes(o, &[0; 300]))[..3], [0x59, 0x01, 0x2c]);
    }
}
