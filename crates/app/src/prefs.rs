//! Saved preferences. Every user setting is listed here once (key, allowed values, default);
//! docs/SETTINGS.md describes each one. The settings page reads and writes them by key.

use std::path::PathBuf;

use serde_json::{json, Value as Json};
use storage::Store;

use crate::profile::account;

pub(crate) fn mode_id(m: memory::Mode) -> &'static str {
    match m {
        memory::Mode::Moderate => "moderate",
        memory::Mode::UltraLite => "ultralite",
        memory::Mode::Balanced => "balanced",
        memory::Mode::Performance => "performance",
    }
}

pub(crate) fn mode_from_id(id: &str) -> Option<memory::Mode> {
    [memory::Mode::Moderate, memory::Mode::UltraLite, memory::Mode::Balanced, memory::Mode::Performance]
        .into_iter()
        .find(|m| mode_id(*m) == id)
}

const ON_OFF: &[&str] = &["1", "0"];
const OFF_ON: &[&str] = &["0", "1"];
const ASK_BLOCK: &[&str] = &["ask", "block"];
const ZOOMS: &[&str] = &["50", "67", "75", "80", "90", "100", "110", "125", "150", "175", "200"];
/// Languages offered for "Translate page" ("auto" = the browser's language).
pub(crate) const LANGUAGES: &[&str] = &[
    "auto", "id", "en", "ms", "jv", "su", "ar", "de", "es", "fr", "hi", "it", "ja", "ko", "nl", "pt", "ru", "th", "tr",
    "vi", "zh-CN", "zh-TW",
];

/// Every setting with a fixed set of values: key, allowed values (first = default unless named).
pub(crate) const SETTABLE: &[(&str, &[&str], &str)] = &[
    // Appearance
    ("appearance.theme", &["auto", "light", "dark"], "auto"),
    ("appearance.animations", ON_OFF, "1"),
    ("appearance.animation_speed", &["normal", "fast", "relaxed"], "normal"),
    ("appearance.smooth_scrolling", ON_OFF, "1"),
    ("appearance.home_button", OFF_ON, "0"),
    ("appearance.bookmark_star", ON_OFF, "1"),
    ("appearance.extensions_button", ON_OFF, "1"),
    ("appearance.tab_search_button", ON_OFF, "1"),
    ("appearance.full_urls", OFF_ON, "0"),
    ("appearance.zoom", ZOOMS, "100"),
    // Tabs
    ("tabs.new_tab_page", &["kakangkung", "blank", "custom"], "kakangkung"),
    ("tabs.position", &["end", "next"], "end"),
    ("tabs.link_focus", ON_OFF, "1"),
    ("tabs.close_last", &["window", "newtab"], "window"),
    ("tabs.confirm_close", OFF_ON, "0"),
    ("tabs.groups", ON_OFF, "1"),
    ("tabs.split_view", ON_OFF, "1"),
    ("tabs.speaker_mutes", ON_OFF, "1"),
    // Page tools in the ⋮ menu
    ("tools.pip", ON_OFF, "1"),
    ("tools.reader", ON_OFF, "1"),
    ("tools.translate", ON_OFF, "1"),
    ("tools.new_window", ON_OFF, "1"),
    ("tools.incognito", ON_OFF, "1"),
    // Search
    ("search.suggestions", ON_OFF, "1"),
    ("search.history_suggestions", ON_OFF, "1"),
    // Privacy and security, autofill
    ("privacy.tracking", &["0", "1", "2", "3"], "2"),
    ("privacy.safe_browsing", ON_OFF, "1"),
    ("privacy.adblock", ON_OFF, "1"),
    ("privacy.https_first", ON_OFF, "1"),
    ("privacy.passwords", ON_OFF, "1"),
    ("privacy.autofill", ON_OFF, "1"),
    ("privacy.clear_on_exit", OFF_ON, "0"),
    ("privacy.clear_range", &["all", "hour", "day", "week", "month"], "all"),
    ("privacy.clear_history", ON_OFF, "1"),
    ("privacy.clear_cookies", ON_OFF, "1"),
    ("privacy.clear_cache", ON_OFF, "1"),
    // Site settings: what every site may do unless you decided otherwise for that site
    ("site.javascript", &["allow", "block"], "allow"),
    ("site.location", ASK_BLOCK, "ask"),
    ("site.camera", ASK_BLOCK, "ask"),
    ("site.microphone", ASK_BLOCK, "ask"),
    ("site.notifications", ASK_BLOCK, "ask"),
    ("site.clipboard", ASK_BLOCK, "ask"),
    ("site.automatic_downloads", ASK_BLOCK, "ask"),
    ("site.sensors", ASK_BLOCK, "ask"),
    ("site.midi", ASK_BLOCK, "ask"),
    ("site.sound", &["allow", "mute"], "allow"),
    ("site.images", &["allow", "block"], "allow"),
    // Downloads
    ("downloads.ask", OFF_ON, "0"),
    ("downloads.show_done", OFF_ON, "0"),
    // Performance
    ("performance.protect_media", ON_OFF, "1"),
    ("performance.protect_pinned", ON_OFF, "1"),
    // Accessibility, system, languages
    ("accessibility.focus_highlight", OFF_ON, "0"),
    ("system.hardware_acceleration", ON_OFF, "1"),
    ("system.auto_update", ON_OFF, "1"),
    ("languages.translate_to", LANGUAGES, "auto"),
    ("languages.preferred", LANGUAGES, "auto"),
    ("languages.ui", &["auto", "en", "id"], "auto"),
];

