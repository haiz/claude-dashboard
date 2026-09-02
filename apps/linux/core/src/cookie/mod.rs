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
//! - **v12**: secret-portal path — HKDF-SHA256 over the portal secret (salt
//!   `"fdo_portal_secret_salt"`, info `"HKDF-SHA-256 AES-256-GCM"`, 32-byte
//!   key: `secret_portal_key_provider.cc:38-39,200-205`), then AES-256-GCM
//!   with the nonce as the first 12 bytes of the body
//!   (`encryptor.cc:36,170-176`). Requires [`PasswordSource::Portal`];
//!   without it a v12 blob reports [`CookieError::NoPortalSecret`].
//!   *Fetching* the portal secret is not this module's job — see the sync
//!   helper; as of this commit no fetcher exists yet, so v12 profiles are
//!   still skipped at runtime.
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
    /// Raw secret retrieved from the XDG secret portal, for `v12` cookies.
    Portal(Vec<u8>),
}

#[derive(Debug, PartialEq)]
pub enum CookieError {
    /// The cookie is a v12 (secret-portal, AES-256-GCM) blob but no portal
    /// secret was supplied ([`PasswordSource::Portal`]).
    NoPortalSecret,
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
        let PasswordSource::Portal(portal_secret) = source else {
            return Err(CookieError::NoPortalSecret);
        };
        return decrypt_v12(&encrypted[3..], portal_secret, host_key, db_schema_version);
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
    strip_domain_hash(&mut pt, host_key, db_schema_version);
    String::from_utf8(pt).map_err(|_| CookieError::BadUtf8)
}

/// Decrypts a `v12` (secret-portal) body: 12-byte GCM nonce, then
/// AES-256-GCM ciphertext+tag, key = HKDF-SHA256(portal secret) with
/// Chromium's fixed salt/info (module docs cite the source lines).
fn decrypt_v12(
    body: &[u8],
    portal_secret: &[u8],
    host_key: &str,
    db_schema_version: i64,
) -> Result<String, CookieError> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

    // Minimum: 12-byte nonce + 16-byte GCM tag (empty plaintext).
    if body.len() < 12 + 16 {
        return Err(CookieError::DecryptFailed);
    }
    let mut key = [0u8; 32];
    hkdf::Hkdf::<Sha256>::new(Some(b"fdo_portal_secret_salt"), portal_secret)
        .expand(b"HKDF-SHA-256 AES-256-GCM", &mut key)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    let mut pt = Aes256Gcm::new(&key.into())
        .decrypt(Nonce::from_slice(&body[..12]), &body[12..])
        .map_err(|_| CookieError::DecryptFailed)?;
    strip_domain_hash(&mut pt, host_key, db_schema_version);
    String::from_utf8(pt).map_err(|_| CookieError::BadUtf8)
}

/// Strips the schema->=24 `SHA256(host_key)` plaintext prefix in place; on a
/// mismatch the bytes are left as-is (spike behaviour — a wrong key already
/// failed PKCS7/GCM before reaching this).
fn strip_domain_hash(pt: &mut Vec<u8>, host_key: &str, db_schema_version: i64) {
    if db_schema_version >= 24 {
        let want = Sha256::digest(host_key.as_bytes());
        if pt.len() >= 32 && pt[..32] == want[..] {
            pt.drain(..32);
        }
    }
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

    // Known-answer test: pins derive_key's PBKDF2 parameters (SHA1, salt
    // "saltysalt", 1 iteration) to Chromium's published constants, verified
    // in Spike 0 against Chromium source and a live Ubuntu run. The other 4
    // tests below only prove decrypt is self-consistent with derive_key --
    // they'd still pass if derive_key's parameters silently drifted (e.g. a
    // higher iteration count). This test fails independently of that.
    #[test]
    fn derive_key_matches_chromium_known_answers() {
        assert_eq!(
            derive_key(b"peanuts"),
            [
                0xfd, 0x62, 0x1f, 0xe5, 0xa2, 0xb4, 0x02, 0x53, 0x9d, 0xfa, 0x14, 0x7c, 0xa9, 0x27,
                0x27, 0x78,
            ]
        );
        // Chromium's empty-key CBC fallback constant (encryptor.cc).
        assert_eq!(
            derive_key(b""),
            [
                0xd0, 0xd0, 0xec, 0x9c, 0x7d, 0x77, 0xd4, 0x3a, 0xc5, 0x41, 0x87, 0xfa, 0x48, 0x18,
                0xd1, 0x7f,
            ]
        );
    }

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

    // Encrypt exactly as Chromium's secret-portal path would (v12, schema >=24
    // domain prefix): HKDF-SHA256(portal secret, salt "fdo_portal_secret_salt",
    // info "HKDF-SHA-256 AES-256-GCM") -> AES-256-GCM, nonce prefixed to the body.
    fn make_v12(host: &str, secret: &str, portal_secret: &[u8]) -> Vec<u8> {
        use aes_gcm::aead::Aead;
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
        let mut key = [0u8; 32];
        hkdf::Hkdf::<Sha256>::new(Some(b"fdo_portal_secret_salt"), portal_secret)
            .expand(b"HKDF-SHA-256 AES-256-GCM", &mut key)
            .unwrap();
        let mut pt = Sha256::digest(host.as_bytes()).to_vec();
        pt.extend_from_slice(secret.as_bytes());
        let nonce = [0x24u8; 12];
        let ct = Aes256Gcm::new(&key.into())
            .encrypt(Nonce::from_slice(&nonce), pt.as_slice())
            .unwrap();
        let mut out = b"v12".to_vec();
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        out
    }

    #[test]
    fn v12_roundtrip_with_domain_prefix() {
        let blob = make_v12("claude.ai", "sk-ant-sid01-V12", b"portal-master-secret");
        let got = decrypt_cookie_value(
            &blob,
            "claude.ai",
            24,
            &PasswordSource::Portal(b"portal-master-secret".to_vec()),
        )
        .unwrap();
        assert_eq!(got, "sk-ant-sid01-V12");
    }

    #[test]
    fn v12_without_portal_secret_reports_no_portal_secret() {
        let blob = make_v12("claude.ai", "sk-ant-sid01-V12", b"portal-master-secret");
        for source in [
            PasswordSource::HardcodedV10,
            PasswordSource::Keyring("safe-storage".into()),
        ] {
            assert!(matches!(
                decrypt_cookie_value(&blob, "claude.ai", 24, &source),
                Err(CookieError::NoPortalSecret)
            ));
        }
    }

    #[test]
    fn v12_tampered_ciphertext_fails_gcm_auth() {
        let mut blob = make_v12("claude.ai", "sk-ant-sid01-V12", b"portal-master-secret");
        let last = blob.len() - 1;
        blob[last] ^= 0x01;
        assert_eq!(
            decrypt_cookie_value(
                &blob,
                "claude.ai",
                24,
                &PasswordSource::Portal(b"portal-master-secret".to_vec()),
            ),
            Err(CookieError::DecryptFailed)
        );
    }

    #[test]
    fn v12_body_shorter_than_nonce_plus_tag_fails() {
        // 3-byte tag + 27 bytes: one short of the 12-byte nonce + 16-byte GCM tag minimum.
        let blob = [b"v12".as_slice(), &[0u8; 27]].concat();
        assert_eq!(
            decrypt_cookie_value(
                &blob,
                "claude.ai",
                24,
                &PasswordSource::Portal(b"portal-master-secret".to_vec()),
            ),
            Err(CookieError::DecryptFailed)
        );
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
