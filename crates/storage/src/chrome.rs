//! Reading other browsers' profiles on this PC: Chrome and the Chromium family (Edge, Brave,
//! Vivaldi, Opera, Chromium) share one file layout; Firefox keeps history and bookmarks in
//! `places.sqlite`. Read-only: the other browsers' files are never modified (copies are read).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use platform::sqlite::Db;
use serde::Serialize;
use serde_json::Value as Json;

use crate::Visit;

/// Microseconds between 1601-01-01 (Chrome's epoch) and 1970-01-01.
const WEBKIT_EPOCH_OFFSET_US: i64 = 11_644_473_600_000_000;

pub(crate) struct Snapshot {
    dir: PathBuf,
    pub(crate) path: PathBuf,
}

impl Snapshot {
    pub(crate) fn read(source: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        let dir = loop {
            let dir = base.join(format!("kakangkung-snapshot-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            match std::fs::create_dir(&dir) {
                Ok(()) => break dir,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        };
        let snapshot = Self { path: dir.join("data.db"), dir };
        Db::snapshot(source, &snapshot.path).map_err(|e| format!("could not snapshot browser data ({e}). Close that browser and try again."))?;
        Ok(snapshot)
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChromeExtension {
    pub id: String,
    pub version: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChromeProfile {
    /// Folder name under "User Data", e.g. "Default" or "Profile 3".
    pub dir: String,
    /// Name the user gave the profile in Chrome.
    pub name: String,
    pub history: String,
    #[serde(rename = "historyBytes")]
    pub history_bytes: u64,
    pub extensions: Vec<ChromeExtension>,
}

/// A browser found on this PC.
#[derive(Debug, Clone, Serialize)]
pub struct Browser {
    pub id: &'static str,
    pub name: &'static str,
    #[serde(skip)]
    pub dir: PathBuf,
    /// Chromium-family profile layout (History/Bookmarks/Extensions); false = Firefox.
    pub chromium: bool,
    pub profiles: Vec<ChromeProfile>,
}

/// Known browsers: (id, name, base folder env var, path below it, Chromium family?).
const KNOWN: [(&str, &str, &str, &str, bool); 8] = [
    ("chrome", "Google Chrome", "LOCALAPPDATA", "Google\\Chrome\\User Data", true),
    ("edge", "Microsoft Edge", "LOCALAPPDATA", "Microsoft\\Edge\\User Data", true),
    ("brave", "Brave", "LOCALAPPDATA", "BraveSoftware\\Brave-Browser\\User Data", true),
    ("vivaldi", "Vivaldi", "LOCALAPPDATA", "Vivaldi\\User Data", true),
    ("chromium", "Chromium", "LOCALAPPDATA", "Chromium\\User Data", true),
    ("opera", "Opera", "APPDATA", "Opera Software\\Opera Stable", true),
    ("operagx", "Opera GX", "APPDATA", "Opera Software\\Opera GX Stable", true),
    ("firefox", "Mozilla Firefox", "APPDATA", "Mozilla\\Firefox\\Profiles", false),
];

/// Where a browser exports its saved passwords to a CSV file: (program, page). Chrome-family
/// browsers lock new passwords to themselves, so this is how they come over.
pub fn password_export_page(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "chrome" => Some(("chrome.exe", "chrome://password-manager/settings")),
        "edge" => Some(("msedge.exe", "edge://settings/passwords")),
        "brave" => Some(("brave.exe", "brave://password-manager/settings")),
        _ => None,
    }
}

/// Every supported browser that has data for this Windows user.
pub fn browsers() -> Vec<Browser> {
    KNOWN
        .iter()
        .filter_map(|&(id, name, env, rel, chromium)| {
            let dir = PathBuf::from(std::env::var_os(env)?).join(rel);
            if !dir.is_dir() {
                return None;
            }
            let profiles = if chromium { profiles(&dir) } else { firefox_profiles(&dir) };
            (!profiles.is_empty()).then_some(Browser { id, name, dir, chromium, profiles })
        })
        .collect()
}

/// One browser by id.
pub fn browser(id: &str) -> Option<Browser> {
    browsers().into_iter().find(|b| b.id == id)
}

/// Metadata only: avoids copying and parsing an unchanged profile every sync round.
pub fn data_stamp(browser: &Browser, profile: &str) -> Result<String, String> {
    let selected = browser.profiles.iter().find(|p| p.dir == profile).ok_or("unknown profile")?;
    let dir = browser.dir.join(&selected.dir);
    let files: &[&str] = if browser.chromium { &["History", "History-wal", "Bookmarks"] } else { &["places.sqlite", "places.sqlite-wal"] };
    let mut stamp = format!("{}:{profile}", browser.dir.display());
    for name in files {
        match std::fs::metadata(dir.join(name)) {
            Ok(m) => {
                let changed = m.modified().map_err(|e| e.to_string())?.duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?;
                stamp.push_str(&format!("|{name}:{}:{}", m.len(), changed.as_nanos()));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => stamp.push_str(&format!("|{name}:missing")),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(stamp)
}

/// The profile to use when none is named: Chromium's last used one, Firefox's most recently used.
pub fn default_profile(b: &Browser) -> Option<String> {
    if b.chromium {
        let d = last_used_profile(&b.dir);
        return b.profiles.iter().any(|p| p.dir == d).then_some(d).or_else(|| b.profiles.first().map(|p| p.dir.clone()));
    }
    b.profiles.iter().max_by_key(|p| p.history_bytes).map(|p| p.dir.clone())
}

/// Firefox profiles: every folder with a places.sqlite.
fn firefox_profiles(dir: &Path) -> Vec<ChromeProfile> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("places.sqlite").is_file())
        .map(|e| {
            let d = e.file_name().to_string_lossy().into_owned();
            let places = e.path().join("places.sqlite");
            ChromeProfile {
                name: d.split_once('.').map(|(_, n)| n.to_string()).unwrap_or_else(|| d.clone()),
                history: places.to_string_lossy().into_owned(),
                history_bytes: places.metadata().map(|m| m.len()).unwrap_or(0),
                extensions: vec![],
                dir: d,
            }
        })
        .collect()
}

/// History and bookmarks from a Firefox `places.sqlite` (read from a copy, WAL file included).
pub fn read_firefox(places: &Path) -> Result<(Vec<Visit>, Vec<crate::Bookmark>), String> {
    let copy = Snapshot::read(places)?;
    let read = || -> Result<(Vec<Visit>, Vec<crate::Bookmark>), String> {
        let db = Db::open(&copy.path, true)?;
        let visits = db
            .query(
                "SELECT url, COALESCE(title, ''), visit_count, last_visit_date FROM moz_places
                 WHERE hidden = 0 AND last_visit_date IS NOT NULL ORDER BY last_visit_date DESC LIMIT 100000",
                &[],
            )?
            .into_iter()
            .map(|r| Visit { url: r[0].text(), title: r[1].text(), count: r[2].int().max(1), last: r[3].int() / 1000 })
            .filter(|v| crate::storable(&v.url))
            .collect();
        let marks = db
            .query(
                "SELECT p.url, COALESCE(b.title, ''), b.dateAdded FROM moz_bookmarks b JOIN moz_places p ON p.id = b.fk WHERE b.type = 1",
                &[],
            )?
            .into_iter()
            .map(|r| crate::Bookmark { url: r[0].text(), title: r[1].text(), added: r[2].int() / 1000 })
            .filter(|b| crate::storable(&b.url))
            .collect();
        Ok((visits, marks))
    };
    read()
}

pub fn user_data_dir() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Google").join("Chrome").join("User Data");
    p.is_dir().then_some(p)
}

/// Profiles that have a History file, named as in Chrome's "Local State".
pub fn profiles(user_data: &Path) -> Vec<ChromeProfile> {
    let names: Json = std::fs::read_to_string(user_data.join("Local State"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Json::Null);
    let mut out: Vec<ChromeProfile> = std::fs::read_dir(user_data)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("History").is_file())
        .filter_map(|e| {
            let dir = e.file_name().to_string_lossy().into_owned();
            if dir == "Guest Profile" || dir == "System Profile" {
                return None;
            }
            let name = names["profile"]["info_cache"][&dir]["name"].as_str().unwrap_or(&dir).to_string();
            Some(ChromeProfile {
                history: e.path().join("History").to_string_lossy().into_owned(),
                history_bytes: e.path().join("History").metadata().map(|m| m.len()).unwrap_or(0),
                extensions: extensions(&e.path()),
                dir,
                name,
            })
        })
        .collect();
    out.sort_by_key(|p| (p.dir != "Default", p.dir.clone()));
    out
}

/// The profile Chrome used last (`profile.last_used` in "Local State"), else "Default".
pub fn last_used_profile(user_data: &Path) -> String {
    std::fs::read_to_string(user_data.join("Local State"))
        .ok()
        .and_then(|s| serde_json::from_str::<Json>(&s).ok())
        .and_then(|v| v["profile"]["last_used"].as_str().map(str::to_string))
        .filter(|d| !d.is_empty() && !d.contains(['/', '\\']) && user_data.join(d).is_dir())
        .unwrap_or_else(|| "Default".into())
}

/// Bookmarks from Chrome's `Bookmarks` JSON file (bookmark bar, other, mobile), oldest first.
pub fn read_bookmarks(file: &Path) -> Result<Vec<crate::Bookmark>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("reading Chrome bookmarks: {e}"))?;
    let v: Json = serde_json::from_str(&text).map_err(|e| format!("Chrome bookmarks file: {e}"))?;
    fn walk(n: &Json, out: &mut Vec<crate::Bookmark>) {
        if n["type"] == "url" {
            let url = n["url"].as_str().unwrap_or("");
            if crate::storable(url) {
                let us: i64 = n["date_added"].as_str().and_then(|d| d.parse().ok()).unwrap_or(0);
                out.push(crate::Bookmark {
                    url: url.into(),
                    title: n["name"].as_str().unwrap_or("").into(),
                    added: (us - WEBKIT_EPOCH_OFFSET_US).max(0) / 1000,
                });
            }
        }
        for child in n["children"].as_array().into_iter().flatten() {
            walk(child, out);
        }
    }
    let mut out = vec![];
    if let Some(roots) = v["roots"].as_object() {
        for root in roots.values() {
            walk(root, &mut out);
        }
    }
    out.sort_by_key(|b| b.added);
    Ok(out)
}

/// Saved passwords of a Chromium-family profile (`Login Data`, read from a copy). They are
/// sealed with the browser's key from "Local State", itself sealed with DPAPI for this Windows
/// user. Chrome 127+ seals new ones with a key only Chrome can reach ("v20", app-bound): those
/// are counted as locked. Returns (logins, locked).
pub fn read_logins(user_data: &Path, profile: &Path) -> Result<(Vec<crate::passwords::Credential>, usize), String> {
    let file = profile.join("Login Data");
    if !file.is_file() {
        return Ok((vec![], 0));
    }
    let key = std::fs::read_to_string(user_data.join("Local State"))
        .ok()
        .and_then(|s| serde_json::from_str::<Json>(&s).ok())
        .and_then(|v| v["os_crypt"]["encrypted_key"].as_str().and_then(platform::base64_decode))
        .and_then(|k| k.strip_prefix(b"DPAPI").map(<[u8]>::to_vec))
        .and_then(|k| platform::unprotect(&k).ok());
    let copy = Snapshot::read(&file)?;
    let rows = Db::open(&copy.path, true).and_then(|db| {
        db.query("SELECT origin_url, username_value, hex(password_value) FROM logins WHERE blacklisted_by_user = 0", &[])
    });
    let (mut out, mut locked) = (vec![], 0);
    for r in rows? {
        let hex = r[2].text();
        let blob: Vec<u8> = (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()).collect();
        let password = match blob.get(..3) {
            Some(b"v10" | b"v11") => key.as_deref().filter(|_| blob.len() > 15).and_then(|k| platform::aes_gcm_decrypt(k, &blob[3..15], &blob[15..])),
            Some(b"v20") => None,
            _ => platform::unprotect(&blob).ok(),
        };
        let Some(origin) = crate::passwords::origin_of(&r[0].text()) else { continue };
        match password.and_then(|p| String::from_utf8(p).ok()) {
            Some(password) if !password.is_empty() => out.push(crate::passwords::Credential { origin, username: r[1].text(), password }),
            Some(_) => {}
            None => locked += 1,
        }
    }
    Ok((out, locked))
}

/// Read Chrome's `urls` table from a (copied) History database.
pub fn read_history(file: &Path) -> Result<Vec<Visit>, String> {
    let db = Db::open(file, true)?;
    let rows = db.query(
        "SELECT url, title, visit_count, last_visit_time FROM urls WHERE hidden = 0 ORDER BY last_visit_time DESC LIMIT 100000",
        &[],
    )?;
    Ok(rows
        .into_iter()
        .map(|r| Visit {
            url: r[0].text(),
            title: r[1].text(),
            count: r[2].int().max(1),
            last: (r[3].int() - WEBKIT_EPOCH_OFFSET_US).max(0) / 1000,
        })
        .filter(|v| crate::storable(&v.url))
        .collect())
}

/// Installed extensions of one profile: `Extensions/<id>/<version>/manifest.json`.
pub fn extensions(profile: &Path) -> Vec<ChromeExtension> {
    let mut out = vec![];
    for ext in std::fs::read_dir(profile.join("Extensions")).into_iter().flatten().flatten() {
        let id = ext.file_name().to_string_lossy().into_owned();
        if id == "Temp" {
            continue;
        }
        // Newest version folder that has a manifest.
        let mut versions: Vec<PathBuf> = std::fs::read_dir(ext.path())
            .into_iter()
            .flatten()
            .flatten()
            .map(|v| v.path())
            .filter(|v| v.join("manifest.json").is_file())
            .collect();
        versions.sort();
        let Some(dir) = versions.pop() else { continue };
        let Some(manifest) = std::fs::read_to_string(dir.join("manifest.json")).ok().and_then(|s| serde_json::from_str::<Json>(&s).ok())
        else {
            continue;
        };
        let raw = manifest["name"].as_str().unwrap_or(&id).to_string();
        out.push(ChromeExtension {
            name: localize(&dir, &manifest, &raw),
            version: manifest["version"].as_str().unwrap_or("").to_string(),
            path: dir.to_string_lossy().into_owned(),
            id,
        });
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

/// Resolve `__MSG_key__` names through `_locales/<default_locale>/messages.json`.
fn localize(dir: &Path, manifest: &Json, raw: &str) -> String {
    let Some(key) = raw.strip_prefix("__MSG_").and_then(|k| k.strip_suffix("__")) else {
        return raw.to_string();
    };
    let locale = manifest["default_locale"].as_str().unwrap_or("en");
    let messages: Json = std::fs::read_to_string(dir.join("_locales").join(locale).join("messages.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Json::Null);
    messages
        .as_object()
        .and_then(|m| m.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)))
        .and_then(|(_, v)| v["message"].as_str())
        .unwrap_or(raw)
        .to_string()
}

/// Copy an extension folder into Kakangkung's own extensions folder (so Chrome updating or removing it
/// later does not break Kakangkung). `_metadata` (Chrome Web Store signatures) is left out.
pub fn copy_extension(src: &Path, dest_root: &Path) -> Result<PathBuf, String> {
    let id = src.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "ext".into());
    let dest = dest_root.join(&id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| format!("replacing {}: {e}", dest.display()))?;
    }
    copy_dir(src, &dest).map_err(|e| format!("copying extension: {e}"))?;
    Ok(dest)
}

fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "_metadata" {
            continue;
        }
        let (from, to) = (entry.path(), dest.join(&name));
        if entry.file_type()?.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("kakangkung-chrome-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn profiles_extensions_and_history() {
        let ud = tree("ud");
        let prof = ud.join("Profile 3");
        let ext = prof.join("Extensions").join("abcdefgh").join("1.2.0_0");
        std::fs::create_dir_all(ext.join("_locales").join("en")).unwrap();
        std::fs::create_dir_all(ext.join("_metadata")).unwrap();
        std::fs::write(ext.join("manifest.json"), r#"{"name":"__MSG_appName__","version":"1.2.0","default_locale":"en"}"#).unwrap();
        std::fs::write(ext.join("_locales/en/messages.json"), r#"{"APPNAME":{"message":"Dark Reader"}}"#).unwrap();
        std::fs::write(ud.join("Local State"), r#"{"profile":{"info_cache":{"Profile 3":{"name":"Kerja"}}}}"#).unwrap();

        // A History file in Chrome's schema.
        let hist = prof.join("History");
        let db = Db::open(&hist, false).unwrap();
        db.batch("CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, typed_count INTEGER, last_visit_time INTEGER, hidden INTEGER)").unwrap();
        // 2024-01-01T00:00:00Z = 1704067200000 ms.
        let t = 1_704_067_200_000_000 + WEBKIT_EPOCH_OFFSET_US;
        db.run_many(
            "INSERT INTO urls(url, title, visit_count, typed_count, last_visit_time, hidden) VALUES(?, ?, ?, 0, ?, ?)",
            [
                vec!["https://example.com/".into(), "Example".into(), 3i64.into(), t.into(), 0i64.into()],
                vec!["https://hidden.example/".into(), "".into(), 1i64.into(), t.into(), 1i64.into()],
                vec!["javascript:alert(1)".into(), "x".into(), 1i64.into(), t.into(), 0i64.into()],
            ],
        )
        .unwrap();
        drop(db);

        // Firefox: a places.sqlite in Firefox's schema.
        let ff = tree("ff").join("abc.default-release");
        std::fs::create_dir_all(&ff).unwrap();
        let fdb = Db::open(&ff.join("places.sqlite"), false).unwrap();
        fdb.batch("CREATE TABLE moz_places(id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, hidden INTEGER, last_visit_date INTEGER);
                   CREATE TABLE moz_bookmarks(id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER, title TEXT, dateAdded INTEGER)").unwrap();
        fdb.batch("INSERT INTO moz_places VALUES(1, 'https://mozilla.org/', 'Mozilla', 4, 0, 1704067200000000);
                   INSERT INTO moz_places VALUES(2, 'place:sort=8', NULL, 0, 0, 1);
                   INSERT INTO moz_bookmarks VALUES(1, 1, 1, 'Moz', 1704067200000000);
                   INSERT INTO moz_bookmarks VALUES(2, 2, NULL, 'Folder', 1)").unwrap();
        drop(fdb);
        let fps = firefox_profiles(ff.parent().unwrap());
        assert_eq!(fps[0].name, "default-release");
        let (fv, fb) = read_firefox(&ff.join("places.sqlite")).unwrap();
        assert_eq!(fv, vec![Visit { url: "https://mozilla.org/".into(), title: "Mozilla".into(), count: 4, last: 1_704_067_200_000 }]);
        assert_eq!(fb.len(), 1);
        assert_eq!(fb[0].added, 1_704_067_200_000);

        let ps = profiles(&ud);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].name, "Kerja");
        assert_eq!(ps[0].extensions.len(), 1);
        assert_eq!(ps[0].extensions[0].name, "Dark Reader");
        assert_eq!(ps[0].extensions[0].version, "1.2.0");

        let v = read_history(&hist).unwrap();
        assert_eq!(v, vec![Visit { url: "https://example.com/".into(), title: "Example".into(), count: 3, last: 1_704_067_200_000 }]);

        std::fs::write(
            prof.join("Bookmarks"),
            r#"{"roots":{"bookmark_bar":{"type":"folder","children":[
                {"type":"url","name":"Rust","url":"https://www.rust-lang.org/","date_added":"13350000000000000"},
                {"type":"folder","name":"Work","children":[{"type":"url","name":"Bad","url":"javascript:alert(1)"},
                  {"type":"url","name":"Docs","url":"https://docs.rs/","date_added":"13340000000000000"}]}]},
              "other":{"type":"folder","children":[]}}}"#,
        )
        .unwrap();
        let b = read_bookmarks(&prof.join("Bookmarks")).unwrap();
        assert_eq!(b.iter().map(|b| b.url.as_str()).collect::<Vec<_>>(), ["https://docs.rs/", "https://www.rust-lang.org/"]);
        assert_eq!(last_used_profile(&ud), "Default", "no last_used recorded");

        let dest = copy_extension(&ext, &tree("dest")).unwrap();
        assert!(dest.ends_with("abcdefgh"));
        assert!(dest.join("manifest.json").is_file());
        assert!(dest.join("_locales/en/messages.json").is_file());
        assert!(!dest.join("_metadata").exists());
    }
}
