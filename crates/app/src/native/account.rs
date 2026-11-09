//! Sign-in and Sync pages, from the `account` and `sync` answers.

use std::collections::HashMap;

use platform::page::{ButtonStyle, Control, PageDoc, Row};
use serde_json::Value as Json;

use super::build::{buttons, empty, initial, nav, section};
use super::when::{clock, local, short_date};

/// What can be synced: (key, label).
pub(crate) const TYPES: &[(&str, &str)] = &[("bookmarks", "Bookmarks"), ("history", "History"), ("tabs", "Open tabs"), ("settings", "Settings"), ("extensions", "Extensions")];

fn account_nav() -> Vec<platform::page::NavItem> {
    nav(&[("open:signin", "Account", "\u{E77B}"), ("open:sync", "Sync and devices", "\u{E895}"), ("open:settings/you", "Settings", "\u{E713}")])
}

/// "8 Nov 2026, 14:05", or "never".
fn when(ms: i64) -> String {
    if ms <= 0 {
        return "never".into();
    }
    let t = local(ms);
    format!("{}, {}", short_date(t), clock(t))
}

/// Is sync type `key` chosen: what the person switched on this page, else what is saved.
pub(crate) fn type_on(a: &Json, inputs: &HashMap<String, String>, key: &str) -> bool {
    match inputs.get(&format!("type:{key}")) {
        Some(v) => v == "1",
        None => a["types"][key].as_bool().unwrap_or(true),
    }
}

/// The Sign-in page for the `account` answer and what was typed so far.
pub(crate) fn signin_doc(a: &Json, inputs: &HashMap<String, String>) -> PageDoc {
    let signed_in = a["signedIn"].as_bool().unwrap_or(false);
    let types: Vec<Row> = TYPES.iter().map(|(k, label)| Row::new(label, "", Control::Toggle { id: format!("type:{k}"), on: type_on(a, inputs, k) })).collect();
    let mut sections = vec![];
    if signed_in {
        let name = a["name"].as_str().unwrap_or("");
        let via = if a["google"].as_bool().unwrap_or(false) {
            "Bookmarks, history, open tabs and settings travel through your Google Drive, in a hidden app folder only Kakangkung can see (none of your own files)."
        } else {
            "Bookmarks, history, open tabs and settings travel through your sync folder; no server and no password are involved."
        };
        sections.push(section("account", "Your account", vec![
            Row::new(name, a["email"].as_str().unwrap_or(""), Control::AvatarImage { initial: initial(name), path: a["picture"].as_str().unwrap_or("").into() }),
            Row::new(if a["enabled"].as_bool().unwrap_or(false) { "Sync is on" } else { "Sync is off" }, via, Control::None),
            Row::new("Sign out", "Bookmarks, history and settings stay on this PC", buttons(&[("signout", "Sign out", ButtonStyle::Danger)])),
        ]));
        sections.push(section("types", "What you sync", types));
    } else {
        let mut rows = vec![Row::new(
            "Your data on every PC",
            "Sign in with your Google account: your bookmarks, history, open tabs and settings follow you to every PC you sign in on. They are kept in your Google Drive, in a hidden app folder only Kakangkung can see.",
            Control::Avatar { initial: String::new() },
        )];
        if !a["googleReady"].as_bool().unwrap_or(true) {
            rows.push(Row::new(
                "Google sign-in is not set up yet",
                "This build has no Google OAuth client. Save the client file from the Google Cloud console as google-client.json in %LOCALAPPDATA%\\Kakangkung (see docs/GOOGLE_SIGNIN.md).",
                Control::None,
            ));
        }
        sections.push(section("account", "Sign in to Kakangkung", rows));
        sections.push(section("types", "What you sync", types));
        sections.push(section("go", "", vec![Row::new(
            "Ready?",
            "Google's sign-in page opens in your browser; sync turns on when you are done there.",
            buttons(&[("signin", "Sign in with Google", ButtonStyle::Primary)]),
        )]));
    }
    PageDoc { title: "Account".into(), nav: account_nav(), nav_on: "open:signin".into(), sections, ..Default::default() }
}

