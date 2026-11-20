# Kakangkung ↔ Ladybird host protocol

Transport: the host process (`kakangkung-ladybird-host`) is spawned by Kakangkung with the
profile directory and optional parent HWND. **stdin** carries commands; **stdout** carries
events. One JSON object per line. No pretty-printing.

## Process arguments

| Arg | Meaning |
|---|---|
| `--profile <dir>` | Profile folder (cookies, storage) |
| `--parent <hwnd>` | Windows HWND to parent page surfaces into (decimal or `0x…`) |
| `--version` | Print engine id and exit |

Environment: `KAKANGKUNG_LADYBIRD_HOST` may point Kakangkung at a non-default host binary.

## Commands (Kakangkung → host)

```json
{"cmd":"create","url":"https://example.com","x":0,"y":0,"w":800,"h":600}
{"cmd":"navigate","id":1,"url":"https://ladybird.org"}
{"cmd":"bounds","id":1,"x":0,"y":80,"w":1200,"h":700}
{"cmd":"visible","id":1,"visible":true}
{"cmd":"reload","id":1}
{"cmd":"stop","id":1}
{"cmd":"back","id":1}
{"cmd":"forward","id":1}
{"cmd":"screenshot","id":1,"path":"C:\\\\temp\\\\page.png"}
{"cmd":"print","id":1}
{"cmd":"zoom","id":1,"factor":1.1}
{"cmd":"zoom","id":1,"absolute":1.25}
{"cmd":"a11y","id":1}
{"cmd":"dom","id":1}
{"cmd":"console","id":1,"clear":false}
{"cmd":"eval","id":1,"js":"document.title"}
{"cmd":"capabilities","id":1}
{"cmd":"ext_reply","id":1,"extension":"my-ext","requestId":"1","response":{"ok":true}}
{"cmd":"ext_drain","id":1}
{"cmd":"download","url":"https://example.com/file.bin","path":"/tmp/file.bin"}
{"cmd":"download_action","downloadId":1,"action":"pause"}
{"cmd":"adblock","enabled":true,"allowlist":"news.example.com\\nother.example.com"}
{"cmd":"clear_data","cookies":true,"cache":true,"since":0}
{"cmd":"find","id":1,"query":"hello","forward":true,"matchCase":false}
{"cmd":"find","id":1,"query":"","end":true}
{"cmd":"permissions_defaults","geolocation":"block","microphone":"ask","notifications":"block"}
{"cmd":"permissions_list"}
{"cmd":"permissions_reset","origin":"https://maps.example","kind":"geolocation"}
{"cmd":"processes"}
{"cmd":"destroy","id":1}
{"cmd":"quit"}
```

## Events (host → Kakangkung)

```json
{"event":"ready","engine":"ladybird"}
{"event":"created","id":1}
{"event":"url","id":1,"url":"https://ladybird.org"}
{"event":"title","id":1,"title":"Ladybird"}
{"event":"history","id":1,"back":true,"forward":false}
{"event":"load","id":1,"done":false}
{"event":"load","id":1,"done":true}
{"event":"screenshot","id":1,"path":"C:\\\\temp\\\\page.png","ok":true}
{"event":"print","id":1,"ok":true}
{"event":"zoom","id":1,"ok":true,"factor":1.1}
{"event":"a11y","id":1,"tree":{"role":"document","name":"…","children":[]}}
{"event":"dom","id":1,"ok":true,"tree":{"type":"element","name":"HTML","children":[]}}
{"event":"console","id":1,"ok":true,"messages":[{"kind":"log","level":"log","args":["hi"]}]}
{"event":"eval","id":1,"ok":true,"result":"Ladybird"}
{"event":"capabilities","id":1,"ok":true,"caps":{"engine":"ladybird","wasm":true,"webgpu":true,"webgpu_adapter_api":true}}
{"event":"ext_message","id":1,"extension":"my-ext","requestId":"1","message":{"hello":true}}
{"event":"ext_reply","id":1,"ok":true}
{"event":"download_started","ok":true,"url":"https://example.com/file.bin","path":"/tmp/file.bin"}
{"event":"download","downloadId":1,"url":"https://example.com/file.bin","path":"/tmp/file.bin","received":0,"total":-1,"state":"in_progress","can_resume":false}
{"event":"download_removed","downloadId":1}
{"event":"adblock","ok":true,"enabled":true}
{"event":"clear_data","ok":true,"cookies":true,"cache":true}
{"event":"find","id":1,"ok":true,"activeMatch":1,"matchCount":3}
{"event":"permissions_defaults","ok":true}
{"event":"permissions_list","ok":true,"permissions":[{"kind":"geolocation","origin":"https://maps.example","allowed":false}]}
{"event":"processes","ok":true,"processes":[{"pid":1234,"kind":"WebContent","ws":1048576,"site":"https://example.com/"}]}
{"event":"permissions_reset","ok":true}
{"event":"page_ipc","id":1,"message":"kk:pw:focus:{…}"}
{"event":"web_content_crashed","id":1,"reason":"ProcessCrash"}
```

