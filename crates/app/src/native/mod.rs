//! Built-in pages drawn natively (`platform::page`): which addresses they are, how each page is
//! described from the browser's data, and which API call each of its controls makes.

mod account;
mod build;
mod lists;
mod settings;
mod start;
mod tools;
mod welcome;
mod when;

pub(crate) use account::{signin_doc, sync_doc, type_on, TYPES};
pub(crate) use lists::{bookmarks_doc, history_doc};
pub(crate) use settings::settings_doc;
pub(crate) use start::{incognito_doc, launch_doc, newtab_doc};
pub(crate) use tools::{downloads_doc, extensions_doc, taskmanager_doc};
pub(crate) use welcome::welcome_doc;

/// Every built-in page, by its path under `ui::INTERNAL` ("" is the New Tab page). Each page's
/// data is the `/api` answer named by `data_path`.
const NATIVE: &[&str] = &["newtab", "settings", "history", "bookmarks", "downloads", "extensions", "taskmanager", "signin", "sync", "welcome", "incognito"];

/// The native page for an address (`https://kakangkung.internal/settings#privacy`), if any.
pub(crate) fn page_of(url: &str) -> Option<&'static str> {
    let path = url.strip_prefix(ui::INTERNAL)?;
    let name = match path.split(['#', '?', '/']).next().unwrap_or("") {
        "" => "newtab",
        name => name,
    };
    NATIVE.iter().copied().find(|p| *p == name)
}

/// Section to show first: `settings#privacy` or `settings/privacy` (Chrome's form).
pub(crate) fn anchor(url: &str) -> Option<&str> {
    let path = url.strip_prefix(ui::INTERNAL)?;
    path.split_once('#').or_else(|| path.split_once('/')).map(|(_, a)| a).filter(|a| !a.is_empty())
}

/// The `/api` answer a page is described from (`None`: the page needs none).
pub(crate) fn data_path(page: &str) -> Option<&str> {
    match page {
        "taskmanager" => Some("memory"),
        "signin" => Some("account"),
        "welcome" => Some("browsers"),
        "newtab" | "incognito" => None,
        other => Some(other),
    }
}

/// The tab title of a built-in page, in the interface language.
pub(crate) fn title(page: &str) -> String {
    crate::i18n::tr(match page {
        "settings" => "Settings",
        "history" => "History",
        "bookmarks" => "Bookmarks",
        "downloads" => "Downloads",
        "extensions" => "Extensions",
        "taskmanager" => "Task manager",
        "newtab" => "New Tab",
        "incognito" => "New Incognito tab",
        "signin" => "Account",
        "sync" => "Sync",
        "welcome" => "Welcome",
        _ => "",
    })
    .into_owned()
}

