//! The Settings page, described from the `settings` API answer. Every setting of
//! docs/SETTINGS.md has its row here; ids of switches and lists are the setting keys.

use platform::page::{Button, ButtonStyle, Control, NavItem, PageDoc, Row, Section};
use serde_json::Value as Json;

const UI_LANGS: &[(&str, &str)] = &[("auto", "Automatic (installer or Windows language)"), ("en", "English"), ("id", "Bahasa Indonesia")];

const LANG_NAMES: &[(&str, &str)] = &[
    ("auto", "The browser's language"),
    ("id", "Bahasa Indonesia"),
    ("en", "English"),
    ("ms", "Bahasa Melayu"),
    ("jv", "Basa Jawa"),
    ("su", "Basa Sunda"),
    ("ar", "العربية"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("fr", "Français"),
    ("hi", "हिन्दी"),
    ("it", "Italiano"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("nl", "Nederlands"),
    ("pt", "Português"),
    ("ru", "Русский"),
    ("th", "ไทย"),
    ("tr", "Türkçe"),
    ("vi", "Tiếng Việt"),
    ("zh-CN", "中文（简体）"),
    ("zh-TW", "中文（繁體）"),
];

const ZOOMS: &[&str] = &["50", "67", "75", "80", "90", "100", "110", "125", "150", "175", "200"];

/// Left navigation: (section id or `open:<page>`, label, icon glyph).
const NAV: &[(&str, &str, &str)] = &[
    ("you", "You and sync", "\u{E77B}"),
    ("autofill", "Autofill and passwords", "\u{E8D7}"),
    ("privacy", "Privacy and security", "\u{EA18}"),
    ("performance", "Performance", "\u{EC4A}"),
    ("appearance", "Appearance", "\u{E790}"),
    ("tabs", "Tabs and windows", "\u{E737}"),
    ("search", "Search engine", "\u{E721}"),
    ("default", "Default browser", "\u{E774}"),
    ("startup", "On startup", "\u{E7E8}"),
    ("languages", "Languages", "\u{F2B7}"),
    ("downloads", "Downloads", "\u{E896}"),
    ("accessibility", "Accessibility", "\u{E776}"),
    ("system", "System", "\u{E770}"),
    ("reset", "Reset settings", "\u{E777}"),
    ("open:extensions", "Extensions", "\u{EA86}"),
    ("about", "About Kakangkung", "\u{E946}"),
];

/// Reads the answer's values.
struct Values<'a>(&'a Json);

impl Values<'_> {
    fn get(&self, key: &str) -> &str {
        self.0["all"][key].as_str().unwrap_or("")
    }

    fn on(&self, key: &str) -> bool {
        self.get(key) == "1"
    }

    fn toggle(&self, key: &str, title: &str, sub: &str) -> Row {
        Row::new(title, sub, Control::Toggle { id: key.into(), on: self.on(key) })
    }

    fn select(&self, key: &str, title: &str, sub: &str, options: &[(&str, &str)]) -> Row {
        let options = options.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect();
        Row::new(title, sub, Control::Select { id: key.into(), value: self.get(key).into(), options })
    }

    fn url_field(&self, key: &str, title: &str, sub: &str) -> Row {
        // Lists are stored one per line and typed on one line, separated by spaces.
        Row::new(title, sub, Control::Text { id: key.into(), value: self.get(key).replace('\n', " "), placeholder: "https://example.com".into(), button: "Save".into() })
    }
}

fn buttons(list: &[(&str, &str, ButtonStyle)]) -> Control {
    Control::Buttons(list.iter().map(|(id, label, style)| Button::new(id, label, *style)).collect())
}

fn section(id: &str, title: &str, keywords: &str, cards: Vec<Vec<Row>>) -> Section {
    Section { id: id.into(), title: title.into(), keywords: keywords.into(), cards }
}

fn radio(id: &str, value: &str, current: &str, title: &str, sub: &str) -> Row {
    Row::new(title, sub, Control::Radio { id: id.into(), value: value.into(), on: value == current })
}

