//! The browser's UI logic: address-bar input, security state, shortcuts, sizes, the logo.

/// Kakangkung's logo (drawn as the window icon).
pub const LOGO_SVG: &str = include_str!("../../../assets/logo.svg");

/// Product name shown in the window title and built-in pages.
pub const APP_NAME: &str = "Kakangkung";

// Browser UI sizes in DIPs.
pub const TAB_STRIP_HEIGHT_DIP: u32 = 41;
/// Toolbar including its 1 px bottom separator.
pub const TOOLBAR_HEIGHT_DIP: u32 = 46;
pub const TAB_HEIGHT_DIP: u32 = 34;
pub const TAB_MAX_WIDTH_DIP: u32 = 240;
pub const TAB_MIN_WIDTH_DIP: u32 = 44;
pub const OMNIBOX_HEIGHT_DIP: u32 = 34;
pub const CAPTION_BUTTON_WIDTH_DIP: u32 = 46;
/// Height of the UI webview: tab strip + toolbar (87).
pub const CHROME_HEIGHT_DIP: f64 = (TAB_STRIP_HEIGHT_DIP + TOOLBAR_HEIGHT_DIP) as f64;

/// Origin of the built-in pages (custom protocol `kakangkung`, served over https).
pub const INTERNAL: &str = "https://kakangkung.internal/";
pub const NEWTAB_URL: &str = INTERNAL;
/// A search engine the address bar can use; `%s` in `url` is replaced by the encoded query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Engine {
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    /// Query-suggestion endpoint (OpenSearch-style JSON reply); `%s` = encoded query.
    pub suggest: &'static str,
}

/// Search engines offered in kakangkung://settings: each engine's public search and suggestion
/// addresses, `%s` standing for the search words. Google is the default.
pub const ENGINES: [Engine; 6] = [
    Engine {
        id: "google",
        name: "Google",
        url: "https://www.google.com/search?q=%s&ie=UTF-8",
        suggest: "https://www.google.com/complete/search?client=chrome&q=%s",
    },
    Engine {
        id: "bing",
        name: "Bing",
        url: "https://www.bing.com/search?q=%s",
        suggest: "https://www.bing.com/osjson.aspx?query=%s&language=en",
    },
    Engine {
        id: "duckduckgo",
        name: "DuckDuckGo",
        url: "https://duckduckgo.com/?q=%s",
        suggest: "https://duckduckgo.com/ac/?q=%s&type=list",
    },
    Engine {
        id: "yahoo",
        name: "Yahoo!",
        url: "https://search.yahoo.com/search?ei=UTF-8&fr=crmas_sfp&p=%s",
        suggest: "https://search.yahoo.com/sugg/chrome?output=fxjson&appid=crmas_sfp&command=%s",
    },
    Engine {
        id: "ecosia",
        name: "Ecosia",
        url: "https://www.ecosia.org/search?q=%s",
        suggest: "https://ac.ecosia.org?q=%s&mkt=en&type=list&language=en",
    },
    Engine {
        id: "brave",
        name: "Brave Search",
        url: "https://search.brave.com/search?q=%s",
        suggest: "https://search.brave.com/api/suggest?q=%s&rich=true&rich_verticals=true",
    },
];

/// Suggestion URL for a saved engine setting and query; custom engines have none.
pub fn suggest_url(setting: &str, query: &str) -> Option<String> {
    let id = if setting.is_empty() { ENGINES[0].id } else { setting };
    ENGINES.iter().find(|e| e.id == id).map(|e| e.suggest.replace("%s", &encode_query(query)))
}

