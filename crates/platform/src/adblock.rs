//! Built-in ad blocker: requests to known ad networks (`adblock-domains.txt`) get an empty
//! 403 instead of going out. WebView2 only calls us for those addresses (one URL filter per
//! domain), so ordinary requests never wait for the UI thread. Switched on and off live
//! through [`crate::Protection::adblock`].
//!
//! An ad network's own site opened from another page is blocked too (WebView2 does not say
//! whether a document request is the tab or an iframe); switching the blocker off allows it.
//!
//! ponytail: a domain list, no EasyList rules or element hiding; ads a site serves from its own
//! domain (YouTube's) still show. Upgrade path: an EasyList parser feeding the same filter.

use std::cell::Cell;
use std::collections::HashSet;

use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_2, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL};
use webview2_com::{take_pwstr, WebResourceRequestedEventHandler};
use windows::core::{Interface, HSTRING, PWSTR};
use wry::{WebView, WebViewExtWindows};

const LIST: &str = include_str!("adblock-domains.txt");

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static BLOCKED: Cell<u64> = const { Cell::new(0) };
    static DOMAINS: HashSet<&'static str> = domains().collect();
}

fn domains() -> impl Iterator<Item = &'static str> {
    LIST.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'))
}

pub(crate) fn set_enabled(on: bool) {
    ON.with(|c| c.set(on));
}

/// Requests blocked since start-up (all tabs).
pub fn ads_blocked() -> u64 {
    BLOCKED.with(Cell::get)
}

/// Host of `url` in lower case ("" when it has none).
fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or("", |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    host.split(':').next().unwrap_or("").to_ascii_lowercase()
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
    let Some(entry) = listed(&host(url)) else { return false };
    let page = host(page);
    !(page == entry || page.ends_with(&format!(".{entry}")))
}

/// Add the ad-network filters and the handler to a tab webview; returns the registration token
/// for `unhook`.
pub(crate) fn watch(wv: &WebView) -> Option<i64> {
    let core = wv.webview();
    for d in domains() {
        for pattern in [format!("*://{d}/*"), format!("*://*.{d}/*")] {
            // SAFETY: plain COM call on the live webview; the pattern outlives the call.
            if let Err(e) = unsafe { core.AddWebResourceRequestedFilter(&HSTRING::from(pattern), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL) } {
                eprintln!("ad blocker: filter for {d}: {e}");
                return None;
            }
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
        if !should_block(&page, &url) {
            return Ok(());
        }
        // SAFETY: plain COM calls on the live sender and event args from the UI thread.
        unsafe {
            let env = sender.cast::<ICoreWebView2_2>()?.Environment()?;
            let response = env.CreateWebResourceResponse(None, 403, &HSTRING::from("Blocked by Kakangkung"), &HSTRING::new())?;
            args.SetResponse(&response)?;
        }
        BLOCKED.with(|b| b.set(b.get() + 1));
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
}
