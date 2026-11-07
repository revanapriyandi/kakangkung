//! Welcome page: import history, bookmarks and extensions from the other browsers on this PC
//! (the `browsers` answer), and keep them in sync.

use platform::page::{ButtonStyle, Control, PageDoc, Row};
use serde_json::Value as Json;

use super::build::{buttons, initial, nav, section};

pub(crate) fn welcome_doc(b: &Json) -> PageDoc {
    let mut sections = vec![section("hello", "", vec![
        Row::new("Welcome to Kakangkung", "A light, safe browser. Bring your history, bookmarks and extensions over from Chrome, Edge, Brave, Firefox and more.", Control::Illustration { glyph: "\u{E8B5}".into() }),
        Row::new("Turn on sync", "Share history and open tabs between your PCs through a OneDrive folder", Control::Link { id: "open:sync".into() }),
        Row::new("Start browsing", "Ctrl+T new tab, Ctrl+L address bar, Ctrl+H history", Control::Link { id: "open:".into() }),
    ])];
    let mut import = vec![Row::new(
        "Your data stays on this PC",
        "Kakangkung reads the other browsers' files on this computer only; nothing is sent anywhere. Turn on \"Keep in sync\" to bring over new history and bookmarks automatically while you use both.",
        Control::None,
    )];
    if let Some(done) = b["imported"].as_str().filter(|s| !s.is_empty()) {
        import.push(Row::new("Imported at the first start", done, Control::None));
    }
    let browsers = b["browsers"].as_array().cloned().unwrap_or_default();
    if browsers.is_empty() {
        import.push(Row::new("No other browser with data was found for this Windows user.", "", Control::None));
    }
    let mut cards = vec![import];
    for br in &browsers {
        let id = br["id"].as_str().unwrap_or("");
        let name = br["name"].as_str().unwrap_or(id);
        let mut rows = vec![
            Row::new(name, "", Control::Avatar { initial: initial(name) }),
            Row::new("Keep in sync", "New history and bookmarks come over automatically", Control::Toggle { id: format!("keep:{id}"), on: br["keep"].as_bool().unwrap_or(false) }),
        ];
        for p in br["profiles"].as_array().into_iter().flatten() {
            let dir = p["dir"].as_str().unwrap_or("");
            let used = if br["default"].as_str() == Some(dir) { " · used last" } else { "" };
            let exts = p["extensions"].as_array().cloned().unwrap_or_default();
            let sub = if exts.is_empty() { format!("Profile folder {dir}{used}") } else { format!("Profile folder {dir}{used} · {} extension(s)", exts.len()) };
            rows.push(Row::new(p["name"].as_str().unwrap_or(dir), &sub, buttons(&[(format!("import:{id}:{dir}"), "Import", ButtonStyle::Primary)])));
            if br["chromium"].as_bool().unwrap_or(false) {
                for x in &exts {
                    let label = format!("{} {}", x["name"].as_str().unwrap_or(""), x["version"].as_str().unwrap_or(""));
                    rows.push(Row::new(&label, "Extension", buttons(&[(format!("addext:{}", x["path"].as_str().unwrap_or("")), "Add to Kakangkung", ButtonStyle::Normal)])).indented());
                }
            }
        }
        cards.push(rows);
    }
    let mut chrome = section("chrome", "Import from your other browsers", vec![]);
    chrome.cards = cards;
    sections.push(chrome);
    PageDoc {
        title: "Welcome".into(),
        nav: nav(&[("open:welcome", "Import", "\u{E8B5}"), ("open:sync", "Sync", "\u{E895}"), ("open:settings", "Settings", "\u{E713}")]),
        nav_on: "open:welcome".into(),
        sections,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn browsers_profiles_and_extensions() {
        let b = json!({ "browsers": [{ "id": "edge", "name": "Microsoft Edge", "chromium": true, "keep": true, "default": "Default",
            "profiles": [{ "dir": "Default", "name": "Work", "extensions": [{ "name": "Blocker", "version": "1.2", "path": "C:\\x\\1.2" }] }] }] });
        let doc = welcome_doc(&b);
        let rows: Vec<&Row> = doc.sections.iter().flat_map(|s| s.cards.iter().flatten()).collect();
        assert!(rows.iter().any(|r| r.control == Control::Toggle { id: "keep:edge".into(), on: true }));
        let profile = rows.iter().find(|r| r.title == "Work").expect("profile");
        assert!(profile.sub.contains("used last"));
        assert!(matches!(&profile.control, Control::Buttons(b) if b[0].id == "import:edge:Default"));
        assert!(rows.iter().any(|r| matches!(&r.control, Control::Buttons(b) if b[0].id == "addext:C:\\x\\1.2")));
    }
}
