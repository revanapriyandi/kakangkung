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
use std::sync::atomic::{AtomicU64, Ordering};

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

/// Download extension `id` into a unique staging folder, preserving installed files. `version` is the browser version the store should pick a compatible release for.
pub(crate) fn install(id: &str, version: &str, root: &Path) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("not an extension id".into());
    }
    let crx = platform::http::download(&crx_url(id, version), 180_000, MAX_CRX).map_err(|e| format!("downloading the extension: {e}"))?;
    let (zip, key) = unpack_crx(&crx, id)?;
    prepare_package(id, zip, key, root)
}

fn prepare_package(id: &str, zip: &[u8], key: Option<Vec<u8>>, root: &Path) -> Result<PathBuf, String> {
    let key = key.ok_or("the package has no public key matching its Chrome Web Store id")?;
    if id_of(&key)? != id {
        return Err("the package public key does not match its Chrome Web Store id".into());
    }
    let dir = staging_dir(root, id)?;
    let result = unpack_zip(zip, &key, &dir);
    let _ = std::fs::remove_file(dir.with_extension("zip"));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result.map(|_| dir)
}

fn staging_dir(root: &Path, id: &str) -> Result<PathBuf, String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let staging = root.join(".staging");
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    loop {
        let dir = staging.join(format!("{id}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn unpack_zip(zip: &[u8], key: &[u8], dir: &Path) -> Result<(), String> {
    let archive = dir.with_extension("zip");
    std::fs::write(&archive, zip).map_err(|e| e.to_string())?;
    // Windows' own bsdtar unpacks ZIP files and refuses absolute paths and ".." entries. Called
    // by full path: another tar on PATH (Git's) cannot read ZIP files.
    let tar = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32").join("tar.exe");
    let out = std::process::Command::new(tar).arg("-xf").arg(&archive).arg("-C").arg(dir).output();
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
    let mut m = serde_json::from_str::<Json>(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("the extension manifest is invalid: {e}"))?;
    let m = m.as_object_mut().ok_or("the extension manifest must be an object")?;
    if !matches!(m.get("manifest_version").and_then(Json::as_u64), Some(2 | 3)) {
        return Err("this extension manifest version is not supported".into());
    }
    // The selected header key determines the store identity, including when an
    // existing manifest key was stale or belonged to another build.
    m.insert("key".into(), Json::String(base64(key)));
    std::fs::write(&manifest_path, serde_json::to_vec(m).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(())
}

/// A promoted package keeps the previous files until the engine accepts it.
pub(crate) struct InstallSwap {
    pub(crate) folder: PathBuf,
    backup: Option<PathBuf>,
}

impl InstallSwap {
    pub(crate) fn finish(self) {
        if let Some(backup) = self.backup {
            let _ = std::fs::remove_dir_all(backup);
        }
    }

    pub(crate) fn rollback(self) -> Result<(), String> {
        std::fs::remove_dir_all(&self.folder).map_err(|e| format!("removing rejected extension: {e}"))?;
        if let Some(backup) = self.backup {
            std::fs::rename(backup, &self.folder).map_err(|e| format!("restoring previous extension: {e}"))?;
        }
        Ok(())
    }
}

pub(crate) fn commit_install(staged: &Path, id: &str, root: &Path) -> Result<InstallSwap, String> {
    if !valid_id(id) { return Err("not an extension id".into()); }
    let staging = root.join(".staging").canonicalize().map_err(|e| e.to_string())?;
    let staged = staged.canonicalize().map_err(|e| e.to_string())?;
    if staged.parent() != Some(staging.as_path()) || !staged.join("manifest.json").is_file() {
        return Err("invalid staged extension folder".into());
    }
    let folder = root.join(id);
    let backup = if folder.exists() {
        let backup = staging_dir(root, id)?;
        std::fs::remove_dir(&backup).map_err(|e| e.to_string())?;
        std::fs::rename(&folder, &backup).map_err(|e| format!("preserving previous extension: {e}"))?;
        Some(backup)
    } else { None };
    if let Err(e) = std::fs::rename(&staged, &folder) {
        if let Some(backup) = &backup {
            std::fs::rename(backup, &folder).map_err(|restore| format!("install failed ({e}); restoring previous extension failed ({restore})"))?;
        }
        return Err(format!("activating extension files: {e}"));
    }
    Ok(InstallSwap { folder, backup })
}

/// The page an installed extension shows from its toolbar button (`action.default_popup`, or
/// the Manifest V2 names), else its options page: `(page, is_popup)`.
pub(crate) fn button_page(folder: &Path) -> Option<(String, bool)> {
    let m = manifest(folder)?;
    ["action", "browser_action", "page_action"]
        .iter()
        .find_map(|k| manifest_page(&m[k]["default_popup"]))
        .map(|p| (p, true))
        .or_else(|| options_of(&m).map(|p| (p, false)))
}

/// An installed extension's options page (`options_ui.page`, else `options_page`).
pub(crate) fn options_page(folder: &Path) -> Option<String> {
    options_of(&manifest(folder)?)
}

/// An installed extension's name as Chrome shows it: `name`, with a `__MSG_key__` name read
/// from the default locale's messages.json.
pub(crate) fn display_name(folder: &Path) -> Option<String> {
    let m = manifest(folder)?;
    let name = m["name"].as_str()?.trim();
    let Some(key) = name.strip_prefix("__MSG_").and_then(|k| k.strip_suffix("__")) else { return Some(name.to_string()) };
    let locale = m["default_locale"].as_str().unwrap_or("en");
    let text = std::fs::read_to_string(folder.join("_locales").join(locale).join("messages.json")).ok()?;
    let msgs: Json = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    // Message keys are case-insensitive.
    let msgs = msgs.as_object()?;
    msgs.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).and_then(|(_, v)| v["message"].as_str()).map(str::to_string)
}

fn manifest(folder: &Path) -> Option<Json> {
    let text = std::fs::read_to_string(folder.join("manifest.json")).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

fn manifest_page(v: &Json) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(|s| s.trim_start_matches('/').to_string())
}

fn options_of(m: &Json) -> Option<String> {
    manifest_page(&m["options_ui"]["page"]).or_else(|| manifest_page(&m["options_page"]))
}

/// What an unpacked extension may do, in Chrome's words for its install dialog ("It can:"),
/// from `permissions`, `host_permissions` (Manifest V2 keeps hosts in `permissions`) and the
/// pages its content scripts run on. Permissions Chrome shows no warning for are left out.
pub(crate) fn permission_warnings(folder: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(folder.join("manifest.json")) else { return vec![] };
    let Ok(m) = serde_json::from_str::<Json>(text.trim_start_matches('\u{feff}')) else { return vec![] };
    let strings = |v: &Json| v.as_array().map(|a| a.iter().filter_map(Json::as_str).map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
    let perms = strings(&m["permissions"]);
    let mut hosts: Vec<String> = strings(&m["host_permissions"]);
    hosts.extend(perms.iter().filter(|p| p.contains("://") || *p == "<all_urls>").cloned());
    if let Some(scripts) = m["content_scripts"].as_array() {
        for s in scripts {
            hosts.extend(strings(&s["matches"]));
        }
    }
    let all = |h: &str| matches!(h, "<all_urls>" | "*://*/*" | "http://*/*" | "https://*/*");
    let mut out = vec![];
    if hosts.iter().any(|h| all(h)) {
        out.push("Read and change all your data on all websites".to_string());
    } else {
        let mut sites: Vec<String> = hosts
            .iter()
            .filter_map(|h| h.split_once("://").map(|(_, r)| r.split('/').next().unwrap_or("").trim_start_matches("*.").to_string()))
            .filter(|h| !h.is_empty())
            .collect();
        sites.sort();
        sites.dedup();
        match sites.len() {
            0 => {}
            1..=3 => out.push(format!("Read and change your data on {}", sites.join(", "))),
            n => out.push(format!("Read and change your data on {} and {} other sites", sites[..2].join(", "), n - 2)),
        }
    }
    const WARNINGS: &[(&str, &str)] = &[
        ("debugger", "Access the page debugger backend"),
        ("history", "Read and change your browsing history on all your signed-in devices"),
        ("tabs", "Read your browsing history"),
        ("webNavigation", "Read your browsing history"),
        ("bookmarks", "Read and change your bookmarks"),
        ("downloads", "Manage your downloads"),
        ("management", "Manage your apps, extensions, and themes"),
        ("nativeMessaging", "Communicate with cooperating native applications"),
        ("privacy", "Change your privacy-related settings"),
        ("proxy", "Read and change all your data on all websites"),
        ("declarativeNetRequest", "Block content on any page"),
        ("geolocation", "Detect your physical location"),
        ("clipboardRead", "Read data you copy and paste"),
        ("clipboardWrite", "Modify data you copy and paste"),
        ("notifications", "Display notifications"),
        ("contentSettings", "Change your settings that control websites' access to features such as cookies, JavaScript, plugins, geolocation, microphone, camera etc."),
        ("topSites", "Read a list of your most frequently visited websites"),
        ("pageCapture", "Read and change all your data on all websites"),
    ];
    for (perm, text) in WARNINGS {
        if perms.iter().any(|p| p == perm) && !out.iter().any(|o| o == text) {
            out.push(text.to_string());
        }
    }
    out
}

/// The icon an installed extension shows on its toolbar button: `action.default_icon` (or the
/// Manifest V2 names, or `icons`), the size closest to 32 px; a file inside `folder`.
pub(crate) fn button_icon(folder: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(folder.join("manifest.json")).ok()?;
    let m: Json = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let pick = |v: &Json| -> Option<String> {
        if let Some(s) = v.as_str() {
            return Some(s.to_string());
        }
        let sizes = v.as_object()?;
        sizes.iter().filter_map(|(k, p)| Some((k.parse::<i32>().ok()?, p.as_str()?))).min_by_key(|(k, _)| (k - 32).abs() + if *k < 32 { 100 } else { 0 }).map(|(_, p)| p.to_string())
    };
    let rel = ["action", "browser_action", "page_action"].iter().find_map(|k| pick(&m[k]["default_icon"])).or_else(|| pick(&m["icons"]))?;
    let rel = rel.trim_start_matches('/');
    // Never a file outside the extension's folder.
    (!rel.split(['/', '\\']).any(|p| p == "..")).then(|| folder.join(rel)).filter(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_package_and_cancelled_staging_preserve_installed_files() {
        let root = std::env::temp_dir().join(format!("kk-stage-preserve-{}", std::process::id()));
        let key = b"synthetic-key";
        let id = id_of(key).unwrap();
        let old = root.join(&id);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("sentinel"), b"previous installation").unwrap();
        assert!(prepare_package(&id, b"not a zip", Some(key.to_vec()), &root).is_err());
        assert_eq!(std::fs::read(old.join("sentinel")).unwrap(), b"previous installation");
        assert!(prepare_package(&id, b"PK", None, &root).is_err());
        let staged = staging_dir(&root, &id).unwrap();
        std::fs::remove_dir_all(staged).unwrap();
        assert!(old.join("sentinel").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn promotion_can_restore_previous_installation_after_engine_rejection() {
        let root = std::env::temp_dir().join(format!("kk-stage-rollback-{}", std::process::id()));
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let old = root.join(id);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("sentinel"), b"old").unwrap();
        let staged = staging_dir(&root, id).unwrap();
        std::fs::write(staged.join("manifest.json"), b"{}").unwrap();
        let swap = commit_install(&staged, id, &root).unwrap();
        assert!(!swap.folder.join("sentinel").exists());
        swap.rollback().unwrap();
        assert_eq!(std::fs::read(old.join("sentinel")).unwrap(), b"old");
        assert!(commit_install(&old, id, &root).is_err(), "a live install is never accepted as staging");
        let staged = staging_dir(&root, id).unwrap();
        std::fs::write(staged.join("manifest.json"), b"{}").unwrap();
        let swap = commit_install(&staged, id, &root).unwrap();
        let backup = swap.backup.clone().unwrap();
        swap.finish();
        assert!(old.join("manifest.json").exists());
        assert!(!backup.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn permission_warnings_use_chromes_words() {
        let dir = std::env::temp_dir().join(format!("kk-perm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let warn = |m: &str| {
            std::fs::write(dir.join("manifest.json"), m).unwrap();
            permission_warnings(&dir)
        };
        assert_eq!(warn(r#"{"permissions":["storage","declarativeNetRequest"],"host_permissions":["<all_urls>"]}"#), ["Read and change all your data on all websites", "Block content on any page"]);
        assert_eq!(warn(r#"{"permissions":["tabs","webNavigation"],"content_scripts":[{"matches":["https://*.github.com/*"]}]}"#), ["Read and change your data on github.com", "Read your browsing history"]);
        assert_eq!(warn(r#"{"host_permissions":["https://a.com/*","https://b.com/*","https://c.com/*","https://d.com/*"]}"#), ["Read and change your data on a.com, b.com and 2 other sites"]);
        assert!(warn(r#"{"permissions":["storage","alarms"]}"#).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn button_icon_prefers_32_px_and_stays_inside() {
        let dir = std::env::temp_dir().join(format!("kk-icon-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("img")).unwrap();
        for f in ["img/16.png", "img/48.png", "img/128.png", "i.png"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let write = |m: &str| std::fs::write(dir.join("manifest.json"), m).unwrap();
        write(r#"{"action":{"default_icon":{"16":"img/16.png","48":"img/48.png","128":"img/128.png"}}}"#);
        assert_eq!(button_icon(&dir), Some(dir.join("img/48.png")));
        write(r#"{"browser_action":{"default_icon":"/i.png"}}"#);
        assert_eq!(button_icon(&dir), Some(dir.join("i.png")));
        write(r#"{"icons":{"16":"img/16.png"}}"#);
        assert_eq!(button_icon(&dir), Some(dir.join("img/16.png")));
        write(r#"{"icons":{"16":"../evil.png"}}"#);
        assert_eq!(button_icon(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        let keep = std::env::var_os("KAKANGKUNG_STORE_TEST_OUTPUT");
        let root = keep.as_ref().map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join(format!("kk-store-install-{}", std::process::id())));
        let id = "ddkjiahejlhfcafbddmgiahcphecmpfh"; // uBlock Origin Lite
        let dir = install(id, "157.0.0.0", &root).unwrap();
        let m: Json = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
        // Only a header key whose id is the store id is written (see unpack_crx).
        assert!(m["key"].as_str().is_some_and(|k| !k.is_empty()), "store key kept");
        assert!(!dir.join("_metadata").exists());
        assert!(button_page(&dir).is_some_and(|(_, popup)| popup));
        if keep.is_some() { println!("STAGED_EXTENSION={}", dir.display()); }
        else { let _ = std::fs::remove_dir_all(&root); }
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

    #[test]
    fn menu_reads_name_and_options() {
        let dir = std::env::temp_dir().join(format!("kk-webstore-name-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("_locales/id")).unwrap();
        let write = |m: &str| std::fs::write(dir.join("manifest.json"), m).unwrap();
        write(r#"{"name":"Plain","action":{"default_popup":"p.html"},"options_ui":{"page":"o.html"}}"#);
        assert_eq!(display_name(&dir).as_deref(), Some("Plain"));
        assert_eq!(options_page(&dir).as_deref(), Some("o.html"));
        std::fs::write(dir.join("_locales/id/messages.json"), r#"{"AppName":{"message":"Nama Lokal"}}"#).unwrap();
        write(r#"{"name":"__MSG_appName__","default_locale":"id"}"#);
        assert_eq!(display_name(&dir).as_deref(), Some("Nama Lokal"));
        assert_eq!(options_page(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
