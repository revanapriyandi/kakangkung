//! HTTP-style responses for the built-in kakangkung:// pages and the browser UI documents.

use std::borrow::Cow;

use serde_json::{json, Value as Json};
use tao::event_loop::EventLoopProxy;
use wry::http::{header, Response};
use wry::RequestAsyncResponder;

use crate::msg::Msg;
use crate::APP;

pub(crate) fn html(body: Cow<'static, [u8]>, status: u16) -> Response<Cow<'static, [u8]>> {
    reply(body, status, "text/html; charset=utf-8")
}

/// Built-in pages may only load their own files, talk to their own API and submit searches.
pub(crate) const INTERNAL_CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
    img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'";

pub(crate) fn reply(body: Cow<'static, [u8]>, status: u16, content_type: &str) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header("X-Content-Type-Options", "nosniff")
        .header("Content-Security-Policy", INTERNAL_CSP)
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap_or_else(|e| {
            eprintln!("building built-in response failed: {e}");
            Response::new(Cow::Borrowed(&[]))
        })
}


pub(crate) fn json_reply(v: Json) -> Response<Cow<'static, [u8]>> {
    let status = if v.get("error").is_some() { 400 } else { 200 };
    reply(v.to_string().into_bytes().into(), status, "application/json")
}

pub(crate) fn api_json<T: serde::Serialize>(r: Result<T, String>) -> Json {
    match r {
        Ok(v) => serde_json::to_value(v).unwrap_or(Json::Null),
        Err(e) => json!({ "error": e }),
    }
}

/// Where an API answer goes: a built-in HTML page's request, or a native page (as a message).
pub(crate) enum Reply {
    Http(RequestAsyncResponder),
    Native { proxy: EventLoopProxy<Msg>, path: String, done: String },
}

impl Reply {
    pub(crate) fn send(self, v: Json) {
        match self {
            Reply::Http(r) => r.respond(json_reply(v)),
            Reply::Native { proxy, path, done } => {
                let _ = proxy.send_event(Msg::PageReply { path, done, json: v });
            }
        }
    }
}

/// What the built-in host serves to webviews: only the self-test's blank page, which carries the
/// session token for its `/api` script. Every page people see is native (`crate::native`).
pub(crate) fn builtin_page(path: &str, token: &str) -> Response<Cow<'static, [u8]>> {
    match path {
        "/selftest" => html(format!("<!doctype html><meta name=kakangkung-token content=\"{token}\"><title>{APP} self-test</title>").into_bytes().into(), 200),
        _ => html(b"<!doctype html><title>Not found</title>".as_slice().into(), 404),
    }
}

/// Unpredictable hex string (session token, device id). `RandomState` keys come from the OS RNG.
pub(crate) fn random_hex() -> String {
    use std::hash::{BuildHasher, Hasher};
    (0..2u128)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(i ^ storage::now_ms() as u128);
            format!("{:016x}", h.finish())
        })
        .collect()
}

pub(crate) fn query_param(query: &str, key: &str) -> String {
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| percent_decode(v))
        .unwrap_or_default()
}

/// `s` as a query-string value (`percent_decode` reverses it).
pub(crate) fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%' && i + 2 < b.len())
            .then(|| std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()))
            .flatten();
        match (b[i], hex) {
            (_, Some(v)) => {
                out.push(v);
                i += 2;
            }
            (b'+', _) => out.push(b' '),
            (c, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn ext_json(e: platform::Extension) -> Json {
    json!({ "id": e.id, "name": e.name, "enabled": e.enabled })
}

#[cfg(test)]
mod tests {
    use super::{percent_decode, percent_encode, query_param};

    #[test]
    fn query_values_round_trip() {
        let words = "rust & c++ 100% ü";
        assert_eq!(percent_decode(&percent_encode(words)), words);
        assert_eq!(query_param(&format!("q={}&limit=3", percent_encode(words)), "q"), words);
    }
}
