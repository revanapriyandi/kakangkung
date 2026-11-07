# Settings (kakangkung://settings)

Every feature of Kakangkung can be switched or tuned in Settings. This page lists each setting,
the key it is stored under, its values (default in **bold**) and when a change takes effect.

- **Live**: applies at once to every open tab and window part.
- **New tabs**: applies to tabs opened (or loaded) after the change.
- **Restart**: applies the next time Kakangkung starts.

Settings are stored in the profile database (`%LOCALAPPDATA%\Kakangkung\kakangkung.db`). The list
of keys, allowed values and defaults lives in one place, `crates/app/src/prefs.rs` (`SETTABLE`,
`URL_SETTINGS`). The Settings page is native (`crates/app/src/native/settings.rs`): every switch,
list and address field carries its key as its id, and a test checks that every key has one.
Each section of the Settings page is a page of its own (`kakangkung://settings/<section>`); the search
box searches all of them.
Invalid stored values fall back to the default. Settings sync to your other PCs with your account,
except *Clear browsing data when you exit* and *graphics acceleration*, which belong to one PC.

To add a feature: add its key to `SETTABLE`, read it in `Settings::read`, act on it, add a
row with a control whose id is the key in `native/settings.rs`, and a row here.

## You and sync

| Setting | Key | Values | Applies |
|---|---|---|---|
| Sign in / out | `account.*` | name and email | Live |
| Sync | `sync.enabled` | on / **off** (on by itself after sign-in) | Live |
| Sync folder | `sync.folder` | a folder your cloud drive syncs (**OneDrive\Kakangkung Sync**) | Live |
| What to sync | `sync.types` | bookmarks, history, open tabs, settings, extensions | Live |
| Import bookmarks and history | — | Chrome, Edge, Brave, Firefox, Vivaldi, Opera; "keep in sync" | Live |

## Autofill and passwords

| Setting | Key | Values | Applies |
|---|---|---|---|
| Offer to save passwords | `privacy.passwords` | **on** / off | Live |
| Autofill addresses and forms | `privacy.autofill` | **on** / off | Live |
| Delete saved passwords and autofill data | — | button | Live |

WebView2 keeps saved passwords inside the profile and has no API to list them one by one.

## Privacy and security

| Setting | Key | Values | Applies |
|---|---|---|---|
| Clear browsing history | — | button | Live |
| Clear all browsing data | — | button (history, cookies, cache, site data) | Live |
| Clear browsing data when you exit | `privacy.clear_on_exit` | on / **off** | On exit (and again at the next start) |
| Safe Browsing (SmartScreen) | `privacy.safe_browsing` | **on** / off | Live |
| Always use secure connections (HTTPS-First) | `privacy.https_first` | **on** / off | Live |
| Tracking prevention | `privacy.tracking` | off / basic / **balanced** / strict | Live |
| Site permissions | — | list with Reset (camera, microphone, location, notifications, …) | Live |

## Performance

| Setting | Key | Values | Applies |
|---|---|---|---|
| Memory Saver | `memory.mode` | moderate (2 h) / **balanced (5 min)** / maximum (30 s) / off | Live |
| Keep calls running | `performance.protect_media` | **on** / off | Live |
| Keep pinned tabs running | `performance.protect_pinned` | **on** / off | Live |

Tabs playing sound are always kept running.

## Appearance

| Setting | Key | Values | Applies |
|---|---|---|---|
| Mode | `appearance.theme` | **device** / light / dark | Live |
| Show Home button | `appearance.home_button` | on / **off** | Live |
| Home page | `appearance.home_url` | an http(s) address (**empty = new tab page**) | Live |
| Show bookmark star | `appearance.bookmark_star` | **on** / off | Live |
| Show Extensions button | `appearance.extensions_button` | **on** / off | Live |
| Show tab search button | `appearance.tab_search_button` | **on** / off (Ctrl+Shift+A always works) | Live |
| Always show full URLs | `appearance.full_urls` | on / **off** | Live |
| Page zoom | `appearance.zoom` | 50 … **100** … 200 % | Live (resets per-tab zoom) |
| Animations | `appearance.animations` | **on** / off (built-in pages fade in, New Tab tiles rise one after another, switches slide, hover highlights fade, pop-ups unfold, new tabs widen) | Live |
| Animation speed | `appearance.animation_speed` | fast / **normal** / relaxed (shown while Animations is on) | Live |
| Smooth scrolling | `appearance.smooth_scrolling` | **on** / off (built-in pages glide to where you scroll) | Live |