/// "3 minutes ago" for a time in ms since the epoch.
fn ago(ms: i64) -> String {
    let mins = (storage::now_ms() - ms).max(0) / 60_000;
    match mins {
        0 => "just now".into(),
        1..=59 => format!("{mins} min ago"),
        60..=1439 => format!("{} h ago", mins / 60),
        _ => format!("{} days ago", mins / 1440),
    }
}

/// The Settings page for the `settings` answer `s`, with the site permissions once loaded.
pub(crate) fn settings_doc(s: &Json, permissions: Option<&[Json]>) -> PageDoc {
    let v = Values(s);
    let text = |k: &str| s[k].as_str().unwrap_or("").to_string();
    let sections = vec![
        you(s),
        section("autofill", "Autofill and passwords", "autofill passwords password manager forms addresses save", vec![vec![
            Row::new("Password Manager", "Saved passwords, import from other browsers and CSV files, export, other password managers", Control::Link { id: "open:passwords".into() }),
            v.toggle("privacy.passwords", "Offer to save passwords", "After you sign in to a site; filled in when you click a login field"),
            v.toggle("privacy.autofill", "Autofill addresses and forms", "Fill in forms with what you typed before"),
            Row::new("Delete saved passwords and autofill data", "Every saved password and what forms remember", buttons(&[("clear-passwords", "Delete", ButtonStyle::Danger)])),
        ]]),
        privacy(&v, permissions),
        performance(&v, s["memory"].as_str().unwrap_or("")),
        appearance(&v),
        tabs(&v),
        search(&v, s),
        section("default", "Default browser", "default browser make default open links windows", vec![vec![{
            let is_default = s["isDefault"].as_bool().unwrap_or(false);
            let sub = if is_default { "Kakangkung is your default browser" } else { "Kakangkung is not your default browser" };
            let control = if is_default { Control::None } else { buttons(&[("make-default", "Make default", ButtonStyle::Primary)]) };
            Row::new("Default browser", sub, control)
        }]]),
        startup(&v, s["startup"].as_str().unwrap_or("newtab")),
        section("languages", "Languages", "languages language translate preferred accept website", vec![vec![
            v.select("languages.ui", "Interface language", "Menus and built-in pages; more languages are welcome on GitHub", UI_LANGS),
            v.select("languages.preferred", "Preferred language for websites", "Sites show pages in this language when they can; takes effect after you restart Kakangkung", LANG_NAMES),
            v.select("languages.translate_to", "Translate pages into", "Used by ⋮ > Translate page (Google Translate)", LANG_NAMES),
        ]]),
        section("downloads", "Downloads", "downloads location folder save ask where done", vec![vec![
            Row::new("Location", &text("downloads"), buttons(&[("dl-pick", "Change", ButtonStyle::Normal)])),
            v.toggle("downloads.ask", "Ask where to save each file before downloading", ""),
            v.toggle("downloads.show_done", "Show downloads when they're done", "Opens the Downloads page"),
            Row::new("Show downloads", "Ctrl+J", Control::None),
        ]]),
        section("accessibility", "Accessibility", "accessibility focus highlight keyboard animations motion zoom", vec![vec![
            v.toggle("accessibility.focus_highlight", "Show a highlight on the focused object", "A thick outline follows the keyboard focus on web pages"),
            Row::new("Animations and smooth scrolling", "Set under Appearance > Motion", Control::Link { id: "open:settings/appearance".into() }),
            Row::new("Page zoom", "Set under Appearance; Ctrl + / Ctrl − change one tab", Control::None),
        ]]),
        section("system", "System", "system hardware acceleration gpu proxy network", vec![vec![
            v.toggle("system.hardware_acceleration", "Use graphics acceleration when available", "Takes effect after you restart Kakangkung"),
            v.toggle("system.auto_update", "Update Kakangkung automatically", "New versions are downloaded in the background, checked, and installed when you close Kakangkung"),
            Row::new("Open your computer's proxy settings", "Kakangkung uses the Windows network settings", buttons(&[("proxy", "Open", ButtonStyle::Normal)])),
        ]]),
        section("reset", "Reset settings", "reset restore defaults original", vec![vec![
            Row::new("Restore settings to their original defaults", "Your account, history, bookmarks and downloads stay", buttons(&[("reset-all", "Reset settings", ButtonStyle::Danger)])),
        ]]),
        section("about", "About Kakangkung", "about version update updates webview2", vec![vec![
            Row::new("Kakangkung", &format!("Version {} · Microsoft Edge WebView2 {}", text("version"), text("webview2")), Control::Avatar { initial: "K".into() }),
            update_row(&s["update"], v.on("system.auto_update")),
            Row::new("Source code, help and releases", "github.com/revanapriyandi/kakangkung", Control::Link { id: "visit:https://github.com/revanapriyandi/kakangkung".into() }),
        ]]),
    ];
    PageDoc {
        title: "Settings".into(),
        search: "Search settings".into(),
        nav: NAV.iter().map(|(id, label, icon)| NavItem { id: (*id).into(), label: (*label).into(), icon: (*icon).into() }).collect(),
        sections,
        paged: true,
        ..Default::default()
    }
}

