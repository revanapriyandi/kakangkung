//! SHA-256 through Windows' own cryptography (CNG): checks downloaded updates, PKCE for Google
//! sign-in, Chrome Web Store extension ids.

use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCryptHash, BCRYPT_SHA256_ALG_HANDLE, BCRYPT_USE_SYSTEM_PREFERRED_RNG};

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> Result<[u8; 32], String> {
    let mut out = [0u8; 32];
    // SAFETY: the pseudo-handle is a constant provider handle; input and output are valid slices.
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    if status.is_err() {
        return Err(format!("SHA-256 failed: 0x{:08x}", status.0));
    }
    Ok(out)
}

/// `n` bytes from the system's cryptographic random generator (OAuth PKCE verifier and state).
pub fn random_bytes(n: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; n];
    // SAFETY: the buffer is a valid writable slice; no algorithm handle with the system RNG flag.
    let status = unsafe { BCryptGenRandom(None, &mut out, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.is_err() {
        return Err(format!("random bytes failed: 0x{:08x}", status.0));
    }
    Ok(out)
}

/// SHA-256 of `data`, as lowercase hex.
pub fn sha256_hex(data: &[u8]) -> Result<String, String> {
    Ok(sha256(data)?.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn known_digests() {
        assert_eq!(sha256_hex(b"").unwrap(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc").unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
