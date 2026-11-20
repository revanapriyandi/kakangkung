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
    #[cfg(windows)]
    surface: Option<hwnd::Surface>,
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
            #[cfg(windows)]
            let surface = hwnd::Surface::create(parent, x, y, w, h, &url);
            #[cfg(not(windows))]
            let _ = (parent, x, y, w, h);
            views.insert(
                id,
                View {
                    url: url.clone(),
                    #[cfg(windows)]
                    surface,
                },
            );
            emit(&format!(r#"{{"event":"created","id":{id}}}"#));
            emit(&format!(r#"{{"event":"url","id":{id},"url":"{url}"}}"#));
            let title = if url == "about:blank" { "New Tab" } else { &url };
            emit(&format!(r#"{{"event":"title","id":{id},"title":"{title}"}}"#));
            emit(&format!(r#"{{"event":"load","id":{id},"done":true}}"#));
            continue;
        }
        if has_cmd(&line, "navigate") {
            if let (Some(id), Some(url)) = (field_int(&line, "id"), field_str(&line, "url")) {
                if let Some(v) = views.get_mut(&id) {
                    v.url = url.clone();
                    #[cfg(windows)]
                    if let Some(s) = v.surface.as_mut() {
                        s.set_label(&url);
                    }
                    emit(&format!(r#"{{"event":"url","id":{id},"url":"{url}"}}"#));
                    emit(&format!(r#"{{"event":"title","id":{id},"title":"{url}"}}"#));
                    emit(&format!(r#"{{"event":"load","id":{id},"done":true}}"#));
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
        if has_cmd(&line, "reload") {
            if let Some(id) = field_int(&line, "id") {
                if let Some(v) = views.get(&id) {
                    let url = v.url.clone();
                    emit(&format!(r#"{{"event":"url","id":{id},"url":"{url}"}}"#));
                    emit(&format!(r#"{{"event":"load","id":{id},"done":true}}"#));
                }
            }
            continue;
        }
        if has_cmd(&line, "stop") {
            continue;
        }
    }
}
