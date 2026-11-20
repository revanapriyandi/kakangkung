//! Ladybird page-engine host: discovery and process lifecycle.
//!
//! Kakangkung paints web pages **only** with Ladybird ([ladybird.org](https://ladybird.org)).
//! There is no WebView2 / Chromium path. The host binary (`kakangkung-ladybird-host`) is the
//! Ladybird UI port under `engines/ladybird`, built preferably on
//! [Boat](https://boat.dev/dashboard). When the host is missing, the native chrome still runs and
//! page loads must fail closed until the host is installed.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use engine::{self, Kind, Resolution};

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

/// A running `kakangkung-ladybird-host` process speaking the JSON-lines protocol.
pub struct Host {
    child: Child,
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
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
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| format!("Ladybird host ready: {e}"))?;
        let engine_id = if line.contains("\"engine\":\"ladybird\"") {
            "ladybird".into()
        } else if line.contains("\"engine\":\"stub\"") {
            "stub".into()
        } else {
            "unknown".into()
        };
        if !line.contains("\"event\":\"ready\"") {
            return Err(format!("Ladybird host: expected ready event, got {line:?}"));
        }
        Ok(Self { child, stdin: Mutex::new(stdin), stdout: Mutex::new(reader), engine_id })
    }

    pub fn send_line(&self, json: &str) -> Result<(), String> {
        let mut stdin = self.stdin.lock().map_err(|_| "Ladybird host stdin poisoned")?;
        writeln!(stdin, "{json}").map_err(|e| format!("Ladybird host write: {e}"))?;
        stdin.flush().map_err(|e| format!("Ladybird host flush: {e}"))
    }

    pub fn read_line(&self) -> Result<String, String> {
        let mut stdout = self.stdout.lock().map_err(|_| "Ladybird host stdout poisoned")?;
        let mut line = String::new();
        stdout.read_line(&mut line).map_err(|e| format!("Ladybird host read: {e}"))?;
        Ok(line)
    }

    /// Read host events until `load` with `done:true` for `want_id` (or any id when `want_id` is None).
    /// Returns the last `url` / history flags seen for that view (from `url` / `history` events).
    fn wait_load_done(&self, want_id: Option<i32>) -> Result<NavResult, String> {
        let mut result = NavResult::default();
        for _ in 0..32 {
            let line = self.read_line()?;
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
        for _ in 0..32 {
            let line = self.read_line()?;
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
        for _ in 0..32 {
            let line = self.read_line()?;
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
        for _ in 0..16 {
            let line = self.read_line()?;
            if line.contains("\"event\":\"print\"") {
                return Ok(());
            }
        }
        Err("Ladybird host: no print event".into())
    }

    pub fn a11y_tree(&self, id: i32) -> Result<String, String> {
        self.send_line(&format!(r#"{{"cmd":"a11y","id":{id}}}"#))?;
        for _ in 0..16 {
            let line = self.read_line()?;
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
