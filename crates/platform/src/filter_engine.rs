//! EasyList/EasyPrivacy matching. One engine is shared by the UI thread's tabs.
//!
//! Lists are bundled for offline startup; source, hashes, dates and licenses live beside them.
//! Unsupported rules are ignored by adblock-rust. Network and element-hiding rules are compiled;
//! `$popup` rules go into a second small engine (adblock-rust has no popup type) and are matched
//! as `document` requests. We do not execute scriptlets, redirect resources or response filters.

use adblock::{cosmetic_filter_cache::UrlSpecificResources, lists::{FilterSet, ParseOptions, RuleTypes}, request::Request, Engine};
use std::cell::RefCell;
use std::collections::HashSet;

const EASYLIST: &str = include_str!("../../../assets/filterlists/easylist.txt");
const EASYPRIVACY: &str = include_str!("../../../assets/filterlists/easyprivacy.txt");
const SITE_FIXES: &str = include_str!("../../../assets/filterlists/kakangkung.txt");

struct Engines { main: Engine, popup: Engine }

thread_local! {
    static ENGINE: RefCell<Option<Engines>> = const { RefCell::new(None) };
}

fn with_engines<T>(read: impl FnOnce(&Engines) -> T) -> T {
    ENGINE.with(|slot| {
        let mut engine = slot.borrow_mut();
        if engine.is_none() {
            let lists = [EASYLIST, EASYPRIVACY, SITE_FIXES];
            *engine = Some(Engines { main: compile(lists), popup: compile_popups(lists) });
            crate::processes::defer_heap_cleanup();
        }
        read(engine.as_ref().unwrap())
    })
}

fn with_engine<T>(read: impl FnOnce(&Engine) -> T) -> T {
    with_engines(|engines| read(&engines.main))
}

pub(crate) fn release_unused() {
    ENGINE.with(|slot| { slot.borrow_mut().take(); });
}

pub(crate) fn loaded() -> bool {
    ENGINE.with(|slot| slot.borrow().is_some())
}

fn compile<'a>(lists: impl IntoIterator<Item = &'a str>) -> Engine {
    let mut filters = FilterSet::new(false);
    for list in lists {
        filters.add_filter_list(
            list.to_owned(),
            ParseOptions {
                rule_types: RuleTypes::All,
                ..ParseOptions::default()
            },
        );
    }
    Engine::new_with_filter_set(filters)
}

/// `$popup` rules rewritten as `$document` rules, so a new window's URL can be matched as a
/// top-level document. Only this engine sees them; the main engine drops `popup` as unknown.
fn compile_popups<'a>(lists: impl IntoIterator<Item = &'a str>) -> Engine {
    let mut rules = String::new();
    for list in lists {
        for line in list.lines().map(str::trim) {
            let Some((pattern, options)) = line.rsplit_once('$') else { continue };
            if line.starts_with('!') || line.contains('#') || !options.split(',').any(|o| o == "popup") { continue; }
            let options: Vec<&str> = options.split(',').map(|o| if o == "popup" { "document" } else { o }).collect();
            rules.push_str(pattern);
            rules.push('$');
            rules.push_str(&options.join(","));
            rules.push('\n');
        }
    }
    let mut filters = FilterSet::new(false);
    filters.add_filter_list(rules, ParseOptions { rule_types: RuleTypes::NetworkOnly, ..ParseOptions::default() });
    Engine::new_with_filter_set(filters)
}

/// Optional eager initialization. Call once on the UI thread, never once per tab.
pub(crate) fn warm_up() {
    with_engine(|_| {});
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
    with_engine(|engine| engine.check_network_request(&request).should_block())
}

/// A new window or script-driven top-level navigation from `page` to `url` matches a `$popup`
/// rule. Plain network rules are not applied: those would also catch ordinary link targets.
pub(crate) fn blocks_popup(page: &str, url: &str) -> bool {
    if !web_url(page) || !web_url(url) { return false; }
    let Ok(request) = Request::new(url, page, "document", "") else { return false };
    with_engines(|engines| engines.popup.check_network_request(&request).should_block())
}

/// Element-hiding CSS for the top-level document at `url`: site-specific and generic selectors,
/// minus exceptions. Generic class/id rules need the page's names (see `generic_selectors`).
pub(crate) fn cosmetic(url: &str) -> Option<UrlSpecificResources> {
    web_url(url).then(|| with_engine(|engine| engine.url_cosmetic_resources(url)))
}

/// Generic `##.class` / `###id` selectors that match names present in the page.
pub(crate) fn generic_selectors(classes: &[String], ids: &[String], exceptions: &HashSet<String>) -> Vec<String> {
    with_engine(|engine| engine.hidden_class_id_selectors(classes, ids, exceptions))
}