/// The Sync page for the `sync` answer: the switch, the folder, and other devices' open tabs.
pub(crate) fn sync_doc(s: &Json) -> PageDoc {
    let enabled = s["enabled"].as_bool().unwrap_or(false);
    let mut sections = vec![section("sync", "Sync", vec![
        Row::new("Sync selected data", if s["google"].as_bool().unwrap_or(false) {
            "Sync selected Kakangkung data through Google Drive. Chrome Sync data is separate; use Import to bring local Chrome bookmarks and history into Kakangkung."
        } else {
            "Each PC writes a device file into your shared cloud folder and reads the files from your other PCs."
        }, Control::Toggle { id: "sync-on".into(), on: enabled }),
        if s["google"].as_bool().unwrap_or(false) {
            Row::new("Synced through", &format!("Google Drive of {} (hidden app folder)", s["email"].as_str().unwrap_or("")), buttons(&[("sync-now", "Sync now", ButtonStyle::Primary)]))
        } else {
            Row::new("Sync folder", s["folder"].as_str().unwrap_or(""), buttons(&[("sync-pick", "Change", ButtonStyle::Normal), ("sync-now", "Sync now", ButtonStyle::Primary)]))
        },
        Row::new("This PC", &format!("{} · last sync {}", s["device"].as_str().unwrap_or(""), when(s["last"].as_i64().unwrap_or(0))), Control::None),
    ])];
    let devices = s["devices"].as_array().cloned().unwrap_or_default();
    if devices.is_empty() {
        let text = if !enabled { "Turn on sync to see the tabs open on your other PCs." } else if s["google"].as_bool().unwrap_or(false) {
            "Sign in to Kakangkung on another PC with the same Google account and the same Kakangkung OAuth application."
        } else { "Sign in on your other PCs with the same sync folder." };
        let heading = if enabled { "No other devices yet" } else { "Sync is off" };
        sections.push(empty("devices", "Tabs from your other devices", "\u{E770}", heading, text));
    }
    for (k, d) in devices.iter().enumerate() {
        let mut rows = vec![Row::new(d["name"].as_str().unwrap_or("Device"), &format!("Updated {}", when(d["updated"].as_i64().unwrap_or(0))), Control::None)];
        for t in d["tabs"].as_array().into_iter().flatten() {
            let url = t["url"].as_str().unwrap_or("");
            let title = Some(t["title"].as_str().unwrap_or("")).filter(|s| !s.is_empty()).unwrap_or(url);
            rows.push(Row::new(title, ui::host(url), Control::Link { id: url.into() }).indented());
        }
        let title = if k == 0 { "Tabs from your other devices" } else { "" };
        sections.push(section(&format!("device-{k}"), title, rows));
    }
    PageDoc { title: "Sync".into(), nav: account_nav(), nav_on: "open:sync".into(), sections, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn controls(doc: &PageDoc) -> Vec<Control> {
        doc.sections.iter().flat_map(|s| s.cards.iter().flatten()).map(|r| r.control.clone()).collect()
    }

    #[test]
    fn signed_out_offers_google_signed_in_the_account() {
        let out = json!({ "signedIn": false, "googleReady": true, "types": { "history": false } });
        let c = controls(&signin_doc(&out, &HashMap::new()));
        assert!(c.iter().any(|c| matches!(c, Control::Buttons(b) if b[0].id == "signin")));
        assert!(!c.iter().any(|c| matches!(c, Control::Text { .. })), "no name/email form any more");
        assert!(c.contains(&Control::Toggle { id: "type:history".into(), on: false }));
        let inn = json!({ "signedIn": true, "name": "Ana", "email": "a@example.com", "types": {} });
        assert!(controls(&signin_doc(&inn, &HashMap::new())).iter().any(|c| matches!(c, Control::Buttons(b) if b[0].id == "signout")));
    }

    #[test]
    fn signed_in_account_reports_disabled_sync() {
        let doc = signin_doc(&json!({"signedIn": true, "enabled": false}), &HashMap::new());
        let serialized = format!("{doc:?}");
        assert!(serialized.contains("Sync is off"));
        assert!(!serialized.contains("Sync is on"));
    }

    #[test]
    fn sync_lists_other_devices_tabs() {
        let s = json!({ "enabled": true, "folder": "C:\\\\S", "device": "PC", "last": 0, "devices": [{ "name": "Laptop", "updated": 0, "tabs": [{ "url": "https://a.example/", "title": "A" }] }] });
        let doc = sync_doc(&s);
        assert!(controls(&doc).contains(&Control::Link { id: "https://a.example/".into() }));
    }
}
