//! Page engine for Kakangkung: **Ladybird only**.
//!
//! Kakangkung no longer embeds Microsoft Edge WebView2 / Chromium. Web pages are rendered by
//! Ladybird (LibWeb / LibWebView) through the UI-port host under `engines/ladybird`. The browser
//! chrome stays native (Win32 + Direct2D). Build the host on [Boat](https://boat.dev/dashboard).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The only page engine Kakangkung drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Ladybird,
}

impl Kind {
    pub const ALL: [Kind; 1] = [Kind::Ladybird];

    pub fn id(self) -> &'static str {
        "ladybird"
    }

    pub fn name(self) -> &'static str {
        "Ladybird"
    }

    pub fn parse(id: &str) -> Option<Self> {
        match id.trim().to_ascii_lowercase().as_str() {
            "" | "ladybird" | "libweb" | "native" => Some(Kind::Ladybird),
            // Former WebView2 / Chromium ids are rejected — they are not supported.
            "webview2" | "edge" | "chromium" | "chrome" => None,
            _ => None,
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Result of looking for the Ladybird UI-port host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub requested: Kind,
    pub active: Kind,
    pub host: Option<PathBuf>,
    pub note: &'static str,
}

impl Resolution {
    /// True when pages cannot be painted yet (host binary missing).
    pub fn host_missing(&self) -> bool {
        self.host.is_none()
    }
}

/// File name of the Ladybird UI-port host next to the executable.
pub fn host_file_name() -> &'static str {
    if cfg!(windows) {
        "kakangkung-ladybird-host.exe"
    } else {
        "kakangkung-ladybird-host"
    }
}

/// Locate a Ladybird host: `KAKANGKUNG_LADYBIRD_HOST`, then next to `exe_dir`, then `PATH`.
pub fn find_ladybird_host(exe_dir: Option<&Path>) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KAKANGKUNG_LADYBIRD_HOST") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    exe_dir.and_then(host_in).or_else(|| which(host_file_name()))
}

/// The host shipped with the app in `exe_dir`. The LibWeb-linked host lives with its DLLs,
/// helper processes (WebContent, …) and share/Lagom in ladybird-runtime\bin, and wins over
/// the stub beside the exe.
fn host_in(exe_dir: &Path) -> Option<PathBuf> {
    [runtime_host(exe_dir), exe_dir.join(host_file_name())].into_iter().find(|p| p.is_file())
}

/// Where a packaged LibWeb runtime keeps its host: `<exe dir>/ladybird-runtime/bin/<host>`.
pub fn runtime_host(exe_dir: &Path) -> PathBuf {
    exe_dir.join("ladybird-runtime").join("bin").join(host_file_name())
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Resolve the page engine. Always Ladybird — there is no WebView2 / Chromium fallback.
///
/// When the host binary is missing, `host` is `None` and the browser must keep the native chrome
/// and refuse (or explain) page loads until the host is built (prefer Boat).
pub fn resolve(_requested_setting: &str, exe_dir: Option<&Path>) -> Resolution {
    let host = find_ladybird_host(exe_dir);
    if host.is_some() {
        Resolution {
            requested: Kind::Ladybird,
            active: Kind::Ladybird,
            host,
            note: "Ladybird host found; pages use LibWeb via the Kakangkung UI port.",
        }
    } else {
        Resolution {
            requested: Kind::Ladybird,
            active: Kind::Ladybird,
            host: None,
            note: "Ladybird host missing. Build engines/ladybird on Boat (https://boat.dev/dashboard) — WebView2/Chromium is not used.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ladybird() {
        assert_eq!(Kind::default(), Kind::Ladybird);
        assert_eq!(Kind::parse("ladybird"), Some(Kind::Ladybird));
        assert_eq!(Kind::parse("native"), Some(Kind::Ladybird));
        assert_eq!(Kind::parse("webview2"), None);
        assert_eq!(Kind::parse("chrome"), None);
        assert_eq!(Kind::parse("chromium"), None);
    }

    #[test]
    fn resolve_never_falls_back_to_webview2() {
        std::env::remove_var("KAKANGKUNG_LADYBIRD_HOST");
        let r = resolve("webview2", Some(Path::new("/no/such/kakangkung/dir")));
        assert_eq!(r.active, Kind::Ladybird);
        assert!(r.host_missing());
    }

    #[test]
    fn resolve_uses_env_host() {
        let dir = std::env::temp_dir().join(format!("kk-ladybird-host-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let host = dir.join(host_file_name());
        std::fs::write(&host, b"stub").unwrap();
        std::env::set_var("KAKANGKUNG_LADYBIRD_HOST", &host);
        let r = resolve("ladybird", None);
        std::env::remove_var("KAKANGKUNG_LADYBIRD_HOST");
        let _ = std::fs::remove_file(&host);
        let _ = std::fs::remove_dir(&dir);
        assert_eq!(r.active, Kind::Ladybird);
        assert_eq!(r.host.as_deref(), Some(host.as_path()));
        assert!(!r.host_missing());
    }

    #[test]
    fn packaged_libweb_host_wins_over_the_stub() {
        let dir = std::env::temp_dir().join(format!("kk-runtime-host-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ladybird-runtime").join("bin")).unwrap();
        assert_eq!(host_in(&dir), None);
        std::fs::write(dir.join(host_file_name()), b"stub").unwrap();
        assert_eq!(host_in(&dir), Some(dir.join(host_file_name())));
        std::fs::write(runtime_host(&dir), b"libweb").unwrap();
        assert_eq!(host_in(&dir), Some(runtime_host(&dir)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
