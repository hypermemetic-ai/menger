//! AUTH-22: the little cryptography the sign-in needs of its own: HMAC
//! (RFC 2104, over SHA-256 for CSRF tokens and SHA-1 for TOTP), TOTP
//! (RFC 6238: 6 digits, 30 s, SHA-1 — what authenticator apps and WorkOS
//! use), base32 (RFC 4648, a TOTP secret's alphabet), and a comparison
//! that takes the same time whatever differs.

use sha2::digest::Digest;

/// HMAC over a hash with a 64-byte block (SHA-1, SHA-256).
pub fn hmac<D: Digest>(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        let h = D::digest(key);
        k[..h.len()].copy_from_slice(&h);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let (mut ipad, mut opad) = ([0x36u8; 64], [0x5cu8; 64]);
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = D::new();
    inner.update(ipad);
    inner.update(msg);
    let inner = inner.finalize();
    let mut outer = D::new();
    outer.update(opad);
    outer.update(inner);
    outer.finalize().to_vec()
}

/// HMAC-SHA256.
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    hmac::<sha2::Sha256>(key, msg)
}

/// Equal, in time that depends only on the lengths.
pub fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Base32 without padding.
pub fn base32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            out.push(B32[((buf >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Base32's bytes (case and padding ignored).
pub fn unbase32(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in text.chars().filter(|c| *c != '=' && !c.is_whitespace()) {
        let v = B32.iter().position(|&x| x as char == c.to_ascii_uppercase())? as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            out.push((buf >> (bits - 8)) as u8);
            bits -= 8;
        }
    }
    Some(out)
}

/// The TOTP code for `secret` at unix time `t`.
pub fn totp(secret: &[u8], t: i64) -> String {
    let counter = (t.max(0) / 30) as u64;
    let mac = hmac::<sha1::Sha1>(secret, &counter.to_be_bytes());
    let off = (mac[mac.len() - 1] & 0x0f) as usize;
    let bin = ((mac[off] as u32 & 0x7f) << 24) | ((mac[off + 1] as u32) << 16) | ((mac[off + 2] as u32) << 8) | mac[off + 3] as u32;
    format!("{:06}", bin % 1_000_000)
}

/// Whether `code` is `secret`'s code now, or one step either side (clock
/// drift).
pub fn totp_matches(secret: &[u8], code: &str, now: i64) -> bool {
    [-30, 0, 30].iter().any(|d| constant_eq(totp(secret, now + d).as_bytes(), code.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn hmac_is_rfc_4231() {
        // Case 2.
        assert_eq!(hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[test]
    fn totp_is_rfc_6238() {
        // Appendix B, SHA-1, the 20-byte secret "12345678901234567890":
        // the 8-digit codes' last six digits.
        let s = b"12345678901234567890";
        assert_eq!(totp(s, 59), "287082");
        assert_eq!(totp(s, 1111111109), "081804");
        assert_eq!(totp(s, 1234567890), "005924");
        assert!(totp_matches(s, "287082", 59 + 25));
        assert!(!totp_matches(s, "287082", 59 + 90));
    }

    #[test]
    fn base32_round_trips() {
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(unbase32("mzxw6ytboi======").unwrap(), b"foobar");
        assert!(unbase32("1").is_none());
        assert!(constant_eq(b"a", b"a") && !constant_eq(b"a", b"b") && !constant_eq(b"a", b"ab"));
    }
}
