//! Sign in with Google, and Kakangkung sync through that account's Google Drive.
//!
//! Sign-in is OAuth 2.0 for installed apps (RFC 8252), the way Google wants desktop apps to do
//! it: the consent page opens in the default browser, Google redirects to a one-shot listener on
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

/// The whole sign-in: consent in the default browser, then code → tokens → name and email.
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
    // The default browser, as Google requires for installed apps (it is Kakangkung itself
    // when Kakangkung is the default browser).
    std::process::Command::new("rundll32.exe").arg("url.dll,FileProtocolHandler").arg(&url).spawn().map_err(|e| format!("cannot open the browser: {e}"))?;
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
    let email = me["email"].as_str().unwrap_or("").to_lowercase();
    if !email.contains('@') {
        return Err("Google did not share the account's email address.".into());
    }
    let name = me["name"].as_str().filter(|n| !n.is_empty()).map(String::from).unwrap_or_else(|| email.split('@').next().unwrap_or("").to_string());
    Ok(Account { name, email, refresh: refresh.into() })
}

/// Keep the signed-in Google account (the refresh token DPAPI-encrypted) and turn sync on.
pub(crate) fn save(s: &Store, a: &Account) -> Result<(), String> {
    s.set_setting("google.token", &hex(&platform::protect(a.refresh.as_bytes())?))?;
    s.set_setting("account.provider", "google")?;
    s.set_setting("account.name", &a.name)?;
    s.set_setting("account.email", &a.email)?;
    s.set_setting("sync.enabled", "1")
}

/// Forget the Google account on this PC and revoke its token at Google (best effort).
pub(crate) fn forget(s: &Store) {
    if let Some(token) = refresh_token(s) {
        std::thread::spawn(move || {
            let _ = post_form(REVOKE, &[("token", &token)]);
        });
    }
    let _ = s.set_setting("google.token", "");
    let _ = s.set_setting("account.provider", "");
}

/// Is the signed-in account a Google account?
pub(crate) fn is_google(s: &Store) -> bool {
    s.setting("account.provider").as_deref() == Some("google")
}

fn refresh_token(s: &Store) -> Option<String> {
    let sealed = unhex(&s.setting("google.token")?)?;
    String::from_utf8(platform::unprotect(&sealed).ok()?).ok()
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
        loop {
            match rx.recv_timeout(MIRROR_EVERY) {
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
        }
    });
}

#[derive(Default)]
struct Mirror {
    access: Option<(String, Instant)>,
    /// Drive file id → modifiedTime last downloaded.
    seen: HashMap<String, String>,
    /// This device's file as last uploaded.
    uploaded: Option<Vec<u8>>,
}

impl Mirror {
    fn token(&mut self, s: &Store) -> Result<String, String> {
        if let Some((t, until)) = &self.access {
            if Instant::now() < *until {
                return Ok(t.clone());
            }
        }
        let c = client().ok_or("no OAuth client")?;
        let refresh = refresh_token(s).ok_or("signed out of Google; sign in again")?;
        let r = post_form(TOKEN, &[("client_id", &c.id), ("client_secret", &c.secret), ("refresh_token", &refresh), ("grant_type", "refresh_token")])?;
        let t = r["access_token"].as_str().ok_or("Google gave no access token")?.to_string();
        let life = r["expires_in"].as_u64().unwrap_or(3600).saturating_sub(120);
        self.access = Some((t.clone(), Instant::now() + Duration::from_secs(life)));
        Ok(t)
    }

    /// One mirror round. True when other devices' files changed (worth a sync now).
    fn round(&mut self, s: &Store) -> Result<bool, String> {
        let (enabled, folder, device) = crate::profile::sync_conf(s);
        if !enabled || !is_google(s) {
            return Ok(false);
        }
        let token = self.token(s)?;
        let auth = bearer(&token);
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let list_url = format!("{FILES}?spaces=appDataFolder&pageSize=1000&fields=files(id%2Cname%2CmodifiedTime)");
        let list: Json = serde_json::from_slice(&platform::http::request("GET", &list_url, &auth, &[], NET_TIMEOUT_MS, 4 << 20)?).map_err(|e| e.to_string())?;
        let files = list["files"].as_array().cloned().unwrap_or_default();
        let own_name = format!("{device}.json");
        let mut changed = false;
        for f in &files {
            let (id, name, modified) = (f["id"].as_str().unwrap_or(""), f["name"].as_str().unwrap_or(""), f["modifiedTime"].as_str().unwrap_or(""));
            if !device_file(name) || name == own_name || self.seen.get(id).is_some_and(|m| m == modified) {
                continue;
            }
            let body = platform::http::request("GET", &format!("{FILES}/{id}?alt=media"), &auth, &[], NET_TIMEOUT_MS * 2, MAX_FILE)?;
            write_atomic(&folder, name, &body)?;
            self.seen.insert(id.into(), modified.into());
            changed = true;
        }
        let mine = folder.join(&own_name);
        if let Ok(bytes) = std::fs::read(&mine) {
            if self.uploaded.as_deref() != Some(bytes.as_slice()) {
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
