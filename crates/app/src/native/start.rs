//! The New Tab and Incognito pages: start pages with a search box that hands typing to the
//! address bar, shortcut tiles and links.

use platform::page::{Bounce, Control, PageDoc, Row, Section};
use serde_json::{json, Value as Json};

use super::build::section;

/// Shortcut tiles on the New Tab page (two rows of four).
const TILES: usize = 8;

/// Sites offered as shortcuts until the person's own history fills the tiles.
const STARTER_SITES: &[(&str, &str)] = &[
    ("YouTube", "https://www.youtube.com/"),
    ("Gmail", "https://mail.google.com/"),
    ("WhatsApp", "https://web.whatsapp.com/"),
    ("Google Drive", "https://drive.google.com/"),
    ("Wikipedia", "https://www.wikipedia.org/"),
    ("Maps", "https://maps.google.com/"),
    ("GitHub", "https://github.com/"),
    ("News", "https://news.google.com/"),
];

/// The most visited sites (one per host, most visited first) as `[{url, title, host}]`, from
/// visits ordered most visited first.
pub(crate) fn top_sites(visits: Vec<storage::Visit>) -> Json {
    let mut out: Vec<Json> = vec![];
    for v in visits {
        let host = ui::host(&v.url).trim_start_matches("www.").to_string();
        if host.is_empty() || v.url.starts_with(ui::INTERNAL) || out.iter().any(|o| o["host"] == host.as_str()) {
            continue;
        }
        out.push(json!({ "url": v.url, "title": v.title, "host": host }));
        if out.len() == TILES {
            break;
        }
    }
    json!(out)
}

/// The New Tab page: the logo, a search box (a real field: suggestions open under it) and
/// shortcut tiles of the most visited sites, topped up with well-known ones while the history
/// is short. `engine` names the search engine; `sites` is the `topsites` answer.
pub(crate) fn newtab_doc(engine: &str, sites: &Json) -> PageDoc {
    let mut tiles: Vec<Row> = vec![];
    let mut hosts: Vec<String> = vec![];
    for s in sites.as_array().into_iter().flatten() {
        let (url, host) = (s["url"].as_str().unwrap_or(""), s["host"].as_str().unwrap_or(""));
        if url.is_empty() || host.is_empty() {
            continue;
        }
        tiles.push(Row::new(&site_name(host), host, Control::Link { id: format!("visit:{url}") }));
        hosts.push(host.to_string());
    }
    for (name, url) in STARTER_SITES {
        if tiles.len() >= TILES {
            break;
        }
        let host = ui::host(url).trim_start_matches("www.").to_string();
        if !hosts.contains(&host) {
            tiles.push(Row::new(name, &host, Control::Link { id: format!("visit:{url}") }));
        }
    }
    PageDoc {
        start: true,
        fakebox: format!("Search {engine} or type a URL"),
        sections: vec![Section { id: platform::page::start::TILES.into(), cards: vec![tiles], ..Default::default() }],
        ..Default::default()
    }
}

/// A short name for a tile: the site's name without its domain ending, capitalised
/// ("tokopedia.co.id" → "Tokopedia").
fn site_name(host: &str) -> String {
    let parts: Vec<&str> = host.split('.').collect();
    // The name before the domain ending ("mail.google.com" → "google"; "detik.com" → "detik").
    let core = match parts.len() {
        0 | 1 => host,
        n if n >= 3 && matches!(parts[n - 2], "co" | "com" | "go" | "ac" | "or" | "net") => parts[n - 3],
        n => parts[n - 2],
    };
    let mut c = core.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// What the page area shows while the first web page gets ready: the logo, hopping.
pub(crate) fn launch_doc() -> PageDoc {
    PageDoc { start: true, logo_bounce: Bounce::Loop, ..Default::default() }
}

/// The Incognito window's new tab page.
pub(crate) fn incognito_doc() -> PageDoc {
    let list = |items: &[&str]| items.iter().map(|t| Row::new(t, "", Control::None)).collect();
    PageDoc {
        start: true,
        title: "You've gone Incognito".into(),
        hero_icon: "\u{E727}".into(),
        hero_text: "Others who use this device won't see your activity, so you can browse more privately. Your downloads and bookmarks are saved.".into(),
        fakebox: "Search or type a URL".into(),
        sections: vec![
            section("unsaved", "Kakangkung won't save", list(&["Your browsing history", "Cookies and site data", "Information entered in forms"])),
            section("visible", "Your activity might still be visible to", list(&["Websites you visit", "Your employer or school", "Your internet service provider"])),
        ],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tab_is_the_logo_the_search_box_and_tiles() {
        let sites = json!([{ "url": "https://www.detik.com/", "title": "detikcom", "host": "detik.com" }]);
        let doc = newtab_doc("Bing", &sites);
        assert!(doc.start && doc.nav.is_empty() && doc.title.is_empty());
        assert_eq!(doc.fakebox, "Search Bing or type a URL");
        let tiles = &doc.sections[0].cards[0];
        assert_eq!(tiles.len(), TILES, "topped up with starter sites");
        assert_eq!(tiles[0].title, "Detik");
        assert_eq!(tiles[0].control, Control::Link { id: "visit:https://www.detik.com/".into() });
        assert_eq!(site_name("mail.google.com"), "Google");
        assert_eq!(site_name("tokopedia.co.id"), "Tokopedia");
        let v = |url: &str, count| storage::Visit { url: url.into(), title: String::new(), count, last: 0 };
        let top = top_sites(vec![v("https://www.youtube.com/watch?v=1", 9), v("https://youtube.com/", 5), v("https://github.com/x", 3)]);
        assert_eq!(top.as_array().map(Vec::len), Some(2), "one tile per site");
        assert!(incognito_doc().sections.iter().all(|s| s.cards[0].len() == 3));
        assert_eq!(launch_doc().logo_bounce, Bounce::Loop);
    }
}
