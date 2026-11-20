# Ladybird — Kakangkung’s only page engine

Kakangkung is a **native** browser: the chrome (tabs, toolbar, menus, built-in pages) is drawn
with Win32 + Direct2D. Web pages are rendered by **[Ladybird](https://ladybird.org)** (LibWeb /
LibWebView) only.

**Microsoft Edge WebView2 and Chromium are not used** for page rendering. There is no fallback
to Chrome’s engine.

## Architecture

```
┌─────────────────────────────────────────────┐
│ Kakangkung (Rust) — native chrome           │
│  tabs · omnibox · Direct2D · settings       │
│  engine = Ladybird only                     │
└───────────────┬─────────────────────────────┘
                │ stdin/stdout JSON lines
                │ (engines/ladybird/protocol.md)
┌───────────────▼─────────────────────────────┐
│ kakangkung-ladybird-host                    │
│  stub (Rust, crates/ladybird-host) or       │
│  LibWeb UI port (engines/ladybird, on Boat) │
└─────────────────────────────────────────────┘
```

Ladybird has no public C embedding API. The supported path is a **UI port**: subclass
`WebView::ViewImplementation` ([Porting.md](https://github.com/LadybirdBrowser/ladybird/blob/master/Documentation/Porting.md)).
`engines/ladybird/` is that port. Until LibWeb is linked, `crates/ladybird-host` provides a
protocol-compatible stub that parents a child HWND into Kakangkung and paints a stub page.

## Development on Boat (preferred)

Use **[boat.dev/dashboard](https://boat.dev/dashboard)** for Ladybird / LibWeb builds.
Sandbox: **`kakangkung-ladybird` / `bx_cnfw3mch`**.

```bash
curl -fsSL https://boat.dev/install | sh
boat login
boat resume bx_cnfw3mch   # or: boat new --name kakangkung-ladybird

boat exec bx_cnfw3mch 'bash engines/ladybird/boat-setup.sh'
boat exec bx_cnfw3mch 'cargo build -p ladybird-host --release'
boat exec bx_cnfw3mch './engines/ladybird/build.sh --stub'

# Full LibWeb (after Ladybird itself builds in the sandbox):
boat exec bx_cnfw3mch './engines/ladybird/build.sh --ladybird'
```

Open the sandbox desktop from the dashboard to watch Qt / LibWeb while bringing up the real host.

## Local Windows build

```powershell
cargo build --release -p ladybird-host -p app
# Both land in target\release\:
#   kakangkung.exe
#   kakangkung-ladybird-host.exe   (stub HWND paint)
cargo run --release -p app
```

Or set `KAKANGKUNG_LADYBIRD_HOST` to the host path.

## Settings

`browser.page_engine` = `ladybird` only. Changing it to WebView2 / Chromium is rejected.

### Windows release packaging (KB-38)

Installers and portable zips **must** include `kakangkung-ladybird-host.exe` beside
`kakangkung.exe` (`installer/bundle-ladybird.ps1`, `release.yml`). Optional
`ladybird-runtime\` (Lagom libs + `share/Lagom`) comes from secret
`LADYBIRD_RUNTIME_URL` or `engines/ladybird/package-runtime.sh` on Boat. See
[RELEASING.md](RELEASING.md).

## Status

| Item | Status |
|---|---|
| Native chrome (no HTML UI) | Done |
| Ladybird-only engine selection | Done — WebView2 removed |
| Rust stub host + child HWND paint | Done (`crates/ladybird-host`) |
| Nav (back/forward/reload/visible/history) | Done (stub + C++ host protocol; empty history acks load) |
| Host wait / HWND parenting | Done — UI pumps messages while waiting (avoids Self-test deadlock) |
| Screenshot / print / a11y / eval cmds | Done (screenshot real on HWND; others stub→LibWeb) |
| MCP server for AI agents | Done (`kakangkung-mcp`) |
| Chrome Web Store CRX → Ladybird registry | Done (MV3 on disk; APIs need LibWebView) |
| Settings UI refresh | Done (Direct2D; quieter cards + nav rail) |
| DevTools | Native `kakangkung://devtools` until LibWeb inspector |
| C++ stub HWND paint | Done (`engines/ladybird` stub) |
| Load events → UI / Self-test Ladybird path | Done |
| Extension pop-ups / `window.open` as Ladybird tabs | Done (no WebView2 surface) |
| LibWeb `HeadlessWebView` + native blit | `view_ladybird.cpp` (HAVE_LADYBIRD=1) — build on Boat `bx_cnfw3mch` |
| Downloads / PastDownload from LibWeb | KB-6 — FileDownloader events → live list + SQLite |
| Page zoom over host protocol | KB-8 — `zoom` cmd + CSS zoom via LibWeb |
| Adblock request intercept | KB-7 — ContentFilter/Blocker + EasyList document gate |
| Memory Saver (discard, not fake Suspend) | KB-9 — Idle timeout → DiscardDue / DropWebview |
| Password manager inject / fill | KB-10 — `eval` + `page_ipc` (`kk:pw:`) |
| Clear cookies / site data / cache | KB-11 — `clear_data` → LibWeb `clear_browsing_data` |
| Find / translate / reading mode | KB-12 — LibWeb find + `eval` (print preview deferred) |
| Task manager process tree | KB-13 — Ladybird host PID + children (session restore already real) |
| PDF in tab after download | KB-18 — open finished `.pdf` via `file://` in Ladybird |
| Site permission defaults / list / reset | KB-19 — LibWeb PermissionManager (geo/mic/notifications) |
| Print preview | KB-20 — screenshot snapshot tab + Print confirm (paginated layout deferred) |
| Folder sync passwords | KB-21 — vault merges via sync folder |
| Process-per-site visibility | KB-22 — LibWeb ProcessManager snapshots in Task Manager |
| Presentation / Chromecast casting | **KB-23 blocked** — LibWeb has no `PresentationRequest` / `RemotePlayback` / Cast API (CompositorPresentation* is GPU frames, not casting). No stub. |
| Crash recovery / cold start | **KB-25** — `web_content_crashed` → reload; `session.running` marker forces restore after unclean exit |
| Extension service workers | **KB-24** — `chrome.storage.local` + messaging demux; full MV3 SW blocked (no extension worker host) |
| Paginated print / PDF | **Blocked** — no LibWeb `print_to_pdf`; KB-20 keeps screenshot preview |
| Chrome-parity pillars | See store `docs/chrome-parity-roadmap.md` (KB-2…KB-25) |
| Stack merge procedure | Store `docs/merge-stack-14-38.md` — merge bottom-up only when a human says merge |

## Related

- [engines/ladybird/README.md](../engines/ladybird/README.md)
- [engines/ladybird/protocol.md](../engines/ladybird/protocol.md)
- [docs/BUILDING.md](BUILDING.md)
- [docs/NATIVE_UI.md](NATIVE_UI.md)
