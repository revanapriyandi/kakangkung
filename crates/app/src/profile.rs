//! Account, sync and import from other browsers (runs on the storage thread).

use std::path::PathBuf;

use serde_json::{json, Value as Json};
use storage::Store;
use tao::event_loop::EventLoopProxy;

use crate::msg::Msg;
use crate::pages::random_hex;
use crate::prefs::{Prefs, SETTABLE, URL_SETTINGS};

/// Import history and bookmarks from the Chrome profile used last. Returns a one-line summary.
pub(crate) fn import_from_chrome(s: &Store) -> Result<String, String> {
    let browsers = storage::chrome::browsers();
    let b = browsers.first().ok_or("No other browser with data was found for this Windows user.")?;
    import_browser_default(s, b)
}

/// Import the browser's default profile (Chromium: last used; Firefox: the busiest) and remember
/// a one-line summary for the welcome and settings pages.
pub(crate) fn import_browser_default(s: &Store, b: &storage::chrome::Browser) -> Result<String, String> {
    let dir = storage::chrome::default_profile(b).ok_or("no profile")?;
    let name = b.profiles.iter().find(|p| p.dir == dir).map(|p| p.name.clone()).unwrap_or_else(|| dir.clone());
    let (pages, marks) = s.import_browser(b, &dir)?;
    let summary = format!("Imported {pages} pages and {marks} bookmarks from {} profile \"{name}\".", b.name);
    s.set_setting("import.summary", &summary)?;
    Ok(summary)
}

/// "Keep in sync with": re-import the chosen browsers so their new history and bookmarks keep
/// arriving (the merge never duplicates). Returns how many browsers were read.
pub(crate) fn keep_browsers_in_sync(s: &Store) -> usize {
    let keep = s.setting("import.keep").unwrap_or_default();
    let mut n = 0;
    for b in storage::chrome::browsers().iter().filter(|b| keep.split(',').any(|k| k == b.id)) {
        match storage::chrome::default_profile(b).map(|d| s.import_browser(b, &d)) {
            Some(Ok(_)) => n += 1,
            Some(Err(e)) => eprintln!("keeping {} in sync: {e}", b.name),
            None => {}
        }
    }
    n
}

/// The signed-in Kakangkung account (name, email), if any.
pub(crate) fn account(s: &Store) -> Option<(String, String)> {
    let email = s.setting("account.email").filter(|e| e.contains('@'))?;
    let name = s.setting("account.name").filter(|n| !n.is_empty()).unwrap_or_else(|| email.split('@').next().unwrap_or("").to_string());
    Some((name, email))
}

/// Settings that travel with the account: every user setting except this PC's own
/// (hardware acceleration, clearing data on exit) and the download folder.
pub(crate) fn synced_setting(key: &str) -> bool {
    let device_only = ["system.hardware_acceleration", "privacy.clear_on_exit"];
    let listed = SETTABLE.iter().any(|(k, _, _)| *k == key) || URL_SETTINGS.contains(&key);
    (listed && !device_only.contains(&key)) || ["search.engine", "memory.mode", "startup"].contains(&key)
}

/// Every key `synced_setting` accepts, for publishing this device's settings.
fn synced_keys() -> impl Iterator<Item = &'static str> {
    SETTABLE.iter().map(|(k, _, _)| *k).chain(URL_SETTINGS.iter().copied()).chain(["search.engine", "memory.mode", "startup"]).filter(|k| synced_setting(k))
}

/// Remember that settings changed on this device now (newest settings win across devices).
pub(crate) fn touch_settings(s: &Store) {
    let _ = s.set_setting("settings.updated", &storage::now_ms().to_string());
}

/// Settings: is sync on, the account's sync folder, this device's id (created on first use).
/// Sync needs a signed-in account, like Chrome.
pub(crate) fn sync_conf(s: &Store) -> (bool, PathBuf, String) {
    let signed_in = account(s);
    let enabled = signed_in.is_some() && s.setting("sync.enabled").is_none_or(|v| v == "1");
    let base = s.setting("sync.folder").map(PathBuf::from).unwrap_or_else(storage::sync::default_folder);
    let folder = signed_in.map_or(base.clone(), |(_, email)| storage::sync::account_dir(&base, &email));
    let device = s.setting("sync.device").unwrap_or_else(|| {
        let id = random_hex();
        let _ = s.set_setting("sync.device", &id);
        id
    });
    (enabled, folder, device)
}

