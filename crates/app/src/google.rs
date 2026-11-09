//! Sign in with Google, and Kakangkung sync through that account's Google Drive.
//!
//! Sign-in is OAuth 2.0 for installed apps (RFC 8252), the way Google wants desktop apps to do
//! it: the consent page opens in an external browser, Google redirects to a one-shot listener on
//! 127.0.0.1, and the code is exchanged with PKCE. Scopes: name and email, plus `drive.appdata`,
//! a hidden Drive folder only Kakangkung can see (none of the person's own files). The refresh
//! token is kept DPAPI-encrypted in the settings (`google.token`).
//!
//! Sync keeps working as it does for a cloud folder (storage::sync merges device files in a
//! local folder). For a Google account that folder is `<data>/Google Drive/account-…`, and the
//! mirror thread keeps it equal to the Drive folder: it uploads this device's file when it
//! changes and downloads the other devices' files, as the Drive app would for a normal folder.
//!
//! The OAuth client is the developer's own (Google Cloud console, type "Desktop app"); see
//! docs/GOOGLE_SIGNIN.md. Blocking network calls: run off the UI thread.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use storage::Store;
use tao::event_loop::EventLoopProxy;

use crate::msg::Msg;
use crate::pages::{percent_encode, query_param};

const AUTH: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN: &str = "https://oauth2.googleapis.com/token";
const REVOKE: &str = "https://oauth2.googleapis.com/revoke";
const USERINFO: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const FILES: &str = "https://www.googleapis.com/drive/v3/files";
const UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3/files";
const SCOPES: &str = "openid email profile https://www.googleapis.com/auth/drive.appdata";
/// How long the sign-in waits for the person to finish on Google's page.
const SIGN_IN_WAIT: Duration = Duration::from_secs(300);
/// The mirror looks at Drive this often (and right away when kicked after a local sync).
const MIRROR_EVERY: Duration = Duration::from_secs(120);
const NET_TIMEOUT_MS: i32 = 30_000;
/// Largest device file moved (the same limit storage::sync reads).
const MAX_FILE: usize = 20 * 1024 * 1024;
/// File name of the OAuth client downloaded from the Google Cloud console, in the data folder.
pub(crate) const CLIENT_FILE: &str = "google-client.json";

/// The developer's OAuth client ("Desktop app").
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Client {
    pub(crate) id: String,
    pub(crate) secret: String,
}

/// The OAuth client: environment variables KAKANGKUNG_GOOGLE_CLIENT_ID / _SECRET (also read at
/// build time, for release builds), else `google-client.json` (the console's download) in the
/// data folder.
pub(crate) fn client() -> Option<Client> {
    let var = |k: &str, built: Option<&str>| std::env::var(k).ok().or(built.map(String::from)).filter(|v| !v.trim().is_empty());
    if let Some(id) = var("KAKANGKUNG_GOOGLE_CLIENT_ID", option_env!("KAKANGKUNG_GOOGLE_CLIENT_ID")) {
        let secret = var("KAKANGKUNG_GOOGLE_CLIENT_SECRET", option_env!("KAKANGKUNG_GOOGLE_CLIENT_SECRET")).unwrap_or_default();
        return Some(Client { id, secret });
    }
    client_from_file(&std::fs::read_to_string(storage::data_dir().join(CLIENT_FILE)).ok()?)
}

/// The console's client JSON: `{"installed": {"client_id": …, "client_secret": …}}`.
fn client_from_file(text: &str) -> Option<Client> {
    // Notepad saves with a byte order mark; serde_json refuses one.
    let j: Json = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let c = j.get("installed").unwrap_or(&j);
    let id = c["client_id"].as_str().filter(|s| !s.is_empty())?.to_string();
    Some(Client { id, secret: c["client_secret"].as_str().unwrap_or("").to_string() })
}

