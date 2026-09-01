//! Decrypts Chrome's encrypted cookie values on Linux.
//!
//! Mirrors Chromium's Linux OS-crypt key providers as proven in Spike 0
//! (`docs/superpowers/spikes/2026-08-30-linux-cookie-decryption.md`), citations
//! to `github.com/chromium/chromium@main`:
//!
//! - KDF: PBKDF2-HMAC-**SHA1**, salt `"saltysalt"`, **1** iteration, 16-byte key
//!   (`async/browser/freedesktop_secret_key_provider.cc:43-45,748`).
//! - Cipher: AES-128-CBC + PKCS7, IV = 16 bytes of `0x20`
//!   (`async/common/encryptor.cc:37-40,176-180`).
//! - 3-byte version tag (`v10`/`v11`/`v12`) is stripped before decrypting.
//! - **v10**: hardcoded key from password `"peanuts"`
//!   (`async/browser/posix_key_provider.cc:15-22`); precomputed bytes
//!   `fd621fe5a2b402539dfa147ca9272778`.
//! - **v11**: key derived from the desktop "Safe Storage" secret, looked up via
//!   `secret-tool` (`freedesktop_secret_key_provider.cc:42,744-751`).
//! - **v12**: secret-portal path, HKDF-SHA256 + AES-256-GCM — a different
//!   scheme entirely. Out of scope here; detected and reported as
//!   [`CookieError::UnsupportedV12`] rather than attempted.
//! - Cookie DB schema **>= 24**: the decrypted plaintext is prefixed with
//!   `SHA256(host_key)` (32 bytes), which must be stripped
//!   (`net/extras/sqlite/sqlite_persistent_cookie_store.cc:213-214,1042-1050`).

use aes::Aes128;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use hmac::Hmac;
use sha2::{Digest, Sha256};
use std::process::Command;

const SALT: &[u8] = b"saltysalt";
const IV: [u8; 16] = [0x20; 16];

/// Which key to derive for decryption. `HardcodedV10` covers the `v10` tag;
/// `Keyring` carries an already-fetched Safe Storage secret for the `v11` tag
/// (fetch it with [`keyring_password`] before calling
/// [`decrypt_cookie_value`] — this module never shells out on the decrypt path
/// itself).
pub enum PasswordSource {
    HardcodedV10,
    Keyring(String),
}

#[derive(Debug, PartialEq)]
pub enum CookieError {
    /// The cookie is encrypted with the v12 (secret-portal, AES-256-GCM)
    /// scheme, which this module does not implement.
    UnsupportedV12,
    /// Fewer than 3 bytes — no room for the version tag.
    TooShort,
    /// AES-CBC/PKCS7 decryption failed (wrong key, or the body's length isn't
    /// a multiple of the 16-byte block size).
    DecryptFailed,
    /// Decrypted plaintext was not valid UTF-8.
    BadUtf8,
}

/// PBKDF2-HMAC-SHA1(password, "saltysalt", 1 iteration, 16-byte key) — the
/// derivation shared by the v10 and v11 key providers.
pub(crate) fn derive_key(password: &[u8]) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2::<Hmac<sha1::Sha1>>(password, SALT, 1, &mut key).expect("pbkdf2");
    key
}

/// Decrypts one Chrome cookie's `encrypted_value` blob.
///
/// `host_key` is the cookie's `host_key` column, used to verify/strip the
/// schema->=24 domain-hash prefix. `db_schema_version` is the cookie DB's
/// `meta.version`. `source` supplies the v11 keyring secret when the tag
/// requires it (see [`PasswordSource`]).
pub fn decrypt_cookie_value(
    encrypted: &[u8],
    host_key: &str,
    db_schema_version: i64,
    source: &PasswordSource,
) -> Result<String, CookieError> {
    if encrypted.len() < 3 {
        return Err(CookieError::TooShort);
    }
    let tag = &encrypted[..3];
    if tag == b"v12" {
        return Err(CookieError::UnsupportedV12);
    }
    let key = match (tag, source) {
        (b"v11", PasswordSource::Keyring(secret)) => derive_key(secret.as_bytes()),
        _ => derive_key(b"peanuts"), // v10, or v11 falling back when no keyring secret
    };
    let body = &encrypted[3..];
    if body.is_empty() || body.len() % 16 != 0 {
        return Err(CookieError::DecryptFailed);
    }
    let mut pt = cbc::Decryptor::<Aes128>::new(&key.into(), &IV.into())
        .decrypt_padded_vec_mut::<Pkcs7>(body)
        .map_err(|_| CookieError::DecryptFailed)?;
    if db_schema_version >= 24 {
        let want = Sha256::digest(host_key.as_bytes());
        if pt.len() >= 32 && pt[..32] == want[..] {
            pt.drain(..32);
        }
        // mismatch: leave bytes as-is (spike behaviour) — a wrong key would
        // already have failed PKCS7 above in practice.
    }
    String::from_utf8(pt).map_err(|_| CookieError::BadUtf8)
}

