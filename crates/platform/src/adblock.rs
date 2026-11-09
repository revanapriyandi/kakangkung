//! Shared native EasyList/EasyPrivacy request filtering with exact-host exceptions.
//! Document requests stay navigable; filtering does not inspect response bodies.

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{take_pwstr, WebResourceRequestedEventHandler};
use windows::core::{Interface, HSTRING, PWSTR};
use wry::{WebView, WebViewExtWindows};

const LIST: &str = include_str!("adblock-domains.txt");
const MAX_COUNTED_SITES: usize = 200;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static BLOCKED: Cell<u64> = const { Cell::new(0) };
    static DOMAINS: HashSet<&'static str> = domains().collect();
    static ALLOWED_SITES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static BLOCKED_BY_SITE: RefCell<VecDeque<(String, u64)>> = const { RefCell::new(VecDeque::new()) };
}

fn domains() -> impl Iterator<Item = &'static str> {
    LIST.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'))
}

pub(crate) fn set_enabled(on: bool) {
    let changed = ON.with(|c| c.replace(on) != on);
    if changed { refresh_scripts(); }
}

fn refresh_scripts() {
    let sites = ALLOWED_SITES.with(|sites| sites.borrow().iter().cloned().collect::<Vec<_>>().join("\n"));
    crate::adblock_script::update_adblock_scripts(ON.with(Cell::get), &sites);
}

/// Requests blocked since start-up (all tabs).
pub fn ads_blocked() -> u64 {
    BLOCKED.with(Cell::get)
}

/// Replace exact-host exceptions from a newline-separated preference (at most 100 hosts).
pub fn configure_adblock_sites(sites: &str) {
    let changed = ALLOWED_SITES.with(|allowed| {
        let next = sites.lines().map(str::trim)
            .filter(|site| !site.is_empty() && site.len() <= 253
                && site.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-')))
            .map(|site| site.trim_end_matches('.').to_ascii_lowercase())
            .filter(|site| !site.is_empty())
            .take(100).collect();
        let changed = *allowed.borrow() != next;
        *allowed.borrow_mut() = next;
        changed
    });
    if changed { refresh_scripts(); }
}

/// Whether this exact host is exempt, independently of the global switch.
pub fn adblock_site_allowed(url: &str) -> bool {
    let site = host(url);
    ALLOWED_SITES.with(|allowed| allowed.borrow().contains(&site))
}

/// Canonical hostname used by the exception preference; ports and paths are not part of it.
pub fn adblock_site_key(url: &str) -> String {
    host(url)
}

/// Whether blocking is enabled globally and for this exact host.
pub fn adblock_site_enabled(url: &str) -> bool {
    ON.with(Cell::get) && !adblock_site_allowed(url)
}

/// Count attributed to the top-level host, across its tabs and paths. Counts are retained for
/// the 200 most recently blocked hosts; an evicted host starts at zero when encountered again.
pub fn blocked_for_site(url: &str) -> u64 {
    let site = host(url);
    BLOCKED_BY_SITE.with(|counts| counts.borrow().iter()
        .find(|(key, _)| key == &site).map_or(0, |(_, count)| *count))
}

fn record_block(page: &str) {
    BLOCKED.with(|b| b.set(b.get().saturating_add(1)));
    let site = host(page);
    if site.is_empty() { return; }
    BLOCKED_BY_SITE.with(|counts| {
        let mut counts = counts.borrow_mut();
        let count = counts.iter().position(|(key, _)| key == &site)
            .and_then(|index| counts.remove(index)).map_or(1, |(_, count)| count.saturating_add(1));
        if counts.len() == MAX_COUNTED_SITES { counts.pop_front(); }
        counts.push_back((site, count));
    });
}

/// Host of `url` in lower case ("" when it has none).
fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or("", |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    host.split(':').next().unwrap_or("").trim_end_matches('.').to_ascii_lowercase()
}

/// The list entry `host` falls under (itself or a parent domain).
fn listed(host: &str) -> Option<&'static str> {
    DOMAINS.with(|d| {
        let mut h = host;
        loop {
            if let Some(entry) = d.get(h) {
                return Some(*entry);
            }
            h = h.split_once('.')?.1;
        }
    })
}

/// Should a request for `url` made by the page at `page` be blocked? Not when the page itself is
/// on that ad network's domain.
fn should_block(page: &str, url: &str) -> bool {
    if adblock_site_allowed(page) { return false; }
    let Some(entry) = listed(&host(url)) else { return false };
    let page = host(page);
    !(page == entry || page.ends_with(&format!(".{entry}")))
}

fn resource_kind(context: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> &'static str {
    match context {
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => "document",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "xmlhttprequest",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => "websocket",
        _ => "other",
    }
}

