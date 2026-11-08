//! Page-level WebView2 controls: freeze, zoom, find, mute, print/save, privacy switches.

use std::path::Path;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Environment13, ICoreWebView2Environment8,
    ICoreWebView2_13, ICoreWebView2_15, ICoreWebView2_19,
    ICoreWebView2_3, ICoreWebView2_8,
};
use webview2_com::{
    take_pwstr,
    CallDevToolsProtocolMethodCompletedHandler, TrySuspendCompletedHandler,
};
use windows::core::{Interface, BOOL, HSTRING, PWSTR};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
};
use wry::{WebView, WebViewExtWindows};
use crate::Result;

/// `ICoreWebView2_3::TrySuspend`. `done(Ok(true))` = the renderer is frozen.
/// The webview must already be hidden (`IsVisible = false`) or WebView2 refuses.
pub fn try_suspend(wv: &WebView, done: impl FnOnce(Result<bool>) + 'static) -> Result<()> {
    let core: ICoreWebView2_3 = wv.webview().cast()?;
    let handler = TrySuspendCompletedHandler::create(Box::new(move |hr, ok| {
        done(hr.map(|_| ok));
        Ok(())
    }));
    // SAFETY: `core` is a live interface obtained from wry's webview; the handler is a
    // COM object that WebView2 AddRefs and calls once on this (UI) thread.
    unsafe { core.TrySuspend(&handler) }
}

/// `ICoreWebView2_3::Resume`. Also happens implicitly when the webview is made visible.
pub fn resume(wv: &WebView) -> Result<()> {
    let core: ICoreWebView2_3 = wv.webview().cast()?;
    // SAFETY: plain COM call on a live interface from the UI thread.
    unsafe { core.Resume() }
}

pub fn is_suspended(wv: &WebView) -> Result<bool> {
    let core: ICoreWebView2_3 = wv.webview().cast()?;
    let mut out = BOOL(0);
    // SAFETY: `out` is a valid, writable BOOL for the duration of the call.
    unsafe { core.IsSuspended(&mut out) }?;
    Ok(out.as_bool())
}

/// `ICoreWebView2::Stop`: stop loading (wry has no wrapper).
pub fn stop(wv: &WebView) -> Result<()> {
    // SAFETY: plain COM call on a live interface from the UI thread.
    unsafe { wv.webview().Stop() }
}

/// Where downloads of this profile are saved (`ICoreWebView2Profile::DefaultDownloadFolderPath`).
pub fn set_download_folder(wv: &WebView, folder: &Path) -> Result<()> {
    let core13: ICoreWebView2_13 = wv.webview().cast()?;
    let path = HSTRING::from(folder.as_os_str());
    // SAFETY: plain COM calls on live interfaces; `path` outlives the call.
    unsafe { core13.Profile()?.SetDefaultDownloadFolderPath(&path) }
}

/// The title of the document the webview shows right now (`ICoreWebView2::DocumentTitle`).
/// Title-changed events can arrive late from the previous page; this is always current.
pub fn document_title(wv: &WebView) -> Option<String> {
    let mut t = PWSTR::null();
    // SAFETY: WebView2 allocates `t` with CoTaskMemAlloc; take_pwstr copies and frees it.
    unsafe { wv.webview().DocumentTitle(&mut t) }.ok()?;
    Some(take_pwstr(t))
}

/// Open WebView2's own find bar on the page (Find… in the menu, same as Ctrl+F).
pub fn open_find(wv: &WebView) -> Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2Environment15, ICoreWebView2_28};
    use webview2_com::FindStartCompletedHandler;
    let env15: ICoreWebView2Environment15 = wv.environment().cast()?;
    let core28: ICoreWebView2_28 = wv.webview().cast()?;
    // SAFETY: plain COM calls on live interfaces; the handler is a valid COM object.
    unsafe {
        let options = env15.CreateFindOptions()?;
        options.SetSuppressDefaultFindDialog(false)?;
        options.SetShouldHighlightAllMatches(true)?;
        let handler = FindStartCompletedHandler::create(Box::new(|_| Ok(())));
        core28.Find()?.Start(&options, &handler)
    }
}

/// "Save page as…" (Ctrl+S): WebView2's own save dialog.
pub fn save_page_as(wv: &WebView) -> Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_25;
    use webview2_com::ShowSaveAsUICompletedHandler;
    let core25: ICoreWebView2_25 = wv.webview().cast()?;
    let handler = ShowSaveAsUICompletedHandler::create(Box::new(|_, _| Ok(())));
    // SAFETY: plain COM call on a live interface; the handler is a valid COM object.
    unsafe { core25.ShowSaveAsUI(&handler) }
}

/// Privacy and safety switches the person controls in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Protection {
    /// 0 = off, 1 = basic, 2 = balanced, 3 = strict (WebView2 tracking prevention).
    pub tracking: u8,
    /// Microsoft Defender SmartScreen checks for sites and downloads.
    pub safe_browsing: bool,
    pub save_passwords: bool,
    pub autofill: bool,
    /// Sites may run JavaScript (Chrome's Site settings > JavaScript). Applies from the next page load.
    pub script: bool,
    /// Bit `1 << kind` set = every request for that `COREWEBVIEW2_PERMISSION_KIND` is denied
    /// without asking (Site settings > "Don't allow sites to ..."). Saved per-site choices still win.
    pub blocked: u32,
    /// Show a Save As dialog for every download (Chrome's "Ask where to save each file").
    pub ask_downloads: bool,
    /// Built-in ad blocker (requests to known ad networks are refused). Live.
    pub adblock: bool,
}

