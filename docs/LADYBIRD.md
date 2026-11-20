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
protocol-compatible stub so the app runs without WebView2.

## Development on Boat (preferred)

Use **[boat.dev/dashboard](https://boat.dev/dashboard)** for Ladybird / LibWeb builds and
iteration (full Ubuntu VM, desktop stream, Qt toolchain).

```bash
curl -fsSL https://boat.dev/install | sh
boat login
boat new --name kakangkung-ladybird   # or: boat resume <id>

# Stub host (protocol only) — also available via cargo on any machine:
boat exec kakangkung-ladybird 'cargo build -p ladybird-host --release'

# Full LibWeb host (after Ladybird itself builds):
boat scp ./engines/ladybird kakangkung-ladybird:~/kakangkung-ladybird
boat exec kakangkung-ladybird './kakangkung-ladybird/build.sh --ladybird'
```

Open the sandbox desktop from the dashboard to watch Ladybird’s Qt UI while bringing up the
real host. Snapshot with `boat stop` between sessions.

## Local Windows build

```powershell
cargo build --release -p ladybird-host -p app
# Both land in target\release\:
#   kakangkung.exe
#   kakangkung-ladybird-host.exe
cargo run --release -p app
```

Or set `KAKANGKUNG_LADYBIRD_HOST` to the host path.

## Settings

`browser.page_engine` = `ladybird` only. Changing it to WebView2 / Chromium is rejected.

## Remaining gaps

| Item | Status |
|---|---|
| Native chrome (no HTML UI) | Done |
| Ladybird-only engine selection | Done — WebView2 removed |
| Rust stub host (`crates/ladybird-host`) | Done — protocol + cargo build |
| LibWeb-linked C++ host | Scaffold in `engines/ladybird` — finish on Boat |
| HWND parenting / paint into Kakangkung | TBD in ViewImplementation |
| `window.open`, Chrome Web Store extensions | Disabled with WebView2; reimplement on LibWebView |
| Memory Saver TrySuspend / SmartScreen | WebView2-era APIs — replace with Ladybird equivalents |
| Windows CI self-test against live pages | Needs host beside exe (`cargo build -p ladybird-host`) |

## Related

- [engines/ladybird/README.md](../engines/ladybird/README.md)
- [engines/ladybird/protocol.md](../engines/ladybird/protocol.md)
- [docs/BUILDING.md](BUILDING.md)
- [docs/NATIVE_UI.md](NATIVE_UI.md)