/// The updates row of About: where the updater is, and what can be done.
fn update_row(u: &Json, automatic: bool) -> Row {
    let version = u["version"].as_str().unwrap_or("");
    let check = ("update-check", "Check for updates", ButtonStyle::Normal);
    let (sub, control) = match u["state"].as_str().unwrap_or("") {
        "checking" => ("Checking for updates…".to_string(), Control::None),
        "downloading" => (format!("Downloading version {version}…"), Control::None),
        "ready" => (format!("Version {version} is ready. It is installed when you close Kakangkung."), buttons(&[("update-restart", "Restart to update", ButtonStyle::Primary)])),
        "available" => {
            let page = format!("visit:{}", u["text"].as_str().unwrap_or(""));
            return Row::new("Updates", &format!("Version {version} is available. This copy was not installed with the installer, so it does not update itself."), Control::Buttons(vec![Button::new(&page, "Get it", ButtonStyle::Primary)]));
        }
        "uptodate" => ("Kakangkung is up to date".to_string(), buttons(&[check])),
        "failed" => (format!("Could not update: {}", u["text"].as_str().unwrap_or("")), buttons(&[check])),
        _ if automatic => ("Updates are checked automatically".to_string(), buttons(&[check])),
        _ => ("Automatic updates are off (Settings > System)".to_string(), buttons(&[check])),
    };
    Row::new("Updates", &sub, control)
}

fn you(s: &Json) -> Section {
    let sync = &s["sync"];
    let signed_in = sync["signedIn"].as_bool().unwrap_or(false);
    let name = sync["name"].as_str().unwrap_or("");
    let enabled = sync["enabled"].as_bool().unwrap_or(false);
    let last = sync["last"].as_i64().unwrap_or(0);
    let state = match (enabled, last) {
        (false, _) => "Off".to_string(),
        (true, 0) => "On".to_string(),
        (true, t) => format!("On · last synced {}", ago(t)),
    };
    let account = if signed_in {
        let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
        Row::new(name, sync["email"].as_str().unwrap_or(""), Control::Avatar { initial })
    } else {
        Row::new("Not signed in", "Sign in to sync bookmarks, history, tabs and settings across your PCs", Control::Avatar { initial: String::new() })
    };
    let imported = s["imported"].as_str().filter(|i| !i.is_empty()).unwrap_or("From Chrome, Edge, Brave, Firefox and more; keep them in sync from \"Choose browser\"");
    let mut rows = vec![
        account,
        Row::new(if signed_in { "Manage your account" } else { "Sign in" }, "", Control::Link { id: "open:signin".into() }),
    ];
    // The sync switch needs an account; signed out it is not offered.
    if signed_in {
        rows.push(Row::new("Sync", &state, Control::Toggle { id: "sync-on".into(), on: enabled }));
    }
    rows.extend([
        Row::new("Sync folder", sync["folder"].as_str().unwrap_or(""), buttons(&[("sync-pick", "Change", ButtonStyle::Normal), ("sync-now", "Sync now", ButtonStyle::Normal)])),
        Row::new("Import bookmarks and history", imported, buttons(&[("open:welcome#chrome", "Choose browser", ButtonStyle::Normal), ("import", "Import now", ButtonStyle::Normal)])),
        Row::new("Tabs from your other devices", "", Control::Link { id: "open:sync".into() }),
    ]);
    section("you", "You and sync", "sync account cloud onedrive import chrome edge brave firefox bookmarks history devices sign in", vec![rows])
}