/// One `display:none` rule per selector: the browser drops a whole rule when any selector in
/// it is unsupported, so grouping would let one exotic selector void its neighbours.
pub(crate) fn hide_css<'a>(selectors: impl IntoIterator<Item = &'a String>) -> String {
    selectors.into_iter().filter(|s| !s.contains(['{', '}'])).map(|selector| format!("{selector}{{display:none!important}}\n")).collect()
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

    #[test]
    fn otakudesu_fixes_block_only_confirmed_banner_assets() {
        let asset = SITE_FIXES.lines().find(|line| line.starts_with("||blogger.googleusercontent.com/")).expect("observed banner rule");
        let image = format!("https://{}", asset.trim_start_matches("||").split('$').next().unwrap());
        assert!(blocks("https://otakudesu.blog/anime/kanata-kara-sub-indo/", &image, "image"));
        assert!(!blocks("https://example.org/", &image, "image"));
        assert!(!blocks("https://otakudesu.blog/", "https://blogger.googleusercontent.com/img/b/legitimate-anime-poster.jpg", "image"));
        assert!(!blocks("https://otakudesu.blog/", "https://desustream.com/embed/video", "subdocument"));
        assert!(blocks("https://otakudesu.blog/", "https://ads.desustream.com/ads/pp/ads.js", "script"));
    }

    #[test]
    fn popup_rules_match_only_new_windows_and_keep_exceptions() {
        let engine = compile_popups(["||popads.test^$popup,third-party\n||banner.test/x.js$script\n@@||popads.test/login$popup\nexample.test##.ad$popup"]);
        let popup = |page: &str, url: &str| Request::new(url, page, "document", "")
            .is_ok_and(|request| engine.check_network_request(&request).should_block());
        assert!(popup("https://news.test/", "https://popads.test/go?id=1"));
        assert!(!popup("https://news.test/", "https://popads.test/login"), "exception");
        assert!(!popup("https://popads.test/", "https://popads.test/go"), "first party");
        assert!(!popup("https://news.test/", "https://banner.test/x.js"), "non-popup rules stay out");
        // Bundled lists: a known pop-under network, and an ordinary destination.
        assert!(blocks_popup("https://example.org/", "https://www.popads.net/"));
        assert!(!blocks_popup("https://example.org/", "https://www.wikipedia.org/"));
        assert!(!blocks_popup("about:blank", "https://www.popads.net/"));
    }

    #[test]
    fn element_hiding_is_site_specific_generic_and_excepted() {
        let resources = cosmetic("https://otakudesu.blog/anime/x/").expect("web page");
        for selector in ["[class^=\"iklan\"]", "[id^=\"iklan\"]", "a[href^=\"https://bit.ly/OTAKU\"]", ".bloxkln"] {
            assert!(resources.hide_selectors.contains(selector), "{selector}");
        }
        assert!(!cosmetic("https://example.org/").unwrap().hide_selectors.iter().any(|s| s.contains("iklan")));
        assert!(cosmetic("https://www.detik.com/").unwrap().hide_selectors.contains(".header__leaderboard"));
        assert!(cosmetic("about:blank").is_none());
        let names = |list: &[&str]| list.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        let generic = generic_selectors(&names(&["content", "ad-slot"]), &names(&["ad_top", "main"]), &resources.exceptions);
        assert!(generic.iter().any(|s| s == "#ad_top"), "{generic:?}");
        assert!(!generic.iter().any(|s| s.contains("content") || s.contains("main")));
        let css = hide_css(&["#a".to_string(), "x{}y".to_string(), ".b".to_string()]);
        assert_eq!(css, "#a{display:none!important}\n.b{display:none!important}\n");
    }

    #[test]
    fn ad_iframes_are_blocked_as_subdocuments() {
        assert!(blocks("https://news.example.com/", "https://googleads.g.doubleclick.net/pagead/ads?client=ca-pub-1", "subdocument"));
        assert!(!blocks("https://news.example.com/", "https://www.youtube.com/embed/abc", "subdocument"));
    }

    #[test]
    fn reloading_after_last_view_closes_preserves_filtering() {
        release_unused();
        assert!(!loaded());
        for _ in 0..3 {
            assert!(blocks("https://example.org/", "https://googleads.g.doubleclick.net/pagead/ads", "script"));
            assert!(!blocks("https://example.org/", "https://example.org/content.js", "script"));
            assert!(loaded());
            release_unused();
            assert!(!loaded());
        }
    }

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
