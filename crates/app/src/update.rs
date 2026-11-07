//! Automatic updates from the project's GitHub releases.
//!
//! In the background Kakangkung asks GitHub for the latest release. When it is newer and this
//! copy was installed with the installer, it downloads that release's installer for this
//! processor, checks it against the release's `SHA256SUMS.txt`, and keeps it. The update is put
//! in silently when Kakangkung closes, or at once with "Restart to update" (the installer then
//! opens Kakangkung again with its tabs). A copy that was not installed (a portable zip or a
//! development build) only says that a newer version exists.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value as Json};

/// Where releases are published ("owner/name"). Forks set `KAKANGKUNG_REPO` when building.
pub(crate) const REPO: &str = match option_env!("KAKANGKUNG_REPO") {
    Some(r) => r,
    None => "revanapriyandi/kakangkung",
};
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
/// First check after start-up (so it never competes with opening the window), then this often.
pub(crate) const FIRST_CHECK_AFTER: Duration = Duration::from_secs(20);
pub(crate) const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// Largest installer accepted (bytes).
const MAX_INSTALLER: usize = 300 * 1024 * 1024;
const SUMS: &str = "SHA256SUMS.txt";

/// Where the updater is.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Status {
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer version exists, but this copy was not installed: only shown, with its page.
    Available { version: String, page: String },
    Downloading { version: String },
    /// Downloaded and checked: put in when Kakangkung closes, or with "Restart to update".
    Ready { version: String, installer: PathBuf },
    Failed(String),
}

impl Status {
    /// For the Settings page.
    pub(crate) fn json(&self) -> Json {
        let (state, version, text) = match self {
            Status::Idle => ("idle", "", String::new()),
            Status::Checking => ("checking", "", String::new()),
            Status::UpToDate => ("uptodate", "", String::new()),
            Status::Available { version, page } => ("available", version.as_str(), page.clone()),
            Status::Downloading { version } => ("downloading", version.as_str(), String::new()),
            Status::Ready { version, .. } => ("ready", version.as_str(), String::new()),
            Status::Failed(e) => ("failed", "", e.clone()),
        };
        json!({ "state": state, "version": version, "text": text, "current": VERSION, "installed": installed() })
    }

    pub(crate) fn busy(&self) -> bool {
        matches!(self, Status::Checking | Status::Downloading { .. })
    }
}

/// This processor's name in the release files ("x64" or "arm64").
pub(crate) fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        _ => "x64",
    }
}

pub(crate) fn installer_name(version: &str, arch: &str) -> String {
    format!("Kakangkung-Setup-{version}-{arch}.exe")
}

/// Is `latest` ("1.2.10", "v1.3.0") a newer version than `current`? Numbers are compared part by
/// part; anything after a "-" (a pre-release) counts as older than the plain version.
pub(crate) fn newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> (Vec<u64>, bool) {
        let v = v.trim().trim_start_matches('v');
        let (main, pre) = v.split_once('-').map_or((v, false), |(m, _)| (m, true));
        (main.split('.').map(|p| p.parse().unwrap_or(0)).collect(), pre)
    }
    let ((mut a, a_pre), (mut b, b_pre)) = (parts(latest), parts(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => b_pre && !a_pre,
    }
}

/// The SHA-256 a `sha256sum`-style list (`<hex>  <file>`) gives for `file`.
pub(crate) fn checksum_for<'a>(sums: &'a str, file: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == file && hash.len() == 64).then_some(hash)
    })
}

/// Was this copy installed with the installer (its uninstaller is next to it)? Only installed
/// copies update themselves; a portable or development copy is left as it is.
pub(crate) fn installed() -> bool {
    std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join("unins000.exe").is_file())).unwrap_or(false)
}

fn asset_url(release: &Json, name: &str) -> Option<String> {
    release["assets"].as_array()?.iter().find(|a| a["name"] == name)?["browser_download_url"].as_str().map(String::from)
}