fn privacy(v: &Values, permissions: Option<&[Json]>) -> Section {
    let mut site = vec![Row::heading("Site settings")];
    match permissions {
        None => site.push(Row::new("Site permissions", "Camera, microphone, location, notifications and more you allowed or blocked", buttons(&[("perm-load", "Show", ButtonStyle::Normal)]))),
        Some(list) => {
            let sub = if list.is_empty() { "No site has a saved permission".to_string() } else { format!("{} site permission(s)", list.len()) };
            site.push(Row::new("Site permissions", &sub, Control::None));
            for p in list {
                let origin = p["origin"].as_str().unwrap_or("");
                let kind = p["kind"].as_i64().unwrap_or(0);
                let state = if p["allowed"].as_bool().unwrap_or(false) { "Allowed" } else { "Blocked" };
                let host = ui::host(origin);
                let reset = format!("perm-reset:{kind}:{origin}");
                let sub = format!("{}: {state}", p["name"].as_str().unwrap_or(""));
                site.push(Row::new(if host.is_empty() { origin } else { host }, &sub, buttons(&[(&reset, "Reset", ButtonStyle::Normal)])).indented());
            }
        }
    }
    site.push(Row::heading("Default behaviour"));
    site.push(v.select("site.javascript", "JavaScript", "Applies from the next page load", &[("allow", "Sites can use JavaScript"), ("block", "Don't allow sites to use JavaScript")]));
    site.push(v.select("site.images", "Images", "Takes effect after you restart Kakangkung", &[("allow", "Sites can show images"), ("block", "Don't allow sites to show images")]));
    site.push(v.select("site.sound", "Sound", "New tabs; unmute one with right-click on the tab", &[("allow", "Sites can play sound"), ("mute", "Mute sites that play sound")]));
    for (key, title, what) in [
        ("site.location", "Location", "see your location"),
        ("site.camera", "Camera", "use your camera"),
        ("site.microphone", "Microphone", "use your microphone"),
        ("site.notifications", "Notifications", "send notifications"),
        ("site.clipboard", "Clipboard", "see text and images copied to the clipboard"),
        ("site.automatic_downloads", "Automatic downloads", "download several files automatically"),
        ("site.sensors", "Motion sensors", "use motion sensors"),
        ("site.midi", "MIDI devices", "connect to MIDI devices"),
    ] {
        let ask = format!("Sites can ask to {what}");
        let block = format!("Don't allow sites to {what}");
        site.push(v.select(key, title, "", &[("ask", ask.as_str()), ("block", block.as_str())]));
    }
    site.push(Row::new("Incognito window", "Browse without saving history, cookies or form data · Ctrl+Shift+N", Control::None));
    let mut rows = vec![
        safety_check(v),
        Row::heading("Browsing data"),
        v.select("privacy.clear_range", "Time range", "", &[("hour", "Last hour"), ("day", "Last 24 hours"), ("week", "Last 7 days"), ("month", "Last 4 weeks"), ("all", "All time")]),
        v.toggle("privacy.clear_history", "Browsing history", "Pages you visited in Kakangkung").indented(),
        v.toggle("privacy.clear_cookies", "Cookies and other site data", "Signs you out of most sites").indented(),
        v.toggle("privacy.clear_cache", "Cached images and files", "Some sites may load more slowly next time").indented(),
        Row::new("Delete browsing data", "What is switched on above, within the time range", buttons(&[("clear-all", "Delete data", ButtonStyle::Danger)])),
        v.toggle("privacy.clear_on_exit", "Clear browsing data when you exit", "History, cookies, cache and site data are removed when Kakangkung closes"),
        Row::heading("Security"),
        v.toggle("privacy.safe_browsing", "Safe Browsing", "Microsoft Defender SmartScreen warns about dangerous sites and downloads"),
        v.toggle("privacy.adblock", "Block ads", "Built-in ad blocker: requests to known ad networks are refused; switch off if a site breaks"),
        v.toggle("privacy.https_first", "Always use secure connections", "HTTPS-First: typed http:// addresses open over https://"),
        v.select("privacy.tracking", "Tracking prevention", "Blocks trackers from sites you haven't visited; Strict may break some sites", &[("0", "Off"), ("1", "Basic"), ("2", "Balanced (recommended)"), ("3", "Strict")]),
        Row::new("Manage certificates", "Windows certificate store", buttons(&[("certs", "Open", ButtonStyle::Normal)])),
    ];
    rows.extend(site);
    section("privacy", "Privacy and security", "privacy security safety check clear delete browsing data time range history cookies cache cached images sound mute certificates safe browsing https tracking site settings permissions javascript camera microphone location notifications clipboard downloads sensors midi", vec![rows])
}

