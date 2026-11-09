# Changelog

All notable changes are listed here. Versions follow [Semantic Versioning](https://semver.org).

## [Unreleased]

## [0.3.0] - 2026-11-09

### Fixed
- Muted video, picture-in-picture and sibling frames now retain media protection correctly;
  detached stopped media releases its references instead of keeping a tab protected forever.
- Google display names and profile photos now refresh for existing signed-in accounts and
  render from a small native image cache, with an initial fallback.
- Web Store packages stay staged until consent; failed replacement restores the previous
  files. Store buttons reject stale SPA routes and duplicate clicks.
- The Downloads toolbar button appears automatically on download activity without changing
  the pinned-button preference.
- Task manager totals include every identified WebView2 environment process, even when
  Windows groups the runtime separately from the app.
- Password origins, existing vault entries, CSV fidelity and form targeting; delayed save
  callbacks survive login redirects and stale callbacks cannot replace newer offers.
- Google account switching no longer reuses another account's mirror caches or reads a
  different refresh credential midway through selecting an account's sync folder.
- Live Chrome history imports include committed WAL data. Automatic refresh skips unchanged
  sources, retries incomplete bookmark reads, and leaves password imports to an explicit action.
- Audio/media and changed protection wake hidden tabs correctly; failed lifecycle commands
  remain retryable and replacing split views hides obsolete tabs.

### Added
- Native tab hover cards with bounded in-memory snapshots and memory diagnostics; affected
  WebView2 runtimes use an explicitly labelled JavaScript-heap fallback. Cards place the
  title/domain above a full-width screenshot and memory usage with an icon in the footer.
- Docked DevTools panel and a page Inspect route; native hosting errors close the inspector
  rather than leaving it detached. A native clipping viewport removes Chromium's extra
  client-painted window title bar while retaining its developer tool tabs.
- Ad blocker toolbar controls, exact-site pause, shared EasyList/EasyPrivacy network matching,
  and bounded YouTube player-data filtering with basic ad-element hiding.
- Maximum Memory Saver is the default for new/reset profiles; protected active media stays
  running and existing saved settings are retained.
- Synthetic password-form regression fixtures, isolated native parity checks, and a
  [feature investigation with verification boundaries](docs/CHROME_PARITY.md).

### Verification and limits
- The signed-out YouTube/GitHub/Instagram/Facebook playback smoke test passed, including
  muted video continuing in a hidden tab; the approximately 100 MiB total-memory target
  remains unmet. Hover-card JS heap values are not whole-tab or whole-browser memory.
- Google consent and authenticated profile/sync delivery still require the application's
  own OAuth setup and have not been verified against a personal Google account.

## [0.2.0] - 2026-11-08

### Fixed
- No console window opens next to Kakangkung any more (the program was built as a console app).
- Many tabs: the tab strip scrolls (mouse wheel or the arrows at its ends) and keeps the selected
  tab in view, instead of pushing tabs off the window where they could not be reached.
- Tabs show the site's icon as soon as it is known, a turning ring while the page loads (it stood
  still), and the site's address until the page has a title (it said "New Tab").
- Confirmation questions on built-in pages are asked in the interface language.
- A tab showing a file on its own (video, audio, PDF) is named after the file, not the site.
- Memory Saver: a tab whose freeze WebView2 refuses is tried again later instead of being counted
  as frozen while it keeps running.

### Added
- Several URLs on the command line open as tabs; `--user-data-dir=<folder>` and `--no-first-run`
  work as in Chrome.
- Memory benchmark against Chrome and Edge: `bench/run.ps1`, results in docs/BENCHMARKS.md.
- Pin to toolbar, like Chrome: extensions from the extensions menu (*Pin to toolbar*), and
  Downloads, History, Bookmarks and Task manager in Settings > Appearance. Nothing is pinned by
  default; right-click a pinned button to unpin it.

- Installing from the Chrome Web Store asks in Kakangkung's own dialog, like Chrome: the
  extension's name, what it can do (from its manifest, in Chrome's words), Add extension / Cancel.
  It is downloaded first and only added after Add; Cancel deletes it. The store's "Switch to
  Chrome" banner is hidden.
- Password manager of its own: offers to save a password after you sign in, lists the saved
  logins under a login field and fills the one you pick, and shows a key in the address bar.
  Passwords are sealed with your Windows account (DPAPI). The new Passwords page searches,
  copies and shows them (after Windows Hello), deletes, imports from Chrome, Edge, Brave and
  the other Chromium browsers and from CSV files of Google Password Manager, Firefox, Safari,
  Bitwarden, 1Password, LastPass, Proton Pass, Dashlane and NordPass, and exports to CSV. It
  links to other password managers (their Chrome Web Store extensions) and explains passkeys.
- Files: Ctrl+O (and ⋮ > More tools > Open file…) opens a PDF, page, picture, video or any file
  from your PC in a tab; Kakangkung is offered under Windows' *Open with* for PDF files, and
  *Make default* in Settings also registers it for PDF, SVG and WebP. A file path on the command
  line opens (Explorer's "Open with"). A downloaded PDF, picture, video or text file opens in a
  tab instead of another app.
- New Tab page: the search box is a real text field (suggestions open under it, ↑ ↓ pick, Enter
  goes), and eight shortcut tiles show the most visited sites with their own icons (fetched once
  from each site, kept in "Site icons"), topped up with well-known sites while history is short.

### Changed
- Every menu (⋮, profile, tab and group right-click, extensions, site info) and the install
  dialog are Kakangkung's own pop-ups instead of Windows menus: icons, dividers, checks,
  submenus that open in place with a way back, keyboard (↑ ↓ Enter Escape), dark mode, and the
  opening animation. A click anywhere else closes them. Submenus (More tools) open beside
  the menu, which stays in view: pointing at the row, clicking it or → opens one; ← or Escape
  goes back. Zoom is one row of the menu, like Chrome's: − 100% + and full screen (← → on the
  row zoom too).
- The Task manager counts memory exactly like Windows Task Manager: every Kakangkung process
  (all windows, every WebView2 process, the crash handler), each at the value of Windows Task
  Manager's Memory column. Windows app grouping can differ from the browser process set.

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
