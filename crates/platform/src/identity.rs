//! How a tab presents itself to sites, and the small pieces a pop-up tab needs.
//!
//! WebView2 adds a "Microsoft Edge WebView2" brand to User-Agent Client Hints (`Sec-CH-UA`,
//! `navigator.userAgentData`), and Google's sign-in refuses embedded webviews by that brand
//! ("This browser or app may not be secure"). A Kakangkung tab is a full browser tab with an
//! address bar, so it presents itself as the Edge engine it runs on: the same User-Agent string,
//! the brands without the WebView2 entry. Neither `--user-agent` nor `Settings.UserAgent` drops
//! that brand (checked on runtime 157), only the DevTools override does.

use std::num::NonZeroIsize;

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings2;
use webview2_com::{take_pwstr, FocusChangedEventHandler, WindowCloseRequestedEventHandler};
use windows::core::{Interface, HSTRING, PWSTR};
use wry::{WebView, WebViewExtWindows};

use crate::Result;

/// `Emulation.setUserAgentOverride` parameters: `ua` unchanged, Edge brands at `full` version.
pub fn edge_identity(ua: &str, full: &str, platform_version: &str) -> String {
    let major = full.split('.').next().unwrap_or("0");
    let brands = |v: &str, grease: &str| {
        serde_json::json!([
            { "brand": "Chromium", "version": v },
            { "brand": "Microsoft Edge", "version": v },
            { "brand": "Not-A.Brand", "version": grease },
        ])
    };
    serde_json::json!({
        "userAgent": ua,
        "userAgentMetadata": {
            "brands": brands(major, "99"),
            "fullVersionList": brands(full, "99.0.0.0"),
            "fullVersion": full,
            "platform": "Windows",
            "platformVersion": platform_version,
            "architecture": "x86",
            "bitness": "64",
            "model": "",
            "mobile": false,
            "wow64": false,
        }
    })
    .to_string()
}

/// What Chromium reports as the Windows `platformVersion` hint: the UniversalApiContract
/// version ("19.0.0" on Windows 11 24H2), from the registry.
fn platform_version() -> String {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
    let (key, value) = (
        HSTRING::from("SOFTWARE\\Microsoft\\WindowsRuntime\\WellKnownContracts"),
        HSTRING::from("Windows.Foundation.UniversalApiContract"),
    );
    let (mut data, mut size) = (0u32, 4u32);
    // SAFETY: `data` is a 4-byte buffer matching `size`; strings outlive the call.
    let rc = unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, &key, &value, RRF_RT_REG_DWORD, None, Some((&mut data as *mut u32).cast()), Some(&mut size))
    };
    if rc.is_ok() && data > 0 {
        format!("{}.{}.0", data >> 16, data & 0xffff)
    } else {
        "10.0.0".into()
    }
}

/// Present `wv` as Edge (see the module notes). Call before its first navigation.
pub fn present_as_edge(wv: &WebView) -> Result<()> {
    let mut ua = PWSTR::null();
    // SAFETY: plain COM getter on a live object; the string is taken (and freed) once.
    let ua = unsafe {
        wv.webview().Settings()?.cast::<ICoreWebView2Settings2>()?.UserAgent(&mut ua)?;
        take_pwstr(ua)
    };
    crate::devtools_call(wv, "Emulation.setUserAgentOverride", &edge_identity(&ua, &runtime_version(wv)?, &platform_version()))
}

/// Run `on_close` when the page asks to close its window (`window.close()` in a pop-up).
pub fn on_close_requested(wv: &WebView, on_close: impl Fn() + 'static) -> Result<()> {
    let handler = WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
        on_close();
        Ok(())
    }));
    let mut token = 0i64;
    // SAFETY: handler is a valid COM object; token is a writable local. The registration lives
    // as long as the webview (pop-up tabs are never discarded and re-created).
    unsafe { wv.webview().add_WindowCloseRequested(&handler, &mut token) }
}

/// Run `on_blur` when the webview loses the keyboard focus (a click elsewhere closes the
/// extension pop-up, like Chrome's).
pub fn on_lost_focus(wv: &WebView, on_blur: impl Fn() + 'static) -> Result<()> {
    let handler = FocusChangedEventHandler::create(Box::new(move |_, _| {
        on_blur();
        Ok(())
    }));
    let mut token = 0i64;
    // SAFETY: as in `on_close_requested`; the controller lives as long as the webview.
    unsafe { wv.controller().add_LostFocus(&handler, &mut token) }
}

/// The WebView2 runtime version ("157.0.4341.42").
pub fn runtime_version(wv: &WebView) -> Result<String> {
    let mut version = PWSTR::null();
    // SAFETY: plain COM getter; the string is taken (and freed) once.
    let version = unsafe {
        wv.environment().BrowserVersionString(&mut version)?;
        take_pwstr(version)
    };
    // A channel name may follow after a space.
    Ok(version.split_whitespace().next().unwrap_or("").to_string())
}

/// The browser window by its handle, to build a webview on it from a callback that has no
/// borrow of the window (a pop-up created inside the new-window handler).
pub struct ParentWindow(pub isize);

impl HasWindowHandle for ParentWindow {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let hwnd = NonZeroIsize::new(self.0).ok_or(HandleError::Unavailable)?;
        // SAFETY: the handle is the browser's main window, which outlives every webview on it.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(hwnd))) })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn edge_identity_drops_the_webview2_brand() {
        let j: serde_json::Value = serde_json::from_str(&super::edge_identity("UA/1", "157.0.4341.42", "19.0.0")).unwrap();
        assert_eq!(j["userAgent"], "UA/1");
        let m = &j["userAgentMetadata"];
        let brands: Vec<&str> = m["brands"].as_array().unwrap().iter().map(|b| b["brand"].as_str().unwrap()).collect();
        assert_eq!(brands, ["Chromium", "Microsoft Edge", "Not-A.Brand"]);
        assert_eq!(m["brands"][1]["version"], "157");
        assert_eq!(m["fullVersionList"][1]["version"], "157.0.4341.42");
        assert_eq!(m["platformVersion"], "19.0.0");
        assert!(!j.to_string().contains("WebView2"));
    }
}
