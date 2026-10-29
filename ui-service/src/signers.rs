//! UI-3: the signers named at start (`--ui-signer <hex public key>`,
//! repeatable). A push's signature is Ed25519 over the bundle's sha256
//! digest by one of them; with none named, every push is refused
//! (`no_signer`).

use ed25519_dalek::VerifyingKey;

/// The public keys a push may be signed by.
#[derive(Debug, Clone, Default)]
pub struct Signers {
    keys: Vec<VerifyingKey>,
}

impl Signers {
    /// From `--ui-signer`'s values: each 64 hex digits, an Ed25519 public
    /// key; a value that is not one is refused, naming it.
    pub fn parse(hex_keys: &[String]) -> Result<Self, String> {
        let keys = hex_keys
            .iter()
            .map(|h| {
                let h = h.trim();
                let bad = || format!("--ui-signer `{h}` is not an Ed25519 public key: 64 hex digits");
                if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(bad());
                }
                let mut bytes = [0u8; 32];
                for (i, b) in bytes.iter_mut().enumerate() {
                    *b = u8::from_str_radix(&h[2 * i..2 * i + 2], 16).map_err(|_| bad())?;
                }
                VerifyingKey::from_bytes(&bytes).map_err(|_| bad())
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { keys })
    }

    /// None named.
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether any is named.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Whether `signature` is one of them over `digest` (a bundle's sha256).
    pub fn verify(&self, digest: &[u8; 32], signature: &[u8; 64]) -> bool {
        let _ = (digest, signature);
        todo!("UI-3 phase 2: verify a push's signature")
    }
}
