# Changelog

All notable changes are listed here. Versions follow [Semantic Versioning](https://semver.org).

## [0.1.1] - 2026-11-08

### Added
- Extensions from the Chrome Web Store: on an extension's page the store's button reads
  *Add to Kakangkung*. The toolbar's extensions menu lists them and opens their pop-ups.
- Sign in to Kakangkung with Google: bookmarks, history, open tabs and settings sync through a
  hidden app folder in your Google Drive (see docs/GOOGLE_SIGNIN.md).
- Google's own sign-in works on websites, including sign-in pop-ups, which now open as tabs and
  keep their link to the page that opened them.
- Built-in ad blocker, on by default (*Settings > Privacy and security > Block ads*).
- Delete browsing data by type (history, cookies, cache) within a time range; site settings for
  sound and images; "Ask where to save each file" and "Show downloads when done"; preferred
  language for websites.
- The installer asks for the language (English or Bahasa Indonesia), and Kakangkung starts in it.

### Changed
- Uninstalling closes Kakangkung if it is open and asks whether to delete your data too.
- The release files can be code-signed (when the signing secrets are set); every CI run builds
  the installer and tests a silent install and uninstall.

### Fixed
- The self-test no longer stops at the Downloads page, and a time-out is reported as a failure.

## [0.1.0] - 2026-11-08

First public version.

- Native interface drawn with Direct2D: tab strip, toolbar, address bar, menus, pop-ups and every
  built-in page; no HTML in the browser's own UI.
- Tabs with groups (right-click a group to rename, recolor, collapse, ungroup or close it),
  site icons, pinning, muting, split view, tab search and session restore.
- Address bar with inline completion and search suggestions; choice of search engine.
- Built-in pages: New Tab, Settings, History, Bookmarks, Downloads, Extensions, Task manager,
  Account, Sync, Welcome, Incognito. The Downloads list is kept across restarts (never for
  Incognito).
- Interface in English or Bahasa Indonesia, following the Windows language by default.
- Memory Saver, Incognito windows, unpacked extensions, import from Chrome, Edge, Brave and
  Firefox, sync through a OneDrive folder.
- Animations with a speed setting, smooth scrolling, illustrations, light and dark mode.
- Installer for Windows x64 and ARM64, portable zips, and automatic updates verified with
  SHA-256.