/// Base64url without padding (PKCE).
fn base64url(data: &[u8]) -> String {
    crate::webstore::base64(data).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// PKCE (RFC 7636, S256): (verifier, challenge).
fn pkce(verifier: String) -> Result<(String, String), String> {
    let challenge = base64url(&platform::sha256(verifier.as_bytes())?);
    Ok((verifier, challenge))
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{k}={}", percent_encode(v))).collect::<Vec<_>>().join("&")
}

fn post_form(url: &str, pairs: &[(&str, &str)]) -> Result<Json, String> {
    let body = platform::http::request("POST", url, "Content-Type: application/x-www-form-urlencoded\r\n", form(pairs).as_bytes(), NET_TIMEOUT_MS, 1 << 20)?;
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

fn bearer(token: &str) -> String {
    format!("Authorization: Bearer {token}\r\n")
}

/// Who signed in, and the refresh token to keep.
#[derive(Debug)]
pub(crate) struct Account {
    pub(crate) name: String,
    pub(crate) email: String,
    pub(crate) refresh: String,
    pub(crate) picture: String,
    pub(crate) subject: String,
}

fn user_profile(me: &Json, refresh: String) -> Result<Account, String> {
    let email = me["email"].as_str().unwrap_or("").trim().to_lowercase();
    if !email.contains('@') {
        return Err("Google did not share the account's email address.".into());
    }
    let subject = me["sub"].as_str().filter(|s| !s.is_empty()).ok_or("Google did not share the account identifier.")?.to_string();
    let name = me["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(String::from)
        .unwrap_or_else(|| email.split('@').next().unwrap_or("").to_string());
    let picture = me["picture"].as_str().filter(|url| trusted_picture_url(url)).unwrap_or("").to_string();
    Ok(Account { name, email, refresh, picture, subject })
}

fn trusted_picture_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split('/').next().unwrap_or("").to_ascii_lowercase();
    url.len() <= 8192 && ["googleusercontent.com", "ggpht.com"].iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

fn picture_bytes(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.len() <= 1024 * 1024 && (
        bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.starts_with(b"\xff\xd8\xff")
        || bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")))
}

fn cache_picture(account: &Account) -> Result<String, String> {
    if account.picture.is_empty() { return Ok(String::new()); }
    let bytes = platform::http::request("GET", &account.picture, "", &[], NET_TIMEOUT_MS, 1024 * 1024 + 1)?;
    if !picture_bytes(&bytes) { return Err("Google profile photo is not a supported image under 1 MiB".into()); }
    let folder = mirror_root().join("avatars");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let name = format!("{}-{}.img", hex(&platform::sha256(account.subject.as_bytes())?), hex(&platform::sha256(&bytes)?));
    write_atomic(&folder, &name, &bytes)?;
    Ok(folder.join(name).to_string_lossy().into_owned())
}

/// The query of the redirect Google sends to the listener (`GET /?code=…&state=… HTTP/1.1`),
/// or None for another request (the browser's favicon request).
fn redirect_query(request: &str) -> Option<&str> {
    let target = request.lines().next()?.strip_prefix("GET ")?.split(' ').next()?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    (path == "/").then_some(query)
}

fn answer(mut stream: &TcpStream, ok: bool, text: &str) {
    let color = if ok { "#188038" } else { "#d93025" };
    let page = format!(
        "<!doctype html><meta charset=utf-8><title>Kakangkung</title><body style=\"font:16px 'Segoe UI',sans-serif;display:grid;place-items:center;height:90vh\"><div style=\"text-align:center\"><h2 style=\"color:{color}\">{text}</h2><p>You can close this tab and go back to Kakangkung.</p></div>"
    );
    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
}

/// Wait for Google's redirect on `listener`; returns the authorization code.
fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + SIGN_IN_WAIT;
    while Instant::now() < deadline {
        let (stream, _) = match listener.accept() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; 8192];
        let n = (&stream).read(&mut buf).unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]);
        let Some(query) = redirect_query(&request) else {
            let _ = (&stream).write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        };
        // Only the redirect of this sign-in counts (another page cannot finish it for us).
        if query_param(query, "state") != state {
            answer(&stream, false, "This sign-in link has expired");
            continue;
        }
        let error = query_param(query, "error");
        if !error.is_empty() {
            answer(&stream, false, "Sign-in was cancelled");
            return Err(if error == "access_denied" { "Sign-in was cancelled.".into() } else { format!("Google refused the sign-in ({error}).") });
        }
        let code = query_param(query, "code");
        if code.is_empty() {
            answer(&stream, false, "Sign-in failed");
            return Err("Google sent no sign-in code.".into());
        }
        answer(&stream, true, "Signed in to Kakangkung");
        return Ok(code);
    }
    Err("Sign-in timed out. Try again.".into())
}

