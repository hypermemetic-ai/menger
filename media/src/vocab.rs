//! The service's typed parameters (ACCESS-8, `typed_params`) — what names
//! and carries an image: [`MediaHash`], [`MediaType`], [`MediaData`]. What
//! an image must be (a kept type, by its bytes; at most 7 MiB) is the
//! store's rule, refused in band naming the limit ([`crate::image::check`]);
//! these hold only the spelling. Each a transparent text whose
//! `Deserialize` is its one rule, so a bad value is `invalid_params` naming
//! the parameter before the method runs.

/// The most `data` may be on the wire: room for [`crate::image::MAX_BYTES`]
/// in base64 and more — an image past the cap is refused by the store, in
/// band, naming the cap, not here.
pub const MEDIA_DATA_WIRE_MAX: usize = 64 * 1024 * 1024;

macro_rules! text_type {
    ($(#[$doc:meta])* $name:ident, $rule:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, schemars::JsonSchema)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Its text, as it goes on the wire.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Its text, owned.
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(text: String) -> Result<Self, String> {
                let rule: fn(&str) -> Result<String, String> = $rule;
                rule(&text).map(Self)
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(text: &str) -> Result<Self, String> {
                let rule: fn(&str) -> Result<String, String> = $rule;
                rule(text).map(Self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::try_from(<String as serde::Deserialize>::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

fn not_a(text: &str, what: &str, shape: &str) -> String {
    format!("`{text}` is not {what}: {shape}")
}

fn hash(text: &str) -> Result<String, String> {
    let t = text.trim();
    let t = t.strip_prefix(crate::image::SCHEME).unwrap_or(t);
    match t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        true => Ok(t.to_ascii_lowercase()),
        false => Err(not_a(t, "an image's hash", "its SHA-256, 64 hex digits (as `media.put` answers it)")),
    }
}

text_type!(
    /// An image's name: its SHA-256, 64 hex digits (lower-cased; a
    /// `media:` before it is read past), as `media.put` answers it.
    MediaHash,
    hash
);

fn media_type(text: &str) -> Result<String, String> {
    let t = text.trim();
    let shape = "a media type, `type/subtype` (image/png, image/jpeg, image/gif or image/webp)";
    let token = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&b));
    let essence = t.split(';').next().unwrap_or("").trim();
    let ok = t.len() <= 255 && essence.split_once('/').is_some_and(|(a, b)| token(a) && token(b));
    match ok {
        true => Ok(t.to_ascii_lowercase()),
        false => Err(not_a(t, "a media type", shape)),
    }
}

text_type!(
    /// What an image is said to be: a media type, `type/subtype` (lower-
    /// cased). Which ones are kept, and whether the bytes are that, is the
    /// store's rule (`image/png`, `image/jpeg`, `image/gif`, `image/webp`).
    MediaType,
    media_type
);

fn data(text: &str) -> Result<String, String> {
    match text.len() <= MEDIA_DATA_WIRE_MAX {
        true => Ok(text.to_string()),
        false => Err(format!("an image's data is {} bytes; at most {MEDIA_DATA_WIRE_MAX}", text.len())),
    }
}

text_type!(
    /// An image's bytes, in standard base64 (`base64 -w0 shot.png`); a
    /// `data:image/png;base64,` before them is read past by the store.
    MediaData,
    data
);

#[cfg(test)]
mod tests {
    use super::*;

    fn holds<T: std::str::FromStr<Err = String> + std::fmt::Debug>(yes: &[&str], no: &[&str]) {
        for y in yes {
            assert!(y.parse::<T>().is_ok(), "{y:?} holds");
        }
        for n in no {
            assert!(n.parse::<T>().is_err(), "{n:?} is refused");
        }
    }

    #[test]
    fn the_spellings_hold_and_refuse() {
        let (hash, named, bad) = ("9f".repeat(32), format!("media:{}", "AB".repeat(32)), "g".repeat(64));
        holds::<MediaHash>(&[hash.as_str(), named.as_str()], &["", "abc", bad.as_str()]);
        assert_eq!(named.parse::<MediaHash>().unwrap().as_str(), "ab".repeat(32));
        holds::<MediaType>(&["image/png", "IMAGE/JPEG", "image/svg+xml", "text/plain; charset=utf-8"], &["", "png", "image/", "/png", "image png"]);
        assert_eq!("IMAGE/JPEG".parse::<MediaType>().unwrap().as_str(), "image/jpeg");
        holds::<MediaData>(&["", "iVBORw0KGgo="], &[]);
        assert!(serde_json::from_str::<MediaHash>("\"abc\"").is_err(), "deserializing is the check");
    }
}
