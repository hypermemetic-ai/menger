//! UI-6: `ui-sign <seed-file> <bundle>` — prints, in standard base64, the
//! Ed25519 signature over the bundle's sha256 digest that `ui.push` checks
//! against a `--ui-signer`. The seed file holds the 32-byte seed: raw, as
//! 64 hex digits, or in base64 (what `PLEXUS_KEY` takes). For the build's
//! push and the browser test (tests/engine_browser.mjs); never a secret in
//! argv.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};

fn seed(bytes: &[u8]) -> Result<[u8; 32], String> {
    if let Ok(raw) = <[u8; 32]>::try_from(bytes) {
        return Ok(raw);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "the seed file is not 32 bytes, hex or base64".to_string())?.trim();
    let decoded = if text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        (0..32).map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).unwrap()).collect()
    } else {
        base64::engine::general_purpose::STANDARD.decode(text).map_err(|_| "the seed file is not 32 bytes, hex or base64".to_string())?
    };
    <[u8; 32]>::try_from(decoded.as_slice()).map_err(|_| format!("the seed is {} bytes, not 32", decoded.len()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [seed_file, bundle] = args.as_slice() else {
        eprintln!("usage: ui-sign <seed-file> <bundle>");
        std::process::exit(2);
    };
    let run = || -> Result<String, String> {
        let key = SigningKey::from_bytes(&seed(&std::fs::read(seed_file).map_err(|e| format!("{seed_file}: {e}"))?)?);
        let bytes = std::fs::read(bundle).map_err(|e| format!("{bundle}: {e}"))?;
        let digest = menger_ui_service::store::digest_of(&bytes);
        Ok(base64::engine::general_purpose::STANDARD.encode(key.sign(&digest).to_bytes()))
    };
    match run() {
        Ok(sig) => println!("{sig}"),
        Err(e) => {
            eprintln!("ui-sign: {e}");
            std::process::exit(1);
        }
    }
}
