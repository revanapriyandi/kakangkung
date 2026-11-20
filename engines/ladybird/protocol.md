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

Strings in this first revision are not fully JSON-escaped; URLs and titles must not contain
raw quotes or newlines. A later revision should switch to a proper encoder once the host is
wired into production Windows builds.
