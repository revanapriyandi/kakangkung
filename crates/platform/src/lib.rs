//! All raw WebView2 COM and Win32 calls. The only crate allowed to use `unsafe`.
//!
//! Completion and event callbacks run on the UI thread from the message pump, like wry's own handlers.

mod completion;
mod downloads;
mod favicon;
mod extensions;
mod hash;
mod hooks;
pub mod motion;
mod processes;
mod profile;
mod shell;
mod time;
mod webview;
mod window;
pub mod frame;
pub mod http;
pub mod menu;
pub mod page;
pub mod popup;
pub mod sqlite;

pub use downloads::{download_action, downloads, take_finished, DownloadAction, DownloadInfo, DownloadState};
pub use hash::sha256_hex;
pub use extensions::{extension_add, extension_change, extensions_list, Extension};
pub use hooks::{hook, unhook, Hooks, Key};
pub use processes::{processes, ProcessMemory};
pub use profile::{clear_passwords, reset_site_permission, site_permissions, SitePermission};
pub use shell::{confirm, dark_mode, is_default_browser, pick_folder, register_as_browser, windows_account_email};
pub use webview::{apply_protection, devtools_call, document_title, is_suspended, open_find, probe_interfaces,
    raise, resume, save_page_as, set_download_folder, set_muted, stop, try_suspend, zoom, Protection};
pub use time::{local_time, LocalTime};
pub use window::capture_window_png;

pub type Result<T> = windows::core::Result<T>;

/// The WebView2 environment the tabs share (created with the first tab).
pub type Environment = webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment;