/// Add one request filter per scheme, using worker/frame coverage where the runtime supports it.
pub(crate) fn watch(wv: &WebView) -> Option<i64> {
    crate::filter_engine::warm_up();
    let core = wv.webview();
    for pattern in ["https://*", "http://*"] {
        // SAFETY: live WebView2 interfaces and strings on the UI thread.
        let result = unsafe {
            match core.cast::<ICoreWebView2_22>() {
                Ok(core22) => core22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                    &HSTRING::from(pattern), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL),
                Err(_) => core.AddWebResourceRequestedFilter(&HSTRING::from(pattern), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL),
            }
        };
        if let Err(e) = result {
            eprintln!("ad blocker: request filter: {e}");
            return None;
        }
    }
    let handler = WebResourceRequestedEventHandler::create(Box::new(|sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
        if !ON.with(Cell::get) {
            return Ok(());
        }
        let (mut url, mut page) = (PWSTR::null(), PWSTR::null());
        // SAFETY: WebView2 allocates both strings with CoTaskMemAlloc; take_pwstr copies and frees.
        let (url, page) = unsafe {
            args.Request()?.Uri(&mut url)?;
            sender.Source(&mut page)?;
            (take_pwstr(url), take_pwstr(page))
        };
        if adblock_site_allowed(&page) { return Ok(()); }
        let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL;
        unsafe { args.ResourceContext(&mut context)?; }
        let kind = resource_kind(context);
        let mut method = PWSTR::null();
        unsafe { args.Request()?.Method(&mut method)?; }
        let method = take_pwstr(method);
        let web_page = page.starts_with("https://") || page.starts_with("http://");
        let blocked = if web_page {
            // EasyList exceptions must win over the old domain-only fallback.
            crate::filter_engine::blocks_with_method(&page, &url, kind, &method)
        } else {
            kind != "document" && should_block(&page, &url)
        };
        if !blocked {
            return Ok(());
        }
        // SAFETY: plain COM calls on the live sender and event args from the UI thread.
        unsafe {
            let env = sender.cast::<ICoreWebView2_2>()?.Environment()?;
            let response = env.CreateWebResourceResponse(None, 403, &HSTRING::from("Blocked by Kakangkung"), &HSTRING::new())?;
            args.SetResponse(&response)?;
        }
        record_block(&page);
        Ok(())
    }));
    let mut token = 0i64;
    // SAFETY: handler is a valid COM object; token is a writable local.
    match unsafe { core.add_WebResourceRequested(&handler, &mut token) } {
        Ok(()) => Some(token),
        Err(e) => {
            eprintln!("ad blocker: WebResourceRequested registration failed: {e}");
            None
        }
    }
}

pub(crate) fn unwatch(wv: &WebView, token: i64) {
    // SAFETY: token came from add_WebResourceRequested on this webview.
    if let Err(e) = unsafe { wv.webview().remove_WebResourceRequested(token) } {
        eprintln!("unhook: WebResourceRequested: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(host("https://User@Ads.Example.COM:8443/x?y#z"), "ads.example.com");
        assert_eq!(host("about:blank"), "");
        assert_eq!(adblock_site_key("https://News.Example.com:8443/path"), "news.example.com");
    }

    #[test]
    fn blocks_ad_networks_on_other_sites_only() {
        let page = "https://news.example.com/article";
        assert!(should_block(page, "https://securepubads.g.doubleclick.net/tag/js/gpt.js"));
        assert!(should_block(page, "https://doubleclick.net/x.gif"));
        assert!(should_block(page, "https://cdn.taboola.com/libtrc/loader.js"));
        assert!(!should_block(page, "https://news.example.com/app.js"), "the site itself");
        assert!(!should_block(page, "https://notdoubleclick.net/x"), "a look-alike name is not a subdomain");
        assert!(!should_block("https://www.taboola.com/", "https://cdn.taboola.com/x.js"), "first party");
        assert!(!should_block(page, "https://www.google.com/search?q=ads"), "only listed hosts, not all of google.com");
    }

    #[test]
    fn list_is_clean() {
        let all: Vec<&str> = domains().collect();
        assert!(all.len() > 50);
        let unique: HashSet<&str> = all.iter().copied().collect();
        assert_eq!(unique.len(), all.len(), "no duplicates");
        assert!(all.iter().all(|d| d.contains('.') && !d.contains(['/', '*', ' ']) && *d == d.to_ascii_lowercase()));
    }

    #[test]
    fn site_exceptions_are_exact_hosts_and_keep_global_switch() {
        configure_adblock_sites("NEWS.Example.com\n\nother.example.com\n");
        set_enabled(true);
        assert!(!adblock_site_enabled("https://news.example.com/article"));
        assert!(!adblock_site_enabled("https://news.example.com:8443/article"));
        assert!(adblock_site_enabled("https://sub.news.example.com/"));
        assert!(adblock_site_enabled("https://notnews.example.com/"));
        assert!(!should_block("https://news.example.com/", "https://doubleclick.net/ad"));
        assert!(should_block("https://sub.news.example.com/", "https://doubleclick.net/ad"));
        set_enabled(false);
        assert!(!adblock_site_enabled("https://sub.news.example.com/"));
        assert!(adblock_site_allowed("https://news.example.com/"));
        assert!(!adblock_site_allowed("https://sub.news.example.com/"));
        configure_adblock_sites("");
        assert!(!adblock_site_allowed("https://news.example.com/"));
    }

    #[test]
    fn counts_belong_to_top_level_host_and_stay_bounded() {
        BLOCKED_BY_SITE.with(|counts| counts.borrow_mut().clear());
        record_block("https://news.example.com/article");
        record_block("https://NEWS.example.com/other");
        record_block("https://other.example.com/");
        assert_eq!(blocked_for_site("https://news.example.com/new"), 2);
        assert_eq!(blocked_for_site("https://sub.news.example.com/"), 0);
        assert_eq!(blocked_for_site("about:blank"), 0);
        for i in 0..MAX_COUNTED_SITES + 10 {
            record_block(&format!("https://site{i}.example.com/"));
        }
        BLOCKED_BY_SITE.with(|counts| assert_eq!(counts.borrow().len(), MAX_COUNTED_SITES));
        assert_eq!(blocked_for_site("https://news.example.com/"), 0, "old hosts are evicted");
        let latest = format!("https://site{}.example.com/", MAX_COUNTED_SITES + 9);
        record_block(&latest);
        assert_eq!(blocked_for_site(&latest), 2);
    }
}
