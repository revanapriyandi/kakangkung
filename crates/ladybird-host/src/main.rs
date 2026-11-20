//! Kakangkung Ladybird host — Rust stub.
//!
//! Speaks the same stdin/stdout JSON-lines protocol as `engines/ladybird` so the browser can run
//! without WebView2 while the real LibWeb-linked host is built on Boat
//! (https://boat.dev/dashboard). This process does not paint HTML; it acknowledges navigation and
//! title/url/load events so the native chrome keeps working.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};

struct View {
    url: String,
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
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--profile" => profile = args.next().unwrap_or_default(),
            "--parent" => {
                let _ = args.next();
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
            views.insert(id, View { url: url.clone() });
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
        if has_cmd(&line, "bounds") || has_cmd(&line, "reload") || has_cmd(&line, "stop") {
            continue;
        }
    }
}
