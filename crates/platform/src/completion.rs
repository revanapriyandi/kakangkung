//! Callbacks of asynchronous WebView2 calls, and the tabs' profile.

use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_13;
use windows::core::Interface;
use wry::{WebView, WebViewExtWindows};
use crate::Result;

pub(crate) type Done<T> = Box<dyn FnOnce(std::result::Result<T, String>)>;

pub(crate) type Slot<T> = std::rc::Rc<std::cell::Cell<Option<Done<T>>>>;

/// If the COM call itself failed, report it now. On success the completion handler owns the
/// callback, so the slot must not be emptied here.
pub(crate) fn fail_now<T>(r: Result<()>, slot: &Slot<T>) {
    if let Err(e) = r {
        if let Some(d) = slot.take() {
            d(Err(e.message()));
        }
    }
}

pub(crate) fn profile<T: Interface>(wv: &WebView) -> Result<T> {
    let core13: ICoreWebView2_13 = wv.webview().cast()?;
    // SAFETY: plain COM getter on a live interface.
    unsafe { core13.Profile() }?.cast()
}
