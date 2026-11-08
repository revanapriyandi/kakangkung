//! Installing extensions from the Chrome Web Store.
//!
//! The store's own "Add to Chrome" button needs Chrome's private store API, which WebView2 does
//! not have. Kakangkung puts its own button on the store's extension pages (assets/webstore.js);
//! a click downloads the extension's CRX from Google's update service, unpacks it into
//! `Extensions/<id>` and loads it unpacked (`AddBrowserExtension`).
//!
//! The CRX's public key goes into the unpacked manifest as `key`, so the extension keeps its
//! store id (its storage, the sync list and the toolbar menu all go by id).
//! Blocking (download, unzip): run off the UI thread.

use std::path::{Path, PathBuf};

use serde_json::Value as Json;

/// Host of the store's pages (the only pages that may ask for an install).
pub(crate) const HOST: &str = "chromewebstore.google.com";
/// Largest CRX accepted (the store's limit is a few hundred MB; real extensions are far smaller).
const MAX_CRX: usize = 200 * 1024 * 1024;

/// An extension id: 32 letters a–p.
pub(crate) fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// The id Chrome derives from a public key: SHA-256, first 16 bytes, each nibble as a–p.
pub(crate) fn id_of(public_key: &[u8]) -> Result<String, String> {
    let digest = platform::sha256(public_key)?;
    Ok(digest[..16].iter().flat_map(|b| [b >> 4, b & 15]).map(|n| (b'a' + n) as char).collect())
}

/// Where Google's update service serves the newest CRX of `id` for browser version `version`.
pub(crate) fn crx_url(id: &str, version: &str) -> String {
    format!("https://clients2.google.com/service/update2/crx?response=redirect&prodversion={version}&acceptformat=crx2,crx3&x=id%3D{id}%26uc")
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Protobuf varint at `*at`.
fn varint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*at)?;
        *at += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Some(v);
        }
    }
    None
}

/// The length-delimited fields `(number, bytes)` of a protobuf message (others are skipped).
fn fields(b: &[u8]) -> Vec<(u64, &[u8])> {
    let (mut out, mut at) = (vec![], 0);
    while at < b.len() {
        let Some(tag) = varint(b, &mut at) else { break };
        match tag & 7 {
            0 => {
                if varint(b, &mut at).is_none() {
                    break;
                }
            }
            1 => at += 8,
            5 => at += 4,
            2 => {
                let Some(len) = varint(b, &mut at).map(|l| l as usize) else { break };
                let Some(bytes) = b.get(at..at.saturating_add(len)) else { break };
                out.push((tag >> 3, bytes));
                at += len;
            }
            _ => break,
        }
    }
    out
}

/// The ZIP inside a CRX (versions 2 and 3) and the public key whose id is `id`, if the header
/// holds it. CRX3: "Cr24", 3, header length, `CrxFileHeader` protobuf (proofs in fields 2 and
/// 3, each with the public key in field 1), then the ZIP.
pub(crate) fn unpack_crx<'a>(crx: &'a [u8], id: &str) -> Result<(&'a [u8], Option<Vec<u8>>), String> {
    if crx.get(..4) != Some(b"Cr24") {
        return Err("the download is not an extension package (CRX)".into());
    }
    let bad = || "the extension package is damaged".to_string();
    match u32_at(crx, 4) {
        Some(2) => {
            let (key_len, sig_len) = (u32_at(crx, 8).ok_or_else(bad)?, u32_at(crx, 12).ok_or_else(bad)?);
            let key = crx.get(16..16 + key_len).ok_or_else(bad)?;
            let zip = crx.get(16 + key_len + sig_len..).ok_or_else(bad)?;
            Ok((zip, Some(key.to_vec())))
        }
        Some(3) => {
            let len = u32_at(crx, 8).ok_or_else(bad)?;
            let header = crx.get(12..12 + len).ok_or_else(bad)?;
            let zip = crx.get(12 + len..).ok_or_else(bad)?;
            let key = fields(header)
                .into_iter()
                .filter(|(n, _)| *n == 2 || *n == 3)
                .flat_map(|(_, proof)| fields(proof).into_iter().filter(|(n, _)| *n == 1).map(|(_, k)| k))
                .find(|k| id_of(k).is_ok_and(|i| i == id));
            Ok((zip, key.map(<[u8]>::to_vec)))
        }
        _ => Err("this extension package version is not supported".into()),
    }
}

