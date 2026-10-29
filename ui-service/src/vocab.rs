//! UI-3: the `ui` activation's vocabulary — every parameter a type (ACCESS-8,
//! `typed_params`), each a transparent text whose `Deserialize` is its one
//! rule, so a bad value is `invalid_params` naming the parameter before the
//! method runs.

use base64::Engine;

/// The most a bundle may be, in bytes: 16 MiB.
pub const BUNDLE_MAX: usize = 16 * 1024 * 1024;

/// The most a manifest may be, in bytes: 64 KiB.
pub const MANIFEST_MAX: usize = 64 * 1024;

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

fn b64(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(text.trim()).ok()
}

fn version_hash(text: &str) -> Result<String, String> {
    let t = text.trim();
    match t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        true => Ok(t.to_ascii_lowercase()),
        false => Err(format!("`{t}` is not a version's hash: its sha256, 64 hex digits (as `ui.push` answers it)")),
    }
}

text_type!(
    /// A version's name: the sha256 of its bundle's bytes, 64 hex digits
    /// (lower-cased), as `ui.push` answers it.
    VersionHash,
    version_hash
);

fn bundle(text: &str) -> Result<String, String> {
    // base64 is 4/3 of the bytes: past this, it cannot decode under the cap.
    if text.trim().len() > BUNDLE_MAX.div_ceil(3) * 4 {
        return Err(format!("a bundle is at most {BUNDLE_MAX} bytes ({} base64 characters given)", text.trim().len()));
    }
    match b64(text) {
        Some(bytes) if bytes.is_empty() => Err("a bundle is not empty".into()),
        Some(bytes) if bytes.len() > BUNDLE_MAX => Err(format!("a bundle is at most {BUNDLE_MAX} bytes ({} given)", bytes.len())),
        Some(_) => Ok(text.trim().to_string()),
        None => Err("a bundle is standard base64 of a tar archive (`base64 -w0 board.tar`)".into()),
    }
}

text_type!(
    /// A UI's bundle: a tar archive whose `index.html` is the entrypoint, in
    /// standard base64, at most 16 MiB of bytes. Opaque until the compiler
    /// exists.
    Bundle,
    bundle
);

impl Bundle {
    /// Its bytes (the rule checked they decode).
    pub fn bytes(&self) -> Vec<u8> {
        b64(&self.0).unwrap_or_default()
    }
}

fn manifest(text: &str) -> Result<String, String> {
    match text.len() {
        0 => Err("a manifest is not empty".into()),
        n if n > MANIFEST_MAX => Err(format!("a manifest is at most {MANIFEST_MAX} bytes ({n} given)")),
        _ => Ok(text.to_string()),
    }
}

text_type!(
    /// A bundle's manifest, as text (its `contract:` and `abi:` lines are
    /// kept; nothing here checks them), at most 64 KiB.
    Manifest,
    manifest
);

fn signature(text: &str) -> Result<String, String> {
    match b64(text) {
        Some(bytes) if bytes.len() == 64 => Ok(text.trim().to_string()),
        Some(bytes) => Err(format!("a signature is 64 bytes in base64 ({} given)", bytes.len())),
        None => Err("a signature is standard base64 of 64 bytes (Ed25519)".into()),
    }
}

text_type!(
    /// An Ed25519 signature over the bundle's sha256 digest, by a signer
    /// named at start (`--ui-signer`): 64 bytes in standard base64.
    Signature,
    signature
);

impl Signature {
    /// Its 64 bytes (the rule checked there are 64).
    pub fn bytes(&self) -> [u8; 64] {
        let mut out = [0u8; 64];
        if let Some(b) = b64(&self.0).filter(|b| b.len() == 64) {
            out.copy_from_slice(&b);
        }
        out
    }
}