/// Settings holding web addresses (checked to be http/https). `startup.pages` is one per line.
pub(crate) const URL_SETTINGS: &[&str] = &["appearance.home_url", "tabs.ntp_url", "startup.pages", "performance.keep_active"];

/// Site permission settings and the `COREWEBVIEW2_PERMISSION_KIND` each one blocks.
pub(crate) const PERMISSION_KINDS: &[(&str, i32)] = &[
    ("site.microphone", 1),
    ("site.camera", 2),
    ("site.location", 3),
    ("site.notifications", 4),
    ("site.sensors", 5),
    ("site.clipboard", 6),
    ("site.automatic_downloads", 7),
    ("site.midi", 11),
];

/// Up to this many addresses in a list setting ("Open a specific page", "Always keep these sites active").
fn max_urls(key: &str) -> usize {
    match key {
        "startup.pages" => 10,
        "performance.keep_active" => 50,
        _ => 1,
    }
}

/// `value` if it is a valid web address list for URL setting `key` ("" clears it).
pub(crate) fn valid_url_setting(key: &str, value: &str) -> Option<String> {
    let max = max_urls(key);
    let urls: Vec<&str> = value.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    (URL_SETTINGS.contains(&key) && urls.len() <= max && urls.iter().all(|u| storage::storable(u))).then(|| urls.join("\n"))
}

/// Saved preferences read at start-up (before the first tab).
pub(crate) struct Prefs {
    pub(crate) engine: String,
    pub(crate) memory: memory::Mode,
    /// "newtab", "continue" (reopen last session's tabs) or "pages" (`startup_pages`).
    pub(crate) startup: String,
    pub(crate) startup_pages: Vec<String>,
    pub(crate) downloads: PathBuf,
    pub(crate) session: Vec<String>,
    pub(crate) session_active: usize,
    pub(crate) settings: Settings,
    /// Signed-in account name, "" when signed out.
    pub(crate) account: String,
}

