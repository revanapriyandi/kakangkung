//! Network-only EasyList/EasyPrivacy matching. One engine is shared by the UI thread's tabs.
//!
//! Lists are bundled for offline startup; source, hashes, dates and licenses live beside them.
//! Unsupported rules are ignored by adblock-rust. We do not execute scriptlets, redirect
//! resources, response-body filters or cosmetic rules through this network interface.

use adblock::{
    lists::{FilterSet, ParseOptions, RuleTypes},
    request::Request,
    Engine,
};

const EASYLIST: &str = include_str!("../../../assets/filterlists/easylist.txt");
const EASYPRIVACY: &str = include_str!("../../../assets/filterlists/easyprivacy.txt");

thread_local! {
    static ENGINE: Engine = compile([EASYLIST, EASYPRIVACY]);
}

fn compile<'a>(lists: impl IntoIterator<Item = &'a str>) -> Engine {
    let mut filters = FilterSet::new(false);
    for list in lists {
        filters.add_filter_list(
            list.to_owned(),
            ParseOptions {
                rule_types: RuleTypes::NetworkOnly,
                ..ParseOptions::default()
            },
        );
    }
    Engine::new_with_filter_set(filters)
}

/// Optional eager initialization. Call once on the UI thread, never once per tab.
pub(crate) fn warm_up() {
    ENGINE.with(|_| {});
}

/// `kind` uses adblock-rust's resource names (`script`, `xmlhttprequest`, `image`,
/// `stylesheet`, `font`, `media`, `subdocument`, `ping`, `other`). The caller must
/// identify top-level navigation as `document`; it is always allowed.
#[cfg(test)]
pub(crate) fn blocks(page: &str, url: &str, kind: &str) -> bool {
    blocks_with_method(page, url, kind, "")
}

/// Prefer this when the WebView request exposes its HTTP method.
pub(crate) fn blocks_with_method(page: &str, url: &str, kind: &str, method: &str) -> bool {
    let Some(mut request) = request(page, url, kind) else {
        return false;
    };
    request.method = method.parse().ok();
    ENGINE.with(|engine| engine.check_network_request(&request).should_block())
}

fn request(page: &str, url: &str, kind: &str) -> Option<Request> {
    if matches!(kind, "document" | "main_frame") || !web_url(page) || !web_url(url) {
        return None;
    }
    Request::new(url, page, kind, "")
        .ok()
        .filter(|request| request.source_hostname_hashes.is_some())
}

fn web_url(url: &str) -> bool {
    url.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(engine: &Engine, page: &str, url: &str, kind: &str) -> bool {
        request(page, url, kind)
            .is_some_and(|request| engine.check_network_request(&request).should_block())
    }

    #[test]
    fn first_party_paths_types_and_exceptions() {
        let engine = compile([
            "||example.test/ads/$script\n@@||example.test/ads/consent.js$script\n||tracker.test^$third-party",
        ]);
        let page = "https://example.test/article";
        assert!(matches(
            &engine,
            page,
            "https://example.test/ads/banner.js",
            "script"
        ));
        assert!(!matches(
            &engine,
            page,
            "https://example.test/ads/consent.js",
            "script"
        ));
        assert!(!matches(
            &engine,
            page,
            "https://example.test/ads/banner.js",
            "image"
        ));
        assert!(!matches(
            &engine,
            page,
            "https://example.test/content.js",
            "script"
        ));
        assert!(matches(
            &engine,
            page,
            "https://tracker.test/pixel",
            "image"
        ));
        assert!(!matches(
            &engine,
            "https://tracker.test/",
            "https://cdn.tracker.test/pixel",
            "image"
        ));
        assert!(matches(
            &engine,
            page,
            "https://tracker.test/frame",
            "subdocument"
        ));
        assert!(!matches(&engine, page, "https://tracker.test/", "document"));
    }

    #[test]
    fn invalid_and_non_web_urls_are_allowed() {
        let engine = compile(["||tracker.test^"]);
        for url in [
            "not a url",
            "https://",
            "data:text/plain,ads",
            "file:///ads.js",
            "about:blank",
        ] {
            assert!(
                !matches(&engine, "https://example.test/", url, "script"),
                "{url}"
            );
        }
        assert!(!matches(
            &engine,
            "about:blank",
            "https://tracker.test/ad.js",
            "script"
        ));
        assert!(!matches(&engine, "https://", "https://tracker.test/ad.js", "script"));
    }

    #[test]
    fn domain_constraints_and_badfilter_are_respected() {
        let engine = compile([
            "||ads.test/banner$script,domain=news.test|~safe.news.test\n||disabled.test^\n||disabled.test^$badfilter",
        ]);
        assert!(matches(
            &engine,
            "https://news.test/",
            "https://ads.test/banner",
            "script"
        ));
        assert!(!matches(
            &engine,
            "https://safe.news.test/",
            "https://ads.test/banner",
            "script"
        ));
        assert!(!matches(
            &engine,
            "https://elsewhere.test/",
            "https://ads.test/banner",
            "script"
        ));
        assert!(!matches(
            &engine,
            "https://news.test/",
            "https://disabled.test/ad.js",
            "script"
        ));
    }

    #[test]
    fn bundled_lists_block_ads_and_allow_content() {
        let started = std::time::Instant::now();
        warm_up();
        eprintln!("bundled network engine initialization: {:?}", started.elapsed());
        assert!(blocks(
            "https://example.org/",
            "https://googleads.g.doubleclick.net/pagead/ads",
            "script"
        ));
        assert!(blocks(
            "https://example.org/",
            "https://www.google-analytics.com/analytics.js",
            "script"
        ));
        assert!(!blocks(
            "https://example.org/",
            "https://example.org/article/main.js",
            "script"
        ));
        assert!(!blocks(
            "https://www.youtube.com/",
            "https://rr1---sn.example.googlevideo.com/videoplayback?id=content",
            "media"
        ));
        assert!(!blocks(
            "https://example.org/",
            "https://googleads.g.doubleclick.net/",
            "document"
        ));
    }
}