/// Chrome's Safety check, from the settings alone: which protections are switched off.
fn safety_check(v: &Values) -> Row {
    let issues: Vec<&str> = [
        (!v.on("privacy.safe_browsing"), "Safe Browsing is off"),
        (!v.on("privacy.https_first"), "secure connections are not enforced"),
        (!v.on("system.auto_update"), "automatic updates are off"),
        (v.get("privacy.tracking") == "0", "tracking prevention is off"),
    ]
    .into_iter()
    .filter_map(|(bad, text)| bad.then_some(text))
    .collect();
    let sub = if issues.is_empty() { "No problems found".to_string() } else { format!("{} problem(s): {}", issues.len(), issues.join(", ")) };
    Row::new("Safety check", &sub, Control::None)
}

fn performance(v: &Values, memory: &str) -> Section {
    let mode = |value: &str, title: &str, sub: &str| radio("memory", value, memory, title, sub).indented();
    section("performance", "Performance", "performance memory saver ram tabs speed fast freeze preload", vec![vec![
        Row::new("Memory Saver", "Frees memory from tabs you are not using. Their pages stay as they are and wake up when you return.", Control::None),
        mode("moderate", "Moderate", "Freeze tabs hidden for 2 hours"),
        mode("balanced", "Balanced (recommended)", "Freeze tabs hidden for 5 minutes"),
        mode("ultralite", "Maximum", "Freeze tabs hidden for 30 seconds"),
        mode("performance", "Off", "Keep every tab running"),
        v.toggle("performance.protect_media", "Keep calls running", "Never freeze tabs using the camera, microphone or screen sharing"),
        v.toggle("performance.protect_pinned", "Keep pinned tabs running", "Memory Saver leaves pinned tabs alone"),
        v.url_field("performance.keep_active", "Always keep these sites active", "Sites and their subdomains, separated by spaces (mail.example.com)"),
    ]])
}