fn external_browser(roots: &[PathBuf]) -> Option<PathBuf> {
    roots.iter().flat_map(|root| [
        root.join("Microsoft/Edge/Application/msedge.exe"),
        root.join("Google/Chrome/Application/chrome.exe"),
        root.join("Mozilla Firefox/firefox.exe"),
    ]).find(|path| path.is_file())
}

fn open_authorization(url: &str) -> Result<(), String> {
    if platform::is_default_browser() {
        // An embedded WebView2 OAuth page is not an external user agent, even if this
        // application is registered as the default browser.
        let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
            .iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
        let browser = external_browser(&roots).ok_or("Google sign-in needs an external browser. Install Microsoft Edge, Google Chrome or Firefox, or make another supported browser the Windows default, then try again.")?;
        std::process::Command::new(browser).arg(url).spawn().map_err(|e| format!("cannot open the external browser: {e}"))?;
    } else {
        std::process::Command::new("rundll32.exe").arg("url.dll,FileProtocolHandler").arg(url).spawn().map_err(|e| format!("cannot open the browser: {e}"))?;
    }
    Ok(())
}

/// The whole sign-in: consent in an external browser, then code → tokens → name and email.
pub(crate) fn sign_in() -> Result<Account, String> {
    let c = client().ok_or("Google sign-in is not set up in this build: add an OAuth client (see docs/GOOGLE_SIGNIN.md).")?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("cannot listen for Google's answer: {e}"))?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr().map_err(|e| e.to_string())?.port());
    let (verifier, challenge) = pkce(base64url(&platform::random_bytes(32)?))?;
    let state = hex(&platform::random_bytes(16)?);
    let url = format!(
        "{AUTH}?{}",
        form(&[
            ("client_id", &c.id),
            ("redirect_uri", &redirect),
            ("response_type", "code"),
            ("scope", SCOPES),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            ("access_type", "offline"),
            ("prompt", "consent select_account"),
        ])
    );
    open_authorization(&url)?;
    let code = wait_for_code(&listener, &state)?;
    let tokens = post_form(
        TOKEN,
        &[("code", &code), ("client_id", &c.id), ("client_secret", &c.secret), ("redirect_uri", &redirect), ("grant_type", "authorization_code"), ("code_verifier", &verifier)],
    )?;
    let (access, refresh) = (tokens["access_token"].as_str().unwrap_or(""), tokens["refresh_token"].as_str().unwrap_or(""));
    if access.is_empty() || refresh.is_empty() {
        return Err("Google did not grant offline access; try signing in again.".into());
    }
    let me: Json = serde_json::from_slice(&platform::http::request("GET", USERINFO, &bearer(access), &[], NET_TIMEOUT_MS, 1 << 20)?).map_err(|e| e.to_string())?;
    user_profile(&me, refresh.into())
}

/// Keep the signed-in Google account (the refresh token DPAPI-encrypted) and turn sync on.
pub(crate) fn save(s: &Store, a: &Account) -> Result<(), String> {
    let sealed = hex(&platform::protect(a.refresh.as_bytes())?);
    let picture = cache_picture(a).unwrap_or_default();
    s.set_settings(&[
        ("google.token", &sealed),
        ("account.provider", "google"),
        ("account.name", &a.name),
        ("account.email", &a.email),
        ("account.picture.path", &picture),
        ("account.picture.url", &a.picture),
        ("google.subject", &a.subject),
        ("sync.enabled", "1"),
    ])
}

/// Forget the Google account on this PC and revoke its token at Google (best effort).
pub(crate) fn forget(s: &Store) {
    if let Some(token) = refresh_token(s) {
        std::thread::spawn(move || {
            let _ = post_form(REVOKE, &[("token", &token)]);
        });
    }
    let _ = s.set_settings(&[("google.token", ""), ("account.provider", ""), ("google.subject", ""), ("account.picture.path", ""), ("account.picture.url", "")]);
}

