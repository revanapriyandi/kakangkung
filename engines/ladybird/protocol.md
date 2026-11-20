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
```

`engine` is `"ladybird"` when LibWeb is linked, `"stub"` for the offline host.

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
