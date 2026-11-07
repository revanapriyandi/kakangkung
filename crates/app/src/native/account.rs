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

/// A form field's value: what was typed, else `fallback`.
pub(crate) fn field<'a>(inputs: &'a HashMap<String, String>, id: &str, fallback: &'a str) -> &'a str {
    inputs.get(id).map_or(fallback, String::as_str)
}

/// The Sign-in page for the `account` answer and what was typed so far.
pub(crate) fn signin_doc(a: &Json, inputs: &HashMap<String, String>) -> PageDoc {
    let signed_in = a["signedIn"].as_bool().unwrap_or(false);
    let types: Vec<Row> = TYPES.iter().map(|(k, label)| Row::new(label, "", Control::Toggle { id: format!("type:{k}"), on: type_on(a, inputs, k) })).collect();
    let mut sections = vec![];
    if signed_in {
        let name = a["name"].as_str().unwrap_or("");
        sections.push(section("account", "Your account", vec![
            Row::new(name, a["email"].as_str().unwrap_or(""), Control::Avatar { initial: initial(name) }),
            Row::new("Sync is on", "Bookmarks, history, open tabs and settings travel through your OneDrive folder; no server and no password are involved.", Control::None),
            Row::new("Sign out", "Bookmarks, history and settings stay on this PC", buttons(&[("signout", "Sign out", ButtonStyle::Danger)])),
        ]));
        sections.push(section("types", "What you sync", types));
    } else {
        let field_row = |id: &str, title: &str, placeholder: &str| {
            let value = field(inputs, id, a["suggested"][id].as_str().unwrap_or("")).to_string();
            Row::new(title, "", Control::Text { id: id.into(), value, placeholder: placeholder.into(), button: String::new() })
        };
        sections.push(section("account", "Sign in to Kakangkung", vec![
            Row::new("Your data on every PC", "Your bookmarks, history, open tabs and settings follow you to every PC you sign in on. They travel through your own OneDrive folder; no server and no password are involved.", Control::Avatar { initial: String::new() }),
            field_row("name", "Name", "Your name"),
            field_row("email", "Email", "you@example.com"),
        ]));
        sections.push(section("types", "What you sync", types));
        sections.push(section("go", "", vec![Row::new("Ready?", "Sync turns on right away.", buttons(&[("signin", "Sign in and turn on sync", ButtonStyle::Primary)]))]));
    }
    PageDoc { title: "Account".into(), nav: account_nav(), nav_on: "open:signin".into(), sections, ..Default::default() }
}

/// The Sync page for the `sync` answer: the switch, the folder, and other devices' open tabs.
pub(crate) fn sync_doc(s: &Json) -> PageDoc {
    let enabled = s["enabled"].as_bool().unwrap_or(false);
    let mut sections = vec![section("sync", "Sync", vec![
        Row::new("Sync history and open tabs", "Each PC writes a small file into a folder your cloud drive (OneDrive, Google Drive, Dropbox…) already keeps in sync, and reads the files of your other PCs.", Control::Toggle { id: "sync-on".into(), on: enabled }),
        Row::new("Sync folder", s["folder"].as_str().unwrap_or(""), buttons(&[("sync-pick", "Change", ButtonStyle::Normal), ("sync-now", "Sync now", ButtonStyle::Primary)])),
        Row::new("This PC", &format!("{} · last sync {}", s["device"].as_str().unwrap_or(""), when(s["last"].as_i64().unwrap_or(0))), Control::None),
    ])];
    let devices = s["devices"].as_array().cloned().unwrap_or_default();
    if devices.is_empty() {
        let text = if enabled { "No other devices yet. Sign in on your other PCs with the same sync folder." } else { "Turn on sync to see the tabs open on your other PCs." };
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
    fn signed_out_shows_a_form_signed_in_the_account() {
        let out = json!({ "signedIn": false, "types": { "history": false }, "suggested": { "name": "Ana", "email": "" } });
        let mut typed = HashMap::new();
        typed.insert("email".to_string(), "ana@example.com".to_string());
        let c = controls(&signin_doc(&out, &typed));
        assert!(c.contains(&Control::Text { id: "name".into(), value: "Ana".into(), placeholder: "Your name".into(), button: String::new() }));
        assert!(c.iter().any(|c| matches!(c, Control::Text { id, value, .. } if id == "email" && value == "ana@example.com")));
        assert!(c.contains(&Control::Toggle { id: "type:history".into(), on: false }));
        let inn = json!({ "signedIn": true, "name": "Ana", "email": "a@example.com", "types": {} });
        assert!(controls(&signin_doc(&inn, &HashMap::new())).iter().any(|c| matches!(c, Control::Buttons(b) if b[0].id == "signout")));
    }

    #[test]
    fn sync_lists_other_devices_tabs() {
        let s = json!({ "enabled": true, "folder": "C:\\\\S", "device": "PC", "last": 0, "devices": [{ "name": "Laptop", "updated": 0, "tabs": [{ "url": "https://a.example/", "title": "A" }] }] });
        let doc = sync_doc(&s);
        assert!(controls(&doc).contains(&Control::Link { id: "https://a.example/".into() }));
    }
}
