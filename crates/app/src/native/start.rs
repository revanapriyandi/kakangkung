//! The New Tab and Incognito pages: start pages with a search box that hands typing to the
//! address bar, shortcut tiles and links.

use platform::page::{Bounce, Control, PageDoc, Row};

use super::build::section;

/// The New Tab page: the logo and a search box (which hands typing to the address bar), in the
/// middle of the page and nothing else. `engine` names the search engine.
pub(crate) fn newtab_doc(engine: &str) -> PageDoc {
    PageDoc { start: true, fakebox: format!("Search {engine} or type a URL"), ..Default::default() }
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
    fn new_tab_is_the_logo_and_the_search_box() {
        let doc = newtab_doc("Bing");
        assert!(doc.start && doc.sections.is_empty() && doc.nav.is_empty() && doc.title.is_empty());
        assert_eq!(doc.fakebox, "Search Bing or type a URL");
        assert!(incognito_doc().sections.iter().all(|s| s.cards[0].len() == 3));
        assert_eq!(launch_doc().logo_bounce, Bounce::Loop);
    }
}