/// Is the signed-in account a Google account?
pub(crate) fn is_google(s: &Store) -> bool {
    s.setting("account.provider").as_deref() == Some("google")
}

fn refresh_token(s: &Store) -> Option<String> {
    decode_refresh_token(&s.setting("google.token")?)
}

fn decode_refresh_token(sealed: &str) -> Option<String> {
    String::from_utf8(platform::unprotect(&unhex(sealed)?).ok()?).ok()
}

/// The local folder mirrored to the account's Drive.
pub(crate) fn mirror_root() -> PathBuf {
    storage::data_dir().join("Google Drive")
}

/// Wakes the mirror (after a sync wrote this device's file, or after signing in).
static KICK: OnceLock<Mutex<mpsc::Sender<()>>> = OnceLock::new();

pub(crate) fn kick() {
    if let Some(tx) = KICK.get().and_then(|k| k.lock().ok()) {
        let _ = tx.send(());
    }
}

/// Start the mirror thread for the profile at `db` (its own database connection).
pub(crate) fn start_mirror(db: PathBuf, proxy: EventLoopProxy<Msg>) {
    let (tx, rx) = mpsc::channel();
    if KICK.set(Mutex::new(tx)).is_err() {
        return;
    }
    std::thread::spawn(move || {
        let mut m = Mirror::default();
        let mut shown_profile = None;
        let mut first = true;
        loop {
            let wait = if first { Duration::ZERO } else { MIRROR_EVERY };
            first = false;
            match rx.recv_timeout(wait) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            while rx.try_recv().is_ok() {}
            let Ok(s) = Store::open(&db) else { continue };
            match m.round(&s) {
                Ok(true) => {
                    let _ = proxy.send_event(Msg::SyncSoon);
                }
                Ok(false) => {}
                Err(e) => {
                    eprintln!("google drive sync: {e}");
                    let _ = proxy.send_event(Msg::Synced(false));
                }
            }
            if let Ok(settings) = s.settings_snapshot() {
                let profile = (settings.get("account.name").cloned().unwrap_or_default(), settings.get("account.picture.path").cloned().unwrap_or_default());
                if shown_profile.as_ref() != Some(&profile) {
                    let _ = proxy.send_event(Msg::AccountProfile(profile.0.clone(), profile.1.clone()));
                    shown_profile = Some(profile);
                }
            }
        }
    });
}

#[derive(Clone, PartialEq, Eq)]
struct RoundConfig {
    folder: PathBuf,
    device: String,
    sealed: String,
}

impl RoundConfig {
    fn account(settings: &HashMap<String, String>) -> Option<Self> {
        if settings.get("account.provider").map(String::as_str) != Some("google") { return None; }
        let email = settings.get("account.email").filter(|e| e.contains('@'))?;
        let sealed = settings.get("google.token").filter(|t| !t.is_empty())?;
        Some(Self {
            folder: storage::sync::account_dir(&mirror_root(), email),
            device: settings.get("sync.device").cloned().unwrap_or_default(),
            sealed: sealed.clone(),
        })
    }

    fn capture(settings: &HashMap<String, String>) -> Option<Self> {
        if settings.get("sync.enabled").is_some_and(|v| v != "1") { return None; }
        let config = Self::account(settings)?;
        device_file(&format!("{}.json", config.device)).then_some(config)
    }

    fn still_current(&self, settings: &HashMap<String, String>) -> bool {
        Self::capture(settings).as_ref() == Some(self)
    }
}

#[derive(Default)]
struct Mirror {
    identity: Option<(PathBuf, String, String)>,
    access: Option<(String, Instant)>,
    /// Drive file id → modifiedTime last downloaded.
    seen: HashMap<String, String>,
    /// This device's file as last uploaded.
    uploaded: Option<Vec<u8>>,
    profile_checked: Option<Instant>,
}