/// Standard base64 with padding (the manifest's `key`).
pub(crate) fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = c.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= c.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// Download extension `id` from the store and unpack it into `root/<id>` (replacing an older
/// copy). `version` is the browser version the store should pick a compatible release for.
pub(crate) fn install(id: &str, version: &str, root: &Path) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("not an extension id".into());
    }
    let crx = platform::http::download(&crx_url(id, version), 180_000, MAX_CRX).map_err(|e| format!("downloading the extension: {e}"))?;
    let (zip, key) = unpack_crx(&crx, id)?;
    let dir = root.join(id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("replacing {}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let archive = root.join(format!("{id}.zip"));
    std::fs::write(&archive, zip).map_err(|e| e.to_string())?;
    // Windows' own bsdtar unpacks ZIP files and refuses absolute paths and ".." entries. Called
    // by full path: another tar on PATH (Git's) cannot read ZIP files.
    let tar = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32").join("tar.exe");
    let out = std::process::Command::new(tar).arg("-xf").arg(&archive).arg("-C").arg(&dir).output();
    let _ = std::fs::remove_file(&archive);
    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => return Err(format!("unpacking the extension: {}", String::from_utf8_lossy(&o.stderr).trim())),
        Err(e) => return Err(format!("unpacking the extension: {e}")),
    }
    // The store's signature folder; names starting with "_" are refused by the unpacked loader.
    let _ = std::fs::remove_dir_all(dir.join("_metadata"));
    let manifest_path = dir.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path).map_err(|_| "the extension has no manifest.json".to_string())?;
    if let (Some(key), Ok(Json::Object(mut m))) = (key, serde_json::from_str::<Json>(text.trim_start_matches('\u{feff}'))) {
        if !m.contains_key("key") {
            m.insert("key".into(), Json::String(base64(&key)));
            std::fs::write(&manifest_path, Json::Object(m).to_string()).map_err(|e| e.to_string())?;
        }
    }
    Ok(dir)
}

/// The page an installed extension shows from its toolbar button (`action.default_popup`, or
/// the Manifest V2 names), else its options page: `(page, is_popup)`.
pub(crate) fn button_page(folder: &Path) -> Option<(String, bool)> {
    let text = std::fs::read_to_string(folder.join("manifest.json")).ok()?;
    let m: Json = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let page = |v: &Json| v.as_str().filter(|s| !s.is_empty()).map(|s| s.trim_start_matches('/').to_string());
    ["action", "browser_action", "page_action"]
        .iter()
        .find_map(|k| page(&m[k]["default_popup"]))
        .map(|p| (p, true))
        .or_else(|| page(&m["options_ui"]["page"]).or_else(|| page(&m["options_page"])).map(|p| (p, false)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_urls() {
        assert!(valid_id("cjpalhdlnbpafiamejdnhcphjbkeiagm"));
        assert!(!valid_id("cjpalhdlnbpafiamejdnhcphjbkeiagz"));
        assert!(!valid_id("../x"));
        assert!(crx_url("abc", "157.0.0.0").contains("prodversion=157.0.0.0") && crx_url("abc", "1").contains("id%3Dabc%26uc"));
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn crx3_gives_the_zip_and_the_key_matching_the_id() {
        let (key, other) = (b"public-key-bytes".to_vec(), b"publisher-key".to_vec());
        let id = id_of(&key).unwrap();
        assert!(valid_id(&id));
        // AsymmetricKeyProof { public_key = 1; signature = 2 }
        let proof = |k: &[u8]| [vec![0x0a, k.len() as u8], k.to_vec(), vec![0x12, 2, 9, 9]].concat();
        let (p1, p2) = (proof(&other), proof(&key));
        let header = [vec![0x12, p1.len() as u8], p1, vec![0x12, p2.len() as u8], p2, vec![0x82, 0xf1, 0x04, 1, 7]].concat();
        let crx = [b"Cr24".to_vec(), 3u32.to_le_bytes().to_vec(), (header.len() as u32).to_le_bytes().to_vec(), header, b"PK-zip".to_vec()].concat();
        let (zip, found) = unpack_crx(&crx, &id).unwrap();
        assert_eq!(zip, b"PK-zip");
        assert_eq!(found, Some(key));
        assert_eq!(unpack_crx(&crx, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap().1, None);
        assert!(unpack_crx(b"PK\x03\x04", &id).is_err());
        assert!(unpack_crx(&crx[..14], &id).is_err());
    }

    /// Network: `cargo test -p app -- --ignored store_install`.
    #[test]
    #[ignore]
    fn store_install_real_extension() {
        let root = std::env::temp_dir().join(format!("kk-store-install-{}", std::process::id()));
        let id = "ddkjiahejlhfcafbddmgiahcphecmpfh"; // uBlock Origin Lite
        let dir = install(id, "157.0.0.0", &root).unwrap();
        let m: Json = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
        // Only a header key whose id is the store id is written (see unpack_crx).
        assert!(m["key"].as_str().is_some_and(|k| !k.is_empty()), "store key kept");
        assert!(!dir.join("_metadata").exists());
        assert!(button_page(&dir).is_some_and(|(_, popup)| popup));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn button_page_prefers_the_popup() {
        let dir = std::env::temp_dir().join(format!("kk-webstore-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |m: &str| std::fs::write(dir.join("manifest.json"), m).unwrap();
        write(r#"{"action":{"default_popup":"popup.html"},"options_page":"o.html"}"#);
        assert_eq!(button_page(&dir), Some(("popup.html".into(), true)));
        write(r#"{"browser_action":{},"options_ui":{"page":"/opts.html"}}"#);
        assert_eq!(button_page(&dir), Some(("opts.html".into(), false)));
        write(r#"{"name":"x"}"#);
        assert_eq!(button_page(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