/// What a control asks the browser to do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    /// An API call: path, body, message when it worked, question to confirm first.
    Call { path: String, body: String, done: String, confirm: Option<&'static str> },
    /// Load an extra list into the page (e.g. site permissions).
    Load(&'static str),
    /// Open another page (internal path or address).
    Open(String),
    /// A section of the page was opened as its own page: the address follows it.
    Section(String),
    /// Go to a web address (an entry of a list was clicked).
    Visit(String),
    /// Search the page's data again with these words.
    Search(String),
    /// Something done to a download of this run (`clear` empties the list of finished ones).
    /// `id`: a download of this run, or `s<n>` for one kept from before.
    Download { id: String, what: String },
    /// The start page's search box: typing goes to the address bar.
    FocusOmnibox,
    /// A form field changed (kept until the form is sent).
    Input { id: String, value: String },
    /// A sync type switched on the Account page.
    SyncType { key: String, on: bool },
    /// The Account page's form was sent.
    SignIn,
    /// "Keep in sync" switched for another browser on the Welcome page.
    Keep { id: String, on: bool },
}

fn call(path: &str, body: impl Into<String>, done: &str) -> Step {
    Step::Call { path: path.into(), body: body.into(), done: done.into(), confirm: None }
}

fn ask(path: &str, body: &str, done: &str, question: &'static str) -> Step {
    Step::Call { path: path.into(), body: body.into(), done: done.into(), confirm: Some(question) }
}

/// The step for `kind` (toggle, select, radio, text, button, link, nav) on control `id` with
/// `value`, on page `page`.
pub(crate) fn step(page: &str, kind: &str, id: &str, value: &str) -> Option<Step> {
    if let Some(target) = id.strip_prefix("open:").filter(|_| matches!(kind, "button" | "link" | "nav")) {
        return Some(Step::Open(target.to_string()));
    }
    if let Some(url) = id.strip_prefix("visit:").filter(|_| matches!(kind, "button" | "link")) {
        return Some(Step::Visit(url.to_string()));
    }
    match page {
        "settings" => settings_step(kind, id, value),
        "history" => match (kind, id) {
            ("link", url) => Some(Step::Visit(url.into())),
            ("button", remove) => Some(call("history/delete", remove.strip_prefix("remove:")?, "Removed from history.")),
            ("nav", "clear") => Some(ask("history/clear", "", "History cleared.", "Clear all browsing history in Kakangkung?")),
            ("search", _) => Some(Step::Search(value.trim().into())),
            _ => None,
        },
        "downloads" => match (kind, id) {
            ("button", d) => {
                let (what, n) = d.strip_prefix("dl-")?.split_once(':')?;
                Some(Step::Download { id: n.into(), what: what.into() })
            }
            ("nav", "folder") => Some(call("downloads/open", "", "")),
            ("nav", "clear") => Some(Step::Download { id: String::new(), what: "clear".into() }),
            _ => None,
        },
        "extensions" => match (kind, id) {
            ("toggle", ext) => {
                let done = if value == "1" { "Extension turned on." } else { "Extension turned off." };
                Some(call("extensions/enable", format!("{}\t{value}", ext.strip_prefix("ext:")?), done))
            }
            ("button", "ext-load") => Some(call("extensions/pick", "", "Extension added.")),
            ("button", remove) => Some(ask("extensions/remove", remove.strip_prefix("ext-remove:")?, "Extension removed.", "Remove this extension from Kakangkung?")),
            _ => None,
        },
        "newtab" | "incognito" => match (kind, id) {
            ("link", "omnibox") => Some(Step::FocusOmnibox),
            ("link", url) => Some(Step::Visit(url.into())),
            _ => None,
        },
        "signin" => match (kind, id) {
            ("input", field) => Some(Step::Input { id: field.into(), value: value.into() }),
            ("toggle", t) => Some(Step::SyncType { key: t.strip_prefix("type:")?.into(), on: value == "1" }),
            ("button", "signin") => Some(Step::SignIn),
            ("button", "signout") => Some(ask("account/signout", "", "Signed out.", "Sign out? Bookmarks, history and settings stay on this PC.")),
            _ => None,
        },
        "sync" => match (kind, id) {
            ("toggle", "sync-on") => Some(call("sync/config", format!("{value}\t"), if value == "1" { "Sync is on." } else { "Sync is off." })),
            ("button", "sync-pick") => Some(call("sync/pick", "", "Sync folder changed.")),
            ("button", "sync-now") => Some(call("sync/now", "", "Synced.")),
            ("link", url) => Some(Step::Visit(url.into())),
            _ => None,
        },
        "welcome" => match (kind, id) {
            ("toggle", keep) => Some(Step::Keep { id: keep.strip_prefix("keep:")?.into(), on: value == "1" }),
            ("button", import) if import.starts_with("import:") => {
                let (browser, dir) = import["import:".len()..].split_once(':')?;
                Some(call("import/browser", format!("{browser}\t{dir}"), ""))
            }
            ("button", ext) => Some(call("chrome/extension", ext.strip_prefix("addext:")?, "Extension added.")),
            _ => None,
        },
        "bookmarks" => match (kind, id) {
            ("link", url) => Some(Step::Visit(url.into())),
            ("button", remove) => Some(call("bookmarks/remove", remove.strip_prefix("unbookmark:")?, "Bookmark removed.")),
            _ => None,
        },
        _ => None,
    }
}

fn settings_step(kind: &str, id: &str, value: &str) -> Option<Step> {
    let saved = "Saved.";
    Some(match (kind, id) {
        ("toggle", "sync-on") => call("sync/config", format!("{value}\t"), ""),
        ("toggle", _) => call("settings/set", format!("{id}\t{value}"), saved),
        ("select", "engine") => call("settings/engine", value, "Search engine changed."),
        ("select", _) => call("settings/set", format!("{id}\t{value}"), saved),
        ("radio", "memory") => call("settings/memory", value, "Memory Saver updated."),
        ("radio", "startup") => call("settings/startup", value, saved),
        ("text", "custom-engine") => call("settings/engine", value.trim(), "Search engine changed."),
        // Several start-up pages are typed on one line, separated by spaces.
        ("text", "startup.pages") => call("settings/url", format!("{id}\t{}", value.split_whitespace().collect::<Vec<_>>().join("\n")), saved),
        // Sites to keep active may be typed bare ("mail.example.com"), like in Chrome.
        ("text", "performance.keep_active") => {
            let urls: Vec<String> = value.split_whitespace().map(|w| if w.contains("://") { w.into() } else { format!("https://{w}") }).collect();
            call("settings/url", format!("{id}\t{}", urls.join("\n")), saved)
        }
        ("text", _) => call("settings/url", format!("{id}\t{}", value.trim()), saved),
        ("button", "sync-pick") => call("sync/pick", "", "Sync folder changed."),
        ("button", "sync-now") => call("sync/now", "", "Synced."),
        ("button", "import") => call("import/chrome", "", ""),
        ("button", "clear-history") => ask("privacy/clear", "history", "History cleared.", "Clear browsing history?"),
        ("button", "clear-all") => ask("privacy/clear", "all", "Browsing data cleared.", "Clear history, cookies, cache and site data?"),
        ("button", "clear-passwords") => ask("privacy/clear", "passwords", "Saved passwords deleted.", "Delete all saved passwords and autofill entries?"),
        ("button", "make-default") => call("default/make", "", "Choose Kakangkung in the Windows window that opened."),
        ("button", "proxy") => call("system/proxy", "", ""),
        ("button", "reset-all") => ask("settings/reset", "", "Settings reset.", "Reset all settings to their defaults?"),
        ("button", "dl-pick") => call("settings/downloads", "", "Download location changed."),
        ("button", "update-check") => call("update/check", "", "Checking for updates…"),
        ("button", "update-restart") => call("update/restart", "", ""),
        ("button", "perm-load") => Step::Load("permissions"),
        ("button", reset) if reset.starts_with("perm-reset:") => {
            // perm-reset:<kind>:<origin>
            let (kind, origin) = reset["perm-reset:".len()..].split_once(':')?;
            call("permissions/reset", format!("{kind}\t{origin}"), "Permission reset.")
        }
        ("nav", section) => Step::Section(section.to_string()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_addresses_and_anchors() {
        let url = format!("{}settings#privacy", ui::INTERNAL);
        assert_eq!(page_of(&url), Some("settings"));
        assert_eq!(anchor(&url), Some("privacy"));
        assert_eq!(anchor(&format!("{}settings/search", ui::INTERNAL)), Some("search"));
        assert_eq!(page_of(&format!("{}history", ui::INTERNAL)), Some("history"));
        assert_eq!(page_of(ui::NEWTAB_URL), Some("newtab"));
        assert_eq!(page_of(&format!("{}nothing", ui::INTERNAL)), None);
        assert_eq!(page_of("https://example.com/settings"), None);
    }

    #[test]
    fn controls_map_to_api_calls() {
        let body = |s: Option<Step>| match s {
            Some(Step::Call { path, body, .. }) => format!("{path} {body}"),
            other => format!("{other:?}"),
        };
        assert_eq!(body(step("settings", "toggle", "tabs.groups", "0")), "settings/set tabs.groups\t0");
        assert_eq!(body(step("settings", "toggle", "sync-on", "1")), "sync/config 1\t");
        assert_eq!(body(step("settings", "text", "startup.pages", " https://a.com  https://b.com ")), "settings/url startup.pages\thttps://a.com\nhttps://b.com");
        assert_eq!(body(step("settings", "button", "perm-reset:3:https://maps.example", "")), "permissions/reset 3\thttps://maps.example");
        assert!(matches!(step("settings", "button", "reset-all", ""), Some(Step::Call { confirm: Some(_), .. })));
        assert_eq!(step("settings", "link", "open:signin", ""), Some(Step::Open("signin".into())));
        assert_eq!(step("settings", "nav", "privacy", ""), Some(Step::Section("privacy".into())));
        assert_eq!(step("settings", "button", "nonsense", ""), None);
        assert_eq!(body(step("history", "button", "remove:https://a.example/", "")), "history/delete https://a.example/");
        assert_eq!(step("history", "link", "https://a.example/", ""), Some(Step::Visit("https://a.example/".into())));
        assert_eq!(step("history", "search", "", " rust "), Some(Step::Search("rust".into())));
        assert_eq!(body(step("bookmarks", "button", "unbookmark:https://a.example/", "")), "bookmarks/remove https://a.example/");
        assert_eq!(step("bookmarks", "nav", "open:history", ""), Some(Step::Open("history".into())));
        assert_eq!(step("downloads", "button", "dl-pause:7", ""), Some(Step::Download { id: "7".into(), what: "pause".into() }));
        assert_eq!(body(step("extensions", "toggle", "ext:abc", "0")), "extensions/enable abc\t0");
        assert_eq!(body(step("extensions", "button", "ext-remove:abc", "")), "extensions/remove abc");
        assert_eq!(step("newtab", "link", "omnibox", ""), Some(Step::FocusOmnibox));
        assert_eq!(step("signin", "toggle", "type:history", "0"), Some(Step::SyncType { key: "history".into(), on: false }));
        assert_eq!(body(step("welcome", "button", "import:edge:Profile 1", "")), "import/browser edge\tProfile 1");
        assert_eq!(body(step("welcome", "button", "addext:C:\\x", "")), "chrome/extension C:\\x");
    }
}