/// Query suggestions from an OpenSearch-style reply `[query, [s1, s2, ...], ...]`. Entries may be
/// strings or objects with a `q` field (Brave's rich format); Google's `)]}'` guard is skipped.
pub fn parse_suggestions(body: &[u8], limit: usize) -> Vec<String> {
    let text = String::from_utf8_lossy(body);
    let json = text.trim_start().strip_prefix(")]}'").unwrap_or(&text);
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    v.get(1)
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(str::to_string).or_else(|| e.get("q").and_then(|q| q.as_str()).map(str::to_string)))
                .filter(|s| !s.trim().is_empty())
                .take(limit)
                .collect()
        })
        .unwrap_or_default()
}

pub const DEFAULT_SEARCH: &str = ENGINES[0].url;

/// Search URL template for a saved setting: a known engine id, or a custom `https://...%s` template.
/// Anything else falls back to Google.
pub fn search_template(setting: &str) -> (String, String) {
    if let Some(e) = ENGINES.iter().find(|e| e.id == setting) {
        return (e.url.into(), e.name.into());
    }
    if valid_custom_engine(setting) {
        let host = setting.trim_start_matches("https://").split('/').next().unwrap_or("custom").to_string();
        return (setting.into(), host);
    }
    (ENGINES[0].url.into(), ENGINES[0].name.into())
}

/// Site search shortcuts beside the engines' own ids: `@yt cats` searches YouTube.
pub const SITE_SEARCH: &[(&str, &str)] = &[
    ("youtube", "https://www.youtube.com/results?search_query=%s"),
    ("yt", "https://www.youtube.com/results?search_query=%s"),
    ("wikipedia", "https://en.wikipedia.org/w/index.php?search=%s"),
    ("github", "https://github.com/search?q=%s"),
    ("maps", "https://www.google.com/maps/search/%s"),
    ("ddg", ENGINES[2].url),
];

/// `@keyword words` -> that engine or site's search for the words (Chrome's site search).
/// The keyword may be shortened (`@wiki`); the first match wins.
// ponytail: built-in keywords only; a user-editable list when someone asks for their own sites.
fn site_search(t: &str) -> Option<String> {
    let (key, q) = t.strip_prefix('@')?.split_once(char::is_whitespace)?;
    let (key, q) = (key.to_ascii_lowercase(), q.trim());
    if key.is_empty() || q.is_empty() {
        return None;
    }
    let url = ENGINES.iter().map(|e| (e.id, e.url)).chain(SITE_SEARCH.iter().copied()).find(|(k, _)| k.starts_with(&key))?.1;
    Some(url.replace("%s", &encode_query(q)))
}

/// A custom engine must be an https URL that contains `%s` exactly once.
pub fn valid_custom_engine(t: &str) -> bool {
    t.starts_with("https://") && t.matches("%s").count() == 1 && !t.contains(char::is_whitespace) && t.len() > 12
}

/// `192.168.1.1`: a numeric IPv4 address (no port).
pub fn numeric_host(host: &str) -> bool {
    host.split('.').count() == 4 && host.split('.').all(|p| p.parse::<u8>().is_ok())
}

/// What the user typed into the address bar -> URL to load. `None` for empty input.
///
/// Security: `javascript:`, `vbscript:` and `data:` are never navigated to from the address bar
/// (they run attacker-supplied code in the current origin when pasted); they become a search.
/// Bare host names are opened over HTTPS.
pub fn resolve_input(text: &str, search: &str) -> Option<String> {
    let t = text.trim().trim_matches('"');
    if t.is_empty() {
        return None;
    }
    // A file on this PC ("C:\Docs\report.pdf", "\\server\share\x.pdf"): Explorer's "Open with",
    // a drop, or typed.
    if let Some(url) = file_url(t) {
        return Some(url);
    }
    if let Some(url) = site_search(t) {
        return Some(url);
    }
    let lower = t.to_ascii_lowercase();
    let search_for = |q: &str| search.replace("%s", &encode_query(q));
    if ["javascript:", "vbscript:", "data:"].iter().any(|s| lower.starts_with(s)) {
        return Some(search_for(t));
    }
    if ["http://", "https://", "file:///"].iter().any(|s| lower.starts_with(s)) || lower == "about:blank" {
        return Some(t.to_string());
    }
    if let Some(page) = lower.strip_prefix("kakangkung://") {
        let page = page.trim_end_matches('/');
        return Some(if page == "newtab" { NEWTAB_URL.into() } else { format!("{INTERNAL}{page}") });
    }
    if t.contains(char::is_whitespace) {
        return Some(search_for(t));
    }
    let host = lower.split(['/', '?', '#']).next().unwrap_or("");
    let bare_host = host.split(':').next().unwrap_or("");
    // A numeric address (a router at 192.168.1.1, a dev server) rarely speaks HTTPS.
    let ipv4 = numeric_host(bare_host);
    let local = bare_host == "localhost" || ipv4 || host.starts_with("[::1]");
    if local {
        return Some(format!("http://{t}"));
    }
    let looks_like_host = bare_host.contains('.')
        && !bare_host.starts_with('.')
        && !bare_host.ends_with('.')
        && bare_host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if looks_like_host {
        Some(format!("https://{t}"))
    } else {
        Some(search_for(t))
    }
}

