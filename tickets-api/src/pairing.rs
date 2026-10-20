//! TETHER-11: a machine's pairing code. `tether pair` prints it beside the
//! URL it opens; the app's `#/pair` computes it again from the principal in
//! that URL; the person checks the two match before granting — that check
//! is what stops a crafted link from enrolling someone else's key.
//!
//! The code is the first [`CODE_CHARS`] characters of the RFC 4648 base32
//! of the SHA-256 of the key's 32 public bytes, grouped by four
//! (`K7QF-29XM`): derived from the key alone, so the server keeps nothing
//! for it. 40 bits: enough that a key crafted to show the same code costs
//! about 2^40 tries — and a person compares it once, while they wait.

use sha2::{Digest, Sha256};

/// How many base32 characters the code has (40 bits).
pub const CODE_CHARS: usize = 8;

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// The 32 public bytes of a `key:<64 hex>` principal; `None` for anything
/// else (another provider's principal, a short or non-hex key).
pub fn key_bytes(principal: &str) -> Option<[u8; 32]> {
    let hex = principal.trim().strip_prefix("key:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// The code of a public key: `XXXX-XXXX`.
pub fn code_of_key(public: &[u8; 32]) -> String {
    let digest = Sha256::digest(public);
    // 8 characters × 5 bits = the digest's first 40 bits (5 bytes).
    let bits = digest[..5].iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b));
    let chars: String = (0..CODE_CHARS).map(|i| ALPHABET[((bits >> (35 - 5 * i)) & 31) as usize] as char).collect();
    format!("{}-{}", &chars[..4], &chars[4..])
}

/// The code of a `key:<hex>` principal; `None` when it is not one.
pub fn code(principal: &str) -> Option<String> {
    key_bytes(principal).map(|k| code_of_key(&k))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture tether's and the app's tests both pin: the public key
    /// whose bytes are 0, 1, …, 31.
    pub const FIXTURE: &str = "key:000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    #[test]
    fn a_key_s_code_is_its_sha256_in_base32_grouped_by_four() {
        // SHA-256(00..1f) = 630dcd29 66c43366 91125448 bbb25b4f f412a49c …;
        // its first 40 bits, 0x630dcd2966, in base32: MMG4 2KLG.
        assert_eq!(code(FIXTURE).as_deref(), Some("MMG4-2KLG"));
        assert_eq!(code(&FIXTURE.to_uppercase().replace("KEY:", "key:")), code(FIXTURE), "hex in either case");
        let c = code(FIXTURE).unwrap();
        assert_eq!(c.len(), CODE_CHARS + 1);
        assert!(c.chars().all(|ch| ch == '-' || ALPHABET.contains(&(ch as u8))));
    }

    #[test]
    fn only_a_key_principal_has_a_code() {
        assert!(code("idp:4b1c").is_none());
        assert!(code("key:0001").is_none());
        assert!(code(&format!("{}zz", &FIXTURE[..FIXTURE.len() - 2])).is_none());
        assert!(code(&format!("{FIXTURE}00")).is_none());
        assert_ne!(code(FIXTURE), code(&format!("{}1e", &FIXTURE[..FIXTURE.len() - 2])), "one byte moves it");
    }
}