`engine` is `"ladybird"` when LibWeb is linked, `"stub"` for the offline host.

**KB-25 crash recovery:** LibWeb respawns WebContent on crash; the host emits `web_content_crashed`
(`reason`: `ProcessCrash` | `RejectedIPC` | `ExceededMemoryLimit`). Kakangkung reloads the tab
URL except for `ExceededMemoryLimit`. Cold start after an unclean exit uses a `session.running`
profile marker to force last-session restore.

With `HAVE_LADYBIRD=1`:
- **screenshot** → wait for presented CPU frame, then `take_frame_snapshot` / `displayed_frame` + `PNGWriter`
- **print** → `run_javascript("window.print()")`
- **zoom** → page CSS zoom on `document.documentElement` (`factor` multiplies, omit resets to 1.0, `absolute` sets)
- **a11y** → `inspect_accessibility_tree` / `on_received_accessibility_tree`
- **dom** → `inspect_dom_tree` / `on_received_dom_tree`
- **console** → buffered `on_console_message` (optional `clear`)
- **eval** → `js_console_input` / `on_received_js_console_result`
- **capabilities** → live LibWeb feature probe (includes `webgpu_adapter_api` for `navigator.gpu.requestAdapter`)
- **ext_reply** → resolves a pending `chrome.runtime.sendMessage` callback in the page

Resources: host prefix must expose `share/Lagom` (symlink from Ladybird `Build/release/share/Lagom`) so `resource://ladybird/…` (WebCompat.json) resolves without site-compat warnings.

Without LibWeb linked, those commands return `ok:false` (no fake trees / stub screenshots).

## MV3 content scripts (KB-3)

On navigation the LibWeb host scans `<profile>/extensions/*/manifest.json`
(skipping folders with `.disabled`), match-tests the page URL against each
`content_scripts[].matches` / `exclude_matches`, injects a real `chrome.runtime`
bootstrap (`getManifest` from disk, messaging with request ids), injects CSS via
`<style>`, then `run_javascript`s each listed JS file. Timing: `document_start` on
load start; `document_end` + `document_idle` on load finish.

`chrome.runtime.sendMessage` pushes onto `window.__kk_ext_queue` (and logs
`kk-ext:…`). The host drains the queue on load-done and via `ext_drain`, emits
`ext_message`, and delivers to same-page `onMessage` listeners. Kakangkung
replies with `ext_reply`.

Clients that `wait` on `load` with `done:true` can rely on **back** / **forward** / **reload**
always emitting that event (even when history is empty — a no-op ack) so waiters never hang.

## Downloads (KB-6)

