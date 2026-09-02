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
//!   (`encryptor.cc:36,170-176`). Needs [`KeySources::portal`], fetched with
//!   [`portal_secret`]; without it a v12 blob reports
//!   [`CookieError::NoPortalSecret`]. Pinned to real Chromium output by
//!   `v12_known_answer_from_real_chromium_blob` below.
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

/// The keys available to a decrypt, picked by the cookie's own version tag —
/// the same key-ring shape Chromium uses (`os_crypt_async.cc:88-92` keeps one
/// key per tag and selects on read).
///
/// Both fields are optional and both are fetched by the caller, never on the
/// decrypt path: `keyring` is the desktop Safe Storage secret for `v11`
/// ([`keyring_password`]), `portal` the raw secret-portal secret for `v12`
/// ([`portal_secret`]). A `v10` cookie needs neither — the default value
/// decrypts it.
#[derive(Default, Clone)]
pub struct KeySources {
    pub keyring: Option<String>,
    pub portal: Option<Vec<u8>>,
}

impl KeySources {
    /// Sources with only the v11 Safe Storage secret — the first pass over a
    /// profile, before any v12 cookie has been seen.
    pub fn keyring(secret: Option<String>) -> Self {
        Self {
            keyring: secret,
            portal: None,
        }
    }

    /// The same sources plus a portal secret, for the v12 retry.
    pub fn with_portal(&self, portal: Vec<u8>) -> Self {
        Self {
            keyring: self.keyring.clone(),
            portal: Some(portal),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum CookieError {
    /// The cookie is a v12 (secret-portal, AES-256-GCM) blob but no portal
    /// secret was supplied ([`KeySources::portal`]).
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
/// `meta.version`. `sources` carries the pre-fetched secrets; which one is
/// used follows the version tag (see [`KeySources`]).
pub fn decrypt_cookie_value(
    encrypted: &[u8],
    host_key: &str,
    db_schema_version: i64,
    sources: &KeySources,
) -> Result<String, CookieError> {
    if encrypted.len() < 3 {
        return Err(CookieError::TooShort);
    }
    let tag = &encrypted[..3];
    if tag == b"v12" {
        let Some(portal_secret) = sources.portal.as_deref() else {
            return Err(CookieError::NoPortalSecret);
        };
        return decrypt_v12(&encrypted[3..], portal_secret, host_key, db_schema_version);
    }
    let key = match (tag, sources.keyring.as_deref()) {
        (b"v11", Some(secret)) => derive_key(secret.as_bytes()),
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

/// Fetches a `v12` secret-portal secret from the desktop keyring via
/// `secret-tool`, keyed by the portal's own attributes: `app_id` plus
/// `xdg:schema=org.freedesktop.portal.Secret` (gnome-keyring 50.0
/// `daemon/dbus/gkd-secret-portal.c:290-405`, read in Spike 1).
///
/// Two ways this differs from [`keyring_password`], both deliberate:
///
/// - The bytes are returned **raw and untrimmed**. The portal secret is 64
///   bytes of `gcry_randomize` output, so a trailing `0x0a` is data, not a
///   line ending. `secret-tool` passes arbitrary bytes through unchanged —
///   verified in Spike 1 against a real portal secret containing non-UTF-8
///   bytes, byte-identical with no added newline.
/// - There is no schema-less fallback lookup. The portal schema is the only
///   place this secret lives.
///
/// `app_id` is the *portal's* view of the calling browser, not a browser
/// name: **empty** for an unsandboxed browser whose desktop gave it no app
/// identity (what real Chromium produced in Spike 1), the Flatpak/Snap app id
/// for a sandboxed one. Callers try their candidates in turn; a wrong secret
/// cannot yield garbage because AES-256-GCM authenticates.
pub fn portal_secret(app_id: &str) -> Option<Vec<u8>> {
    let out = Command::new("secret-tool")
        .args([
            "lookup",
            "app_id",
            app_id,
            "xdg:schema",
            "org.freedesktop.portal.Secret",
        ])
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    Some(out.stdout)
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
        let got = decrypt_cookie_value(&blob, "claude.ai", 24, &KeySources::default()).unwrap();
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
            &KeySources::keyring(Some("the-safe-storage-secret".into())),
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
            &KeySources::default().with_portal(b"portal-master-secret".to_vec()),
        )
        .unwrap();
        assert_eq!(got, "sk-ant-sid01-V12");
    }

    /// Hex -> bytes for the pinned known-answer blobs below.
    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    // Known-answer test against REAL Chromium output. Everything above only
    // proves decrypt_v12 is self-consistent with a fixture this file builds
    // itself; this pins it to bytes Chromium actually wrote.
    //
    // Provenance (Spike 1, 2026-09-03, Ubuntu 26.04 arm64 VM): Chromium
    // 151.0.7922.34 launched with `--password-store=gnome-libsecret
    // --enable-features=SecretPortalKeyProviderUseForEncryption` against
    // xdg-desktop-portal 1.21.1 with gnome-keyring 50.0 as the
    // org.freedesktop.impl.portal.Secret backend, wrote the sentinel cookie
    // below; `Local State` recorded os_crypt.portal.prev_init_success = true
    // and the on-disk tag was v12. The 64-byte portal secret is the one
    // gnome-keyring minted for that run, fetched with
    // `secret-tool lookup app_id "" xdg:schema org.freedesktop.portal.Secret`.
    //
    // Both constants are safe to publish: the secret belongs to a throwaway
    // VM keyring that no longer exists, and the only thing it ever encrypted
    // is this sentinel string.
    #[test]
    fn v12_known_answer_from_real_chromium_blob() {
        let blob = unhex(
            "7631324059DF1F739CD94CD1842BF4D54D7C518C0B6ABDF87183EAD40E6C9994\
             723272B82D881CC98E298F3E393AF4C250D26824888DEE10C78CA885E5D9B70C\
             4B294510E2F55E0D522C0C31DF721F71AAA3CAD4135596E215547E3DF04ED2",
        );
        let portal_secret = unhex(
            "e6d4a11c2cd9116dcf93d02003b7a9ec601d2a4796b35d89664d328e71b55270\
             1cb58770920c10806e7089d15216b4c0f6904178ef624fcbbbaad2bcc436a346",
        );
        assert_eq!(&blob[..3], b"v12");
        assert_eq!(portal_secret.len(), 64); // PORTAL_DEFAULT_KEY_SIZE
        let got = decrypt_cookie_value(
            &blob,
            "claude.ai",
            24,
            &KeySources::default().with_portal(portal_secret),
        )
        .expect("real Chromium v12 blob decrypts");
        assert_eq!(got, "sk-ant-sid01-V12-REAL-PORTAL-abc");
    }

    #[test]
    fn v12_without_portal_secret_reports_no_portal_secret() {
        let blob = make_v12("claude.ai", "sk-ant-sid01-V12", b"portal-master-secret");
        for source in [
            KeySources::default(),
            KeySources::keyring(Some("safe-storage".into())),
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
                &KeySources::default().with_portal(b"portal-master-secret".to_vec()),
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
                &KeySources::default().with_portal(b"portal-master-secret".to_vec()),
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
        let got = decrypt_cookie_value(&blob, "claude.ai", 23, &KeySources::default()).unwrap();
        assert_eq!(got, "plain-no-prefix");
    }
}
