//! Tiny HTTPS GET through WinHTTP (part of Windows): search-engine query suggestions and the
//! updater (release list, checksums, installer).
//! Blocking: call it from a worker thread, never the UI thread.

use std::ffi::c_void;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

/// Largest response read (suggestion lists are a few KB).
const MAX_BODY: usize = 256 * 1024;

/// Closes a WinHTTP handle when dropped.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle came from WinHttpOpen/Connect/OpenRequest and is closed once.
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn check(h: *mut c_void, what: &str) -> Result<Handle, String> {
    if h.is_null() {
        Err(format!("{what} failed: {}", windows::core::Error::from_thread()))
    } else {
        Ok(Handle(h))
    }
}

/// GET an `https://` URL. Returns the body of a 200 response.
pub fn get(url: &str, timeout_ms: i32) -> Result<Vec<u8>, String> {
    get_limited(url, timeout_ms, MAX_BODY)
}

/// Like [`get`] for bigger bodies (an installer): at most `max` bytes, else an error.
pub fn download(url: &str, timeout_ms: i32, max: usize) -> Result<Vec<u8>, String> {
    let body = get_limited(url, timeout_ms, max + 1)?;
    if body.len() > max {
        return Err(format!("{url} is bigger than {max} bytes"));
    }
    Ok(body)
}

fn get_limited(url: &str, timeout_ms: i32, max_body: usize) -> Result<Vec<u8>, String> {
    let rest = url.strip_prefix("https://").ok_or("only https URLs are fetched")?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    let (agent, host_w, verb, path_w) = (HSTRING::from("Kakangkung"), HSTRING::from(host), HSTRING::from("GET"), HSTRING::from(path));
    // SAFETY: every string argument is a NUL-terminated HSTRING that outlives the call; handles are
    // checked for null and closed by `Handle`; the read buffer is sized by `chunk.len()`.
    unsafe {
        let session = check(
            WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
            "WinHttpOpen",
        )?;
        WinHttpSetTimeouts(session.0, timeout_ms, timeout_ms, timeout_ms, timeout_ms).map_err(|e| e.to_string())?;
        let conn = check(WinHttpConnect(session.0, &host_w, 443, 0), "WinHttpConnect")?;
        let req = check(
            WinHttpOpenRequest(conn.0, &verb, &path_w, PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
            "WinHttpOpenRequest",
        )?;
        WinHttpSendRequest(req.0, None, None, 0, 0, 0).map_err(|e| e.to_string())?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|e| e.to_string())?;
        let (mut status, mut len) = (0u32, 4u32);
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut len,
            std::ptr::null_mut(),
        )
        .map_err(|e| e.to_string())?;
        if status != 200 {
            return Err(format!("HTTP {status}"));
        }
        let mut body = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let mut got = 0u32;
            WinHttpReadData(req.0, chunk.as_mut_ptr().cast(), chunk.len() as u32, &mut got).map_err(|e| e.to_string())?;
            if got == 0 || body.len() >= max_body {
                break;
            }
            body.extend_from_slice(&chunk[..got as usize]);
        }
        Ok(body)
    }
}