/// `file:///C:/Docs/a%20b.pdf` for a Windows path (`C:\Docs\a b.pdf`, `\\server\share\x`);
/// `None` for anything else.
pub fn file_url(path: &str) -> Option<String> {
    let b = path.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    let unc = path.starts_with("\\\\") && path.len() > 2;
    if !drive && !unc {
        return None;
    }
    let mut out = String::from(if unc { "file:" } else { "file:///" });
    for c in path.chars() {
        match c {
            '\\' => out.push('/'),
            ' ' | '%' | '#' | '?' | '"' | '<' | '>' | '`' | '{' | '}' => out.push_str(&format!("%{:02X}", c as u32)),
            c => out.push(c),
        }
    }
    Some(out)
}

/// What a tab of `url` is called before (or without) a page title: the file name for a file
/// (`report.pdf`, `song.mp3`, a local file), else the site's address.
pub fn page_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let path = path.split_once("://").map_or(path, |(_, rest)| rest.find('/').map_or("", |i| &rest[i..]));
    let last = path.rsplit('/').next().unwrap_or("");
    if last.contains('.') && !last.ends_with('.') {
        let decoded = percent_decode(last);
        if !decoded.trim().is_empty() {
            return decoded;
        }
    }
    let h = host(url);
    if h.is_empty() { url.to_string() } else { h.to_string() }
}

/// `%20` and friends back to text (invalid sequences stay as they are).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Files Kakangkung shows itself (PDF viewer, images, media, text), by extension: a download
/// of one opens in a tab.
pub fn viewable_file(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(
        ext.as_str(),
        "pdf" | "htm" | "html" | "xhtml" | "svg" | "txt" | "json" | "xml" | "csv" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "ico"
            | "mp4" | "webm" | "ogg" | "ogv" | "mp3" | "wav" | "m4a" | "flac" | "opus"
    )
}

pub fn encode_query(q: &str) -> String {
    let mut out = String::new();
    for b in q.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Connection state shown in the address bar chip, like Chrome's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// Built-in page (new tab, about).
    Internal,
    /// HTTPS: lock / "tune" icon.
    Secure,
    /// Plain HTTP to a remote host: "Not secure".
    Insecure,
    /// file:// or localhost: info icon.
    Local,
}

impl Security {
    pub fn of(url: &str) -> Self {
        let lower = url.to_ascii_lowercase();
        if lower.starts_with(INTERNAL) || lower.starts_with("about:") || lower.is_empty() {
            Security::Internal
        } else if lower.starts_with("https://") {
            Security::Secure
        } else if lower.starts_with("file:") || matches!(host(&lower).rsplit_once(':').map_or(host(&lower), |(h, port)| if port.contains(']') { host(&lower) } else { h }), "localhost" | "127.0.0.1" | "[::1]") {
            Security::Local
        } else {
            Security::Insecure
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Security::Internal => "internal",
            Security::Secure => "secure",
            Security::Insecure => "insecure",
            Security::Local => "local",
        }
    }
}