impl Mirror {
    // Tokens and transfer caches belong to one account, OAuth client and credential.
    fn select_identity(&mut self, identity: Option<(PathBuf, String, String)>) {
        if self.identity != identity {
            *self = Self { identity, ..Self::default() };
        }
    }

    fn token(&mut self, c: &Client, sealed: &str) -> Result<String, String> {
        if let Some((t, until)) = &self.access {
            if Instant::now() < *until {
                return Ok(t.clone());
            }
        }
        let refresh = decode_refresh_token(sealed).ok_or("signed out of Google; sign in again")?;
        let r = post_form(TOKEN, &[("client_id", &c.id), ("client_secret", &c.secret), ("refresh_token", &refresh), ("grant_type", "refresh_token")])?;
        let t = r["access_token"].as_str().ok_or("Google gave no access token")?.to_string();
        let life = r["expires_in"].as_u64().unwrap_or(3600).saturating_sub(120);
        self.access = Some((t.clone(), Instant::now() + Duration::from_secs(life)));
        Ok(t)
    }

    /// One mirror round. True when other devices' files changed (worth a sync now).
    fn round(&mut self, s: &Store) -> Result<bool, String> {
        let snapshot = s.settings_snapshot()?;
        let Some(account_config) = RoundConfig::account(&snapshot) else {
            self.select_identity(None);
            return Ok(false);
        };
        let c = client().ok_or("no OAuth client")?;
        self.select_identity(Some((account_config.folder.clone(), c.id.clone(), account_config.sealed.clone())));
        let token = self.token(&c, &account_config.sealed)?;
        if self.profile_checked.is_none_or(|checked| checked.elapsed() >= Duration::from_secs(3600)) {
            // Missing/offline profile images must not prevent bookmarks or history syncing.
            if refresh_profile(s, &snapshot, &token).is_ok() {
                self.profile_checked = Some(Instant::now());
            }
        }
        let Some(config) = RoundConfig::capture(&snapshot) else { return Ok(false) };
        let (folder, device) = (&config.folder, &config.device);
        let auth = bearer(&token);
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        let files = list_device_files(|url| {
            let bytes = platform::http::request("GET", url, &auth, &[], NET_TIMEOUT_MS, 4 << 20)?;
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())
        })?;
        let own_name = format!("{device}.json");
        let mut changed = false;
        for f in &files {
            let (id, name, modified) = (f["id"].as_str().unwrap_or(""), f["name"].as_str().unwrap_or(""), f["modifiedTime"].as_str().unwrap_or(""));
            if id.is_empty() || modified.is_empty() || !device_file(name) || name == own_name || self.seen.get(id).is_some_and(|m| m == modified) {
                continue;
            }
            let body = platform::http::request("GET", &format!("{FILES}/{id}?alt=media"), &auth, &[], NET_TIMEOUT_MS * 2, MAX_FILE)?;
            if body.len() > MAX_FILE {
                return Err("Google Drive device file is too large".into());
            }
            if !config.still_current(&s.settings_snapshot()?) || client().as_ref() != Some(&c) {
                self.select_identity(None);
                return Ok(false);
            }
            write_atomic(folder, name, &body)?;
            self.seen.insert(id.into(), modified.into());
            changed = true;
        }
        let mine = folder.join(&own_name);
        if let Ok(bytes) = std::fs::read(&mine) {
            if bytes.len() > MAX_FILE {
                return Err("Local sync file is too large for Google Drive sync".into());
            }
            if self.uploaded.as_deref() != Some(bytes.as_slice()) {
                if !config.still_current(&s.settings_snapshot()?) || client().as_ref() != Some(&c) {
                    self.select_identity(None);
                    return Ok(false);
                }
                match files.iter().find(|f| f["name"].as_str() == Some(own_name.as_str())).and_then(|f| f["id"].as_str()) {
                    Some(id) => {
                        let headers = format!("{auth}Content-Type: application/json\r\n");
                        platform::http::request("PATCH", &format!("{UPLOAD}/{id}?uploadType=media"), &headers, &bytes, NET_TIMEOUT_MS * 2, 1 << 20)?;
                    }
                    None => {
                        let (body, kind) = multipart(&json!({ "name": own_name, "parents": ["appDataFolder"] }), &bytes);
                        platform::http::request("POST", &format!("{UPLOAD}?uploadType=multipart"), &format!("{auth}Content-Type: {kind}\r\n"), &body, NET_TIMEOUT_MS * 2, 1 << 20)?;
                    }
                }
                self.uploaded = Some(bytes);
            }
        }
        Ok(changed)
    }
}

