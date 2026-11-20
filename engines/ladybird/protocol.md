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
{"cmd":"a11y","id":1}
{"cmd":"eval","id":1,"js":"document.title"}
{"cmd":"capabilities","id":1}
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
{"event":"a11y","id":1,"tree":{"role":"document","name":"…","children":[]}}
{"event":"eval","id":1,"ok":true,"result":"Ladybird"}
{"event":"capabilities","id":1,"ok":true,"caps":{"engine":"ladybird","wasm":true,"webgpu":true}}
```

`engine` is `"ladybird"` when LibWeb is linked, `"stub"` for the offline host.

With `HAVE_LADYBIRD=1`:
- **screenshot** → `ViewImplementation::take_frame_snapshot` + `PNGWriter`
- **print** → `run_javascript("window.print()")`
- **a11y** → `inspect_accessibility_tree` / `on_received_accessibility_tree`
- **eval** → `js_console_input` / `on_received_js_console_result`
- **capabilities** → live LibWeb feature probe (CSS/JS/WASM/WebGL/WebGPU/media codecs)

Without LibWeb linked, those commands return `ok:false` (no fake trees / stub screenshots).

## MV3 content scripts (KB-3)

On `load` `done:true`, the LibWeb host scans `<profile>/extensions/*/manifest.json`
(skipping folders with `.disabled`), match-tests the page URL against each
`content_scripts[].matches` pattern, injects a real `chrome.runtime` bootstrap for
that extension id, then `run_javascript`s each listed JS file from disk. No empty
stub scripts.

Clients that `wait` on `load` with `done:true` can rely on **back** / **forward** / **reload**
always emitting that event (even when history is empty — a no-op ack) so waiters never hang.