/// URL as Chrome shows it when the address bar is not focused: no scheme, no lone trailing slash,
/// empty on the new-tab page.
/// Host part of a URL ("" when there is none), e.g. `mail.google.com` for `https://mail.google.com/u/0`.
pub fn host(url: &str) -> &str {
    url.split_once("://").map_or("", |(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
}

pub fn display_url(url: &str) -> String {
    if url == NEWTAB_URL {
        return String::new();
    }
    if let Some(page) = url.strip_prefix(INTERNAL) {
        return format!("kakangkung://{page}");
    }
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).unwrap_or(url);
    match rest.find('/') {
        Some(i) if &rest[i..] == "/" => rest[..i].to_string(),
        _ => rest.to_string(),
    }
}

/// Browser commands reachable from keyboard shortcuts and the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NewTab,
    /// Incognito: InPrivate webview, nothing saved (Ctrl+Shift+N).
    NewPrivateTab,
    /// Ctrl+N: a second browser window.
    NewWindow,
    /// Ctrl+Shift+A: list of open tabs to jump to.
    TabSearch,
    CloseTab,
    ReopenClosed,
    NextTab,
    PrevTab,
    /// 0-based index; `usize::MAX` = last tab (Ctrl+9).
    SelectTab(usize),
    FocusOmnibox,
    History,
    Bookmark,
    Bookmarks,
    Downloads,
    Fullscreen,
    SavePage,
    DevTools,
    TaskManager,
    Reload,
    Back,
    Forward,
    Home,
    /// Ctrl+O: open a file from this PC (PDF, page, image, media) in a tab.
    OpenFile,
    MenuCommand(&'static str),
    ReloadBypass,
}

/// Chrome's shortcuts on Windows. `vk` is a Win32 virtual-key code.
pub fn shortcut(vk: u32, ctrl: bool, shift: bool, alt: bool) -> Option<Action> {
    use Action::*;
    const TAB: u32 = 0x09;
    const PGUP: u32 = 0x21;
    const PGDN: u32 = 0x22;
    const HOME: u32 = 0x24;
    const LEFT: u32 = 0x25;
    const RIGHT: u32 = 0x27;
    const F4: u32 = 0x73;
    const F5: u32 = 0x74;
    const F6: u32 = 0x75;
    const F11: u32 = 0x7A;
    const F12: u32 = 0x7B;
    const ESC: u32 = 0x1B;
    let ch = char::from_u32(vk).filter(|c| c.is_ascii_alphanumeric());
    match (ctrl, shift, alt, vk, ch) {
        (true, false, false, _, Some('F')) => Some(MenuCommand("find")),
        (true, false, false, _, Some('P')) => Some(MenuCommand("print")),
        (true, true, false, _, Some('W')) => Some(MenuCommand("exit")),
        (true, true, false, 0x2e, _) => Some(MenuCommand("clear")),
        (true, _, false, 0xbb | 0x6b, _) => Some(MenuCommand("zoomin")),
        (true, _, false, 0xbd | 0x6d, _) => Some(MenuCommand("zoomout")),
        (true, false, false, _, Some('0')) => Some(MenuCommand("zoomreset")),
        (true, true, false, _, Some('R')) | (true, false, false, F5, _) | (false, true, false, F5, _) => Some(ReloadBypass),
        (true, false, false, _, Some('T')) => Some(NewTab),
        (true, true, false, _, Some('N')) => Some(NewPrivateTab),
        (true, false, false, _, Some('N')) => Some(NewWindow),
        (true, true, false, _, Some('A')) => Some(TabSearch),
        (true, true, false, _, Some('T')) => Some(ReopenClosed),
        (true, false, false, _, Some('W')) | (true, false, false, F4, _) => Some(CloseTab),
        (true, false, false, _, Some('L')) | (false, false, true, _, Some('D')) | (false, false, false, F6, _) => {
            Some(FocusOmnibox)
        }
        (true, false, false, _, Some('H')) => Some(History),
        (true, false, false, _, Some('D')) => Some(Bookmark),
        (true, true, false, _, Some('O')) => Some(Bookmarks),
        (true, false, false, _, Some('J')) => Some(Downloads),
        (false, false, false, F11, _) => Some(Fullscreen),
        (true, false, false, _, Some('S')) => Some(SavePage),
        (true, false, false, _, Some('O')) => Some(OpenFile),
        (false, false, false, F12, _) | (true, true, false, _, Some('I')) => Some(DevTools),
        (false, true, false, ESC, _) => Some(TaskManager),
        (true, false, false, TAB | PGDN, _) => Some(NextTab),
        (true, true, false, TAB, _) | (true, false, false, PGUP, _) => Some(PrevTab),
        (true, false, false, _, Some('9')) => Some(SelectTab(usize::MAX)),
        (true, false, false, _, Some(c @ '1'..='8')) => Some(SelectTab(c as usize - '1' as usize)),
        (true, false, false, _, Some('R')) | (false, false, false, F5, _) => Some(Reload),
        (false, false, true, LEFT, _) => Some(Back),
        (false, false, true, RIGHT, _) => Some(Forward),
        (false, false, true, HOME, _) => Some(Home),
        _ => None,
    }
}

/// One tab as the UI webview draws it.
#[derive(Debug, Clone, Default)]
pub struct TabView {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub favicon: String,
    pub loading: bool,
    pub audio: bool,
    /// The page holds a live camera, microphone or screen-capture track (Memory Saver protection).
    pub media: bool,
    pub discarded: bool,
    /// Incognito tab: nothing it does is stored.
    pub private: bool,
    /// Pinned tabs sit at the start of the strip, icon only, and are never frozen.
    pub pinned: bool,
    pub muted: bool,
    /// Tab group id (0 = none), see [`Group`].
    pub group: u32,
}

/// A named, coloured run of tabs in the strip.
#[derive(Debug, Clone)]
pub struct Group {
    pub id: u32,
    pub name: String,
    pub color: &'static str,
    /// Collapsed groups show only their label (and the active tab, if it is in the group).
    pub collapsed: bool,
}

/// Group colours, in the order new groups take them.
pub const GROUP_COLORS: [&str; 8] = ["#8ab4f8", "#f28b82", "#fdd663", "#81c995", "#ff8bcb", "#c58af9", "#78d9ec", "#fcad70"];

/// The logo as a `data:` URI (percent-encoded SVG), usable where the `kakangkung` protocol is not registered.
pub fn logo_data_uri() -> String {
    let mut out = String::from("data:image/svg+xml,");
    for b in LOGO_SVG.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b' ' | b'=' | b':' | b'/' | b'.' | b'-' | b'(' | b')' | b',' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The brand mark drawn procedurally as RGBA (window and taskbar icon): violet-to-indigo disc with
/// a white "K" monogram, matching assets/logo.svg (64-unit grid). 4x4 supersampled.
pub fn brand_icon_rgba(size: u32) -> Vec<u8> {
    let n = size as f32;
    // Strokes of the K in logo.svg units, 8 units wide with round caps.
    let strokes = [((23.0f32, 17.0f32), (23.0f32, 47.0f32)), ((25.0, 33.0), (43.0, 17.0)), ((30.0, 29.0), (44.0, 47.0))];
    let near = |x: f32, y: f32| {
        strokes.iter().any(|&((ax, ay), (bx, by))| {
            let (vx, vy) = (bx - ax, by - ay);
            let t = (((x - ax) * vx + (y - ay) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
            let (dx, dy) = (x - ax - t * vx, y - ay - t * vy);
            dx * dx + dy * dy <= 16.0
        })
    };
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let mut acc = [0f32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = (px as f32 + (sx as f32 + 0.5) / 4.0) / n;
                    let y = (py as f32 + (sy as f32 + 0.5) / 4.0) / n;
                    let (dx, dy) = (x - 0.5, y - 0.5);
                    if dx * dx + dy * dy > 0.47 * 0.47 {
                        continue;
                    }
                    let rgb = if near(x * 64.0, y * 64.0) {
                        [255.0, 255.0, 255.0]
                    } else {
                        // #9333ea -> #4f46e5 along the diagonal.
                        let t = ((x + y) / 2.0).clamp(0.0, 1.0);
                        [147.0 + (79.0 - 147.0) * t, 51.0 + (70.0 - 51.0) * t, 234.0 + (229.0 - 234.0) * t]
                    };
                    acc[0] += rgb[0];
                    acc[1] += rgb[1];
                    acc[2] += rgb[2];
                    acc[3] += 1.0;
                }
            }
            if acc[3] == 0.0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let a = acc[3];
                out.extend_from_slice(&[(acc[0] / a) as u8, (acc[1] / a) as u8, (acc[2] / a) as u8, (a / 16.0 * 255.0) as u8]);
            }
        }
    }
    out
}

