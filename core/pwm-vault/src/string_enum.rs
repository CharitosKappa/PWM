//! String-valued enums that keep values from newer app versions.

/// Declares an enum serialized as a string. Values this version does not know
/// become `Other(String)` and are written back unchanged (decision D18).
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum $name {
            $($(#[$vmeta])* $variant,)+
            /// A value from a newer app version, kept as-is.
            Other(String),
        }

        impl $name {
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $text,)+
                    Self::Other(s) => s,
                }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                Ok(match s.as_str() {
                    $($text => Self::$variant,)+
                    _ => Self::Other(s),
                })
            }
        }

        impl zeroize::Zeroize for $name {
            fn zeroize(&mut self) {
                if let Self::Other(s) = self {
                    s.zeroize();
                }
            }
        }
    };
}

pub(crate) use string_enum;
