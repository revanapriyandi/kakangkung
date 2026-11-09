//! The built-in cosmetic/YouTube helper must receive current preferences at document start.
//! Registration is asynchronous; initial navigation waits for its completion.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
use webview2_com::{AddScriptToExecuteOnDocumentCreatedCompletedHandler, ExecuteScriptCompletedHandler};
use windows::core::{Interface, IUnknown, HSTRING};
use wry::{WebView, WebViewExtWindows};

const HELPER: &str = include_str!("../../../assets/adblock.js");

struct Registration {
    core: ICoreWebView2,
    script_id: RefCell<Option<String>>,
    revision: Cell<u64>,
    detached: Cell<bool>,
    navigation: RefCell<Option<String>>,
}

thread_local! {
    static TABS: RefCell<HashMap<usize, Rc<Registration>>> = RefCell::new(HashMap::new());
}

fn key(core: &ICoreWebView2) -> Result<usize, String> {
    core.cast::<IUnknown>().map(|identity| identity.as_raw() as usize).map_err(|error| error.to_string())
}

fn script(enabled: bool, allowed_sites: &str) -> String {
    let sites: Vec<&str> = allowed_sites.lines().map(str::trim).filter(|site| !site.is_empty()).take(100).collect();
    let config = serde_json::json!({ "enabled": enabled, "allowedSites": sites });
    format!("({HELPER})({config});")
}

fn remove(core: &ICoreWebView2, id: String) {
    // SAFETY: id was returned by this core's AddScriptToExecuteOnDocumentCreated callback.
    if let Err(error) = unsafe { core.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id)) } {
        eprintln!("ad blocker: remove document script: {error}");
    }
}

fn navigate(state: &Registration) {
    if state.detached.get() { return; }
    let url = state.navigation.borrow_mut().take();
    if let Some(url) = url {
        // SAFETY: registration and its callback run on the live webview's UI thread.
        if let Err(error) = unsafe { state.core.Navigate(&HSTRING::from(url)) } {
            eprintln!("ad blocker: initial navigation: {error}");
        }
    }
}

fn apply(state: &Rc<Registration>, source: String) -> Result<(), String> {
    if state.detached.get() { return Ok(()); }
    let revision = state.revision.get().wrapping_add(1);
    state.revision.set(revision);
    // Removing the old script first prevents a paused site's older helper from pruning data
    // during the short asynchronous registration window.
    let old = state.script_id.borrow_mut().take();
    if let Some(id) = old { remove(&state.core, id); }
    let pending = state.clone();
    let handler = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(move |result, id| {
        if pending.detached.get() || pending.revision.get() != revision {
            if result.is_ok() && !id.is_empty() { remove(&pending.core, id); }
            return Ok(());
        }
        match result {
            Ok(()) if !id.is_empty() => { *pending.script_id.borrow_mut() = Some(id); }
            Ok(()) => eprintln!("ad blocker: empty document-script registration id"),
            Err(error) => eprintln!("ad blocker: document-script registration: {error}"),
        }
        navigate(&pending);
        Ok(())
    }));
    // SAFETY: COM registration on the UI thread; WebView2 retains the completion handler.
    let result = unsafe { state.core.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(source.as_str()), &handler) };
    if let Err(error) = result {
        navigate(state);
        return Err(error.to_string());
    }
    Ok(())
}

/// Attach to a tab and optionally navigate after document-start registration completes. On
/// registration failure, the requested navigation still occurs; callers should log the error.
pub fn prepare_adblock_script(wv: &WebView, url: Option<&str>, enabled: bool, allowed_sites: &str) -> Result<(), String> {
    let core = wv.webview();
    let key = key(&core)?;
    let state = TABS.with(|tabs| {
        tabs.borrow_mut().entry(key).or_insert_with(|| Rc::new(Registration {
            core, script_id: RefCell::new(None), revision: Cell::new(0), detached: Cell::new(false),
            navigation: RefCell::new(None),
        })).clone()
    });
    if let Some(url) = url { *state.navigation.borrow_mut() = Some(url.to_string()); }
    apply(&state, script(enabled, allowed_sites))
}

/// Update existing documents and replace their future-document registration with current prefs.
pub fn update_adblock_scripts(enabled: bool, allowed_sites: &str) {
    let source = script(enabled, allowed_sites);
    let states: Vec<_> = TABS.with(|tabs| tabs.borrow().values().cloned().collect());
    for state in states {
        let completion = ExecuteScriptCompletedHandler::create(Box::new(|result, _| {
            if let Err(error) = result { eprintln!("ad blocker: update page helper: {error}"); }
            Ok(())
        }));
        // SAFETY: each retained core belongs to a live tab and this function runs on the UI thread.
        if let Err(error) = unsafe { state.core.ExecuteScript(&HSTRING::from(source.as_str()), &completion) } {
            eprintln!("ad blocker: update page helper: {error}");
        }
        if let Err(error) = apply(&state, source.clone()) {
            eprintln!("ad blocker: update document script: {error}");
        }
    }
}

/// Whether the latest document-start registration completed successfully for this tab.
pub fn adblock_script_ready(wv: &WebView) -> bool {
    let Ok(key) = key(&wv.webview()) else { return false; };
    TABS.with(|tabs| tabs.borrow().get(&key).is_some_and(|state| !state.detached.get() && state.script_id.borrow().is_some()))
}

/// Cancel deferred initial navigation and remove the retained script before dropping a tab.
pub fn detach_adblock_script(wv: &WebView) {
    let core = wv.webview();
    let Ok(key) = key(&core) else { return; };
    let state = TABS.with(|tabs| tabs.borrow_mut().remove(&key));
    if let Some(state) = state {
        state.detached.set(true);
        state.navigation.borrow_mut().take();
        let old = state.script_id.borrow_mut().take();
        if let Some(id) = old { remove(&state.core, id); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_is_serialized_and_bounded() {
        let generated = script(true, "news.example.com\nquote\".example\n");
        assert!(generated.ends_with("({\"allowedSites\":[\"news.example.com\",\"quote\\\".example\"],\"enabled\":true});"));
        let input = (0..120).map(|i| format!("site{i}.example\n")).collect::<String>();
        let generated = script(false, &input);
        assert!(generated.contains("site99.example"));
        assert!(!generated.contains("site100.example"));
    }
}