fn appearance(v: &Values) -> Section {
    let home = v.get("appearance.home_url");
    let mut rows = vec![
        v.select("appearance.theme", "Mode", "", &[("auto", "Device"), ("light", "Light"), ("dark", "Dark")]),
        v.toggle("appearance.home_button", "Show Home button", &format!("Opens {}", if home.is_empty() { "the new tab page" } else { home })),
    ];
    if v.on("appearance.home_button") {
        rows.push(v.url_field("appearance.home_url", "Home page", "Empty = the new tab page").indented());
    }
    let zooms: Vec<(String, String)> = ZOOMS.iter().map(|z| (z.to_string(), format!("{z}%"))).collect();
    let zooms: Vec<(&str, &str)> = zooms.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    rows.extend([
        v.toggle("appearance.bookmark_star", "Show bookmark star", "The ☆ in the address bar"),
        v.toggle("appearance.extensions_button", "Show Extensions button", "Pin extensions to the toolbar from its menu"),
        Row::heading("Pinned to the toolbar"),
        v.toggle("appearance.downloads_button", "Downloads", ""),
        v.toggle("appearance.history_button", "History", ""),
        v.toggle("appearance.bookmarks_button", "Bookmarks", ""),
        v.toggle("appearance.taskmanager_button", "Task manager", ""),
        Row::heading("Address bar and zoom"),
        v.toggle("appearance.tab_search_button", "Show tab search button", "The search button at the start of the tab strip; Ctrl+Shift+A always works"),
        v.toggle("appearance.full_urls", "Always show full URLs", "Show https:// and the whole address in the address bar"),
        v.select("appearance.zoom", "Page zoom", "Default zoom of every tab", &zooms),
        Row::heading("Motion"),
        v.toggle("appearance.animations", "Animations", "Pages fade in, tiles rise, switches slide, menus unfold and new tabs widen"),
    ]);
    if v.on("appearance.animations") {
        rows.push(v.select("appearance.animation_speed", "Animation speed", "How long the animations take", &[("fast", "Fast"), ("normal", "Normal"), ("relaxed", "Relaxed")]).indented());
    }
    rows.push(v.toggle("appearance.smooth_scrolling", "Smooth scrolling", "Built-in pages glide to where you scroll; off: they jump"));
    section("appearance", "Appearance", "appearance theme dark light mode home button bookmark star extensions toolbar address full url zoom motion animations speed smooth scrolling", vec![rows])
}

fn tabs(v: &Values) -> Section {
    let mut rows = vec![
        Row::heading("New tabs"),
        v.select("tabs.new_tab_page", "New tab page", "What a new tab shows", &[("kakangkung", "Kakangkung page"), ("blank", "Blank page"), ("custom", "A page of your choice")]),
    ];
    if v.get("tabs.new_tab_page") == "custom" {
        rows.push(v.url_field("tabs.ntp_url", "Address", "").indented());
    }
    rows.extend([
        v.select("tabs.position", "Where new tabs open", "", &[("end", "At the end of the tab strip"), ("next", "Next to the current tab")]),
        v.toggle("tabs.link_focus", "Switch to tabs opened from links", "Off: links opened in a new tab load in the background"),
        Row::heading("Closing"),
        v.select("tabs.close_last", "When you close the last tab", "", &[("window", "Close the window"), ("newtab", "Keep the window with a new tab")]),
        v.toggle("tabs.confirm_close", "Ask before closing a window with several tabs", ""),
        Row::heading("Tab features"),
        v.toggle("tabs.groups", "Tab groups", "Off: existing groups are removed"),
        v.toggle("tabs.split_view", "Split view", "Two tabs side by side"),
        v.toggle("tabs.speaker_mutes", "Mute a tab by clicking its speaker icon", ""),
        Row::heading("Page tools in the ⋮ menu"),
        v.toggle("tools.new_window", "New window (Ctrl+N)", ""),
        v.toggle("tools.incognito", "Incognito windows (Ctrl+Shift+N)", "A separate window whose tabs keep no history, cookies or form data"),
        v.toggle("tools.pip", "Picture in picture", ""),
        v.toggle("tools.reader", "Reading mode", ""),
        v.toggle("tools.translate", "Translate page", ""),
    ]);
    section("tabs", "Tabs and windows", "tabs windows new tab page position open links background close last tab confirm groups split view mute speaker picture in picture reading mode translate new window tools", vec![rows])
}

