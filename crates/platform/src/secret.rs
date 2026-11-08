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

/// Base64 (standard alphabet) to bytes, through Windows' own decoder.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{CryptStringToBinaryW, CRYPT_STRING_BASE64};
    let wide: Vec<u16> = text.trim().encode_utf16().collect();
    let mut len = 0u32;
    // SAFETY: first call sizes the output, second fills a buffer of that size.
    unsafe {
        CryptStringToBinaryW(&wide, CRYPT_STRING_BASE64, None, &mut len, None, None).ok()?;
        let mut out = vec![0u8; len as usize];
        CryptStringToBinaryW(&wide, CRYPT_STRING_BASE64, Some(out.as_mut_ptr()), &mut len, None, None).ok()?;
        out.truncate(len as usize);
        Some(out)
    }
}

/// AES-256-GCM decryption (Windows CNG): `data` is the ciphertext followed by its 16-byte tag.
/// `None` when the key, nonce or tag do not match. Used to read other browsers' saved passwords.
pub fn aes_gcm_decrypt(key: &[u8], nonce: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    use windows::Win32::Security::Cryptography::*;
    if data.len() < 16 {
        return None;
    }
    let (ct, tag) = data.split_at(data.len() - 16);
    let (mut nonce, mut tag) = (nonce.to_vec(), tag.to_vec());
    // SAFETY: CNG handles are closed on every path; the buffers outlive the calls using them, and
    // the cipher-mode info points into `nonce` and `tag`, which live until the end.
    unsafe {
        let mut alg = BCRYPT_ALG_HANDLE::default();
        BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_AES_ALGORITHM, None, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0)).ok().ok()?;
        let mode: Vec<u8> = BCRYPT_CHAIN_MODE_GCM.as_wide().iter().chain([&0u16]).flat_map(|c| c.to_le_bytes()).collect();
        let mut hkey = BCRYPT_KEY_HANDLE::default();
        let ready = BCryptSetProperty(alg.into(), BCRYPT_CHAINING_MODE, &mode, 0).is_ok()
            && BCryptGenerateSymmetricKey(alg, &mut hkey, None, key, 0).is_ok();
        let mut out = vec![0u8; ct.len()];
        let mut done = 0u32;
        let ok = ready && {
            let info = BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO {
                cbSize: std::mem::size_of::<BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO>() as u32,
                dwInfoVersion: BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO_VERSION,
                pbNonce: nonce.as_mut_ptr(),
                cbNonce: nonce.len() as u32,
                pbTag: tag.as_mut_ptr(),
                cbTag: tag.len() as u32,
                ..Default::default()
            };
            BCryptDecrypt(hkey, Some(ct), Some(&info as *const _ as *const _), None, Some(&mut out), &mut done, BCRYPT_FLAGS(0)).is_ok()
        };
        if !hkey.is_invalid() {
            let _ = BCryptDestroyKey(hkey);
        }
        let _ = BCryptCloseAlgorithmProvider(alg, 0);
        ok.then(|| {
            out.truncate(done as usize);
            out
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_and_aes_gcm() {
        assert_eq!(super::base64_decode("aGVsbG8=").unwrap(), b"hello");
        // NIST GCM test case 14: AES-256, zero key, zero 96-bit IV, one zero block.
        let hex = |s: &str| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect::<Vec<u8>>();
        let data = hex("cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919");
        assert_eq!(super::aes_gcm_decrypt(&[0; 32], &[0; 12], &data).unwrap(), vec![0u8; 16]);
        let mut bad = data.clone();
        bad[0] ^= 1;
        assert!(super::aes_gcm_decrypt(&[0; 32], &[0; 12], &bad).is_none());
    }

    #[test]
    fn round_trip() {
        let sealed = super::protect(b"refresh-token").unwrap();
        assert_ne!(sealed, b"refresh-token");
        assert_eq!(super::unprotect(&sealed).unwrap(), b"refresh-token");
        assert!(super::unprotect(b"not dpapi").is_err());
    }
}
