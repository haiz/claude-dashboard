//! Chromium cookie values on Windows.
//!
//! - `v10`/`v11`: AES-256-GCM, body = `nonce(12) || ciphertext || tag` — the
//!   same layout [`super::gcm_open`] already decodes for Linux `v12`. The key
//!   is `Local State`'s `os_crypt.encrypted_key`: base64, a `"DPAPI"` prefix,
//!   then a blob sealed to the current user.
//! - `v20`: app-bound encryption (Chrome 127+), whose key is bound to the
//!   browser through a SYSTEM service. Not opened here; reported as
//!   [`CookieError::AppBoundEncrypted`] so the app points the user at the
//!   browser extension instead.
//! - Schema >= 24 domain-hash prefix: stripped by `gcm_open`, exactly as on
//!   Linux.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::Value;

use super::{gcm_open, CookieError};

/// Decodes one cookie's `encrypted_value` with the profile's unwrapped
/// `Local State` key.
pub fn decode_value(
    encrypted: &[u8],
    host_key: &str,
    db_schema_version: i64,
    key: &[u8; 32],
) -> Result<String, CookieError> {
    if encrypted.len() < 3 {
        return Err(CookieError::TooShort);
    }
    match &encrypted[..3] {
        b"v20" => Err(CookieError::AppBoundEncrypted),
        b"v10" | b"v11" => gcm_open(key, &encrypted[3..], host_key, db_schema_version),
        // Pre-2020 cookies were whole-value DPAPI blobs with no version tag.
        // Claude sessions are far younger than that; not supported.
        _ => Err(CookieError::DecryptFailed),
    }
}

/// The sealed AES key from a `Local State` JSON document:
/// `os_crypt.encrypted_key`, base64-decoded, with its `"DPAPI"` prefix
/// removed. `None` when any step is missing or the prefix is absent.
pub fn local_state_wrapped_key(local_state_json: &str) -> Option<Vec<u8>> {
    let v: Value = serde_json::from_str(local_state_json).ok()?;
    let b64 = v.get("os_crypt")?.get("encrypted_key")?.as_str()?;
    let raw = BASE64.decode(b64).ok()?;
    raw.strip_prefix(b"DPAPI").map(<[u8]>::to_vec)
}

/// Reads `local_state`, unwraps its key with DPAPI, and returns the 32-byte
/// AES key. `None` if the file, the key, the unwrap or the length is wrong.
#[cfg(windows)]
pub fn profile_key(local_state: &std::path::Path) -> Option<[u8; 32]> {
    let json = std::fs::read_to_string(local_state).ok()?;
    let sealed = local_state_wrapped_key(&json)?;
    crate::userprotect::unprotect(&sealed)?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
    use sha2::{Digest, Sha256};

    const KEY: [u8; 32] = [7u8; 32];

    /// Builds a cookie blob the way Chromium would on Windows: version tag,
    /// 12-byte nonce, then AES-256-GCM over `SHA256(host) || secret`.
    fn make(tag: &[u8], host: &str, secret: &str) -> Vec<u8> {
        let mut pt = Sha256::digest(host.as_bytes()).to_vec();
        pt.extend_from_slice(secret.as_bytes());
        let nonce = [9u8; 12];
        let ct = Aes256Gcm::new(&KEY.into())
            .encrypt(Nonce::from_slice(&nonce), pt.as_slice())
            .unwrap();
        [tag, &nonce, &ct].concat()
    }

    #[test]
    fn v10_decodes_and_strips_the_domain_hash() {
        let blob = make(b"v10", "claude.ai", "sk-ant-sid01-WIN");
        assert_eq!(
            decode_value(&blob, "claude.ai", 24, &KEY).unwrap(),
            "sk-ant-sid01-WIN"
        );
    }

    #[test]
    fn v20_is_reported_as_app_bound_not_garbage() {
        let blob = make(b"v20", "claude.ai", "sk-ant-sid01-WIN");
        assert_eq!(
            decode_value(&blob, "claude.ai", 24, &KEY),
            Err(CookieError::AppBoundEncrypted)
        );
    }

    #[test]
    fn wrong_key_fails_authentication() {
        let blob = make(b"v10", "claude.ai", "sk");
        assert_eq!(
            decode_value(&blob, "claude.ai", 24, &[8u8; 32]),
            Err(CookieError::DecryptFailed)
        );
    }

    #[test]
    fn short_and_unknown_tags_fail_cleanly() {
        assert_eq!(decode_value(b"v1", "claude.ai", 24, &KEY), Err(CookieError::TooShort));
        assert_eq!(
            decode_value(b"\x01\x00\x00raw", "claude.ai", 24, &KEY),
            Err(CookieError::DecryptFailed)
        );
    }

    #[test]
    fn local_state_key_is_base64_minus_the_prefix() {
        let wrapped = [b"DPAPI".as_slice(), &[1, 2, 3]].concat();
        let json = format!(r#"{{"os_crypt":{{"encrypted_key":"{}"}}}}"#, BASE64.encode(&wrapped));
        assert_eq!(local_state_wrapped_key(&json), Some(vec![1, 2, 3]));
    }

    #[test]
    fn local_state_without_a_wrapped_key_is_none() {
        assert_eq!(local_state_wrapped_key("{}"), None);
        assert_eq!(local_state_wrapped_key("not json"), None);
        let no_prefix = format!(
            r#"{{"os_crypt":{{"encrypted_key":"{}"}}}}"#,
            BASE64.encode(b"XXXXXyz")
        );
        assert_eq!(local_state_wrapped_key(&no_prefix), None);
    }
}
