//! Kakangkung account sync through a folder that a cloud drive (OneDrive, Google Drive, Dropbox…)
//! already replicates. Every PC signed in with the same account email uses the same sub-folder;
//! each writes `<device-id>.json` (history, bookmarks, open tabs, settings, extension names — the
//! data types the person chose) and merges the files of the other devices. No server.
//! Conflicts: newest visit wins for history, bookmarks are united, newest settings win.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{now_ms, storable, Bookmark, Store, Visit};

/// History entries each device publishes (newest first).
const HISTORY_LIMIT: i64 = 5000;
/// Ignore files larger than this (not written by Kakangkung, or damaged).
const MAX_FILE: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncTab {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeviceFile {
    version: u32,
    device: String,
    name: String,
    updated: i64,
    tabs: Vec<SyncTab>,
    history: Vec<Visit>,
    #[serde(default)]
    bookmarks: Vec<Bookmark>,
    #[serde(default)]
    settings: BTreeMap<String, String>,
    #[serde(default)]
    settings_updated: i64,
    #[serde(default)]
    extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteDevice {
    pub name: String,
    pub updated: i64,
    pub tabs: Vec<SyncTab>,
    pub extensions: Vec<String>,
}

/// What the person chose to sync (Chrome's "Manage what you sync").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Types {
    pub history: bool,
    pub bookmarks: bool,
    pub tabs: bool,
    pub settings: bool,
    pub extensions: bool,
}

impl Types {
    pub const ALL: Types = Types { history: true, bookmarks: true, tabs: true, settings: true, extensions: true };
    pub const NAMES: [&'static str; 5] = ["history", "bookmarks", "tabs", "settings", "extensions"];

    /// From a comma list like "history,bookmarks" (empty setting = everything).
    pub fn parse(list: Option<&str>) -> Types {
        let Some(list) = list else { return Types::ALL };
        let has = |n: &str| list.split(',').any(|x| x.trim() == n);
        Types { history: has("history"), bookmarks: has("bookmarks"), tabs: has("tabs"), settings: has("settings"), extensions: has("extensions") }
    }

    pub fn list(&self) -> String {
        let on = [self.history, self.bookmarks, self.tabs, self.settings, self.extensions];
        Types::NAMES.iter().zip(on).filter(|(_, o)| *o).map(|(n, _)| *n).collect::<Vec<_>>().join(",")
    }
}

/// This device's side of a sync.
#[derive(Debug, Clone, Default)]
pub struct Outgoing {
    pub tabs: Vec<SyncTab>,
    pub settings: BTreeMap<String, String>,
    /// When the settings were last changed on this device (ms).
    pub settings_updated: i64,
    pub extensions: Vec<String>,
}

/// What a sync brought in.
#[derive(Debug, Clone, Default)]
pub struct Incoming {
    pub devices: Vec<RemoteDevice>,
    /// Settings from another device that changed them more recently than this one.
    pub settings: Option<(i64, BTreeMap<String, String>)>,
    pub new_bookmarks: usize,
}

/// The account's own sub-folder of the sync folder (FNV-1a of the lower-cased email), so several
/// people can share one cloud folder without mixing data.
pub fn account_dir(folder: &Path, email: &str) -> PathBuf {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in email.trim().to_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    folder.join(format!("account-{h:016x}"))
}

/// Default folder: `%OneDrive%\Kakangkung Sync` when OneDrive is set up, else Documents.
pub fn default_folder() -> PathBuf {
    std::env::var_os("OneDrive")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents")))
        .unwrap_or_else(std::env::temp_dir)
        .join("Kakangkung Sync")
}

fn others(folder: &Path, own: &str) -> Vec<DeviceFile> {
    let mut out: Vec<DeviceFile> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter(|e| e.metadata().map(|m| m.len() <= MAX_FILE).unwrap_or(false))
        .filter_map(|e| serde_json::from_slice::<DeviceFile>(&std::fs::read(e.path()).ok()?).ok())
        .filter(|f| (f.version == 1 || f.version == 2) && f.device != own)
        .collect();
    out.sort_by_key(|f| std::cmp::Reverse(f.updated));
    out
}

fn summary(files: &[DeviceFile]) -> Vec<RemoteDevice> {
    files
        .iter()
        .map(|f| RemoteDevice {
            name: f.name.clone(),
            updated: f.updated,
            tabs: f.tabs.iter().filter(|t| storable(&t.url)).cloned().collect(),
            extensions: f.extensions.clone(),
        })
        .collect()
}

/// The other devices currently in `folder` (read only).
pub fn devices(folder: &Path, own: &str) -> Vec<RemoteDevice> {
    summary(&others(folder, own))
}

/// Merge the other devices' data (the chosen types) into `store`, then publish this device's file.
pub fn sync(store: &Store, folder: &Path, device: &str, name: &str, out: Outgoing, types: Types) -> Result<Incoming, String> {
    if device.is_empty() || device.len() > 64 || !device.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid device id".into());
    }
    std::fs::create_dir_all(folder).map_err(|e| format!("cannot use sync folder {}: {e}", folder.display()))?;
    let remote = others(folder, device);
    let mut incoming = Incoming::default();
    for f in &remote {
        if types.history {
            store.merge(&f.history)?;
        }
        if types.bookmarks {
            incoming.new_bookmarks += store.import_bookmarks(&f.bookmarks)?;
        }
    }
    if types.settings {
        incoming.settings = remote
            .iter()
            .filter(|f| !f.settings.is_empty() && f.settings_updated > out.settings_updated)
            .max_by_key(|f| f.settings_updated)
            .map(|f| (f.settings_updated, f.settings.clone()));
    }
    let mine = DeviceFile {
        version: 2,
        device: device.into(),
        name: name.into(),
        updated: now_ms(),
        tabs: if types.tabs { out.tabs.into_iter().filter(|t| storable(&t.url)).collect() } else { vec![] },
        history: if types.history { store.history("", HISTORY_LIMIT)? } else { vec![] },
        bookmarks: if types.bookmarks { store.bookmarks()? } else { vec![] },
        settings: if types.settings { out.settings } else { BTreeMap::new() },
        settings_updated: out.settings_updated,
        extensions: if types.extensions { out.extensions } else { vec![] },
    };
    let json = serde_json::to_vec(&mine).map_err(|e| e.to_string())?;
    if json.len() as u64 > MAX_FILE {
        return Err("sync data exceeds the 20 MiB device-file limit".into());
    }
    let (tmp, file) = (folder.join(format!("{device}.json.tmp")), folder.join(format!("{device}.json")));
    std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &file)).map_err(|e| format!("writing {}: {e}", file.display()))?;
    incoming.devices = summary(&remote);
    Ok(incoming)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_files_that_other_devices_cannot_read() {
        let root = std::env::temp_dir().join(format!("kakangkung-sync-size-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(&root.join("store.db")).unwrap();
        assert!(sync(&store, &root, &"a".repeat(65), "PC", Outgoing::default(), Types::ALL).is_err());
        let file = root.join("aa.json");
        std::fs::write(&file, b"previous snapshot").unwrap();
        let outgoing = Outgoing { extensions: vec!["x".repeat(MAX_FILE as usize)], ..Default::default() };
        assert!(sync(&store, &root, "aa", "PC", outgoing, Types::ALL).is_err());
        assert_eq!(std::fs::read(file).unwrap(), b"previous snapshot", "oversized sync preserves last snapshot");
    }

    #[test]
    fn two_devices_exchange_history_and_tabs() {
        let root = std::env::temp_dir().join(format!("kakangkung-sync-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = root.join("cloud");
        let open = |n: &str| Store::open(&root.join(format!("{n}.db"))).unwrap();
        let (a, b) = (open("a"), open("b"));
        a.add_visit("https://a.example/", "A page", 1000).unwrap();
        b.add_visit("https://b.example/", "B page", 2000).unwrap();
        let tab = |u: &str| SyncTab { url: u.into(), title: u.into() };

        let out = |tabs: Vec<SyncTab>| Outgoing { tabs, ..Default::default() };
        assert!(sync(&a, &folder, "aa01", "Laptop", out(vec![tab("https://a.example/")]), Types::ALL).unwrap().devices.is_empty());
        let seen_by_b = sync(&b, &folder, "bb02", "Desktop", out(vec![tab("https://b.example/"), tab("javascript:x")]), Types::ALL).unwrap().devices;
        assert_eq!(seen_by_b.len(), 1);
        assert_eq!(seen_by_b[0].name, "Laptop");
        assert_eq!(seen_by_b[0].tabs, vec![tab("https://a.example/")]);
        assert_eq!(b.history("", 10).unwrap().len(), 2, "B merged A's history");

        let seen_by_a = sync(&a, &folder, "aa01", "Laptop", out(vec![]), Types::ALL).unwrap().devices;
        assert_eq!(seen_by_a[0].tabs, vec![tab("https://b.example/")], "script URLs are not shared");
        assert_eq!(a.history("", 10).unwrap().len(), 2, "A merged B's history");
        assert!(sync(&a, &folder, "../x", "bad", out(vec![]), Types::ALL).is_err());
    }

    #[test]
    fn bookmarks_settings_and_chosen_types() {
        let root = std::env::temp_dir().join(format!("kakangkung-sync2-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = account_dir(&root, "Me@Example.com");
        assert_eq!(folder, account_dir(&root, " me@example.com "), "same account, same folder");
        assert_ne!(folder, account_dir(&root, "other@example.com"));
        let open = |n: &str| Store::open(&root.join(format!("{n}.db"))).unwrap();
        let (a, b) = (open("a"), open("b"));
        a.set_bookmark("https://rust-lang.org/", "Rust", true).unwrap();
        a.add_visit("https://secret.example/", "x", 1).unwrap();
        let settings = |v: &str| BTreeMap::from([("appearance.theme".to_string(), v.to_string())]);
        let only_bookmarks_and_settings = Types::parse(Some("bookmarks,settings"));
        assert_eq!(only_bookmarks_and_settings.list(), "bookmarks,settings");
        let a_out = Outgoing { settings: settings("dark"), settings_updated: 200, extensions: vec!["Dark Reader".into()], ..Default::default() };
        sync(&a, &folder, "aa01", "Laptop", a_out, only_bookmarks_and_settings).unwrap();
        let b_out = Outgoing { settings: settings("light"), settings_updated: 100, ..Default::default() };
        let got = sync(&b, &folder, "bb02", "Desktop", b_out, Types::ALL).unwrap();
        assert_eq!(got.new_bookmarks, 1, "bookmark came over");
        assert!(b.is_bookmarked("https://rust-lang.org/"));
        assert!(b.history("", 10).unwrap().is_empty(), "history was not chosen on A, so not shared");
        assert_eq!(got.settings, Some((200, settings("dark"))), "newer settings win");
        assert!(got.devices[0].extensions.is_empty(), "extensions were not chosen on A");
        assert_eq!(Types::parse(None), Types::ALL);
    }
}
