//! Site permissions and saved passwords of the tabs' profile.

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Profile2, ICoreWebView2Profile4, COREWEBVIEW2_BROWSING_DATA_KINDS,
    COREWEBVIEW2_BROWSING_DATA_KINDS_GENERAL_AUTOFILL, COREWEBVIEW2_BROWSING_DATA_KINDS_PASSWORD_AUTOSAVE,
    COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_STATE_ALLOW, COREWEBVIEW2_PERMISSION_STATE_DEFAULT,
};
use webview2_com::{
    take_pwstr, ClearBrowsingDataCompletedHandler,
    GetNonDefaultPermissionSettingsCompletedHandler, SetPermissionStateCompletedHandler,
};
use windows::core::{HSTRING, PWSTR};
use wry::WebView;
use crate::completion::Done;
use crate::completion::fail_now;
use crate::completion::profile;

/// A site permission that differs from "ask" (camera, location, notifications, ...).
#[derive(Debug, Clone)]
pub struct SitePermission {
    /// `COREWEBVIEW2_PERMISSION_KIND` value, passed back to [`reset_site_permission`].
    pub kind: i32,
    pub origin: String,
    pub allowed: bool,
}

/// Every non-default permission of the tabs' profile (`ICoreWebView2Profile4`, runtime 117+).
pub fn site_permissions(wv: &WebView, done: impl FnOnce(std::result::Result<Vec<SitePermission>, String>) + 'static) {
    let done: Done<Vec<SitePermission>> = Box::new(done);
    let slot = std::rc::Rc::new(std::cell::Cell::new(Some(done)));
    let s2 = slot.clone();
    let handler = GetNonDefaultPermissionSettingsCompletedHandler::create(Box::new(move |hr, list| {
        let r = hr.and_then(|_| {
            let Some(list) = list else { return Ok(vec![]) };
            let mut n = 0u32;
            // SAFETY: out-params are valid locals; indexes are < Count; PWSTR freed by take_pwstr.
            unsafe {
                list.Count(&mut n)?;
                (0..n)
                    .map(|i| {
                        let p = list.GetValueAtIndex(i)?;
                        let (mut kind, mut origin, mut state) = (Default::default(), PWSTR::null(), Default::default());
                        p.PermissionKind(&mut kind)?;
                        p.PermissionOrigin(&mut origin)?;
                        p.PermissionState(&mut state)?;
                        Ok(SitePermission { kind: kind.0, origin: take_pwstr(origin), allowed: state == COREWEBVIEW2_PERMISSION_STATE_ALLOW })
                    })
                    .collect()
            }
        });
        if let Some(d) = s2.take() {
            d(r.map_err(|e| e.message()));
        }
        Ok(())
    }));
    // SAFETY: valid profile interface and handler; completion runs on this (UI) thread.
    let r = profile::<ICoreWebView2Profile4>(wv).and_then(|p| unsafe { p.GetNonDefaultPermissionSettings(&handler) });
    fail_now(r, &slot);
}

/// Put one site permission back to "ask".
pub fn reset_site_permission(wv: &WebView, kind: i32, origin: &str, done: impl FnOnce(std::result::Result<(), String>) + 'static) {
    let done: Done<()> = Box::new(done);
    let slot = std::rc::Rc::new(std::cell::Cell::new(Some(done)));
    let s2 = slot.clone();
    let handler = SetPermissionStateCompletedHandler::create(Box::new(move |hr| {
        if let Some(d) = s2.take() {
            d(hr.map_err(|e| e.message()));
        }
        Ok(())
    }));
    let origin = HSTRING::from(origin);
    // SAFETY: `origin` outlives the call; handler is a valid COM object.
    let r = profile::<ICoreWebView2Profile4>(wv).and_then(|p| unsafe {
        p.SetPermissionState(COREWEBVIEW2_PERMISSION_KIND(kind), &origin, COREWEBVIEW2_PERMISSION_STATE_DEFAULT, &handler)
    });
    fail_now(r, &slot);
}

/// Delete the passwords and autofill entries WebView2 saved for the tabs' profile.
pub fn clear_passwords(wv: &WebView, done: impl FnOnce(std::result::Result<(), String>) + 'static) {
    let done: Done<()> = Box::new(done);
    let slot = std::rc::Rc::new(std::cell::Cell::new(Some(done)));
    let s2 = slot.clone();
    let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |hr| {
        if let Some(d) = s2.take() {
            d(hr.map_err(|e| e.message()));
        }
        Ok(())
    }));
    let kinds = COREWEBVIEW2_BROWSING_DATA_KINDS(
        COREWEBVIEW2_BROWSING_DATA_KINDS_PASSWORD_AUTOSAVE.0 | COREWEBVIEW2_BROWSING_DATA_KINDS_GENERAL_AUTOFILL.0,
    );
    // SAFETY: valid profile interface and handler; completion runs on this (UI) thread.
    let r = profile::<ICoreWebView2Profile2>(wv).and_then(|p| unsafe { p.ClearBrowsingData(kinds, &handler) });
    fail_now(r, &slot);
}
