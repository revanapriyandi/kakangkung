//! All raw WebView2 COM and Win32 calls. The only crate allowed to use `unsafe`.
//!
//! Completion and event callbacks run on the UI thread from the message pump, like wry's own handlers.

mod adblock;
mod adblock_script;
pub use adblock_script::{prepare_adblock_script, adblock_script_ready};
mod filter_engine;
mod completion;
mod downloads;
mod devtools;
pub use devtools::DockedDevTools;
mod favicon;
pub use favicon::load_avatar_file;
mod extensions;
mod hash;
mod hooks;
mod identity;
pub mod motion;
mod processes;
mod tab_data;
mod tab_preview;
pub use tab_data::{capture_thumbnail, tab_memory, Thumbnail, TabMemory};
pub use tab_preview::{TabPreview, TabPreviewData};
mod profile;
mod secret;
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
pub use hash::{random_bytes, sha256, sha256_hex};
pub use secret::{aes_gcm_decrypt, base64_decode, protect, unprotect};

pub use extensions::{extension_add, extension_change, extensions_list, Extension};
pub use adblock::{ads_blocked, configure_adblock_sites, adblock_site_allowed, adblock_site_enabled, blocked_for_site, adblock_site_key};
pub use hooks::{hook, unhook, Hooks, Key};
pub use identity::{edge_identity, on_close_requested, on_lost_focus, present_as_edge, runtime_version, ParentWindow};
pub use processes::{processes, ProcessMemory};
pub use profile::{clear_browsing_data, clear_passwords, reset_site_permission, site_permissions, SitePermission};
pub use shell::{confirm, copy_secret, dark_mode, ui_language, is_default_browser, pick_file, pick_folder, register_as_browser, save_file, verify_user, windows_account_email};
pub use webview::{apply_protection, collect_idle_garbage, devtools_call, document_title, is_suspended, memory_status, open_find, probe_interfaces,
    raise, resume, save_page_as, set_download_folder, set_muted, stop, try_suspend, zoom, Protection};
pub use time::{local_time, LocalTime};
pub use favicon::{load_file as load_icon_file, load_file_as as load_icon_file_as, loaded as favicon_loaded};
pub use window::capture_window_png;

pub type Result<T> = windows::core::Result<T>;

/// The WebView2 environment the tabs share (created with the first tab).
pub type Environment = webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment;