thread_local! {
    static BLOCKED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Permission kinds denied by default (read by the `PermissionRequested` hook).
pub(crate) fn blocked_permissions() -> u32 {
    BLOCKED.with(|b| b.get())
}

/// Apply `p` to a tab webview: profile-wide switches (tracking, passwords, autofill) and this
/// webview's SmartScreen setting. Each part is applied on its own so one missing interface
/// (older runtime) does not block the others.
pub fn apply_protection(wv: &WebView, p: Protection) -> Vec<String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Profile3, ICoreWebView2Profile6, ICoreWebView2Settings8, COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED,
        COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BASIC, COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE,
        COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
    };
    let mut errors = vec![];
    BLOCKED.with(|b| b.set(p.blocked));
    crate::downloads::set_ask(p.ask_downloads);
    crate::adblock::set_enabled(p.adblock);
    let level = match p.tracking {
        0 => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_NONE,
        1 => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BASIC,
        3 => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT,
        _ => COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_BALANCED,
    };
    // SAFETY: plain COM getters/setters on live interfaces from the UI thread.
    unsafe {
        match wv.webview().cast::<ICoreWebView2_13>().and_then(|c| c.Profile()) {
            Ok(profile) => {
                if let Err(e) = profile.cast::<ICoreWebView2Profile3>().and_then(|p3| p3.SetPreferredTrackingPreventionLevel(level)) {
                    errors.push(format!("tracking prevention: {e}"));
                }
                if let Err(e) = profile.cast::<ICoreWebView2Profile6>().and_then(|p6| {
                    p6.SetIsPasswordAutosaveEnabled(p.save_passwords)?;
                    p6.SetIsGeneralAutofillEnabled(p.autofill)
                }) {
                    errors.push(format!("passwords/autofill: {e}"));
                }
            }
            Err(e) => errors.push(format!("profile: {e}")),
        }
        if let Err(e) = wv.webview().Settings().and_then(|s| s.cast::<ICoreWebView2Settings8>()).and_then(|s8| s8.SetIsReputationCheckingRequired(p.safe_browsing)) {
            errors.push(format!("SmartScreen: {e}"));
        }
        if let Err(e) = wv.webview().Settings().and_then(|s| s.SetIsScriptEnabled(p.script)) {
            errors.push(format!("JavaScript: {e}"));
        }
    }
    errors
}

/// Put the webview's window above its siblings (pop-ups must cover the tab webviews,
/// which are separate child windows created later).
pub fn raise(wv: &WebView) -> Result<()> {
    // SAFETY: wv.hwnd() is the live container window wry created for this webview.
    unsafe { SetWindowPos(wv.hwnd(), Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) }
}


/// Multiply the page zoom by `factor`, or reset it to 100 % when `factor` is `None`.
pub fn zoom(wv: &WebView, factor: Option<f64>) -> Result<f64> {
    let c = wv.controller();
    let mut now = 1.0f64;
    // SAFETY: `now` is a writable local; plain COM calls on the live controller.
    unsafe {
        c.ZoomFactor(&mut now)?;
        let next = factor.map_or(1.0, |f| (now * f).clamp(0.25, 5.0));
        c.SetZoomFactor(next)?;
        Ok(next)
    }
}

/// Mute or unmute everything the tab plays (`ICoreWebView2_8::IsMuted`).
pub fn set_muted(wv: &WebView, muted: bool) -> Result<()> {
    let core: ICoreWebView2_8 = wv.webview().cast()?;
    // SAFETY: plain COM setter on a live interface from the UI thread.
    unsafe { core.SetIsMuted(muted) }
}

/// Run a DevTools Protocol method on the page, fire and forget. `params` must be JSON.
/// Used for `Runtime.evaluate` with `userGesture: true` (e.g. picture-in-picture needs one).
pub fn devtools_call(wv: &WebView, method: &str, params: &str) -> Result<()> {
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
    let (method, params) = (HSTRING::from(method), HSTRING::from(params));
    // SAFETY: the HSTRINGs outlive the call; the handler is a valid COM object.
    unsafe { wv.webview().CallDevToolsProtocolMethod(&method, &params, &handler) }
}

/// Which of the interfaces in docs/API_MAP.md the installed runtime answers to.
pub fn probe_interfaces(wv: &WebView) -> Vec<(&'static str, bool)> {
    let core = wv.webview();
    let env = wv.environment();
    vec![
        ("ICoreWebView2_3 (TrySuspend)", core.cast::<ICoreWebView2_3>().is_ok()),
        ("ICoreWebView2_8 (audio)", core.cast::<ICoreWebView2_8>().is_ok()),
        ("ICoreWebView2_15 (favicon)", core.cast::<ICoreWebView2_15>().is_ok()),
        ("ICoreWebView2_19 (memory level)", core.cast::<ICoreWebView2_19>().is_ok()),
        ("ICoreWebView2Environment8 (process infos)", env.cast::<ICoreWebView2Environment8>().is_ok()),
        ("ICoreWebView2Environment13 (extended infos)", env.cast::<ICoreWebView2Environment13>().is_ok()),
    ]
}