fn refresh_profile(s: &Store, snapshot: &HashMap<String, String>, access: &str) -> Result<(), String> {
    let bytes = platform::http::request("GET", USERINFO, &bearer(access), &[], NET_TIMEOUT_MS, 1 << 20)?;
    let profile = user_profile(&serde_json::from_slice(&bytes).map_err(|e| e.to_string())?, String::new())?;
    let email = snapshot.get("account.email").ok_or("signed out")?;
    if profile.email != email.trim().to_lowercase() { return Err("Google profile account changed".into()); }
    if snapshot.get("google.subject").is_some_and(|sub| !sub.is_empty() && sub != &profile.subject) {
        return Err("Google profile account identifier changed".into());
    }
    let picture = cache_picture(&profile).unwrap_or_else(|_| {
        snapshot.get("account.picture.path").cloned().unwrap_or_default()
    });
    let sealed = snapshot.get("google.token").ok_or("signed out")?;
    s.set_settings_if_current(&[("account.provider", "google"), ("account.email", email), ("google.token", sealed)], &[
        ("account.name", &profile.name), ("google.subject", &profile.subject),
        ("account.picture.url", &profile.picture), ("account.picture.path", &picture),
    ])?;
    Ok(())
}

/// Follow all pages: Drive may return fewer than pageSize items even before the last page.
fn list_device_files(mut fetch: impl FnMut(&str) -> Result<Json, String>) -> Result<Vec<Json>, String> {
    let mut files = Vec::new();
    let mut page = String::new();
    let mut pages = std::collections::HashSet::new();
    loop {
        let url = format!("{FILES}?spaces=appDataFolder&q=trashed%3Dfalse&pageSize=1000&fields=nextPageToken%2Cfiles(id%2Cname%2CmodifiedTime)&pageToken={}", percent_encode(&page));
        let response = fetch(&url)?;
        let entries = response["files"].as_array().ok_or("Google Drive returned no file list")?;
        files.extend(entries.iter().filter(|f| f["name"].as_str().is_some_and(device_file)).cloned());
        let next = response["nextPageToken"].as_str().unwrap_or("");
        if next.is_empty() {
            return Ok(files);
        }
        if !pages.insert(next.to_string()) {
            return Err("Google Drive repeated a page token".into());
        }
        page = next.to_string();
    }
}

/// A sync device file: `<hex device id>.json` (anything else in the Drive folder is ignored).
fn device_file(name: &str) -> bool {
    name.strip_suffix(".json").is_some_and(|d| !d.is_empty() && d.len() <= 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn write_atomic(folder: &Path, name: &str, data: &[u8]) -> Result<(), String> {
    let (tmp, file) = (folder.join(format!("{name}.download")), folder.join(name));
    std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, &file)).map_err(|e| format!("writing {}: {e}", file.display()))
}