pub(crate) fn device_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "This PC".into())
}

pub(crate) fn sync_status(s: &Store) -> Json {
    let (enabled, folder, device) = sync_conf(s);
    let devices = if enabled { storage::sync::devices(&folder, &device) } else { vec![] };
    let acct = account(s);
    json!({
        "enabled": enabled,
        "signedIn": acct.is_some(),
        "name": acct.as_ref().map(|a| a.0.clone()).unwrap_or_default(),
        "email": acct.as_ref().map(|a| a.1.clone()).unwrap_or_default(),
        "types": storage::sync::Types::parse(s.setting("sync.types").as_deref()),
        "folder": s.setting("sync.folder").map(PathBuf::from).unwrap_or_else(storage::sync::default_folder).display().to_string(),
        "device": device_name(),
        "last": s.setting("sync.last").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0),
        "devices": devices,
    })
}

/// One sync round. Settings that came in from a newer device are applied here and the UI thread
/// is told through `p`.
pub(crate) fn run_sync(s: &Store, tabs: Vec<storage::sync::SyncTab>, p: &EventLoopProxy<Msg>) -> Result<(), String> {
    let (enabled, folder, device) = sync_conf(s);
    if !enabled {
        return Err(if account(s).is_none() { "Sign in to Kakangkung to turn on sync." } else { "Sync is off. Turn it on first." }.into());
    }
    let out = storage::sync::Outgoing {
        tabs,
        settings: synced_keys().filter_map(|k| s.setting(k).map(|v| (k.to_string(), v))).collect(),
        settings_updated: s.setting("settings.updated").and_then(|v| v.parse().ok()).unwrap_or(0),
        extensions: s.setting("extensions.names").and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default(),
    };
    let types = storage::sync::Types::parse(s.setting("sync.types").as_deref());
    let incoming = storage::sync::sync(s, &folder, &device, &device_name(), out, types)?;
    if let Some((when, settings)) = incoming.settings {
        for (k, v) in settings.iter().filter(|(k, _)| synced_setting(k)) {
            s.set_setting(k, v)?;
        }
        s.set_setting("settings.updated", &when.to_string())?;
        let prefs = Prefs::read(s);
        let _ = p.send_event(Msg::SettingsChanged(prefs.settings));
        let _ = p.send_event(Msg::Engine(prefs.engine));
        let _ = p.send_event(Msg::MemoryMode(prefs.memory));
    }
    if incoming.new_bookmarks > 0 {
        let _ = p.send_event(Msg::BookmarksChanged);
    }
    s.set_setting("sync.last", &storage::now_ms().to_string())
}

/// Sign in (or switch account) and turn sync on for the chosen data types.
pub(crate) fn sign_in(s: &Store, name: &str, email: &str, types: &str) -> Result<(), String> {
    let email = email.trim().to_lowercase();
    let name = name.trim();
    let valid_email = email.len() <= 254
        && email.split_once('@').is_some_and(|(u, d)| !u.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.'))
        && !email.contains(char::is_whitespace);
    if !valid_email {
        return Err("Enter a valid email address.".into());
    }
    if name.is_empty() || name.chars().count() > 60 {
        return Err("Enter your name (up to 60 characters).".into());
    }
    let types = storage::sync::Types::parse(Some(types));
    s.set_setting("account.name", name)?;
    s.set_setting("account.email", &email)?;
    s.set_setting("sync.types", &types.list())?;
    s.set_setting("sync.enabled", "1")
}

pub(crate) fn sign_out(s: &Store) -> Result<(), String> {
    s.set_setting("account.email", "")?;
    s.set_setting("account.name", "")?;
    s.set_setting("sync.enabled", "0")
}
