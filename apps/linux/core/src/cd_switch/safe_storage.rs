//! Chromium `os_crypt` blobs as Claude Desktop stores them in `config.json`:
//! base64 of `"v10" || nonce(12) || AES-256-GCM(ciphertext || tag)`. `v20` (app-bound)
//! and anything else is refused.

use base64::{engine::general_purpose::STANDARD, Engine};

use crate::cookie::{gcm_open_raw, gcm_seal};

const PREFIX: &[u8] = b"v10";

/// Seals `plain` under the 32-byte safe-storage key.
pub fn encrypt(plain: &[u8], key: &[u8; 32]) -> String {
    let mut out = PREFIX.to_vec();
    out.extend_from_slice(&gcm_seal(key, plain, None));
    STANDARD.encode(out)
}

/// Opens a blob made by [`encrypt`] (or Claude Desktop). `None` on bad base64, a prefix
/// other than `v10`, or an authentication failure.
pub fn decrypt(b64: &str, key: &[u8; 32]) -> Option<Vec<u8>> {
    let raw = STANDARD.decode(b64.trim()).ok()?;
    let body = raw.strip_prefix(PREFIX)?;
    gcm_open_raw(key, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [3u8; 32];

    #[test]
    fn round_trip() {
        let plain = br#"{"oauth:tokenCacheV2":{}}"#;
        assert_eq!(decrypt(&encrypt(plain, &KEY), &KEY).unwrap(), plain);
    }

    #[test]
    fn wrong_key_is_none() {
        assert!(decrypt(&encrypt(b"x", &KEY), &[4u8; 32]).is_none());
    }

    #[test]
    fn v20_prefix_is_none() {
        let mut raw = b"v20".to_vec();
        raw.extend_from_slice(&[0u8; 40]);
        assert!(decrypt(&STANDARD.encode(raw), &KEY).is_none());
    }

    #[test]
    fn bad_base64_is_none() {
        assert!(decrypt("!!not base64!!", &KEY).is_none());
    }

    #[test]
    fn blob_starts_with_v10() {
        let raw = STANDARD.decode(encrypt(b"x", &KEY)).unwrap();
        assert!(raw.starts_with(b"v10"));
    }
}
