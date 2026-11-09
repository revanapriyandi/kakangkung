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
    TabThumbnail { id: u64, revision: u64, result: Result<platform::Thumbnail, String> },
    TabMemory { id: u64, revision: u64, result: Result<platform::TabMemory, String> },
    Inspect(u64),
    /// The updater moved on (checking, downloading, ready…).
    Update(crate::update::Status),
    /// An API answer for a native page: what was asked, the message to show when it worked.
    PageReply { path: String, done: String, json: Json },
    /// A pop-up webview was built for tab `opener` (see `browser::take_popup`); show it as a tab.
    Popup { id: u64, opener: u64, url: String },
    /// The page in tab `id` called `window.close()`.
    CloseTab(u64),
    /// A message from the Chrome Web Store page in tab `tab` ("check\t<id>", "add\t<id>\t<name>").
    Store { tab: u64, cmd: String },
    /// A store extension was downloaded and unpacked (or failed); install it on the UI thread.
    StoreUnpacked { tab: u64, ext: String, result: Result<PathBuf, String> },
    StorePrepared { tab: u64, ext: String, result: Result<crate::webstore::InstallSwap, String> },
    StoreInstalled { tab: u64, ext: String, result: Result<(), String> },
    /// Show `text` on the store page's button for extension `ext` ("ready" = clickable).
    StoreButton { tab: u64, ext: String, state: &'static str, text: String },
    /// The extensions list for the toolbar's extensions menu, opened at (x, y).
    ExtMenu { list: Vec<platform::Extension>, x: f64, y: f64 },
    /// The extension pop-up asks to close, lost focus, or reported its content size.
    Bubble { close: bool, size: Option<(f64, f64)> },
    /// New data from other devices arrived (Google Drive mirror): run a sync round now.
    SyncSoon,
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
    /// Refreshed Google display name and locally cached profile photo.
    AccountProfile(String, String),
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
    /// A New Tab tile's icon is on disk (`None`: the site has none).
    SiteIcon { host: String, path: Option<std::path::PathBuf> },
    Audio { id: u64, playing: bool },
    /// `kk:pw:<what>` from the password manager's script in the page of tab `tab` at `url`.
    Password { tab: u64, url: String, what: String },
    /// Saved logins for the login field that got focus in tab `tab`.
    Logins { tab: u64, origin: String, logins: Vec<storage::passwords::Login> },
    /// A login went through that is not saved yet (`update`: a new password for a saved one).
    PasswordAsk { tab: u64, cred: storage::passwords::Credential, update: bool, sequence: u64 },
    /// How many logins are saved for the active page's origin (the key in the address bar).
    SavedLogins { origin: String, n: usize },
    /// The login picked under a field, to fill into tab `tab` while it shows `origin`.
    PasswordFill { tab: u64, origin: String, user: String, pass: String },
    /// The tab's page started or stopped using the camera, microphone or screen capture.
    Media { id: u64, frame: String, live: bool },
    Do(Action),
    OpenTab(String),
    Suspended { id: u64, ok: bool },
    Script(String),
}