/// The settings the running browser applies live (re-read after every change).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Settings {
    /// "auto", "light" or "dark".
    pub(crate) theme: String,
    pub(crate) animations: bool,
    /// Length of animations: 1.0 normal, below 1 faster, above 1 slower.
    pub(crate) animation_speed: f32,
    pub(crate) smooth_scrolling: bool,
    pub(crate) protection: platform::Protection,
    /// Upgrade typed http:// addresses to https:// (HTTPS-First).
    pub(crate) https_first: bool,
    pub(crate) clear_on_exit: bool,
    /// "Delete browsing data" time range: "all", "hour", "day", "week" or "month".
    pub(crate) clear_range: String,
    /// What "Delete browsing data" removes.
    pub(crate) clear_history: bool,
    pub(crate) clear_cookies: bool,
    pub(crate) clear_cache: bool,
    /// Sound site setting "Don't allow sites to play sound": new tabs start muted.
    pub(crate) mute_sites: bool,
    /// Site settings > Images "Don't allow sites to show images" (a start-up switch).
    pub(crate) block_images: bool,
    /// Open the Downloads page when a download finishes.
    pub(crate) show_downloads_done: bool,
    /// Language sites are asked for (Accept-Language), "auto" = Windows' language.
    pub(crate) preferred_language: String,
    /// Hosts Memory Saver never freezes ("Always keep these sites active").
    pub(crate) keep_active: Vec<String>,
    /// Address the Home button opens.
    pub(crate) home_url: String,
    pub(crate) full_urls: bool,
    /// Default page zoom of new tabs (1.0 = 100 %).
    pub(crate) zoom: f64,
    /// Address a new tab opens.
    pub(crate) new_tab_url: String,
    /// New tabs open next to the current one instead of at the end.
    pub(crate) new_tab_next: bool,
    /// Tabs opened from links are selected at once.
    pub(crate) link_focus: bool,
    /// Closing the last tab opens a new tab instead of closing the window.
    pub(crate) keep_window: bool,
    pub(crate) confirm_close: bool,
    pub(crate) groups: bool,
    pub(crate) split_view: bool,
    pub(crate) pip: bool,
    pub(crate) reader: bool,
    pub(crate) translate: bool,
    pub(crate) new_window: bool,
    /// Incognito windows may be opened (Ctrl+Shift+N).
    pub(crate) incognito: bool,
    pub(crate) suggestions: bool,
    pub(crate) history_suggestions: bool,
    pub(crate) protect_media: bool,
    pub(crate) protect_pinned: bool,
    pub(crate) focus_highlight: bool,
    pub(crate) hardware_acceleration: bool,
    /// Download new versions in the background and install them when Kakangkung closes.
    pub(crate) auto_update: bool,
    /// Target language of "Translate page" ("auto" = the browser's language).
    pub(crate) translate_to: String,
    /// Interface language ("auto": the Windows display language).
    pub(crate) ui_language: String,
    pub(crate) home_button: bool,
    pub(crate) bookmark_star: bool,
    pub(crate) extensions_button: bool,
    pub(crate) tab_search_button: bool,
    /// Clicking a tab's speaker icon mutes it.
    pub(crate) speaker_mutes: bool,
}

