//! Ladybird page-engine host: discovery and process lifecycle.
//!
//! Kakangkung paints web pages **only** with Ladybird ([ladybird.org](https://ladybird.org)).
//! There is no WebView2 / Chromium path. The host binary (`kakangkung-ladybird-host`) is the
//! Ladybird UI port under `engines/ladybird`, built preferably on
//! [Boat](https://boat.dev/dashboard). When the host is missing, the native chrome still runs and
//! page loads must fail closed until the host is installed.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use engine::{self, Kind, Resolution};

use crate::downloads::{remove_ladybird, upsert_ladybird, DownloadInfo, DownloadState};

/// How long a single host wait (create / navigate / screenshot / …) may block the UI thread.
const HOST_WAIT: Duration = Duration::from_secs(8);

/// Resolve the page engine (always Ladybird).
pub fn resolve_page_engine(requested_setting: &str) -> Resolution {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    engine::resolve(requested_setting, exe_dir.as_deref())
}

/// Path of the Ladybird host next to this executable, if any.
pub fn host_path() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    engine::find_ladybird_host(exe_dir.as_deref())
}

/// Pump Win32 messages so a cross-process child HWND (`CreateWindowEx` in the host) can finish
/// while this process waits on the host's stdout pipe. Without this, Self-test / navigate deadlock.
#[cfg(windows)]
fn pump_messages() {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(windows))]
fn pump_messages() {}

/// A running `kakangkung-ladybird-host` process speaking the JSON-lines protocol.
pub struct Host {
    child: Child,
    stdin: Mutex<ChildStdin>,
    lines: Receiver<String>,
    pub engine_id: String,
}

