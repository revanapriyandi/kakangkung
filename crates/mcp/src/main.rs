//! Kakangkung MCP server — stdio JSON-RPC for AI agents.
//!
//! Spawns (or reuses) `kakangkung-ladybird-host` and exposes tools for tabs, navigation,
//! screenshots, accessibility snapshots, print, and eval. No WebView2 / Chrome.
//!
//! Transport: newline-delimited JSON-RPC 2.0 on stdin/stdout (MCP tools/list + tools/call).

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use serde_json::{json, Value};

struct Host {
    child: Child,
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
    active: i32,
}

impl Host {
    fn spawn() -> Result<Self, String> {
        let host = find_host().ok_or_else(|| {
            "kakangkung-ladybird-host not found (build -p ladybird-host or set KAKANGKUNG_LADYBIRD_HOST)".to_string()
        })?;
        let profile = std::env::temp_dir().join(format!("kakangkung-mcp-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&profile);
        let mut child = Command::new(&host)
            .arg("--profile")
            .arg(&profile)
            .arg("--parent")
            .arg("0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("spawn host: {e}"))?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if !line.contains("\"event\":\"ready\"") {
            return Err(format!("expected ready, got {line}"));
        }
        Ok(Self {
            child,
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(reader),
            active: 0,
        })
    }

    fn send(&self, json: &str) -> Result<(), String> {
        let mut stdin = self.stdin.lock().map_err(|_| "stdin poisoned")?;
        writeln!(stdin, "{json}").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())
    }

    fn read(&self) -> Result<String, String> {
        let mut stdout = self.stdout.lock().map_err(|_| "stdout poisoned")?;
        let mut line = String::new();
        stdout.read_line(&mut line).map_err(|e| e.to_string())?;
        Ok(line)
    }

    fn wait_event(&self, event: &str, id: Option<i32>) -> Result<String, String> {
        for _ in 0..48 {
            let line = self.read()?;
            if !line.contains(&format!("\"event\":\"{event}\"")) {
                continue;
            }
            if let Some(id) = id {
                if line.contains(&format!("\"id\":{id}")) || line.contains(&format!("\"id\": {id}")) {
                    return Ok(line);
                }
            } else {
                return Ok(line);
            }
        }
        Err(format!("no {event} event"))
    }

    fn create(&mut self, url: &str) -> Result<i32, String> {
        let escaped = url.replace('\\', "\\\\").replace('"', "\\\"");
        self.send(&format!(r#"{{"cmd":"create","url":"{escaped}","x":0,"y":0,"w":1280,"h":800}}"#))?;
        let mut id = 0i32;
        for _ in 0..32 {
            let line = self.read()?;
            if line.contains("\"event\":\"created\"") {
                if let Some(rest) = line.split("\"id\":").nth(1) {
                    id = rest.trim().trim_matches(|c: char| !c.is_ascii_digit()).parse().unwrap_or(0);
                }
            }
            if id > 0 && line.contains("\"event\":\"load\"") && line.contains("\"done\":true") {
                self.active = id;
                return Ok(id);
            }
        }
        if id > 0 {
            self.active = id;
            return Ok(id);
        }
        Err("create failed".into())
    }

    fn navigate(&self, id: i32, url: &str) -> Result<(), String> {
        let escaped = url.replace('\\', "\\\\").replace('"', "\\\"");
        self.send(&format!(r#"{{"cmd":"navigate","id":{id},"url":"{escaped}"}}"#))?;
        let _ = self.wait_event("load", Some(id))?;
        Ok(())
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.send(r#"{"cmd":"quit"}"#);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn find_host() -> Option<PathBuf> {
    engine::find_ladybird_host(
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from))
            .as_deref(),
    )
}

fn tools() -> Value {
    json!([
        {"name":"browser_open","description":"Create a Ladybird page view and navigate to a URL","inputSchema":{"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}},
        {"name":"browser_navigate","description":"Navigate the active view","inputSchema":{"type":"object","properties":{"url":{"type":"string"},"id":{"type":"integer"}},"required":["url"]}},
        {"name":"browser_back","description":"Go back in history","inputSchema":{"type":"object","properties":{"id":{"type":"integer"}}}},
        {"name":"browser_forward","description":"Go forward in history","inputSchema":{"type":"object","properties":{"id":{"type":"integer"}}}},
        {"name":"browser_reload","description":"Reload the active view","inputSchema":{"type":"object","properties":{"id":{"type":"integer"}}}},
        {"name":"browser_screenshot","description":"Capture the page surface to a PNG path","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"id":{"type":"integer"}},"required":["path"]}},
        {"name":"browser_a11y","description":"Accessibility tree snapshot (document stub until LibWeb)","inputSchema":{"type":"object","properties":{"id":{"type":"integer"}}}},
        {"name":"browser_print","description":"Request print (LibWeb when linked)","inputSchema":{"type":"object","properties":{"id":{"type":"integer"}}}},
        {"name":"browser_eval","description":"Evaluate JavaScript in the page (requires LibWeb host)","inputSchema":{"type":"object","properties":{"js":{"type":"string"},"id":{"type":"integer"}},"required":["js"]}},
        {"name":"browser_info","description":"Active view id and engine status","inputSchema":{"type":"object","properties":{}}}
    ])
}

fn ok_text(id: Value, text: String) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":text}],"isError":false}})
}

fn err_rpc(id: Option<Value>, code: i64, message: &str) -> Option<Value> {
    id.map(|i| json!({"jsonrpc":"2.0","id":i,"error":{"code":code,"message":message}}))
}

fn call_tool(host: &mut Host, name: &str, args: &Value) -> Result<String, String> {
    let id = args.get("id").and_then(|v| v.as_i64()).unwrap_or(host.active as i64) as i32;
    match name {
        "browser_open" => {
            let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("about:blank");
            let vid = host.create(url)?;
            Ok(format!("opened view {vid} → {url}"))
        }
        "browser_navigate" => {
            let url = args.get("url").and_then(|v| v.as_str()).ok_or("url required")?;
            let id = if id > 0 { id } else { host.active };
            if id <= 0 {
                let vid = host.create(url)?;
                return Ok(format!("opened view {vid} → {url}"));
            }
            host.navigate(id, url)?;
            Ok(format!("navigated {id} → {url}"))
        }
        "browser_back" => {
            host.send(&format!(r#"{{"cmd":"back","id":{id}}}"#))?;
            let _ = host.wait_event("load", Some(id))?;
            Ok(format!("back on {id}"))
        }
        "browser_forward" => {
            host.send(&format!(r#"{{"cmd":"forward","id":{id}}}"#))?;
            let _ = host.wait_event("load", Some(id))?;
            Ok(format!("forward on {id}"))
        }
        "browser_reload" => {
            host.send(&format!(r#"{{"cmd":"reload","id":{id}}}"#))?;
            let _ = host.wait_event("load", Some(id))?;
            Ok(format!("reloaded {id}"))
        }
        "browser_screenshot" => {
            let path = args.get("path").and_then(|v| v.as_str()).ok_or("path required")?;
            let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
            host.send(&format!(r#"{{"cmd":"screenshot","id":{id},"path":"{escaped}"}}"#))?;
            let line = host.wait_event("screenshot", Some(id))?;
            Ok(line.trim().to_string())
        }
        "browser_a11y" => {
            host.send(&format!(r#"{{"cmd":"a11y","id":{id}}}"#))?;
            let line = host.wait_event("a11y", Some(id))?;
            Ok(line.trim().to_string())
        }
        "browser_print" => {
            host.send(&format!(r#"{{"cmd":"print","id":{id}}}"#))?;
            let line = host.wait_event("print", Some(id))?;
            Ok(line.trim().to_string())
        }
        "browser_eval" => {
            let js = args.get("js").and_then(|v| v.as_str()).ok_or("js required")?;
            let escaped = js.replace('\\', "\\\\").replace('"', "\\\"");
            host.send(&format!(r#"{{"cmd":"eval","id":{id},"js":"{escaped}"}}"#))?;
            let line = host.wait_event("eval", Some(id))?;
            Ok(line.trim().to_string())
        }
        "browser_info" => Ok(format!(
            "active={} host={}",
            host.active,
            find_host().map(|p| p.display().to_string()).unwrap_or_else(|| "missing".into())
        )),
        other => Err(format!("unknown tool: {other}")),
    }
}

fn handle(host: &mut Host, req: Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    match method {
        "initialize" => Some(json!({
            "jsonrpc":"2.0",
            "id": id,
            "result": {
                "protocolVersion":"2024-11-05",
                "capabilities":{"tools":{}},
                "serverInfo":{"name":"kakangkung-mcp","version":env!("CARGO_PKG_VERSION")}
            }
        })),
        "notifications/initialized" | "initialized" => None,
        "ping" => id.map(|i| json!({"jsonrpc":"2.0","id":i,"result":{}})),
        "tools/list" => Some(json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools()}})),
        "tools/call" => {
            let i = id?;
            let name = req.pointer("/params/name").and_then(|v| v.as_str()).unwrap_or("");
            let args = req.pointer("/params/arguments").cloned().unwrap_or_else(|| json!({}));
            match call_tool(host, name, &args) {
                Ok(text) => Some(ok_text(i, text)),
                Err(e) => Some(json!({"jsonrpc":"2.0","id":i,"result":{"content":[{"type":"text","text":e}],"isError":true}})),
            }
        }
        "" => err_rpc(id, -32600, "invalid request"),
        other => err_rpc(id, -32601, &format!("method not found: {other}")),
    }
}

fn main() {
    let mut host = match Host::spawn() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("kakangkung-mcp: {e}");
            std::process::exit(1);
        }
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(line) else {
            let _ = writeln!(stdout, "{}", json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}));
            let _ = stdout.flush();
            continue;
        };
        if let Some(resp) = handle(&mut host, req) {
            let _ = writeln!(stdout, "{resp}");
            let _ = stdout.flush();
        }
    }
}
