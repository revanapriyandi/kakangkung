//! Messages from webview callbacks and worker threads to the UI thread's event loop.

use std::path::PathBuf;

use ui::Action;
use serde_json::Value as Json;

use crate::pages::Reply;
use crate::prefs::Settings;

pub(crate) enum Msg {
    Ipc(String),
    /// A download started or moved on (for the Downloads page).
    DownloadsChanged,
    /// The updater moved on (checking, downloading, ready…).
    Update(crate::update::Status),
    /// An API answer for a native page: what was asked, the message to show when it worked.
    PageReply { path: String, done: String, json: Json },
    /// A token-checked `/api/...` request from a built-in page.
    Api { path: String, query: String, body: String, respond: Reply },
    /// An extension folder is ready on disk; install it on the UI thread.
    ExtAdd { folder: PathBuf, respond: Reply },
    Synced(bool),
    /// Address-bar suggestions for request `seq`, as JSON for chrome.html's `suggestions()`.
    Suggest { seq: u64, json: String },
    Bookmarked { url: String, on: bool },
    /// Saved search engine setting (engine id or custom template).
    Engine(String),
    /// Signed-in account name ("" = signed out), for the avatar.
    Account(String),
    /// Bookmarks arrived from another device.
    BookmarksChanged,
    /// Appearance or privacy settings changed.
    SettingsChanged(Settings),
    /// Memory saver mode changed in settings.
    MemoryMode(memory::Mode),
    /// Query suggestions from the search engine for address-bar request `seq`.
    RemoteSuggest { seq: u64, items: Vec<String> },
    Load { id: u64, url: String, done: bool },
    Title { id: u64, title: String },
    Favicon { id: u64, uri: String },
    Audio { id: u64, playing: bool },
    /// The tab's page started or stopped using the camera, microphone or screen capture.
    Media { id: u64, live: bool },
    Do(Action),
    OpenTab(String),
    Suspended { id: u64, ok: bool },
    Script(String),
}