/// Escape for a JSON string literal. Also escapes `<`, `>` and U+2028/2029 so the result is
/// safe inside a `<script>`/JS source passed to `evaluate_script`.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(t: &str) -> String {
        resolve_input(t, DEFAULT_SEARCH).unwrap_or_default()
    }

    #[test]
    fn address_bar_input() {
        assert_eq!(r("example.com"), "https://example.com");
        assert_eq!(r("en.wikipedia.org/wiki/Rust"), "https://en.wikipedia.org/wiki/Rust");
        assert_eq!(r("http://neverssl.com"), "http://neverssl.com");
        assert_eq!(r("localhost:8080"), "http://localhost:8080");
        assert_eq!(r("192.168.1.1"), "http://192.168.1.1");
        assert_eq!(r("10.0.0.2:8080/admin"), "http://10.0.0.2:8080/admin");
        assert_eq!(r("1.2.3.com"), "https://1.2.3.com");
        assert_eq!(r("rust lang"), "https://www.google.com/search?q=rust+lang&ie=UTF-8");
        assert_eq!(r("rust"), "https://www.google.com/search?q=rust&ie=UTF-8");
        assert_eq!(r("c++ & go"), "https://www.google.com/search?q=c%2B%2B+%26+go&ie=UTF-8");
        assert_eq!(r("  "), "");
        assert_eq!(r("kakangkung://history"), "https://kakangkung.internal/history");
        assert_eq!(r("kakangkung://newtab"), NEWTAB_URL);
    }

    #[test]
    fn script_urls_are_never_navigated() {
        for bad in ["javascript:alert(1)", "JavaScript:alert(1)", " vbscript:x", "data:text/html,<script>"] {
            assert!(r(bad).starts_with("https://www.google.com/search?q="), "{bad}");
        }
    }

    #[test]
    fn host_of_urls() {
        assert_eq!(host("https://mail.google.com/mail/u/0/"), "mail.google.com");
        assert_eq!(host("http://example.com?q=1"), "example.com");
        assert_eq!(host("about:blank"), "");
    }

    #[test]
    fn security_and_display() {
        assert_eq!(Security::of("https://example.com/"), Security::Secure);
        assert_eq!(Security::of("http://example.com/"), Security::Insecure);
        assert_eq!(Security::of("http://localhost:3000/"), Security::Local);
        assert_eq!(Security::of("http://[::1]:3000/"), Security::Local);
        assert_eq!(Security::of("http://localhost.example.com/"), Security::Insecure);
        assert_eq!(Security::of("http://127.0.0.1.example.com/"), Security::Insecure);
        assert_eq!(Security::of(NEWTAB_URL), Security::Internal);
        assert_eq!(display_url("https://example.com/"), "example.com");
        assert_eq!(display_url("https://example.com/a?b"), "example.com/a?b");
        assert_eq!(display_url(NEWTAB_URL), "");
        assert_eq!(display_url("https://kakangkung.internal/history"), "kakangkung://history");
    }

    #[test]
    fn search_engines() {
        assert_eq!(search_template("").1, "Google");
        assert_eq!(search_template("bing").0, "https://www.bing.com/search?q=%s");
        assert_eq!(search_template("https://kagi.com/search?q=%s").1, "kagi.com");
        assert_eq!(search_template("javascript:%s").1, "Google", "unsafe custom engines are refused");
        assert_eq!(search_template("http://plain.example/?q=%s").1, "Google", "https only");
        assert_eq!(resolve_input("rust", ENGINES[2].url).unwrap(), "https://duckduckgo.com/?q=rust");
        assert_eq!(resolve_input("\"C:\\Docs\\Q3 report.pdf\"", DEFAULT_SEARCH).unwrap(), "file:///C:/Docs/Q3%20report.pdf");
        assert_eq!(resolve_input("\\\\nas\\share\\a.pdf", DEFAULT_SEARCH).unwrap(), "file://nas/share/a.pdf");
        assert_eq!(page_name("https://cdn.example/files/Q3%20report.pdf?dl=1"), "Q3 report.pdf");
        assert_eq!(page_name("file:///C:/Music/song.mp3"), "song.mp3");
        assert_eq!(page_name("https://example.com/docs/"), "example.com");
        assert_eq!(page_name("https://example.com"), "example.com");
        assert!(viewable_file("C:\\Downloads\\Report.PDF") && viewable_file("a.webm") && !viewable_file("setup.exe"));
        assert_eq!(shortcut(u32::from(b'O'), true, false, false), Some(Action::OpenFile));
        assert_eq!(resolve_input("@bing  rust lang", DEFAULT_SEARCH).unwrap(), "https://www.bing.com/search?q=rust+lang");
        assert_eq!(resolve_input("@YT cats", DEFAULT_SEARCH).unwrap(), "https://www.youtube.com/results?search_query=cats");
        assert_eq!(resolve_input("@wiki Jakarta", DEFAULT_SEARCH).unwrap(), "https://en.wikipedia.org/w/index.php?search=Jakarta");
        assert_eq!(resolve_input("@nope x", DEFAULT_SEARCH).unwrap(), "https://www.google.com/search?q=%40nope+x&ie=UTF-8", "unknown keyword = a search");
        assert_eq!(resolve_input("@bing", DEFAULT_SEARCH).unwrap(), "https://www.google.com/search?q=%40bing&ie=UTF-8");
        assert_eq!(suggest_url("", "a b").unwrap(), "https://www.google.com/complete/search?client=chrome&q=a+b");
        assert_eq!(suggest_url("https://kagi.com/search?q=%s", "x"), None);
        assert_eq!(parse_suggestions(br#"["rust",["rust lang","rust game"]]"#, 8), ["rust lang", "rust game"]);
        assert_eq!(parse_suggestions(b")]}'
[\"r\",[\"rust\"],[\"\"],[]]", 8), ["rust"]);
        assert_eq!(parse_suggestions(br#"["r",[{"q":"rust","is_entity":false}]]"#, 8), ["rust"]);
        assert!(parse_suggestions(b"<html>", 8).is_empty());
    }

    #[test]
    fn chrome_shortcuts() {
        use Action::*;
        let k = |vk: u8, c, s, a| shortcut(vk as u32, c, s, a);
        assert_eq!(k(b'T', true, false, false), Some(NewTab));
        assert_eq!(k(b'T', true, true, false), Some(ReopenClosed));
        assert_eq!(k(b'N', true, true, false), Some(NewPrivateTab));
        assert_eq!(k(b'N', true, false, false), Some(NewWindow));
        assert_eq!(k(b'A', true, true, false), Some(TabSearch));
        assert_eq!(k(b'W', true, false, false), Some(CloseTab));
        assert_eq!(k(b'L', true, false, false), Some(FocusOmnibox));
        assert_eq!(k(0x09, true, false, false), Some(NextTab));
        assert_eq!(k(0x09, true, true, false), Some(PrevTab));
        assert_eq!(k(b'R', true, false, false), Some(Reload));
        assert_eq!(k(0x74, false, false, false), Some(Reload));
        assert_eq!(k(0x25, false, false, true), Some(Back));
        assert_eq!(k(0x27, false, false, true), Some(Forward));
        assert_eq!(k(b'1', true, false, false), Some(SelectTab(0)));
        assert_eq!(k(b'H', true, false, false), Some(History));
        assert_eq!(k(b'D', true, false, false), Some(Bookmark));
        assert_eq!(k(b'O', true, true, false), Some(Bookmarks));
        assert_eq!(k(b'J', true, false, false), Some(Downloads));
        assert_eq!(k(0x7A, false, false, false), Some(Fullscreen));
        assert_eq!(k(b'S', true, false, false), Some(SavePage));
        assert_eq!(k(0x7B, false, false, false), Some(DevTools));
        assert_eq!(k(b'I', true, true, false), Some(DevTools));
        assert_eq!(k(0x1B, false, true, false), Some(TaskManager));
        assert_eq!(k(0x1B, false, false, false), None, "plain Esc stays with the page");
        // Browser commands also work while a native edit control has focus.
        assert_eq!(k(0xBB, true, false, false), Some(MenuCommand("zoomin")));
        assert_eq!(k(b'F', true, false, false), Some(MenuCommand("find")));
        assert_eq!(k(b'P', true, false, false), Some(MenuCommand("print")));
        assert_eq!(k(b'9', true, false, false), Some(SelectTab(usize::MAX)));
        // Plain text and editing shortcuts stay with the focused control.
        assert_eq!(k(b'T', false, false, false), None);
        assert_eq!(k(b'C', true, false, false), None);
        assert_eq!(k(b'V', true, false, false), None);
        assert_eq!(k(b'W', true, true, false), Some(MenuCommand("exit")));
        assert_eq!(k(b'F', true, false, false), Some(MenuCommand("find")));
        assert_eq!(k(b'P', true, false, false), Some(MenuCommand("print")));
        assert_eq!(k(0xbb, true, true, false), Some(MenuCommand("zoomin")));
        assert_eq!(k(0xbd, true, false, false), Some(MenuCommand("zoomout")));
        assert_eq!(k(b'0', true, false, false), Some(MenuCommand("zoomreset")));
        assert_eq!(k(b'R', true, true, false), Some(ReloadBypass));
        assert_eq!(k(b'A', true, false, false), None);
    }

    #[test]
    fn brand_icon_has_disc_and_k() {
        let px = brand_icon_rgba(64);
        assert_eq!(px.len(), 64 * 64 * 4);
        let at = |x: usize, y: usize| &px[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(at(0, 0)[3], 0, "corner is transparent");
        assert_eq!(at(23, 32), &[255, 255, 255, 255], "stem of the K");
        assert_eq!(at(42, 44), &[255, 255, 255, 255], "lower leg of the K");
        assert!(at(12, 32)[2] > 200 && at(12, 32)[3] == 255, "violet disc");
        assert!(logo_data_uri().starts_with("data:image/svg+xml,%3Csvg"));
    }

    #[test]
    fn chrome_height_is_strip_plus_toolbar() {
        assert_eq!(CHROME_HEIGHT_DIP, 87.0);
    }

    #[test]
    fn json_is_script_safe() {
        assert_eq!(json_escape("a\"b\\c\n"), "a\\\"b\\\\c\\u000a");
        assert_eq!(json_escape("</script>"), "\\u003c/script\\u003e");
    }
}