/// Look for a newer release and, for an installed copy, download and check its installer into
/// `dir`. Blocking (run it on its own thread); `progress` hears each state on the way.
pub(crate) fn run(dir: &Path, progress: impl Fn(Status)) -> Status {
    progress(Status::Checking);
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let release: Json = match platform::http::get(&url, 15_000) {
        Ok(body) => serde_json::from_slice(&body).unwrap_or(Json::Null),
        // No release published yet is not a failure.
        Err(e) if e == "HTTP 404" => return Status::UpToDate,
        Err(e) => return Status::Failed(format!("could not reach GitHub ({e})")),
    };
    let version = release["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string();
    if version.is_empty() || !newer(&version, VERSION) {
        // Installers of versions already in place are not needed any more.
        let _ = std::fs::remove_dir_all(dir);
        return Status::UpToDate;
    }
    if !installed() {
        return Status::Available { version, page: release["html_url"].as_str().unwrap_or("").to_string() };
    }
    let name = installer_name(&version, arch());
    let (Some(installer_url), Some(sums_url)) = (asset_url(&release, &name), asset_url(&release, SUMS)) else {
        return Status::Failed(format!("version {version} has no installer for this PC ({})", arch()));
    };
    let target = dir.join(&name);
    progress(Status::Downloading { version: version.clone() });
    let sums = match platform::http::get(&sums_url, 30_000) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => return Status::Failed(format!("could not download the checksums ({e})")),
    };
    let Some(expected) = checksum_for(&sums, &name).map(str::to_lowercase) else {
        return Status::Failed(format!("{SUMS} does not list {name}"));
    };
    // Downloaded before (and still intact): nothing to fetch.
    if std::fs::read(&target).ok().and_then(|b| platform::sha256_hex(&b).ok()).as_deref() == Some(expected.as_str()) {
        return Status::Ready { version, installer: target };
    }
    let bytes = match platform::http::download(&installer_url, 180_000, MAX_INSTALLER) {
        Ok(b) => b,
        Err(e) => return Status::Failed(format!("download failed ({e})")),
    };
    match platform::sha256_hex(&bytes) {
        Ok(got) if got == expected => {}
        _ => return Status::Failed("the download did not match its checksum; it was thrown away".into()),
    }
    let part = dir.join(format!("{name}.part"));
    let saved = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&part, &bytes)).and_then(|_| std::fs::rename(&part, &target));
    match saved {
        Ok(()) => Status::Ready { version, installer: target },
        Err(e) => Status::Failed(format!("could not save the update ({e})")),
    }
}

/// Put the update in: the installer runs silently, closes Kakangkung if it still runs, and with
/// `relaunch` opens it again afterwards with the tabs that were open.
pub(crate) fn install(installer: &Path, relaunch: bool) -> Result<(), String> {
    let mut cmd = std::process::Command::new(installer);
    cmd.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"]);
    if relaunch {
        cmd.arg("/RELAUNCH");
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("could not start the installer ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_part_by_part() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("v1.10.0", "1.9.9"));
        assert!(newer("1.0", "0.9.12"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.2.0"));
        assert!(newer("0.2.0", "0.2.0-beta.1"), "a release beats its pre-release");
        assert!(!newer("0.2.0-beta.2", "0.2.0"));
    }

    #[test]
    fn checksums_are_found_by_file_name() {
        let h = "a".repeat(64);
        let sums = format!("{h}  Kakangkung-Setup-0.2.0-x64.exe\n{}  *Kakangkung-0.2.0-x64.zip\n", "b".repeat(64));
        assert_eq!(checksum_for(&sums, "Kakangkung-Setup-0.2.0-x64.exe"), Some(h.as_str()));
        assert_eq!(checksum_for(&sums, "Kakangkung-0.2.0-x64.zip").map(str::len), Some(64));
        assert_eq!(checksum_for(&sums, "Kakangkung-Setup-0.2.0-arm64.exe"), None);
        assert_eq!(installer_name("0.2.0", "arm64"), "Kakangkung-Setup-0.2.0-arm64.exe");
    }

    #[test]
    fn the_release_assets_are_picked_by_name() {
        let r = json!({ "assets": [{ "name": "SHA256SUMS.txt", "browser_download_url": "https://x/s" }, { "name": "Kakangkung-Setup-1.0.0-x64.exe", "browser_download_url": "https://x/i" }] });
        assert_eq!(asset_url(&r, "SHA256SUMS.txt").as_deref(), Some("https://x/s"));
        assert_eq!(asset_url(&r, &installer_name("1.0.0", "x64")).as_deref(), Some("https://x/i"));
        assert_eq!(asset_url(&r, "nope"), None);
    }
}
