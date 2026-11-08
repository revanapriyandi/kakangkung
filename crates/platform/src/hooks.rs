//! Per-tab event handlers: shortcuts while a page has focus, favicon and audio changes, downloads.

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2_15, ICoreWebView2_8, COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
    COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN, COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_STATE_DENY,
};
use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler, FaviconChangedEventHandler,
    IsDocumentPlayingAudioChangedEventHandler, PermissionRequestedEventHandler,
};
use windows::core::{Interface, BOOL, PWSTR};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use wry::{WebView, WebViewExtWindows};

/// A key press seen by WebView2 before the page gets it.
#[derive(Debug, Clone, Copy)]
pub struct Key {
    pub vk: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Event registrations the app added to one webview. Pass to [`unhook`] before dropping it.
#[derive(Debug, Default)]
pub struct Hooks {
    accel: Option<i64>,
    favicon: Option<i64>,
    audio: Option<i64>,
    downloads: Option<i64>,
    permission: Option<i64>,
}

impl Hooks {
    /// Number of live registrations (the self-test checks none are leaked).
    pub fn count(&self) -> usize {
        [self.accel, self.favicon, self.audio, self.downloads, self.permission].iter().flatten().count()
    }
}

fn key_down() -> fn(i32) -> bool {
    // SAFETY: GetKeyState only reads the calling thread's keyboard state.
    |vk| unsafe { GetKeyState(vk) } < 0
}

/// Register the per-tab handlers: browser shortcuts while the page has focus
/// (`on_key` returns true when it handled the key), favicon URL, audio playing, and downloads
/// (`on_download` runs when one starts or moves on; see [`crate::downloads`]).
pub fn hook(
    wv: &WebView,
    mut on_key: impl FnMut(Key) -> bool + 'static,
    mut on_favicon: impl FnMut(String) + 'static,
    mut on_audio: impl FnMut(bool) + 'static,
    on_download: impl Fn() + 'static,
) -> Hooks {
    let mut hooks = Hooks { downloads: crate::downloads::watch(wv, std::rc::Rc::new(on_download)), ..Default::default() };
    let controller = wv.controller();
    let accel = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
        let mut vk = 0u32;
        // SAFETY: out-params are valid locals; args is the live event-args object.
        unsafe {
            args.KeyEventKind(&mut kind)?;
            args.VirtualKey(&mut vk)?;
        }
        if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN {
            return Ok(());
        }
        let down = key_down();
        let key = Key {
            vk,
            ctrl: down(VK_CONTROL.0 as i32),
            shift: down(VK_SHIFT.0 as i32),
            alt: down(VK_MENU.0 as i32),
        };
        if on_key(key) {
            // SAFETY: plain COM call on the live args object.
            unsafe { args.SetHandled(true)? };
        }
        Ok(())
    }));
    let mut token = 0i64;
    // SAFETY: handler is a valid COM object; token is a writable local.
    match unsafe { controller.add_AcceleratorKeyPressed(&accel, &mut token) } {
        Ok(()) => hooks.accel = Some(token),
        Err(e) => eprintln!("hook: AcceleratorKeyPressed registration failed: {e}"),
    }

    if let Ok(core15) = wv.webview().cast::<ICoreWebView2_15>() {
        let reader = core15.clone();
        let fav = FaviconChangedEventHandler::create(Box::new(move |_, _| {
            let mut uri = PWSTR::null();
            // SAFETY: WebView2 allocates `uri` with CoTaskMemAlloc; take_pwstr copies and frees it.
            unsafe { reader.FaviconUri(&mut uri)? };
            on_favicon(take_pwstr(uri));
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: as above.
        match unsafe { core15.add_FaviconChanged(&fav, &mut token) } {
            Ok(()) => hooks.favicon = Some(token),
            Err(e) => eprintln!("hook: FaviconChanged registration failed: {e}"),
        }
    }

    if let Ok(core8) = wv.webview().cast::<ICoreWebView2_8>() {
        let reader = core8.clone();
        let audio = IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |_, _| {
            let mut playing = BOOL(0);
            // SAFETY: `playing` is a writable local.
            unsafe { reader.IsDocumentPlayingAudio(&mut playing)? };
            on_audio(playing.as_bool());
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: as above.
        match unsafe { core8.add_IsDocumentPlayingAudioChanged(&audio, &mut token) } {
            Ok(()) => hooks.audio = Some(token),
            Err(e) => eprintln!("hook: IsDocumentPlayingAudioChanged registration failed: {e}"),
        }
    }
    // Site settings: kinds switched to "Don't allow" are denied without a prompt.
    let perm = PermissionRequestedEventHandler::create(Box::new(|_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
        // SAFETY: `kind` is a writable local; plain COM calls on the live event args.
        unsafe {
            args.PermissionKind(&mut kind)?;
            if (0..32).contains(&kind.0) && crate::webview::blocked_permissions() & (1 << kind.0) != 0 {
                args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
            }
        }
        Ok(())
    }));
    let mut token = 0i64;
    // SAFETY: handler is a valid COM object; token is a writable local.
    match unsafe { wv.webview().add_PermissionRequested(&perm, &mut token) } {
        Ok(()) => hooks.permission = Some(token),
        Err(e) => eprintln!("hook: PermissionRequested registration failed: {e}"),
    }
    hooks
}

/// Remove every registration in `hooks`. Call before the webview is dropped.
pub fn unhook(wv: &WebView, hooks: &mut Hooks) {
    if let Some(t) = hooks.downloads.take() {
        crate::downloads::unwatch(wv, t);
    }
    if let Some(t) = hooks.permission.take() {
        // SAFETY: token came from add_PermissionRequested on this webview.
        if let Err(e) = unsafe { wv.webview().remove_PermissionRequested(t) } {
            eprintln!("unhook: PermissionRequested: {e}");
        }
    }
    if let Some(t) = hooks.accel.take() {
        // SAFETY: token came from add_AcceleratorKeyPressed on this controller.
        if let Err(e) = unsafe { wv.controller().remove_AcceleratorKeyPressed(t) } {
            eprintln!("unhook: AcceleratorKeyPressed: {e}");
        }
    }
    if let (Some(t), Ok(c)) = (hooks.favicon.take(), wv.webview().cast::<ICoreWebView2_15>()) {
        // SAFETY: token came from add_FaviconChanged on this webview.
        if let Err(e) = unsafe { c.remove_FaviconChanged(t) } {
            eprintln!("unhook: FaviconChanged: {e}");
        }
    }
    if let (Some(t), Ok(c)) = (hooks.audio.take(), wv.webview().cast::<ICoreWebView2_8>()) {
        // SAFETY: token came from add_IsDocumentPlayingAudioChanged on this webview.
        if let Err(e) = unsafe { c.remove_IsDocumentPlayingAudioChanged(t) } {
            eprintln!("unhook: IsDocumentPlayingAudioChanged: {e}");
        }
    }
}
