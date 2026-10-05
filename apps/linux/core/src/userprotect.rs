//! Per-user data protection for values stored at rest, via the Windows
//! Data Protection API (`CryptProtectData` / `CryptUnprotectData`,
//! current-user scope).
//!
//! This is the Windows counterpart of the Unix at-rest scheme in
//! [`crate::store`] (HKDF over the machine id). Both seal `sessionKey` so a
//! copy of the store is useless off the owning account; neither is portable
//! between machines (see `contract/account-schema.md`, "sessionKey is not
//! portable").

use std::ptr;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

fn input_blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    }
}

/// Copies DPAPI's output out and frees it with `LocalFree`, as the API
/// requires.
///
/// # Safety
/// `out` must be a blob DPAPI just filled on a successful call.
unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
    LocalFree(out.pbData as _);
    bytes
}

/// Seals `plain` to the current Windows user. `None` only if the API fails.
pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    let input = input_blob(plain);
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    // SAFETY: `input` points at `plain` for the duration of the call; every
    // other in-pointer is null (DPAPI treats these as "no description / no
    // entropy / no prompt"); `out` is freed by `take` on success.
    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    };
    if ok == 0 {
        return None;
    }
    Some(unsafe { take(out) })
}

/// Opens a blob sealed by [`protect`] (or by Chromium for its `Local State`
/// key). `None` for anything DPAPI rejects: another user's blob, garbage,
/// truncation.
pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    let input = input_blob(sealed);
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    // SAFETY: as in `protect`.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    };
    if ok == 0 {
        return None;
    }
    Some(unsafe { take(out) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protect_then_unprotect_round_trips() {
        let sealed = protect(b"sk-ant-sid01-example").expect("protect");
        assert_ne!(sealed.as_slice(), b"sk-ant-sid01-example");
        assert_eq!(unprotect(&sealed).expect("unprotect"), b"sk-ant-sid01-example");
    }

    #[test]
    fn unprotect_rejects_garbage() {
        assert_eq!(unprotect(b"not a protected blob"), None);
    }
}