Real LibWeb `FileDownloader` (not WebView2). Host registers a `FileDownloaderObserver` and
emits `download` / `download_removed` as transfers progress. Start with `download` (url +
absolute path); control with `download_action` (`pause` / `resume` / `cancel`).

Kakangkung demuxes those events in every host wait loop and polls host stdout while idle
(`Host::poll_events`). Live rows feed `platform::downloads()` / the Downloads page; finished
non-incognito rows go through `take_finished` → `PastDownload` SQLite. Pause / resume / cancel
from the Downloads page call `Host::download_action` when `ladybird_owns(id)`.

## Adblock (KB-7)

`adblock` loads Kakangkung's ad-network domain list into LibWeb `ContentFilter` /
`ContentBlocker` so `ResourceLoader` refuses matching requests. Domain file:
`data/adblock-domains.txt` (or `KAKANGKUNG_ADBLOCK_LIST`). `enabled:false` clears patterns.
Document navigations are also gated in Rust via EasyList (`blocks_document_navigation`).
Per-site allowlist is enforced in Rust; patterns stay global in LibWeb when enabled.

## Passwords (KB-10)

After load, Kakangkung may `eval` `assets/passwords.js` with a Ladybird `window.ipc`
polyfill that logs `kk-pw:` + message. The host forwards those as `page_ipc` events.
Fill uses `eval` of `window.__kkPwFill(user, pass)` gated on `location.origin`.

## Clear browsing data (KB-11)

`clear_data` calls LibWeb `Application::clear_browsing_data`: cookies + site storage
(`delete_site_data`) and/or HTTP cache (`delete_cached_files`). `since` is Unix ms
(0 = all time). History stays in Kakangkung SQLite. Fail closed when LibWeb is unavailable.

## Find / translate / reader (KB-12)

`find` uses LibWeb `ViewImplementation::find_in_page` (real highlights + match counts).
`end:true` or an empty query clears the search. Translate and Reading mode use host
`eval` (`assets/reader.js` / Google Translate navigation). Print preview is deferred.

## Process model (KB-13 / KB-22)

One `kakangkung-ladybird-host` process owns many views. Task Manager seeds that host PID
(and ToolHelp children) as kind `Ladybird`. KB-22 adds `processes` → LibWeb
`process_snapshots(ProcessManager)` so WebContent / RequestServer / helper PIDs appear
with their site URLs. LibWeb already swaps WebContent on cross-site navigation; Kakangkung
surfaces that tree rather than inventing fake per-tab PIDs.

## PDF / sync / casting (KB-18 / KB-21 / KB-23)

Finished `.pdf` downloads open as `file://` tabs in the Ladybird host (LibWeb navigates /
renders when linked). Folder/Drive sync covers bookmarks/history/tabs/settings/extensions
and (KB-21) passwords when that sync type is on.

### KB-23 blocker — Presentation / Chromecast casting

LibWeb (headers surveyed under Ladybird `Libraries/LibWeb` + IDL) exposes **no**
`PresentationRequest`, `navigator.presentation`, `RemotePlayback`, or Chromecast/Cast
receiver/session APIs. Names containing "Presentation" in-tree
(`CompositorPresentationClient`, Qt `DownloadPresentation`, DRM presentation helpers)
refer to **GPU/compositor frame presentation**, not media casting.

Until LibWeb lands a real Presentation / Cast surface, Kakangkung will not ship a fake
cast picker or stub protocol. Revisit when Ladybird adds the Web APIs.

## Permissions (KB-19)

`permissions_defaults` / `permissions_list` / `permissions_reset` wrap LibWeb
`PermissionManager` for geolocation, microphone, and notifications. Settings site.*
ask/block maps to global defaults. Camera and process-per-site isolation remain deferred.

## Print preview (KB-20)

Ctrl+P / ⋮ Print on a Ladybird tab captures a real page screenshot (`screenshot` cmd),
opens it as a preview tab, then confirms before `print` (`window.print()`). Paginated
print-layout preview is not productized yet.
