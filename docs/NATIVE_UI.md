# Native UI: plan and status

Goal: no HTML in Kakangkung's own UI. Like Chrome and Edge, the browser draws its frame itself;
only web pages are rendered by the engine (WebView2, later also Servo). Every built-in page is
redesigned to look like Chrome's.

## Decisions (with evidence)

| Option | Result | Evidence |
|---|---|---|
| Slint (winit) frame + WebView2 page | Frame and page work, ~100 MB less memory than the HTML UI, but **no pop-up can show** over its window: neither Slint's own pop-up windows nor native Windows menus (`TrackPopupMenuEx` blocks, no `#32768` window appears). A browser needs pop-ups everywhere, so Slint is dropped. | `spikes/native-shell`, probes in this repo's history |
| **tao + Direct2D/DirectWrite** (chosen) | Native Windows menus show above the page, dark mode, submenus, shortcuts. The same approach Chrome uses (it draws its UI itself). | Stage 1 below, clicked for real |

Memory of the spike (one example.com tab, isolated profile): HTML UI 274 MB private / 10
processes; native frame 155–173 MB / 7 processes. The saving comes from dropping the two UI
webviews (tab strip/toolbar and pop-up) and their renderer processes.

## Stages

Each stage leaves a working browser; `--self-test` must pass after each.

1. **Native menus — done.** ⋮ (with Zoom and More tools submenus), tab right-click, profile and
   site-info menus are native Windows menus (`platform::menu`), themed light/dark from Settings.
   The self-test reads their entries through `Browser::menu_probe` (a shown menu would wait).
2. **Native frame — done.** A child window at the top of the tao window, drawn with Direct2D +
   DirectWrite: tab strip (tabs, groups, pinned, audio/mute, drag to reorder; site favicons are
   still a globe icon — decoding them with WIC is the open item of this stage),
   toolbar buttons with vector icons, window caption buttons, and the address bar as a native
   EDIT control (IME, clipboard, screen readers for free). It emits the same commands the HTML
   tab strip sends today (`select`, `close`, `nav`, …), so `Browser::on_ipc` stays the single
   place that acts on them; `chrome.html` and the UI webview are deleted. The address bar's
   behaviour (typing, inline completion, ↑/↓, Enter, Escape) lives in `browser/omnibox.rs`;
   shortcuts typed in it reach the browser through the control's subclass. The tabs' WebView2
   environment is now created by the first tab. Code: `crates/platform/src/frame/`
   (`mod.rs` API, `style.rs`, `layout.rs`, `paint.rs`, `input.rs`).
3. **Native pop-ups — done.** Address-bar suggestions and tab search are one native list pop-up
   (`crates/platform/src/popup/`: `mod.rs` API, `paint.rs`, `input.rs`): a borderless window owned
   by the browser window, Windows 11 rounded corners and shadow, drawn with Direct2D in the
   frame's colours. Suggestions never take the keyboard (the address bar keeps typing); tab
   search activates the pop-up for its native filter box and closes when it loses focus.
   `overlay.html`, its webview and the `kakangkungui` protocol are deleted: the browser's own
   UI no longer contains any HTML. Checked for real: typing "rust" shows the suggestion list
   under the address bar; the ⌄ button opens tab search. Like Chrome's, the button sits at the start of the tab strip
   (only the window buttons are on the right) and the panel lists Open Tabs (most recently used
   first) and a foldable Recently Closed section, each entry with a site badge, "host • 3 mins
   ago" and, for open tabs, a × that closes it while the panel stays open
   (`crates/app/src/browser/tab_search.rs`). A click above a tab, up to the window's top edge,
   picks that tab. Self-test step `tabstrip` checks both with the frame's own button messages. Not yet confirmed with synthetic keys:
   Ctrl+Shift+A (the test tool's modifier keys did not reach the address bar's key report).