## Tabs and windows

| Setting | Key | Values | Applies |
|---|---|---|---|
| New tab page | `tabs.new_tab_page` | **Kakangkung page** / blank page / a page of your choice | New tabs |
| New tab page address | `tabs.ntp_url` | an http(s) address | New tabs |
| Where new tabs open | `tabs.position` | **at the end** / next to the current tab | New tabs |
| Switch to tabs opened from links | `tabs.link_focus` | **on** / off (open in the background) | New tabs |
| When you close the last tab | `tabs.close_last` | **close the window** / keep the window with a new tab | Live |
| Ask before closing a window with several tabs | `tabs.confirm_close` | on / **off** | Live |
| Tab groups | `tabs.groups` | **on** / off (off removes existing groups) | Live |
| Split view | `tabs.split_view` | **on** / off (off closes an open split) | Live |
| Mute a tab by clicking its speaker icon | `tabs.speaker_mutes` | **on** / off | Live |
| New window (Ctrl+N) | `tools.new_window` | **on** / off | Live |
| Incognito windows (Ctrl+Shift+N) | `tools.incognito` | **on** / off | Live |
| Picture in picture | `tools.pip` | **on** / off | Live |
| Reading mode | `tools.reader` | **on** / off | Live |
| Translate page | `tools.translate` | **on** / off | Live |

Switched-off features disappear from the ⋮ menu and the tab right-click menu.

## Search engine

| Setting | Key | Values | Applies |
|---|---|---|---|
| Search engine | `search.engine` | **Google** / Bing / DuckDuckGo / Yahoo! / Ecosia / Brave / custom `https://…%s` | Live |
| Search suggestions | `search.suggestions` | **on** / off | Live |
| Suggestions from history and bookmarks | `search.history_suggestions` | **on** / off (also inline completion) | Live |

## Default browser

"Make default" registers Kakangkung for http, https, .htm and .html (current user only, no admin
rights) and opens Windows *Settings > Default apps*, where Windows lets you choose it. The page
shows whether Kakangkung is the default.

## On startup

| Setting | Key | Values | Applies |
|---|---|---|---|
| On startup | `startup` | **open the New Tab page** / continue where you left off / open specific pages | Restart |
| Pages to open | `startup.pages` | up to 10 http(s) addresses, one per line | Restart |

## Languages

| Setting | Key | Values | Applies |
|---|---|---|---|
| Interface language | `languages.ui` | **Windows language** / English / Bahasa Indonesia | Live |
| Translate pages into | `languages.translate_to` | **the browser's language** / Indonesian / English / … (21 languages) | Live |

## Downloads

| Setting | Key | Values | Applies |
|---|---|---|---|
| Location | `downloads.folder` | a folder (**Downloads**) | Live |

## Accessibility

| Setting | Key | Values | Applies |
|---|---|---|---|
| Show a highlight on the focused object | `accessibility.focus_highlight` | on / **off** | Live |
| Animations, smooth scrolling | see Appearance > Motion | | |
| Page zoom | see Appearance | | |

## System

| Setting | Key | Values | Applies |
|---|---|---|---|
| Use graphics acceleration when available | `system.hardware_acceleration` | **on** / off | Restart |
| Update Kakangkung automatically | `system.auto_update` | **on** / off (check GitHub releases, download and verify in the background, install when Kakangkung closes; see docs/UPDATES.md) | Live |
| Open your computer's proxy settings | — | opens Windows proxy settings | — |

## Reset settings

"Restore settings to their original defaults" puts every setting above back to its default.
Account, sync, history, bookmarks and the download folder are kept.
