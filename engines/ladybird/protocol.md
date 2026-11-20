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
{"event":"print","id":1,"ok":false,"reason":"stub"}
{"event":"a11y","id":1,"tree":[{"role":"document","name":"…","children":[]}]}
{"event":"eval","id":1,"ok":false,"result":"…"}
```

`engine` is `"ladybird"` when LibWeb is linked, `"stub"` for the offline host.

The Rust stub host (`crates/ladybird-host`) JSON-escapes string fields in events. Clients that
`wait` on `load` with `done:true` can rely on **back** / **forward** / **reload** always emitting
that event (even when history is empty — a no-op ack) so waiters never hang.

C++ host string fields are still lightly escaped until LibWeb production wiring; prefer the Rust
stub for agent/MCP work until then.
