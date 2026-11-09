//! Starting WebView2 ahead of the first web page. Opening on a native page (New Tab) leaves no
//! web engine running, so the first navigation used to wait about two seconds for WebView2's
//! browser process. Creating the environment asynchronously while the person types hides that.

use std::cell::RefCell;
use std::path::Path;
use std::sync::mpsc;

use webview2_com::Microsoft::Web::WebView2::Win32::{CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2EnvironmentOptions, COREWEBVIEW2_SCROLLBAR_STYLE_DEFAULT};
use webview2_com::{CoreWebView2EnvironmentOptions, CreateCoreWebView2EnvironmentCompletedHandler};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::E_POINTER;
use windows::Win32::Globalization::{GetUserDefaultUILanguage, LCIDToLocaleName, LOCALE_ALLOW_NEUTRAL_NAMES, MAX_LOCALE_NAME};

use crate::Environment;

thread_local! {
    /// The environment being started (UI thread only).
    static PENDING: RefCell<Option<mpsc::Receiver<windows::core::Result<Environment>>>> = const { RefCell::new(None) };
}

/// Start WebView2's browser process for the profile at `data_dir` without waiting, with the
/// options wry would give it (`args`, extensions on, the Windows display language).
/// Does nothing when one is already starting.
pub fn prewarm_environment(data_dir: &Path, args: &str) {
    if PENDING.with(|p| p.borrow().is_some()) {
        return;
    }
    let (tx, rx) = mpsc::channel();
    let options = CoreWebView2EnvironmentOptions::default();
    // SAFETY: documented WebView2 calls with valid arguments; the completion handler runs later
    // on this thread's message loop and only sends the result through the channel.
    let started = unsafe {
        options.set_additional_browser_arguments(args.to_string());
        options.set_are_browser_extensions_enabled(true);
        let mut lang = [0u16; MAX_LOCALE_NAME as usize];
        LCIDToLocaleName(GetUserDefaultUILanguage() as u32, Some(&mut lang), LOCALE_ALLOW_NEUTRAL_NAMES);
        let end = lang.iter().position(|&c| c == 0).unwrap_or(lang.len());
        options.set_language(String::from_utf16_lossy(&lang[..end]));
        options.set_scroll_bar_style(COREWEBVIEW2_SCROLLBAR_STYLE_DEFAULT);
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &HSTRING::from(data_dir),
            &ICoreWebView2EnvironmentOptions::from(options),
            &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |error, environment| {
                let result = error.and_then(|_| environment.ok_or_else(|| E_POINTER.into()));
                let _ = tx.send(result);
                Ok(())
            })),
        )
    };
    match started {
        Ok(()) => PENDING.with(|p| *p.borrow_mut() = Some(rx)),
        Err(e) => eprintln!("WebView2 pre-start: {e}"),
    }
}

/// The environment started by `prewarm_environment`, waiting for it (the UI keeps handling its
/// messages meanwhile) if it is still starting. `None` when none was started or it failed.
pub fn take_prewarmed_environment() -> Option<Environment> {
    let rx = PENDING.with(|p| p.borrow_mut().take())?;
    match webview2_com::wait_with_pump(rx) {
        Ok(Ok(env)) => Some(env),
        Ok(Err(e)) => {
            eprintln!("WebView2 pre-start: {e}");
            None
        }
        Err(e) => {
            eprintln!("WebView2 pre-start: {e:?}");
            None
        }
    }
}