impl Host {
    /// Spawn the host for `profile_dir`. `parent_hwnd` is the HWND to parent page surfaces into (0 = none).
    pub fn spawn(host: &Path, profile_dir: &Path, parent_hwnd: usize) -> Result<Self, String> {
        let mut child = Command::new(host)
            .arg("--profile")
            .arg(profile_dir)
            .arg("--parent")
            .arg(format!("0x{parent_hwnd:x}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("spawn Ladybird host: {e}"))?;
        let stdout = child.stdout.take().ok_or_else(|| "Ladybird host: no stdout".to_string())?;
        let stdin = child.stdin.take().ok_or_else(|| "Ladybird host: no stdin".to_string())?;
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::Builder::new()
            .name("ladybird-host-stdout".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            if tx.send(line.clone()).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| format!("Ladybird host stdout thread: {e}"))?;
        let mut this = Self {
            child,
            stdin: Mutex::new(stdin),
            lines: rx,
            engine_id: String::new(),
        };
        let line = this.read_line_timeout(HOST_WAIT)?;
        this.engine_id = if line.contains("\"engine\":\"ladybird\"") {
            "ladybird".into()
        } else if line.contains("\"engine\":\"stub\"") {
            "stub".into()
        } else {
            "unknown".into()
        };
        if !line.contains("\"event\":\"ready\"") {
            return Err(format!("Ladybird host: expected ready event, got {line:?}"));
        }
        Ok(this)
    }

    pub fn send_line(&self, json: &str) -> Result<(), String> {
        let mut stdin = self.stdin.lock().map_err(|_| "Ladybird host stdin poisoned")?;
        writeln!(stdin, "{json}").map_err(|e| format!("Ladybird host write: {e}"))?;
        stdin.flush().map_err(|e| format!("Ladybird host flush: {e}"))
    }

    /// Apply spontaneous host events (downloads, …). Returns true when the line was consumed
    /// as a side effect and should not satisfy a wait for another event.
    fn apply_side_effect(line: &str) -> bool {
        if line.contains("\"event\":\"download_removed\"") {
            if let Some(id) = json_u64_field(line, "downloadId") {
                remove_ladybird(id);
                return true;
            }
        }
        if line.contains("\"event\":\"download\"") && line.contains("\"downloadId\"") {
            if let Some(info) = parse_download_event(line) {
                upsert_ladybird(info);
                return true;
            }
        }
        false
    }

    /// Drain queued host stdout without blocking. Returns true if any download side effect applied.
    pub fn poll_events(&self) -> bool {
        let mut any = false;
        loop {
            match self.lines.try_recv() {
                Ok(line) => {
                    if Self::apply_side_effect(&line) {
                        any = true;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        any
    }

    /// Read one host line, pumping Win32 messages so cross-process HWND parenting can proceed.
    pub fn read_line(&self) -> Result<String, String> {
        self.read_line_timeout(HOST_WAIT)
    }

    fn read_line_timeout(&self, budget: Duration) -> Result<String, String> {
        let deadline = Instant::now() + budget;
        loop {
            match self.lines.recv_timeout(Duration::from_millis(16)) {
                Ok(line) => return Ok(line),
                Err(RecvTimeoutError::Timeout) => {
                    pump_messages();
                    if Instant::now() >= deadline {
                        return Err("Ladybird host: timed out waiting for a line".into());
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("Ladybird host: stdout closed".into());
                }
            }
        }
    }

    /// Read host events until `load` with `done:true` for `want_id` (or any id when `want_id` is None).
    /// Returns the last `url` / history flags seen for that view (from `url` / `history` events).
    fn wait_load_done(&self, want_id: Option<i32>) -> Result<NavResult, String> {
        let mut result = NavResult::default();
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let line = self.read_line_timeout(deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50)))?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if let Some(id) = want_id {
                if !(line.contains(&format!("\"id\":{id}")) || line.contains(&format!("\"id\": {id}"))) {
                    continue;
                }
            }
            if line.contains("\"event\":\"url\"") {
                if let Some(u) = json_str_field(&line, "url") {
                    result.url = Some(u);
                }
            }
            if line.contains("\"event\":\"title\"") {
                if let Some(t) = json_str_field(&line, "title") {
                    result.title = Some(t);
                }
            }
            if line.contains("\"event\":\"history\"") {
                result.can_back = line.contains("\"back\":true");
                result.can_forward = line.contains("\"forward\":true");
            }
            if line.contains("\"event\":\"load\"") && line.contains("\"done\":true") {
                return Ok(result);
            }
        }
        Err("Ladybird host: no load done event".into())
    }

    pub fn create_view(&self, url: &str, x: i32, y: i32, w: i32, h: i32) -> Result<i32, String> {
        let escaped = url.replace('\\', "\\\\").replace('"', "\\\"");
        self.send_line(&format!(
            r#"{{"cmd":"create","url":"{escaped}","x":{x},"y":{y},"w":{w},"h":{h}}}"#
        ))?;
        let mut id = 0i32;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"created\"") {
                if let Some(rest) = line.split("\"id\":").nth(1) {
                    id = rest.trim().trim_matches(|c: char| !c.is_ascii_digit()).parse().unwrap_or(0);
                }
            }
            if id > 0 && line.contains("\"event\":\"load\"") && line.contains("\"done\":true") {
                return Ok(id);
            }
        }
        if id > 0 {
            // created without a trailing load line — still usable
            return Ok(id);
        }
        Err("Ladybird host: no created event".into())
    }

    pub fn navigate(&self, id: i32, url: &str) -> Result<NavResult, String> {
        let escaped = url.replace('\\', "\\\\").replace('"', "\\\"");
        self.send_line(&format!(r#"{{"cmd":"navigate","id":{id},"url":"{escaped}"}}"#))?;
        self.wait_load_done(Some(id))
    }

    pub fn reload(&self, id: i32) -> Result<NavResult, String> {
        self.send_line(&format!(r#"{{"cmd":"reload","id":{id}}}"#))?;
        self.wait_load_done(Some(id))
    }

    pub fn stop(&self, id: i32) -> Result<(), String> {
        self.send_line(&format!(r#"{{"cmd":"stop","id":{id}}}"#))
    }

    pub fn go_back(&self, id: i32) -> Result<NavResult, String> {
        self.send_line(&format!(r#"{{"cmd":"back","id":{id}}}"#))?;
        self.wait_load_done(Some(id))
    }

    pub fn go_forward(&self, id: i32) -> Result<NavResult, String> {
        self.send_line(&format!(r#"{{"cmd":"forward","id":{id}}}"#))?;
        self.wait_load_done(Some(id))
    }

    pub fn set_visible(&self, id: i32, visible: bool) -> Result<(), String> {
        self.send_line(&format!(
            r#"{{"cmd":"visible","id":{id},"visible":{}}}"#,
            if visible { "true" } else { "false" }
        ))
    }

    pub fn screenshot(&self, id: i32, path: &str) -> Result<(), String> {
        let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
        self.send_line(&format!(r#"{{"cmd":"screenshot","id":{id},"path":"{escaped}"}}"#))?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"screenshot\"")
                && (line.contains(&format!("\"id\":{id}")) || line.contains(&format!("\"id\": {id}")))
            {
                return if line.contains("\"ok\":true") {
                    Ok(())
                } else {
                    Err(format!("screenshot failed: {line}"))
                };
            }
        }
        Err("Ladybird host: no screenshot event".into())
    }

    pub fn print_page(&self, id: i32) -> Result<(), String> {
        self.send_line(&format!(r#"{{"cmd":"print","id":{id}}}"#))?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"print\"") {
                return Ok(());
            }
        }
        Err("Ladybird host: no print event".into())
    }

    pub fn a11y_tree(&self, id: i32) -> Result<String, String> {
        self.send_line(&format!(r#"{{"cmd":"a11y","id":{id}}}"#))?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"a11y\"") {
                return Ok(line);
            }
        }
        Err("Ladybird host: no a11y event".into())
    }

    pub fn set_bounds(&self, id: i32, x: i32, y: i32, w: i32, h: i32) -> Result<(), String> {
        self.send_line(&format!(r#"{{"cmd":"bounds","id":{id},"x":{x},"y":{y},"w":{w},"h":{h}}}"#))
    }

    pub fn destroy_view(&self, id: i32) -> Result<(), String> {
        self.send_line(&format!(r#"{{"cmd":"destroy","id":{id}}}"#))
    }

    /// Start a real LibWeb FileDownloader transfer to `path`.
    pub fn download_start(&self, url: &str, path: &str) -> Result<(), String> {
        let url = url.replace('\\', "\\\\").replace('"', "\\\"");
        let path = path.replace('\\', "\\\\").replace('"', "\\\"");
        self.send_line(&format!(r#"{{"cmd":"download","url":"{url}","path":"{path}"}}"#))?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"download_started\"") {
                return if line.contains("\"ok\":true") {
                    Ok(())
                } else {
                    Err(format!("download start failed: {line}"))
                };
            }
        }
        Err("Ladybird host: no download_started event".into())
    }

    pub fn download_action(&self, download_id: u64, action: &str) -> Result<(), String> {
        let action = action.replace('\\', "\\\\").replace('"', "\\\"");
        self.send_line(&format!(
            r#"{{"cmd":"download_action","downloadId":{download_id},"action":"{action}"}}"#
        ))
    }

    pub fn quit(&self) -> Result<(), String> {
        self.send_line(r#"{"cmd":"quit"}"#)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.quit();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Snapshot of navigation state after a host command that ends with `load` done.
#[derive(Debug, Default, Clone)]
pub struct NavResult {
    pub url: Option<String>,
    pub title: Option<String>,
    pub can_back: bool,
    pub can_forward: bool,
}

fn json_str_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let rest = line.split(&needle).nth(1)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn json_u64_field(line: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\":");
    let rest = line.split(&needle).nth(1)?;
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse().ok()
}

fn json_i64_field(line: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{key}\":");
    let rest = line.split(&needle).nth(1)?;
    let mut chars = rest.chars().peekable();
    let neg = chars.peek() == Some(&'-');
    if neg {
        chars.next();
    }
    let digits: String = chars.take_while(|c| c.is_ascii_digit()).collect();
    let n: i64 = digits.parse().ok()?;
    Some(if neg { -n } else { n })
}

fn parse_download_event(line: &str) -> Option<DownloadInfo> {
    let id = json_u64_field(line, "downloadId")?;
    let state = if line.contains("\"state\":\"done\"") {
        DownloadState::Done
    } else if line.contains("\"state\":\"paused\"") {
        DownloadState::Paused
    } else if line.contains("\"state\":\"stopped\"") {
        DownloadState::Stopped
    } else {
        DownloadState::InProgress
    };
    Some(DownloadInfo {
        id,
        url: json_str_field(line, "url").unwrap_or_default(),
        path: json_str_field(line, "path").unwrap_or_default(),
        received: json_i64_field(line, "received").unwrap_or(0),
        total: json_i64_field(line, "total").unwrap_or(-1),
        state,
        can_resume: line.contains("\"can_resume\":true"),
        started: 0,
        private: false,
    })
}

/// Human-readable status for Settings / diagnostics.
pub fn status_line(resolution: &Resolution) -> String {
    match (&resolution.host, resolution.active) {
        (Some(path), Kind::Ladybird) => format!(
            "Page engine: Ladybird ({})",
            path.to_str().unwrap_or("host")
        ),
        (None, Kind::Ladybird) => {
            "Page engine: Ladybird (host missing — build on Boat, see docs/LADYBIRD.md; WebView2 is disabled)".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_download_progress_line() {
        let line = r#"{"event":"download","downloadId":3,"url":"https://ex/a.bin","path":"C:\\a.bin","received":12,"total":-1,"state":"in_progress","can_resume":false}"#;
        let info = parse_download_event(line).expect("parse");
        assert_eq!(info.id, 3);
        assert_eq!(info.received, 12);
        assert_eq!(info.total, -1);
        assert_eq!(info.state, DownloadState::InProgress);
        assert_eq!(info.url, "https://ex/a.bin");
    }

    #[test]
    fn parse_download_done_and_id() {
        let line = r#"{"event":"download","downloadId":9,"url":"u","path":"p","received":100,"total":100,"state":"done","can_resume":false}"#;
        let info = parse_download_event(line).unwrap();
        assert_eq!(info.state, DownloadState::Done);
        assert_eq!(json_u64_field(r#"{"event":"download_removed","downloadId":9}"#, "downloadId"), Some(9));
    }
}
