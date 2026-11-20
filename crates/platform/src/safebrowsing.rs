//! Ladybird Safe Browsing foundation (B6).
//!
//! WebView2 used Microsoft Defender SmartScreen. Ladybird has no SmartScreen hook, so we check
//! navigations against a bundled dangerous-host list. This is a real gate (blocks matching hosts),
//! not a fake UI — expanding to Google Safe Browsing API / list updates is the next step.

use std::collections::HashSet;
use std::sync::OnceLock;

static HOSTS: OnceLock<HashSet<String>> = OnceLock::new();

fn host_set() -> &'static HashSet<String> {
    HOSTS.get_or_init(|| {
        include_str!("../../../assets/safebrowsing/dangerous-hosts.txt")
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.to_ascii_lowercase())
            .collect()
    })
}

/// Extract a lowercase hostname from a URL-ish string.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url);
    let hostport = rest.split('/').next().unwrap_or("").split('@').next_back().unwrap_or("");
    let host = hostport.split(':').next().unwrap_or("").trim();
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// True when Safe Browsing should block this navigation.
pub fn blocks_url(url: &str) -> bool {
    let Some(host) = host_of(url) else {
        return false;
    };
    let set = host_set();
    if set.contains(&host) {
        return true;
    }
    // Match parent domains: evil.testsafebrowsing.appspot.com → testsafebrowsing.appspot.com
    for h in set {
        if host.ends_with(h) && host.len() > h.len() {
            let boundary = host.as_bytes().get(host.len() - h.len() - 1).copied();
            if boundary == Some(b'.') {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_google_test_malware_host() {
        assert!(blocks_url("https://testsafebrowsing.appspot.com/s/malware.html"));
        assert!(blocks_url("http://malware.testing.google.test/"));
    }

    #[test]
    fn allows_ordinary_hosts() {
        assert!(!blocks_url("https://example.com/"));
        assert!(!blocks_url("https://youtube.com/watch?v=1"));
    }
}
