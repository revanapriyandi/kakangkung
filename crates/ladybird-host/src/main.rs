//! Kakangkung Ladybird host — Rust stub.
//!
//! Speaks the same stdin/stdout JSON-lines protocol as `engines/ladybird` so the browser can run
//! without WebView2 while the real LibWeb-linked host is built on Boat
//! (https://boat.dev/dashboard). On Windows this process parents a child HWND into Kakangkung and
//! paints a stub page; LibWeb paint replaces that surface when HAVE_LADYBIRD is linked.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};

#[cfg(windows)]
mod hwnd;

struct View {
    url: String,
    back: Vec<String>,
    forward: Vec<String>,
    #[cfg(windows)]
    surface: Option<hwnd::Surface>,
}

/// Escape a string for embedding in a JSON string value (protocol lines are hand-built).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

impl View {
    fn paint_label(&mut self) {
        #[cfg(windows)]
        if let Some(s) = self.surface.as_mut() {
            s.set_label(&self.url);
        }
    }

    fn emit_nav(&self, id: i32) {
        let url = json_escape(&self.url);
        let title = if self.url == "about:blank" {
            "New Tab".to_string()
        } else {
            json_escape(&self.url)
        };
        emit(&format!(r#"{{"event":"url","id":{id},"url":"{url}"}}"#));
        emit(&format!(r#"{{"event":"title","id":{id},"title":"{title}"}}"#));
        emit(&format!(
            r#"{{"event":"history","id":{id},"back":{},"forward":{}}}"#,
            if self.back.is_empty() { "false" } else { "true" },
            if self.forward.is_empty() { "false" } else { "true" }
        ));
        // Clients (`wait_load_done`, MCP) block until done:true — always emit it, even for no-op nav.
        emit(&format!(r#"{{"event":"load","id":{id},"done":true}}"#));
    }

    fn go_to(&mut self, url: String, record: bool) {
        if record && self.url != url {
            self.back.push(std::mem::replace(&mut self.url, url));
            self.forward.clear();
        } else {
            self.url = url;
        }
        self.paint_label();
    }
}

fn emit(json: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{json}");
    let _ = out.flush();
}

fn field_int(line: &str, key: &str) -> Option<i32> {
    let needle = format!("\"{key}\":");
    let rest = line.split(&needle).nth(1)?;
    rest.trim()
        .trim_start_matches(|c: char| !c.is_ascii_digit() && c != '-')
        .split(|c: char| !c.is_ascii_digit() && c != '-')
        .next()
        .and_then(|s| s.parse().ok())
}

fn field_str(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let rest = line.split(&needle).nth(1)?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn has_cmd(line: &str, cmd: &str) -> bool {
    line.contains(&format!("\"cmd\":\"{cmd}\""))
}

/// Optionally parent a stub HWND without blocking stdin.
///
/// Cross-process `CreateWindowEx(WS_CHILD, parent)` waits for the parent's message pump. Doing that
/// on the stdin thread deadlocks Kakangkung's next `send_line`. HWND is also `!Send`, so we cannot
/// create on a helper and return the surface here. Fire-and-forget a helper that owns (and leaks)
/// the surface if create ever completes; protocol already acked `load` with `surface: None`.
#[cfg(windows)]
fn spawn_surface_best_effort(parent: usize, x: i32, y: i32, w: i32, h: i32, url: &str) {
    if parent == 0 {
        return;
    }
    let url = url.to_string();
    let _ = std::thread::Builder::new()
        .name("ladybird-stub-hwnd".into())
        .spawn(move || {
            if let Some(surface) = hwnd::Surface::create(parent, x, y, w, h, &url) {
                // Keep the HWND alive for the process lifetime; we are not tracking it on View.
                std::mem::forget(surface);
            }
        });
}

fn main() {
    let mut profile = String::new();
    let mut parent: usize = 0;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--profile" => profile = args.next().unwrap_or_default(),
            "--parent" => {
                let raw = args.next().unwrap_or_default();
                parent = usize::from_str_radix(raw.trim_start_matches("0x").trim_start_matches("0X"), 16)
                    .or_else(|_| raw.parse())
                    .unwrap_or(0);
            }
            "--version" => {
                println!("kakangkung-ladybird-host engine=stub");
                return;
            }
            _ => {}
        }
    }
    let _ = profile;

    let mut views: HashMap<i32, View> = HashMap::new();
    let mut next_id = 1i32;
    emit(r#"{"event":"ready","engine":"stub"}"#);

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if has_cmd(&line, "quit") {
            break;
        }
        if has_cmd(&line, "create") {
            let id = next_id;
            next_id += 1;
            let url = field_str(&line, "url").unwrap_or_else(|| "about:blank".into());
            let x = field_int(&line, "x").unwrap_or(0);
            let y = field_int(&line, "y").unwrap_or(0);
            let w = field_int(&line, "w").unwrap_or(800);
            let h = field_int(&line, "h").unwrap_or(600);
            // Protocol first: Kakangkung's create_view waits on `created` + `load`. Never block
            // those behind CreateWindowEx — cross-process WS_CHILD parenting can stall until the
            // parent pumps, and while we stall we stop reading stdin so Kakangkung's next
            // send_line fills the pipe and deadlocks the UI thread (Self-test hung for hours).
            emit(&format!(r#"{{"event":"created","id":{id}}}"#));
            let v = View {
                url: url.clone(),
                back: vec![],
                forward: vec![],
                #[cfg(windows)]
                surface: None,
            };
            v.emit_nav(id);
            views.insert(id, v);
            #[cfg(windows)]
            spawn_surface_best_effort(parent, x, y, w, h, &url);
            #[cfg(not(windows))]
            let _ = (parent, x, y, w, h);
            continue;
        }
        if has_cmd(&line, "navigate") {
            if let (Some(id), Some(url)) = (field_int(&line, "id"), field_str(&line, "url")) {
                if let Some(v) = views.get_mut(&id) {
                    v.go_to(url, true);
                    v.emit_nav(id);
                }
            }
            continue;
        }
        if has_cmd(&line, "back") {
            if let Some(id) = field_int(&line, "id") {
                if let Some(v) = views.get_mut(&id) {
                    if let Some(prev) = v.back.pop() {
                        v.forward.push(std::mem::replace(&mut v.url, prev));
                        v.paint_label();
                    }
                    // Always ack so UI/MCP waiters never hang on empty history.
                    v.emit_nav(id);
                }
            }
            continue;
        }
        if has_cmd(&line, "forward") {
            if let Some(id) = field_int(&line, "id") {
                if let Some(v) = views.get_mut(&id) {
                    if let Some(next) = v.forward.pop() {
                        v.back.push(std::mem::replace(&mut v.url, next));
                        v.paint_label();
                    }
                    v.emit_nav(id);
                }
            }
            continue;
        }
        if has_cmd(&line, "destroy") {
            if let Some(id) = field_int(&line, "id") {
                views.remove(&id);
            }
            continue;
        }
        if has_cmd(&line, "bounds") {
            if let Some(id) = field_int(&line, "id") {
                let x = field_int(&line, "x").unwrap_or(0);
                let y = field_int(&line, "y").unwrap_or(0);
                let w = field_int(&line, "w").unwrap_or(800);
                let h = field_int(&line, "h").unwrap_or(600);
                if let Some(v) = views.get_mut(&id) {
                    #[cfg(windows)]
                    if let Some(s) = v.surface.as_mut() {
                        s.set_bounds(x, y, w, h);
                    }
                    #[cfg(not(windows))]
                    let _ = (v, x, y, w, h);
                }
            }
            continue;
        }
        if has_cmd(&line, "visible") {
            if let Some(id) = field_int(&line, "id") {
                let on = line.contains("\"visible\":true") || line.contains("\"visible\":1");
                if let Some(v) = views.get_mut(&id) {
                    #[cfg(windows)]
                    if let Some(s) = v.surface.as_mut() {
                        s.set_visible(on);
                    }
                    #[cfg(not(windows))]
                    let _ = (v, on);
                }
            }
            continue;
        }
        if has_cmd(&line, "reload") {
            if let Some(id) = field_int(&line, "id") {
                if let Some(v) = views.get_mut(&id) {
                    #[cfg(windows)]
                    if let Some(s) = v.surface.as_mut() {
                        s.set_label(&v.url);
                    }
                    v.emit_nav(id);
                }
            }
            continue;
        }
        if has_cmd(&line, "stop") {
            if let Some(id) = field_int(&line, "id") {
                emit(&format!(r#"{{"event":"load","id":{id},"done":true}}"#));
            }
            continue;
        }
        if has_cmd(&line, "screenshot") {
            if let Some(id) = field_int(&line, "id") {
                let path = field_str(&line, "path").unwrap_or_else(|| format!("kakangkung-{id}.png"));
                let ok = views.get(&id).map(|v| {
                    #[cfg(windows)]
                    {
                        v.surface.as_ref().map(|s| s.capture_png(&path).is_ok()).unwrap_or(false)
                    }
                    #[cfg(not(windows))]
                    {
                        let _ = (&v, &path);
                        // Minimal valid 1×1 PNG so agents can exercise the tool off Windows.
                        let png: &[u8] = &[
                            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
                            0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
                            0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8,
                            0xcf, 0xc0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xfe, 0xd4, 0xef, 0x00, 0x00,
                            0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
                        ];
                        std::fs::write(&path, png).is_ok()
                    }
                }).unwrap_or(false);
                let path_esc = json_escape(&path);
                emit(&format!(
                    r#"{{"event":"screenshot","id":{id},"path":"{path_esc}","ok":{}}}"#,
                    if ok { "true" } else { "false" }
                ));
            }
            continue;
        }
        if has_cmd(&line, "print") {
            if let Some(id) = field_int(&line, "id") {
                // LibWeb print lands with HAVE_LADYBIRD; stub acknowledges the request.
                emit(&format!(r#"{{"event":"print","id":{id},"ok":false,"reason":"stub"}}"#));
            }
            continue;
        }
        if has_cmd(&line, "ext_reply") {
            // Stub has no page JS to resolve; still ack so Kakangkung waiters complete.
            if let Some(id) = field_int(&line, "id") {
                emit(&format!(r#"{{"event":"ext_reply","id":{id},"ok":true}}"#));
            }
            continue;
        }
        if has_cmd(&line, "ext_drain") {
            // No content-script queue in the stub host.
            continue;
        }
        if has_cmd(&line, "a11y") {
            if let Some(id) = field_int(&line, "id") {
                emit(&format!(
                    r#"{{"event":"a11y","id":{id},"tree":null,"ok":false,"reason":"a11y requires LibWeb-linked host"}}"#
                ));
            }
            continue;
        }
        if has_cmd(&line, "dom") {
            if let Some(id) = field_int(&line, "id") {
                emit(&format!(
                    r#"{{"event":"dom","id":{id},"tree":null,"ok":false,"reason":"dom requires LibWeb-linked host"}}"#
                ));
            }
            continue;
        }
        if has_cmd(&line, "console") {
            if let Some(id) = field_int(&line, "id") {
                emit(&format!(
                    r#"{{"event":"console","id":{id},"messages":null,"ok":false,"reason":"console requires LibWeb-linked host"}}"#
                ));
            }
            continue;
        }
        if has_cmd(&line, "eval") {
            if let Some(id) = field_int(&line, "id") {
                let _ = field_str(&line, "js");
                emit(&format!(
                    r#"{{"event":"eval","id":{id},"ok":false,"result":"eval requires LibWeb-linked host"}}"#
                ));
            }
            continue;
        }
        if has_cmd(&line, "capabilities") {
            if let Some(id) = field_int(&line, "id") {
                emit(&format!(
                    r#"{{"event":"capabilities","id":{id},"ok":false,"caps":null,"reason":"capabilities requires LibWeb-linked host"}}"#
                ));
            }
            continue;
        }
        // Anything else (adblock, permissions_*, zoom, find, processes, clear_data, download…)
        // needs LibWeb. Answer at once: Kakangkung blocks its UI thread up to 8 s per unanswered
        // command, which froze start-up for 16 s on adblock + permissions alone.
        if let Some(reply) = unsupported_reply(&line) {
            emit(&reply);
        }
    }
}

/// The `ok:false` answer to a command the stub cannot do, named like the event Kakangkung waits
/// for (the command name; `download` is answered by `download_started`).
fn unsupported_reply(line: &str) -> Option<String> {
    let cmd = field_str(line, "cmd")?;
    let event = if cmd == "download" { "download_started".to_string() } else { json_escape(&cmd) };
    let id = field_int(line, "id").map(|id| format!(r#","id":{id}"#)).unwrap_or_default();
    Some(format!(r#"{{"event":"{event}"{id},"ok":false,"reason":"requires LibWeb-linked host"}}"#))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escape_quotes_and_controls() {
        assert_eq!(json_escape(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(json_escape("a\nb"), r#"a\nb"#);
    }

    #[test]
    fn field_parsers() {
        let line = r#"{"cmd":"navigate","id":3,"url":"https://example.com/x"}"#;
        assert!(has_cmd(line, "navigate"));
        assert_eq!(field_int(line, "id"), Some(3));
        assert_eq!(field_str(line, "url").as_deref(), Some("https://example.com/x"));
    }

    #[test]
    fn unsupported_commands_are_answered_at_once() {
        assert_eq!(
            unsupported_reply(r#"{"cmd":"adblock","enabled":true}"#).as_deref(),
            Some(r#"{"event":"adblock","ok":false,"reason":"requires LibWeb-linked host"}"#)
        );
        assert_eq!(
            unsupported_reply(r#"{"cmd":"zoom","id":2,"factor":1.1}"#).as_deref(),
            Some(r#"{"event":"zoom","id":2,"ok":false,"reason":"requires LibWeb-linked host"}"#)
        );
        assert!(unsupported_reply(r#"{"cmd":"download","url":"https://e/x"}"#).unwrap().starts_with(r#"{"event":"download_started""#));
        assert_eq!(unsupported_reply("not json"), None);
    }
}