impl Settings {
    pub(crate) fn read(get: impl Fn(&str) -> Option<String>) -> Settings {
        let v = |key: &str| {
            let (_, allowed, default) = SETTABLE.iter().find(|(k, _, _)| *k == key).copied().unwrap_or((key, &[], ""));
            get(key).filter(|x| allowed.contains(&x.as_str())).unwrap_or_else(|| default.to_string())
        };
        let on = |key: &str| v(key) == "1";
        let url = |key: &str| get(key).and_then(|u| valid_url_setting(key, &u)).filter(|u| !u.is_empty());
        let new_tab_url = match v("tabs.new_tab_page").as_str() {
            "blank" => "about:blank".to_string(),
            "custom" => url("tabs.ntp_url").unwrap_or_else(|| ui::NEWTAB_URL.into()),
            _ => ui::NEWTAB_URL.into(),
        };
        Settings {
            theme: v("appearance.theme"),
            animations: on("appearance.animations"),
            animation_speed: match v("appearance.animation_speed").as_str() {
                "fast" => 0.6,
                "relaxed" => 1.6,
                _ => 1.0,
            },
            smooth_scrolling: on("appearance.smooth_scrolling"),
            protection: platform::Protection {
                tracking: v("privacy.tracking").parse().unwrap_or(2),
                safe_browsing: on("privacy.safe_browsing"),
                save_passwords: on("privacy.passwords"),
                autofill: on("privacy.autofill"),
                script: v("site.javascript") == "allow",
                blocked: PERMISSION_KINDS.iter().filter(|(k, _)| v(k) == "block").fold(0, |m, (_, kind)| m | 1 << kind),
                ask_downloads: on("downloads.ask"),
                adblock: on("privacy.adblock"),
            },
            https_first: on("privacy.https_first"),
            clear_on_exit: on("privacy.clear_on_exit"),
            clear_range: v("privacy.clear_range"),
            clear_history: on("privacy.clear_history"),
            clear_cookies: on("privacy.clear_cookies"),
            clear_cache: on("privacy.clear_cache"),
            mute_sites: v("site.sound") == "mute",
            block_images: v("site.images") == "block",
            show_downloads_done: on("downloads.show_done"),
            preferred_language: v("languages.preferred"),
            keep_active: url("performance.keep_active").unwrap_or_default().lines().map(|u| ui::host(u).to_string()).collect(),
            home_url: url("appearance.home_url").unwrap_or_else(|| new_tab_url.clone()),
            full_urls: on("appearance.full_urls"),
            zoom: v("appearance.zoom").parse::<f64>().unwrap_or(100.0) / 100.0,
            new_tab_url,
            new_tab_next: v("tabs.position") == "next",
            link_focus: on("tabs.link_focus"),
            keep_window: v("tabs.close_last") == "newtab",
            confirm_close: on("tabs.confirm_close"),
            groups: on("tabs.groups"),
            split_view: on("tabs.split_view"),
            pip: on("tools.pip"),
            reader: on("tools.reader"),
            translate: on("tools.translate"),
            new_window: on("tools.new_window"),
            incognito: on("tools.incognito"),
            suggestions: on("search.suggestions"),
            history_suggestions: on("search.history_suggestions"),
            protect_media: on("performance.protect_media"),
            protect_pinned: on("performance.protect_pinned"),
            focus_highlight: on("accessibility.focus_highlight"),
            hardware_acceleration: on("system.hardware_acceleration"),
            auto_update: on("system.auto_update"),
            translate_to: v("languages.translate_to"),
            ui_language: v("languages.ui"),
            home_button: on("appearance.home_button"),
            bookmark_star: on("appearance.bookmark_star"),
            extensions_button: on("appearance.extensions_button"),
            tab_search_button: on("appearance.tab_search_button"),
            speaker_mutes: on("tabs.speaker_mutes"),
        }
    }

    /// Start (Unix ms) of the "Delete browsing data" range, `None` = all time.
    pub(crate) fn clear_since(&self, now_ms: i64) -> Option<i64> {
        const HOUR: i64 = 3_600_000;
        let span = match self.clear_range.as_str() {
            "hour" => HOUR,
            "day" => 24 * HOUR,
            "week" => 7 * 24 * HOUR,
            "month" => 28 * 24 * HOUR,
            _ => return None,
        };
        Some(now_ms - span)
    }

    /// Memory Saver leaves this page alone (it or a parent domain is on the keep-active list).
    pub(crate) fn kept_active(&self, url: &str) -> bool {
        let host = ui::host(url);
        self.keep_active.iter().any(|k| host == k || host.ends_with(&format!(".{k}")))
    }

    pub(crate) fn wry_theme(&self) -> wry::Theme {
        match self.theme.as_str() {
            "light" => wry::Theme::Light,
            "dark" => wry::Theme::Dark,
            _ => wry::Theme::Auto,
        }
    }
}

/// Every setting's current value by key, for the settings page.
pub(crate) fn all_values(s: &Store) -> Json {
    let mut out = serde_json::Map::new();
    for (key, allowed, default) in SETTABLE {
        let value = s.setting(key).filter(|x| allowed.contains(&x.as_str())).unwrap_or_else(|| default.to_string());
        out.insert(key.to_string(), json!(value));
    }
    for key in URL_SETTINGS {
        out.insert(key.to_string(), json!(s.setting(key).unwrap_or_default()));
    }
    Json::Object(out)
}

/// Put every setting back to its default ("Reset settings"). Account, sync, history,
/// bookmarks and the download folder are kept, like Chrome's reset.
pub(crate) fn reset(s: &Store) -> Result<(), String> {
    for (key, _, default) in SETTABLE {
        s.set_setting(key, default)?;
    }
    for key in URL_SETTINGS {
        s.set_setting(key, "")?;
    }
    s.set_setting("search.engine", "")?;
    s.set_setting("memory.mode", mode_id(memory::Mode::default()))?;
    s.set_setting("startup", "newtab")
}