fn search(v: &Values, s: &Json) -> Section {
    let current = s["engine"].as_str().unwrap_or("");
    let mut engines: Vec<(String, String)> = s["engines"]
        .as_array()
        .map(|a| a.iter().map(|e| (e["id"].as_str().unwrap_or("").to_string(), e["name"].as_str().unwrap_or("").to_string())).collect())
        .unwrap_or_default();
    let custom = !engines.iter().any(|(id, _)| id == current);
    if custom {
        engines.push((current.to_string(), "Custom".into()));
    }
    section("search", "Search engine", "search engine google bing duckduckgo address bar suggestions autocomplete history", vec![vec![
        Row::new("Search engine used in the address bar", "", Control::Select { id: "engine".into(), value: current.into(), options: engines }),
        Row::new("Custom search engine", "An https address with %s where the search words go", Control::Text {
            id: "custom-engine".into(),
            value: if custom { current.into() } else { String::new() },
            placeholder: "https://example.com/search?q=%s".into(),
            button: "Use".into(),
        }),
        v.toggle("search.suggestions", "Search suggestions", "Ask the search engine for suggestions while you type"),
        v.toggle("search.history_suggestions", "Suggestions from history and bookmarks", "Also completes addresses as you type"),
    ]])
}

fn startup(v: &Values, current: &str) -> Section {
    let mut rows = vec![
        radio("startup", "newtab", current, "Open the New Tab page", ""),
        radio("startup", "continue", current, "Continue where you left off", ""),
        radio("startup", "pages", current, "Open a specific page or set of pages", ""),
    ];
    if current == "pages" {
        let mut field = v.url_field("startup.pages", "Pages", "Up to 10 addresses, separated by spaces");
        field = field.indented();
        rows.push(field);
    }
    section("startup", "On startup", "startup start open continue session restore new tab specific pages", vec![rows])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::SETTABLE;
    use serde_json::json;

    fn answer() -> Json {
        let all: serde_json::Map<String, Json> = SETTABLE.iter().map(|(k, _, d)| (k.to_string(), json!(d))).collect();
        json!({ "all": all, "engine": "google", "engines": [{ "id": "google", "name": "Google" }], "memory": "balanced", "startup": "newtab",
                "sync": { "signedIn": false }, "isDefault": false, "version": "1", "webview2": "2" })
    }

    fn controls(doc: &PageDoc) -> Vec<String> {
        doc.sections
            .iter()
            .flat_map(|s| s.cards.iter().flatten())
            .filter_map(|r| match &r.control {
                Control::Toggle { id, .. } | Control::Select { id, .. } | Control::Text { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_settable_key_has_a_control() {
        let mut s = answer();
        // Dependent rows show once their switch allows them.
        s["all"]["appearance.home_button"] = json!("1");
        s["all"]["tabs.new_tab_page"] = json!("custom");
        s["startup"] = json!("pages");
        let ids = controls(&settings_doc(&s, None));
        for (key, _, _) in SETTABLE {
            let radio = ["memory.mode", "startup"].contains(key);
            assert!(radio || ids.iter().any(|i| i == key), "no control for {key}");
        }
        for key in crate::prefs::URL_SETTINGS {
            assert!(ids.iter().any(|i| i == key), "no field for {key}");
        }
    }

    #[test]
    fn navigation_points_at_sections_or_pages() {
        let doc = settings_doc(&answer(), None);
        for n in &doc.nav {
            assert!(n.id.starts_with("open:") || doc.sections.iter().any(|s| s.id == n.id), "{}", n.id);
        }
    }

    #[test]
    fn permissions_list_once_loaded() {
        let perms = [json!({ "origin": "https://maps.example", "kind": 3, "name": "Location", "allowed": true })];
        let doc = settings_doc(&answer(), Some(&perms));
        let privacy = doc.sections.iter().find(|s| s.id == "privacy").unwrap();
        let row = privacy.cards[0].iter().find(|r| r.title == "maps.example").expect("permission row");
        assert_eq!(row.sub, "Location: Allowed");
        assert!(matches!(&row.control, Control::Buttons(b) if b[0].id == "perm-reset:3:https://maps.example"));
    }
}
