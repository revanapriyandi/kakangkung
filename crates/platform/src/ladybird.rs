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

use std::cell::RefCell;

use crate::downloads::{remove_ladybird, upsert_ladybird, DownloadInfo, DownloadState};

thread_local! {
    /// Spontaneous `page_ipc` events from the host (password manager, …), drained by the UI.
    static PAGE_IPC: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
}

/// Take queued page IPC messages `(ladybird_view_id, message)`.
pub fn take_page_ipc() -> Vec<(i32, String)> {
    PAGE_IPC.with(|q| std::mem::take(&mut *q.borrow_mut()))
}

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
    /// OS process id of the Ladybird host (for task manager / memory diagnostics).
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

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

    /// Apply spontaneous host events (downloads, page IPC, …). Returns true when the line was
    /// consumed as a side effect and should not satisfy a wait for another event.
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
        if line.contains("\"event\":\"page_ipc\"") {
            if let (Some(id), Some(msg)) = (json_i64_field(line, "id").map(|n| n as i32), json_str_field(line, "message")) {
                PAGE_IPC.with(|q| q.borrow_mut().push((id, msg)));
                return true;
            }
        }
        false
    }

    /// Drain queued host stdout without blocking. Returns true if any side effect applied.
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

    /// Run JavaScript in the page (LibWeb `js_console_input` / `run_javascript` path).
    pub fn eval(&self, id: i32, js: &str) -> Result<String, String> {
        let escaped = js
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");
        self.send_line(&format!(r#"{{"cmd":"eval","id":{id},"js":"{escaped}"}}"#))?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"eval\"")
                && (line.contains(&format!("\"id\":{id}")) || line.contains(&format!("\"id\": {id}")))
            {
                if !line.contains("\"ok\":true") {
                    return Err(format!("eval failed: {line}"));
                }
                return Ok(json_str_field(&line, "result").unwrap_or_default());
            }
        }
        Err("Ladybird host: no eval event".into())
    }

    /// Inject the password-manager page script (assets/passwords.js) with a Ladybird IPC bridge.
    pub fn inject_passwords(&self, id: i32, script: &str) -> Result<(), String> {
        let bootstrap = format!(
            "(function(){{if(window.top!==window||window.__kkPwFill)return;\
window.ipc=window.ipc||{{postMessage:function(m){{try{{console.log('kk-pw:'+m)}}catch(e){{}}}}}};}})();\n{script}"
        );
        self.eval(id, &bootstrap).map(|_| ())
    }

    /// KB-19: push Settings site.* ask/block defaults into LibWeb PermissionManager.
    pub fn permissions_defaults(&self, geolocation_block: bool, microphone_block: bool, notifications_block: bool) -> Result<(), String> {
        let geo = if geolocation_block { "block" } else { "ask" };
        let mic = if microphone_block { "block" } else { "ask" };
        let notif = if notifications_block { "block" } else { "ask" };
        self.send_line(&format!(
            r#"{{"cmd":"permissions_defaults","geolocation":"{geo}","microphone":"{mic}","notifications":"{notif}"}}"#
        ))?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"permissions_defaults\"") {
                return if line.contains("\"ok\":true") {
                    Ok(())
                } else {
                    Err(format!("permissions_defaults failed: {line}"))
                };
            }
        }
        Err("Ladybird host: no permissions_defaults event".into())
    }

    /// KB-22: Ladybird ProcessManager snapshots as a JSON array text.
    pub fn processes_json(&self) -> Result<String, String> {
        self.send_line(r#"{"cmd":"processes"}"#)?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"processes\"") {
                if !line.contains("\"ok\":true") {
                    return Err(format!("processes failed: {line}"));
                }
                if let Some(start) = line.find("\"processes\":") {
                    let rest = &line[start + "\"processes\":".len()..];
                    let rest = rest.trim_start();
                    if rest.starts_with('[') {
                        let mut depth = 0i32;
                        for (i, c) in rest.char_indices() {
                            match c {
                                '[' => depth += 1,
                                ']' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        return Ok(rest[..=i].to_string());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                return Ok("[]".into());
            }
        }
        Err("Ladybird host: no processes event".into())
    }

    /// KB-19: list non-default site permission decisions as JSON array text.
    pub fn permissions_list(&self) -> Result<String, String> {
        self.send_line(r#"{"cmd":"permissions_list"}"#)?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"permissions_list\"") {
                if !line.contains("\"ok\":true") {
                    return Err(format!("permissions_list failed: {line}"));
                }
                // Extract the permissions array payload.
                if let Some(start) = line.find("\"permissions\":") {
                    let rest = &line[start + "\"permissions\":".len()..];
                    let rest = rest.trim_start();
                    if rest.starts_with('[') {
                        let mut depth = 0i32;
                        for (i, c) in rest.char_indices() {
                            match c {
                                '[' => depth += 1,
                                ']' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        return Ok(rest[..=i].to_string());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                return Ok("[]".into());
            }
        }
        Err("Ladybird host: no permissions_list event".into())
    }

    /// KB-19: reset one permission kind (`geolocation` / `microphone` / `notifications`) or all when `kind` is empty.
    pub fn permissions_reset(&self, origin: &str, kind: &str) -> Result<(), String> {
        let origin = origin.replace('\\', "\\\\").replace('"', "\\\"");
        let kind = kind.replace('\\', "\\\\").replace('"', "\\\"");
        self.send_line(&format!(r#"{{"cmd":"permissions_reset","origin":"{origin}","kind":"{kind}"}}"#))?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"permissions_reset\"") {
                return if line.contains("\"ok\":true") {
                    Ok(())
                } else {
                    Err(format!("permissions_reset failed: {line}"))
                };
            }
        }
        Err("Ladybird host: no permissions_reset event".into())
    }

    /// KB-12: find-in-page. Empty query or `end` clears highlights.
    /// Returns (active_match 1-based, match_count).
    pub fn find(
        &self,
        id: i32,
        query: &str,
        match_case: bool,
        forward: bool,
        end: bool,
    ) -> Result<(i32, i32), String> {
        let escaped = query.replace('\\', "\\\\").replace('"', "\\\"");
        self.send_line(&format!(
            r#"{{"cmd":"find","id":{id},"query":"{escaped}","matchCase":{},"forward":{},"end":{}}}"#,
            if match_case { "true" } else { "false" },
            if forward { "true" } else { "false" },
            if end { "true" } else { "false" },
        ))?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"find\"")
                && (line.contains(&format!("\"id\":{id}")) || line.contains(&format!("\"id\": {id}")))
            {
                if !line.contains("\"ok\":true") {
                    return Err(format!("find failed: {line}"));
                }
                let active = json_i64_field(&line, "activeMatch").unwrap_or(0) as i32;
                let count = json_i64_field(&line, "matchCount").unwrap_or(0) as i32;
                return Ok((active, count));
            }
        }
        Err("Ladybird host: no find event".into())
    }

    /// KB-11: clear LibWeb cookies/site data and/or HTTP cache (`since_ms` 0 = all time).
    pub fn clear_data(&self, cookies: bool, cache: bool, since_ms: i64) -> Result<(), String> {
        self.send_line(&format!(
            r#"{{"cmd":"clear_data","cookies":{},"cache":{},"since":{since_ms}}}"#,
            if cookies { "true" } else { "false" },
            if cache { "true" } else { "false" },
        ))?;
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"clear_data\"") {
                return if line.contains("\"ok\":true") {
                    Ok(())
                } else {
                    Err(format!("clear_data failed: {line}"))
                };
            }
        }
        Err("Ladybird host: no clear_data event".into())
    }

    /// KB-7: push adblock on/off + allowlist into LibWeb ContentFilter/ContentBlocker.
    pub fn adblock_configure(&self, enabled: bool, allowlist: &str) -> Result<(), String> {
        let allow = allowlist.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        self.send_line(&format!(
            r#"{{"cmd":"adblock","enabled":{},"allowlist":"{allow}"}}"#,
            if enabled { "true" } else { "false" }
        ))?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"adblock\"") {
                return if line.contains("\"ok\":true") {
                    Ok(())
                } else {
                    Err(format!("adblock configure failed: {line}"))
                };
            }
        }
        Err("Ladybird host: no adblock event".into())
    }

    /// Page zoom. `None` resets to 100%; `Some(f)` multiplies the current factor (clamped 0.25…5).
    /// Matches `platform::zoom` semantics for menu Ctrl+/- / reset.
    pub fn zoom(&self, id: i32, factor: Option<f64>) -> Result<f64, String> {
        let cmd = match factor {
            Some(f) => format!(r#"{{"cmd":"zoom","id":{id},"factor":{f}}}"#),
            None => format!(r#"{{"cmd":"zoom","id":{id}}}"#),
        };
        self.send_line(&cmd)?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"zoom\"")
                && (line.contains(&format!("\"id\":{id}")) || line.contains(&format!("\"id\": {id}")))
            {
                if !line.contains("\"ok\":true") {
                    return Err(format!("zoom failed: {line}"));
                }
                return Ok(json_f64_field(&line, "factor").unwrap_or(1.0));
            }
        }
        Err("Ladybird host: no zoom event".into())
    }

    /// Set absolute page zoom (Settings default / tab create).
    pub fn zoom_absolute(&self, id: i32, absolute: f64) -> Result<f64, String> {
        self.send_line(&format!(r#"{{"cmd":"zoom","id":{id},"absolute":{absolute}}}"#))?;
        let deadline = Instant::now() + HOST_WAIT;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(50));
            let line = self.read_line_timeout(remain)?;
            if Self::apply_side_effect(&line) {
                continue;
            }
            if line.contains("\"event\":\"zoom\"")
                && (line.contains(&format!("\"id\":{id}")) || line.contains(&format!("\"id\": {id}")))
            {
                if !line.contains("\"ok\":true") {
                    return Err(format!("zoom failed: {line}"));
                }
                return Ok(json_f64_field(&line, "factor").unwrap_or(absolute));
            }
        }
        Err("Ladybird host: no zoom event".into())
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

fn json_f64_field(line: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{key}\":");
    let rest = line.split(&needle).nth(1)?;
    let num: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == 'e' || *c == 'E' || *c == '+')
        .collect();
    num.parse().ok()
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
