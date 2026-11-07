//! Browser extensions of the tabs' profile.

use std::path::Path;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2BrowserExtension, ICoreWebView2Profile7,
};
use webview2_com::{
    take_pwstr, BrowserExtensionEnableCompletedHandler,
    BrowserExtensionRemoveCompletedHandler, ProfileAddBrowserExtensionCompletedHandler,
    ProfileGetBrowserExtensionsCompletedHandler,
};
use windows::core::{BOOL, HSTRING, PWSTR};
use windows::Win32::Foundation::E_POINTER;
use wry::WebView;
use crate::Result;
use crate::completion::Done;
use crate::completion::fail_now;
use crate::completion::profile;

/// A browser extension installed in the webview's profile.
#[derive(Debug, Clone)]
pub struct Extension {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

fn describe(ext: &ICoreWebView2BrowserExtension) -> Result<Extension> {
    let (mut id, mut name, mut on) = (PWSTR::null(), PWSTR::null(), BOOL(0));
    // SAFETY: out-params are valid locals; the PWSTRs are CoTaskMem strings freed by take_pwstr.
    unsafe {
        ext.Id(&mut id)?;
        ext.Name(&mut name)?;
        ext.IsEnabled(&mut on)?;
    }
    Ok(Extension { id: take_pwstr(id), name: take_pwstr(name), enabled: on.as_bool() })
}

/// All extensions of the profile, via `GetBrowserExtensions`, as COM objects.
fn with_extensions(wv: &WebView, done: Box<dyn FnOnce(Result<Vec<ICoreWebView2BrowserExtension>>)>) -> Result<()> {
    let handler = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |hr, list| {
        let r = hr.and_then(|_| {
            let Some(list) = list else { return Ok(vec![]) };
            let mut n = 0u32;
            // SAFETY: `n` is a writable local; indexes are < Count.
            unsafe {
                list.Count(&mut n)?;
                (0..n).map(|i| list.GetValueAtIndex(i)).collect()
            }
        });
        done(r);
        Ok(())
    }));
    // SAFETY: valid profile interface and handler; completion runs on this (UI) thread.
    unsafe { profile::<ICoreWebView2Profile7>(wv)?.GetBrowserExtensions(&handler) }
}

/// Requires the environment to be created with browser extensions enabled (runtime 120+).
pub fn extensions_list(wv: &WebView, done: impl FnOnce(std::result::Result<Vec<Extension>, String>) + 'static) {
    let done: Done<Vec<Extension>> = Box::new(done);
    let slot = std::rc::Rc::new(std::cell::Cell::new(Some(done)));
    let s2 = slot.clone();
    let r = with_extensions(
        wv,
        Box::new(move |r| {
            if let Some(d) = s2.take() {
                d(r.and_then(|v| v.iter().map(describe).collect()).map_err(|e| e.message()));
            }
        }),
    );
    fail_now(r, &slot);
}

/// Install an unpacked extension folder (the one holding manifest.json) into the profile.
pub fn extension_add(wv: &WebView, folder: &Path, done: impl FnOnce(std::result::Result<Extension, String>) + 'static) {
    let done: Done<Extension> = Box::new(done);
    let slot = std::rc::Rc::new(std::cell::Cell::new(Some(done)));
    let s2 = slot.clone();
    let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |hr, ext| {
        if let Some(d) = s2.take() {
            let r = hr.and_then(|_| ext.ok_or_else(|| windows::core::Error::from(E_POINTER))).and_then(|e| describe(&e));
            d(r.map_err(|e| e.message()));
        }
        Ok(())
    }));
    let path = HSTRING::from(folder.as_os_str());
    // SAFETY: `path` outlives the call; handler is a valid COM object.
    let r = profile::<ICoreWebView2Profile7>(wv).and_then(|p| unsafe { p.AddBrowserExtension(&path, &handler) });
    fail_now(r, &slot);
}

/// Enable/disable (`Some(bool)`) or remove (`None`) the extension with `id`.
pub fn extension_change(wv: &WebView, id: &str, enable: Option<bool>, done: impl FnOnce(std::result::Result<(), String>) + 'static) {
    let done: Done<()> = Box::new(done);
    let slot = std::rc::Rc::new(std::cell::Cell::new(Some(done)));
    let (s2, id) = (slot.clone(), id.to_string());
    let r = with_extensions(
        wv,
        Box::new(move |r| {
            let Some(d) = s2.take() else { return };
            let found = r.map(|v| v.into_iter().find(|e| describe(e).map(|x| x.id == id).unwrap_or(false)));
            let ext = match found {
                Ok(Some(e)) => e,
                Ok(None) => return d(Err(format!("extension {id} is not installed"))),
                Err(e) => return d(Err(e.message())),
            };
            let cell = std::rc::Rc::new(std::cell::Cell::new(Some(d)));
            let c2 = cell.clone();
            let finish = move |hr: Result<()>| {
                if let Some(d) = c2.take() {
                    d(hr.map_err(|e| e.message()));
                }
                Ok(())
            };
            // SAFETY: `ext` is a live extension object; handlers are valid COM objects.
            let r = unsafe {
                match enable {
                    Some(on) => ext.Enable(on, &BrowserExtensionEnableCompletedHandler::create(Box::new(finish))),
                    None => ext.Remove(&BrowserExtensionRemoveCompletedHandler::create(Box::new(finish))),
                }
            };
            fail_now(r, &cell);
        }),
    );
    fail_now(r, &slot);
}
