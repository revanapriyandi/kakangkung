//! Secrets at rest (the Google sign-in refresh token) through Windows DPAPI: only this Windows
//! user on this PC can decrypt them, like Chrome's own cookie/password key.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
}

/// Copies DPAPI's output and frees it.
fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    // SAFETY: on success DPAPI hands back `cbData` bytes at `pbData`, allocated with LocalAlloc;
    // they are copied once and freed once.
    unsafe {
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        v
    }
}

/// Encrypt `data` for the current Windows user.
pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: the input blob points into `data`, which outlives the call (DPAPI only reads it).
    unsafe { CryptProtectData(&blob(data), PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }
        .map_err(|e| format!("protecting a secret: {e}"))?;
    Ok(take(out))
}

/// Decrypt what [`protect`] gave, for the same Windows user.
pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: as in `protect`.
    unsafe { CryptUnprotectData(&blob(data), None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }
        .map_err(|e| format!("reading a secret: {e}"))?;
    Ok(take(out))
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let sealed = super::protect(b"refresh-token").unwrap();
        assert_ne!(sealed, b"refresh-token");
        assert_eq!(super::unprotect(&sealed).unwrap(), b"refresh-token");
        assert!(super::unprotect(b"not dpapi").is_err());
    }
}
