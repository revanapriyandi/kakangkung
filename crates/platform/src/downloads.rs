//! Downloads of this browser window: WebView2 reports each one (`DownloadStarting`); the list
//! and the operations (pause, resume, cancel) live here, on the UI thread, for the Downloads page.

use std::cell::RefCell;
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2DownloadOperation, ICoreWebView2_13, ICoreWebView2_4, COREWEBVIEW2_DOWNLOAD_STATE, COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
    COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED,
};
use webview2_com::{take_pwstr, BytesReceivedChangedEventHandler, DownloadStartingEventHandler, StateChangedEventHandler};
use windows::core::{Interface, BOOL, PWSTR};
use wry::{WebView, WebViewExtWindows};

/// Where a download is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    InProgress,
    Paused,
    Done,
    /// Failed or cancelled.
    Stopped,
}

/// One download, as the Downloads page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadInfo {
    pub id: u64,
    pub url: String,
    /// Where the file is (or will be) saved.
    pub path: String,
    pub received: i64,
    /// -1 when the size is unknown.
    pub total: i64,
    pub state: DownloadState,
    pub can_resume: bool,
    /// Unix ms when it started.
    pub started: i64,
    /// From an Incognito window: never written to disk.
    pub private: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadAction {
    Pause,
    Resume,
    Cancel,
    /// Take it off the list (the file stays).
    Forget,
}

struct Item {
    id: u64,
    op: ICoreWebView2DownloadOperation,
    paused: bool,
    started: i64,
    private: bool,
}

thread_local! {
    static LIST: RefCell<Vec<Item>> = const { RefCell::new(Vec::new()) };
    static NEXT: RefCell<u64> = const { RefCell::new(1) };
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Report every download of `wv` here; `on_change` runs when one starts or moves on. Returns
/// the registration token for [`unwatch`].
pub(crate) fn watch(wv: &WebView, on_change: Rc<dyn Fn()>) -> Option<i64> {
    let core4 = wv.webview().cast::<ICoreWebView2_4>().ok()?;
    let mut private = BOOL(0);
    if let Ok(core13) = wv.webview().cast::<ICoreWebView2_13>() {
        // SAFETY: plain COM calls; `private` is a writable local.
        unsafe {
            if let Ok(profile) = core13.Profile() {
                let _ = profile.IsInPrivateModeEnabled(&mut private);
            }
        }
    }
    let private = private.as_bool();
    let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        // SAFETY: plain COM calls on the live event args and the operation they hand over.
        let op = unsafe { args.DownloadOperation()? };
        let id = NEXT.with(|n| {
            let mut n = n.borrow_mut();
            *n += 1;
            *n - 1
        });
        for which in [true, false] {
            let changed = on_change.clone();
            let mut token = 0i64;
            // SAFETY: handlers are valid COM objects, token a writable local. They live as long
            // as the operation (released with it).
            unsafe {
                if which {
                    let h = BytesReceivedChangedEventHandler::create(Box::new(move |_, _| {
                        changed();
                        Ok(())
                    }));
                    op.add_BytesReceivedChanged(&h, &mut token)?;
                } else {
                    let h = StateChangedEventHandler::create(Box::new(move |_, _| {
                        changed();
                        Ok(())
                    }));
                    op.add_StateChanged(&h, &mut token)?;
                }
            }
        }
        LIST.with(|l| l.borrow_mut().push(Item { id, op, paused: false, started: now_ms(), private }));
        on_change();
        Ok(())
    }));
    let mut token = 0i64;
    // SAFETY: handler is a valid COM object; token is a writable local.
    match unsafe { core4.add_DownloadStarting(&handler, &mut token) } {
        Ok(()) => Some(token),
        Err(e) => {
            eprintln!("downloads: DownloadStarting registration failed: {e}");
            None
        }
    }
}

pub(crate) fn unwatch(wv: &WebView, token: i64) {
    if let Ok(core4) = wv.webview().cast::<ICoreWebView2_4>() {
        // SAFETY: token came from add_DownloadStarting on this webview.
        if let Err(e) = unsafe { core4.remove_DownloadStarting(token) } {
            eprintln!("unhook: DownloadStarting: {e}");
        }
    }
}

fn info(item: &Item) -> DownloadInfo {
    let (mut url, mut path) = (PWSTR::null(), PWSTR::null());
    let (mut received, mut total) = (0i64, -1i64);
    let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
    let mut can_resume = BOOL(0);
    // SAFETY: out-params are valid locals; WebView2 allocates the strings, take_pwstr frees them.
    let (url, path) = unsafe {
        let _ = item.op.Uri(&mut url);
        let _ = item.op.ResultFilePath(&mut path);
        let _ = item.op.BytesReceived(&mut received);
        let _ = item.op.TotalBytesToReceive(&mut total);
        let _ = item.op.State(&mut state);
        let _ = item.op.CanResume(&mut can_resume);
        (take_pwstr(url), take_pwstr(path))
    };
    let state = match state {
        COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED => DownloadState::Done,
        COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED if !can_resume.as_bool() => DownloadState::Stopped,
        COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED => DownloadState::Paused,
        _ if item.paused => DownloadState::Paused,
        _ => DownloadState::InProgress,
    };
    DownloadInfo { id: item.id, url, path, received, total, state, can_resume: can_resume.as_bool(), started: item.started, private: item.private }
}

/// Every download of this run, newest first.
pub fn downloads() -> Vec<DownloadInfo> {
    LIST.with(|l| l.borrow().iter().rev().map(info).collect())
}

/// Take the downloads that ended (done, cancelled or failed) off this list, except Incognito
/// ones, for the caller to keep on disk.
pub fn take_finished() -> Vec<DownloadInfo> {
    LIST.with(|l| {
        let mut list = l.borrow_mut();
        let mut finished = vec![];
        list.retain(|item| {
            let d = info(item);
            let ended = matches!(d.state, DownloadState::Done | DownloadState::Stopped) && !d.private;
            if ended {
                finished.push(d);
            }
            !ended
        });
        finished
    })
}

pub fn download_action(id: u64, action: DownloadAction) -> crate::Result<()> {
    LIST.with(|l| {
        let mut list = l.borrow_mut();
        let Some(i) = list.iter().position(|d| d.id == id) else { return Ok(()) };
        // SAFETY: plain COM calls on a live download operation.
        unsafe {
            match action {
                DownloadAction::Pause => {
                    list[i].op.Pause()?;
                    list[i].paused = true;
                }
                DownloadAction::Resume => {
                    list[i].op.Resume()?;
                    list[i].paused = false;
                }
                DownloadAction::Cancel => list[i].op.Cancel()?,
                DownloadAction::Forget => {
                    list.remove(i);
                }
            }
        }
        Ok(())
    })
}