/// A Drive multipart upload body (metadata + content) and its Content-Type.
fn multipart(meta: &Json, content: &[u8]) -> (Vec<u8>, String) {
    let boundary = "kakangkung-sync-boundary";
    let mut body = format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: application/json\r\n\r\n").into_bytes();
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--").as_bytes());
    (body, format!("multipart/related; boundary={boundary}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_profile_uses_name_and_optional_safe_picture() {
        let account = user_profile(&json!({"sub":"fixture-account", "email":" Person@Example.test ", "name":"  Mira Example  ", "picture":"https://lh3.googleusercontent.com/a/photo"}), String::new()).unwrap();
        assert_eq!(account.name, "Mira Example");
        assert_eq!(account.email, "person@example.test");
        assert_eq!(account.subject, "fixture-account");
        assert_eq!(account.picture, "https://lh3.googleusercontent.com/a/photo");
        let fallback = user_profile(&json!({"sub":"fixture", "email":"a@example.test", "picture":"http://localhost/private"}), String::new()).unwrap();
        assert_eq!(fallback.name, "a");
        assert!(fallback.picture.is_empty());
        assert!(user_profile(&json!({"email":"a@example.test"}), String::new()).is_err());
        assert!(user_profile(&json!({"sub":"fixture"}), String::new()).is_err());
    }

    #[test]
    fn profile_photo_allows_only_google_https_and_bounded_image_content() {
        assert!(trusted_picture_url("https://lh3.googleusercontent.com/a/photo"));
        assert!(trusted_picture_url("https://example.ggpht.com/photo"));
        for url in ["http://lh3.googleusercontent.com/a", "https://googleusercontent.com.attacker.test/a", "https://lh3.googleusercontent.com@localhost/a", "https://lh3.googleusercontent.com:1234/a", "https://127.0.0.1/photo"] {
            assert!(!trusted_picture_url(url));
        }
        assert!(picture_bytes(b"\x89PNG\r\n\x1a\nfixture"));
        assert!(picture_bytes(b"\xff\xd8\xfffixture"));
        assert!(!picture_bytes(b"<html>not an image</html>"));
        let mut huge = vec![0; 1024 * 1024 + 1];
        huge[..3].copy_from_slice(b"\xff\xd8\xff");
        assert!(!picture_bytes(&huge));
    }

    #[test]
    fn round_configuration_captures_one_account_and_rejects_session_changes() {
        let mut settings: HashMap<String, String> = [
            ("account.provider", "google"), ("account.email", "a@example.test"),
            ("sync.enabled", "1"), ("sync.device", "aa01"), ("google.token", "sealed-a"),
        ].into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        let config = RoundConfig::capture(&settings).unwrap();
        assert!(config.still_current(&settings));
        for (key, value) in [
            ("account.email", "b@example.test"), ("google.token", "sealed-b"),
            ("sync.device", "bb02"), ("sync.enabled", "0"), ("account.provider", ""),
        ] {
            let mut changed = settings.clone();
            changed.insert(key.into(), value.into());
            assert!(!config.still_current(&changed), "changed session must stop mirror writes");
        }
        settings.remove("google.token");
        assert!(RoundConfig::capture(&settings).is_none());
        assert_eq!(config.folder, storage::sync::account_dir(&mirror_root(), "a@example.test"));
    }

    #[test]
    fn external_browser_requires_an_installed_executable() {
        let root = std::env::temp_dir().join(format!("kakangkung-oauth-browser-{}", std::process::id()));
        let candidate = root.join("Google/Chrome/Application/chrome.exe");
        let _ = std::fs::remove_file(&candidate);
        assert!(external_browser(std::slice::from_ref(&root)).is_none());
        std::fs::create_dir_all(candidate.parent().unwrap()).unwrap();
        std::fs::write(&candidate, b"test fixture").unwrap();
        assert_eq!(external_browser(&[root]), Some(candidate.clone()));
        std::fs::remove_file(candidate).unwrap();
    }

    #[test]
    fn account_or_credential_changes_clear_all_mirror_caches() {
        let identity = |account: &str, token: &str| Some((PathBuf::from(account), "client".into(), token.into()));
        let mut mirror = Mirror::default();
        mirror.select_identity(identity("a", "token-a"));
        mirror.access = Some(("old-access".into(), Instant::now() + Duration::from_secs(60)));
        mirror.seen.insert("file".into(), "time".into());
        mirror.uploaded = Some(vec![1]);
        mirror.select_identity(identity("a", "token-a"));
        assert!(mirror.access.is_some(), "same session retains its cache");
        mirror.select_identity(identity("b", "token-b"));
        assert!(mirror.access.is_none() && mirror.seen.is_empty() && mirror.uploaded.is_none());
        mirror.access = Some(("access-b".into(), Instant::now()));
        mirror.select_identity(identity("b", "new-token-b"));
        assert!(mirror.access.is_none(), "reauthorization invalidates cached credentials");
        mirror.uploaded = Some(vec![2]);
        mirror.select_identity(Some((PathBuf::from("b"), "another-client".into(), "new-token-b".into())));
        assert!(mirror.uploaded.is_none(), "OAuth application changes invalidate transfer caches");
        mirror.uploaded = Some(vec![3]);
        mirror.select_identity(None);
        assert!(mirror.identity.is_none() && mirror.uploaded.is_none());
    }

    #[test]
    fn drive_listing_follows_pages_and_rejects_cycles() {
        let mut calls = 0;
        let files = list_device_files(|url| {
            calls += 1;
            assert!(url.contains("nextPageToken"));
            Ok(if calls == 1 {
                json!({"files": [{"name":"aa.json"}], "nextPageToken":"a+/="})
            } else {
                assert!(url.contains("pageToken=a%2B%2F%3D"));
                json!({"files": [{"name":"bb.json"}, {"name":"notes.txt"}]})
            })
        }).unwrap();
        assert_eq!(calls, 2);
        assert_eq!(files.len(), 2);
        assert!(list_device_files(|_| Ok(json!({"files": [], "nextPageToken":"repeat"}))).is_err());
        assert!(list_device_files(|_| Ok(json!({"error":"denied"}))).is_err());
    }

    #[test]
    fn pkce_matches_rfc7636_example() {
        let (_, challenge) = pkce("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into()).unwrap();
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn hex_round_trip_and_client_file() {
        assert_eq!(unhex(&hex(&[0, 255, 16])), Some(vec![0, 255, 16]));
        assert_eq!(unhex("zz"), None);
        let c = client_from_file(r#"{"installed":{"client_id":"123.apps.googleusercontent.com","client_secret":"s3"}}"#).unwrap();
        assert_eq!(c, Client { id: "123.apps.googleusercontent.com".into(), secret: "s3".into() });
        assert!(client_from_file(r#"{"installed":{}}"#).is_none());
        assert_eq!(client_from_file("\u{feff}{\"installed\":{\"client_id\":\"1.apps.googleusercontent.com\"}}").map(|c| c.id), Some("1.apps.googleusercontent.com".into()), "byte order mark");
        assert!(client_from_file("not json").is_none());
    }

    #[test]
    fn redirect_requests() {
        assert_eq!(redirect_query("GET /?code=4%2Fabc&state=x HTTP/1.1\r\nHost: 127.0.0.1\r\n"), Some("code=4%2Fabc&state=x"));
        assert_eq!(redirect_query("GET /favicon.ico HTTP/1.1\r\n"), None);
        assert_eq!(redirect_query("POST /?code=1 HTTP/1.1\r\n"), None);
        assert_eq!(query_param("code=4%2Fabc&state=x", "code"), "4/abc");
    }

    #[test]
    fn only_device_files_are_mirrored() {
        assert!(device_file("0123abcdef0123abcdef0123abcdef01.json"));
        assert!(!device_file("../x.json") && !device_file("a.json.tmp") && !device_file(".json") && !device_file("notes.txt"));
    }

    #[test]
    fn multipart_body() {
        let (body, kind) = multipart(&json!({ "name": "a.json" }), b"{}");
        let text = String::from_utf8(body).unwrap();
        assert!(kind.starts_with("multipart/related; boundary="));
        assert!(text.contains("{\"name\":\"a.json\"}") && text.ends_with("{}\r\n--kakangkung-sync-boundary--"));
    }

    #[test]
    fn loopback_listener_takes_the_matching_redirect() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // The browser's requests, one after another, while the listener waits.
        let browser = std::thread::spawn(move || {
            ["/favicon.ico", "/?code=evil&state=other", "/?code=good&state=s1"].map(|path| {
                let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                let mut out = String::new();
                let _ = s.read_to_string(&mut out);
                out
            })
        });
        assert_eq!(wait_for_code(&listener, "s1").unwrap(), "good");
        let [favicon, wrong, right] = browser.join().unwrap();
        assert!(favicon.starts_with("HTTP/1.1 404"));
        assert!(wrong.contains("expired"));
        assert!(right.contains("Signed in"));
    }
}
