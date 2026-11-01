//! Item ids: 16 random bytes (UUIDv4), written as `8-4-4-4-12` hex in JSON.

use core::fmt;
use core::str::FromStr;

use pwm_crypto::{CryptoError, Id};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Id of an item or folder. Ids are not secret: the server stores them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(Id);

impl ItemId {
    pub fn generate() -> Result<Self, CryptoError> {
        pwm_crypto::generate_id().map(Self)
    }

    pub fn from_bytes(bytes: Id) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &Id {
        &self.0
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ItemId({self})")
    }
}

/// The id is not valid `8-4-4-4-12` hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseIdError;

impl fmt::Display for ParseIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid item id")
    }
}

impl FromStr for ItemId {
    type Err = ParseIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != 36 || [8, 13, 18, 23].iter().any(|&i| bytes[i] != b'-') {
            return Err(ParseIdError);
        }
        let mut digits = bytes.iter().filter(|&&c| c != b'-');
        let mut id = [0u8; 16];
        for byte in &mut id {
            let hi = hex_value(*digits.next().ok_or(ParseIdError)?)?;
            let lo = hex_value(*digits.next().ok_or(ParseIdError)?)?;
            *byte = (hi << 4) | lo;
        }
        Ok(Self(id))
    }
}

fn hex_value(c: u8) -> Result<u8, ParseIdError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(ParseIdError),
    }
}

impl Serialize for ItemId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ItemId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse()
            .map_err(|_| serde::de::Error::custom("invalid item id"))
    }
}