impl Prefs {
    pub(crate) fn read(s: &Store) -> Prefs {
        let downloads = s.setting("downloads.folder").map(PathBuf::from).unwrap_or_else(|| {
            std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default().join("Downloads")
        });
        let session: Vec<String> =
            s.setting("session.tabs").and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
        let startup_pages = s.setting("startup.pages").unwrap_or_default().lines().map(String::from).collect();
        Prefs {
            engine: s.setting("search.engine").unwrap_or_default(),
            memory: s.setting("memory.mode").and_then(|m| mode_from_id(&m)).unwrap_or_default(),
            startup: s.setting("startup").unwrap_or_else(|| "newtab".into()),
            startup_pages,
            downloads,
            session: session.into_iter().filter(|u| storage::storable(u) || u.starts_with(ui::INTERNAL)).collect(),
            session_active: s.setting("session.active").and_then(|v| v.parse().ok()).unwrap_or(0),
            settings: Settings::read(|k| s.setting(k)),
            account: account(s).map(|a| a.0).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_allowed_values() {
        for (key, allowed, default) in SETTABLE {
            assert!(allowed.contains(default), "{key}: default {default} not allowed");
        }
    }

    #[test]
    fn invalid_values_fall_back_to_defaults() {
        let s = Settings::read(|k| match k {
            "appearance.zoom" => Some("999".into()),
            "tabs.new_tab_page" => Some("custom".into()),
            "tabs.ntp_url" => Some("javascript:alert(1)".into()),
            "tabs.position" => Some("next".into()),
            _ => None,
        });
        assert_eq!(s.zoom, 1.0);
        assert_eq!(s.new_tab_url, ui::NEWTAB_URL, "unsafe custom new tab page is ignored");
        assert!(s.new_tab_next);
        assert!(s.link_focus && s.groups && s.pip && !s.confirm_close);
    }

    #[test]
    fn url_settings_accept_only_web_addresses() {
        assert_eq!(valid_url_setting("appearance.home_url", " https://example.com ").as_deref(), Some("https://example.com"));
        assert_eq!(valid_url_setting("appearance.home_url", "").as_deref(), Some(""));
        assert!(valid_url_setting("appearance.home_url", "file:///c:/x").is_none());
        assert!(valid_url_setting("appearance.home_url", "https://a.com\nhttps://b.com").is_none(), "one address only");
        assert!(valid_url_setting("startup.pages", "https://a.com\nhttps://b.com").is_some());
        assert!(valid_url_setting("privacy.tracking", "https://a.com").is_none(), "not a URL setting");
    }

    #[test]
    fn site_settings_and_ranges() {
        let s = Settings::read(|k| match k {
            "site.javascript" => Some("block".into()),
            "site.camera" | "site.notifications" => Some("block".into()),
            "privacy.clear_range" => Some("hour".into()),
            "privacy.clear_cache" => Some("0".into()),
            "site.sound" => Some("mute".into()),
            "site.images" => Some("block".into()),
            "downloads.ask" => Some("1".into()),
            "languages.preferred" => Some("id".into()),
            "performance.keep_active" => Some("https://mail.example.com\nhttps://music.test".into()),
            _ => None,
        });
        assert!(!s.protection.script);
        assert_eq!(s.protection.blocked, 1 << 2 | 1 << 4);
        assert_eq!(s.clear_since(10_000_000), Some(10_000_000 - 3_600_000));
        assert!(s.kept_active("https://mail.example.com/inbox") && s.kept_active("https://a.music.test/"));
        assert!(!s.kept_active("https://example.com/") && !s.kept_active("https://evilmusic.test/"));
        let d = Settings::read(|_| None);
        assert!(s.clear_history && s.clear_cookies && !s.clear_cache && s.mute_sites && s.preferred_language == "id");
        assert!(d.protection.script && d.protection.blocked == 0 && d.clear_since(1).is_none());
        assert!(d.protection.adblock, "the ad blocker is on unless switched off");
        assert!(s.block_images && s.protection.ask_downloads && !s.show_downloads_done);
        assert!(d.clear_cache && !d.mute_sites && d.preferred_language == "auto" && !d.block_images && !d.protection.ask_downloads);
    }
}
