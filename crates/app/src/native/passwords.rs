//! The Passwords page (Kakangkung's password manager), described from the `passwords` answer:
//! the saved logins, imports from other browsers and CSV files, export, and other password
//! managers people can use instead.

use platform::page::{ButtonStyle, Control, PageDoc, Row, Section};
use serde_json::Value as Json;

use super::build::{buttons, empty, nav, section};

const KEY: &str = "\u{E8D7}";
const IMPORT: &str = "\u{E8B5}";
const OTHER: &str = "\u{E71D}";

/// Password managers that come as Chrome Web Store extensions: (name, extension id).
const MANAGERS: [(&str, &str); 6] = [
    ("Bitwarden", "nngceckbapebfimnlniiiahkandclblb"),
    ("1Password", "aeblfdkhhhdcdjpifhhbdiojplfjncoa"),
    ("Proton Pass", "ghmbeldphafepmbegfdlkpapadhbakde"),
    ("Dashlane", "fdjamakpfbbddfjaooikfcpapjohcfmg"),
    ("NordPass", "eiaeiblijfjekdanodkjadfinkhbfgcd"),
    ("LastPass", "hdokiejnpimakedhajhdlcegeplioahd"),
];

fn host(origin: &str) -> &str {
    origin.split_once("://").map_or(origin, |(_, h)| h)
}

/// The Passwords page for the `passwords` answer.
pub(crate) fn passwords_doc(d: &Json) -> PageDoc {
    let logins = d["logins"].as_array().cloned().unwrap_or_default();
    let on = d["on"].as_bool().unwrap_or(true);
    let rows: Vec<Row> = logins
        .iter()
        .map(|l| {
            let id = l["id"].as_i64().unwrap_or(0);
            let origin = l["origin"].as_str().unwrap_or("");
            let user = Some(l["username"].as_str().unwrap_or("")).filter(|u| !u.is_empty()).unwrap_or("(no username)");
            let shown = if origin.starts_with("http://") { format!("{user} · not secure (http)") } else { user.to_string() };
            Row::new(
                host(origin),
                &shown,
                buttons(&[(format!("pw-copy:{id}"), "Copy", ButtonStyle::Normal), (format!("pw-show:{id}"), "Show", ButtonStyle::Normal), (format!("pw-del:{id}"), "Delete", ButtonStyle::Danger)]),
            )
        })
        .collect();
    let toggle = Row::new("Offer to save passwords", "After you sign in to a site. Saved on this PC, sealed with your Windows account; Windows Hello is asked before one is shown or copied.", Control::Toggle { id: "privacy.passwords".into(), on });
    let mut sections = vec![section("pw-on", "", vec![toggle])];
    sections.push(if rows.is_empty() {
        empty("pw-list", "Saved passwords", KEY, "No saved passwords yet", "When you sign in to a site, Kakangkung offers to save the password. Or bring yours over from another browser or password manager below.")
    } else {
        Section { id: "pw-list".into(), title: format!("Saved passwords ({})", rows.len()), keywords: String::new(), cards: vec![rows] }
    });

    let mut import: Vec<Row> = d["browsers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|b| {
            let (id, name, profile) = (b["id"].as_str().unwrap_or(""), b["name"].as_str().unwrap_or(""), b["profile"].as_str().unwrap_or(""));
            let mut list = vec![(format!("pwimport:{id}:{profile}"), "Import", ButtonStyle::Normal)];
            if b["export"].as_bool() == Some(true) {
                list.push((format!("pwexport:{id}"), "Export from it", ButtonStyle::Normal));
            }
            Row::new(name, "Bookmarks, history and the passwords it lets other apps read. It locks newer ones to itself: Export from it saves them to a CSV file to import below.", buttons(&list))
        })
        .collect();
    import.push(Row::new(
        "From a CSV file",
        "Exported from Google Password Manager, Chrome, Edge, Firefox (about:logins), Safari, Bitwarden, 1Password, LastPass, Proton Pass, Dashlane or NordPass",
        buttons(&[("pw-csv", "Choose file", ButtonStyle::Primary)]),
    ));
    import.push(Row::new("Export passwords", "To a CSV file every browser and password manager imports. Anyone who has the file can read them.", buttons(&[("pw-export", "Export", ButtonStyle::Normal)])));
    import.push(Row::new("Delete all saved passwords", "From Kakangkung on this PC", buttons(&[("pw-clear", "Delete all", ButtonStyle::Danger)])));
    sections.push(section("pw-import", "Import and export", import));

    let mut other = vec![Row::new(
        "Google Password Manager",
        "See your Google passwords on the web. To have them filled in here, export them there and import the file above.",
        Control::Link { id: "visit:https://passwords.google.com/".into() },
    )];
    other.extend(MANAGERS.iter().map(|(name, id)| Row::new(name, "Extension from the Chrome Web Store: fills in your vault on every site", Control::Link { id: format!("visit:https://chromewebstore.google.com/detail/{id}") })));
    other.push(Row::new("Passkeys", "Sites that offer passkeys use Windows Hello, a security key, or your phone (scan the QR code): passkeys saved in Google Password Manager or iCloud on your phone work too.", Control::None));
    other.push(Row::new("Using another password manager?", "Turn off Offer to save passwords above so only one of them asks.", Control::None));
    sections.push(section("pw-other", "Other password managers", other));

    PageDoc {
        title: "Passwords".into(),
        search: "Search passwords".into(),
        nav: nav(&[
            ("pw-list", "Passwords", KEY),
            ("pw-import", "Import and export", IMPORT),
            ("pw-other", "Other password managers", OTHER),
            ("open:settings#autofill", "Settings", "\u{E713}"),
        ]),
        sections,
        nav_on: "pw-list".into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn logins_imports_and_managers() {
        let d = json!({
            "on": true,
            "logins": [{ "id": 4, "origin": "https://mail.example", "username": "ana", "used": 0 }, { "id": 5, "origin": "http://old.example", "username": "", "used": 0 }],
            "browsers": [{ "id": "edge", "name": "Microsoft Edge", "profile": "Default", "export": true }],
        });
        let doc = passwords_doc(&d);
        let list = doc.sections.iter().find(|s| s.id == "pw-list").unwrap();
        assert_eq!(list.title, "Saved passwords (2)");
        assert_eq!(list.cards[0][0].title, "mail.example");
        assert!(matches!(&list.cards[0][0].control, Control::Buttons(b) if b[0].id == "pw-copy:4" && b[2].id == "pw-del:4"));
        assert!(list.cards[0][1].sub.contains("not secure"));
        let import = doc.sections.iter().find(|s| s.id == "pw-import").unwrap();
        assert!(matches!(&import.cards[0][0].control, Control::Buttons(b) if b[0].id == "pwimport:edge:Default" && b[1].id == "pwexport:edge"));
        let other = doc.sections.iter().find(|s| s.id == "pw-other").unwrap();
        assert!(other.cards[0].iter().any(|r| matches!(&r.control, Control::Link { id } if id.ends_with("nngceckbapebfimnlniiiahkandclblb"))));
        assert!(passwords_doc(&json!({ "logins": [] })).sections.iter().any(|s| s.id == "pw-list" && s.title == "Saved passwords"));
    }
}