4. **Native built-in pages, designed like Chrome — done.** Every built-in page is native and
   every `assets/*.html` page is deleted; `assets/` keeps only the logo and the scripts injected
   into web pages (media watch, reading mode, focus highlight).
   - **Page host (done):** `crates/platform/src/page/` (`model.rs` the description a page is
     drawn from, `layout.rs`, `paint.rs`, `input.rs`, `mod.rs` API). Chrome's layout: header
     with a search box, navigation on the left, section title and cards of rows; switches,
     dropdowns (native menus), radio choices, buttons, native text fields, links. Smooth wheel
     scrolling, a slim scrollbar that only shows while the pointer is over the page, a message
     pill after an action. Light and dark from Settings > Appearance.
   - **Tabs without a webview (done):** a tab whose address is a native page has no webview and
     no renderer process; one page window per browser window is placed where the tab's page goes
     (also in split view). Code: `crates/app/src/browser/page_host.rs`, routing in
     `crates/app/src/native/mod.rs`.
   - **Same handlers (done):** a control's action calls the same `/api` handler the HTML pages
     use, answering through `Reply::Native` instead of HTTP, so no setting is implemented twice.
   - **Settings (done):** native, `settings.html` deleted. Each section is a page of its own
     (`kakangkung://settings/privacy`, …), chosen in the navigation; the search box searches all
     sections. Self-test step `nativepages`: opened without a webview, 15 sections / 16
     navigation entries, search, the navigation switches the page and the address, a switch saves
     its setting.
   - **History and Bookmarks (done):** native, `history.html` and `bookmarks.html` deleted
     (`crates/app/src/native/lists.rs`). History is grouped by day ("Today · …", "Yesterday ·
     …"), each entry opens its page and has a remove button; its search box asks the browser
     (the same `history?q=` query), so it searches everything, not only what is listed. The
     navigation links to tabs from other devices, import, and clearing history (with a
     confirmation). Bookmarks lists every bookmark with a remove button; its search box keeps the
     matching rows. Local dates and times come from Windows' time zone rules
     (`platform::local_time`). Self-test step `nativepages` also opens both (Ctrl+H,
     Ctrl+Shift+O) with seeded example entries and searches History.
   - **Downloads, Extensions and Task manager (done):** native, `extensions.html` and
     `taskmanager.html` deleted (`crates/app/src/native/tools.rs`).
     - *Downloads* (Ctrl+J) is a page now, not WebView2's flyout: every tab reports its
       downloads (`platform::downloads`, `DownloadStarting` plus each download's progress
       events), grouped by day with size and percentage, and Pause / Resume / Cancel while
       running, Open / Show in folder / Remove when done. The navigation opens the downloads
       folder or clears finished entries. The list covers this run (not kept after a restart).
     - *Extensions*: one card per extension (name, id, on/off switch, Remove with a
       confirmation), Load unpacked, Import from a browser, and what WebView2 extensions can do.
     - *Task manager* (Shift+Esc): memory in use, tabs running / frozen / not loaded (native
       pages count as none), Memory Saver mode, and every process with its private working set
       and commit; it refreshes itself every 2 seconds while shown.
     - Self-test step `nativepages` opens all three. Not yet checked with a real file download.
   - **New Tab, Incognito, Account, Sync and Welcome (done)** (`native/start.rs`,
     `account.rs`, `welcome.rs`).
     - *New Tab* and *Incognito* are start pages (`platform/src/page/start.rs`). New Tab is just
       the logo and a search box that hands the typing to the address bar, like Chrome's;
       Incognito adds its icon, title, text and two cards. Incognito pages are always dark.
     - *Account* (kakangkung://signin): signed out, a form (name, email, what to sync) whose
       fields are kept by the browser until it is sent; signed in, the account, Sign out (with a
       confirmation) and the sync types, applied at once.
     - *Sync*: the switch, the folder, Sync now, and the tabs open on the other devices.
     - *Welcome*: import from each browser profile on this PC, its extensions, and "Keep in
       sync" per browser.
     - Incognito is always a window of its own, never a tab: Ctrl+Shift+N and ⋮ > New
       Incognito window open one from any window (also from an Incognito one), and the
       Incognito page's address typed elsewhere opens one too. It starts on the Incognito page.
     - Back works across native and web pages (a tab remembers the addresses it left when it
       switched between the two).
     - The background "spare" New Tab webview and its setting (`performance.spare_tab`) are
       removed: a native New Tab page opens at once without a renderer.
     - The self-test's API script runs in a blank built-in page (`kakangkung://selftest`), the
       only page the built-in host still serves; `nativepages` visits every native page and
       saves screenshots with `KAKANGKUNG_SHOT_DIR`.

## Memory, start-up, motion and art

Measured on the development laptop (Core i5-13420H, 16 GB) with the release build and a fresh profile (private working set, Task
Manager's "Memory", summed over the browser and its WebView2 processes):

| | Before | After |
|---|---|---|
| New Tab page only (no web page) | 51.3 MB, 1 process | about 25 MB (23–33 MB between runs) |
| One web page (example.com), all processes | 146.6 MB, 7 processes | 114–125 MB, 7 processes (Kakangkung itself 26.9 → 9.6 MB) |
| Window on screen when the first page is a web page | 969 ms | 75–212 ms |

- **Direct2D without a graphics device each.** Every hardware Direct2D window target made its
  own D3D device (about 14 MB each). The tab strip/toolbar and the pop-ups are small, so they are
  drawn by the CPU; built-in pages are too on ordinary screens (5–8 ms for a whole 1600×900
  page) and use the GPU only above 2.2 megapixels, so scrolling stays smooth on very large
  screens. A built-in page's drawing surface is let go while a web page is shown.
- **The window shows at once.** It used to wait for WebView2 (most of a second) behind a splash
  logo; now the native frame shows as soon as it exists and the page area fills in when WebView2
  is ready. The splash is gone. Applying the download folder and "clear browsing data when you
  exit" now waits for the first webview (they were skipped when the first tab was a built-in
  page).
- **No spare New Tab webview** (removed with the native New Tab page, see stage 4).
- WebView2's own processes (browser, GPU, network, storage, renderers) are left as they are:
  cutting them would cost features (video and page acceleration, isolation).

**Motion** (`crates/platform/src/motion.rs`; Settings > Appearance > Motion: Animations, Animation
speed fast/normal/relaxed, Smooth scrolling): built-in pages fade in and rise (the clock starts at
the first frame drawn), New Tab tiles rise one after another, switches slide, hover highlights
fade in, the pop-ups unfold downwards and new tabs widen into place. Frames run at about 60 per
second only while something moves.

**Opening**: the New Tab page is the logo and the search box only, big and in the middle of
the page (no tiles, links or greeting); when the browser opens on it, the logo drops in and
bounces until it rests. When the first tab is a web page, the page area shows the logo hopping
quickly up and down (squashed for a moment on the ground, its shadow growing and shrinking)
while WebView2 starts, until the page is on screen. Both follow Settings > Appearance > Motion.

**Art** (`crates/platform/src/page/art.rs`): vector illustrations drawn with Direct2D, no image
files: empty states (no history, no matches, no bookmarks, no downloads, no extensions, no other
devices) and the Welcome page show an icon on a floating card over soft colour shapes with dots, a
ring and sparkles; the start pages have small shapes around the logo. Large screen-wide gradients
were tried and dropped: drawn by the CPU they show bands.

**Program icon**: `crates/app/build.rs` draws the brand mark at 16–256 px into an .ico and embeds
it with version details ("Kakangkung") using the Windows SDK's rc.exe, so Task Manager and
Explorer show the logo and the name.

## Rules for the native code

- Unsafe Win32/Direct2D calls live only in `crates/platform` (as today).
- The frame and pages draw from plain state (`ChromeState`, settings values), never from JSON
  strings passed to a script.
- Every visible feature keeps its setting (docs/SETTINGS.md) and its self-test step.