/// Fetches the desktop "Safe Storage" secret via `secret-tool` (the
/// freedesktop Secret Service CLI), trying the exact schema Chromium writes
/// first, then falling back to an application-only lookup.
///
/// `app` is the browser's keyring `application` attribute, e.g. `"chrome"`,
/// `"chromium"`, `"brave"`, or `"microsoft-edge"`.
pub fn keyring_password(app: &str) -> Option<String> {
    for args in [
        vec![
            "lookup",
            "application",
            app,
            "xdg:schema",
            "chrome_libsecret_os_crypt_password_v2",
        ],
        vec!["lookup", "application", app],
    ] {
        if let Ok(out) = Command::new("secret-tool").args(&args).output() {
            if out.status.success() && !out.stdout.is_empty() {
                return Some(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::Aes128;
    use cbc::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    use sha2::{Digest, Sha256};

    fn v10_key() -> [u8; 16] {
        derive_key(b"peanuts")
    }

    // Encrypt exactly as Chromium would (v10, schema >=24 domain prefix).
    fn make_v10(host: &str, secret: &str) -> Vec<u8> {
        type Enc = cbc::Encryptor<Aes128>;
        let mut pt = Sha256::digest(host.as_bytes()).to_vec();
        pt.extend_from_slice(secret.as_bytes());
        let ct = Enc::new(&v10_key().into(), &[0x20u8; 16].into())
            .encrypt_padded_vec_mut::<Pkcs7>(&pt);
        let mut out = b"v10".to_vec();
        out.extend_from_slice(&ct);
        out
    }

    #[test]
    fn v10_roundtrip_with_domain_prefix() {
        let blob = make_v10("claude.ai", "sk-ant-sid01-SPIKE");
        let got = decrypt_cookie_value(&blob, "claude.ai", 24, &PasswordSource::HardcodedV10).unwrap();
        assert_eq!(got, "sk-ant-sid01-SPIKE");
    }

    #[test]
    fn v11_roundtrip_with_keyring_secret() {
        // Encrypt with a key derived from a known "keyring" secret, decrypt via Keyring source.
        type Enc = cbc::Encryptor<Aes128>;
        let key = derive_key(b"the-safe-storage-secret");
        let mut pt = Sha256::digest("claude.ai".as_bytes()).to_vec();
        pt.extend_from_slice(b"sk-ant-sid01-V11");
        let ct = Enc::new(&key.into(), &[0x20u8; 16].into()).encrypt_padded_vec_mut::<Pkcs7>(&pt);
        let mut blob = b"v11".to_vec();
        blob.extend_from_slice(&ct);
        let got = decrypt_cookie_value(
            &blob,
            "claude.ai",
            24,
            &PasswordSource::Keyring("the-safe-storage-secret".into()),
        )
        .unwrap();
        assert_eq!(got, "sk-ant-sid01-V11");
    }

    #[test]
    fn v12_is_unsupported() {
        let blob = b"v12\x00\x01\x02".to_vec();
        assert!(matches!(
            decrypt_cookie_value(&blob, "claude.ai", 24, &PasswordSource::HardcodedV10),
            Err(CookieError::UnsupportedV12)
        ));
    }

    #[test]
    fn schema_below_24_does_not_strip_domain_hash() {
        // Encrypt WITHOUT the domain prefix, decrypt with schema 23.
        type Enc = cbc::Encryptor<Aes128>;
        let ct = Enc::new(&v10_key().into(), &[0x20u8; 16].into())
            .encrypt_padded_vec_mut::<Pkcs7>(b"plain-no-prefix");
        let mut blob = b"v10".to_vec();
        blob.extend_from_slice(&ct);
        let got = decrypt_cookie_value(&blob, "claude.ai", 23, &PasswordSource::HardcodedV10).unwrap();
        assert_eq!(got, "plain-no-prefix");
    }
}
