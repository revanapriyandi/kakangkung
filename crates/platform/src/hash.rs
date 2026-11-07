//! SHA-256 through Windows' own cryptography (CNG), to check downloaded updates.

use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};

/// SHA-256 of `data`, as lowercase hex.
pub fn sha256_hex(data: &[u8]) -> Result<String, String> {
    let mut out = [0u8; 32];
    // SAFETY: the pseudo-handle is a constant provider handle; input and output are valid slices.
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    if status.is_err() {
        return Err(format!("SHA-256 failed: 0x{:08x}", status.0));
    }
    Ok(out.iter().map(|b| format!("{b:02x}")).collect())
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
