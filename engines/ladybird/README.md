# Kakangkung × Ladybird

Kakangkung's preferred page engine is [Ladybird](https://ladybird.org) (LibWeb / LibWebView).
This folder is a **Ladybird UI port**: Kakangkung keeps its native Win32/Direct2D chrome and
drives Ladybird's multi-process web stack the way Ladybird's own Qt UI does — by subclassing
`WebView::ViewImplementation` and `WebView::Application` (see Ladybird
[Documentation/Porting.md](https://github.com/LadybirdBrowser/ladybird/blob/master/Documentation/Porting.md)).

The empty Servo spike that used to live under `engines/servo` is replaced by this path.

## Layout

```
engines/ladybird/
  README.md                 this file
  build.sh / build.ps1      fetch Ladybird (optional) and build the host
  CMakeLists.txt            host project (links LibWebView when HAVE_LADYBIRD=1)
  include/kakangkung_ladybird.h   C ABI the Rust side talks to
  src/
    host_main.cpp           process entry (kakangkung-ladybird-host)
    view.cpp / view.h       ViewImplementation UI-port (real or stub)
    application.cpp / .h    WebView::Application subclass (real or stub)
    abi.cpp                 C ABI wrappers
    stub_page.cpp           offline stub page when Ladybird is not linked
  protocol.md               stdin/stdout line protocol with Kakangkung
```

## Build (with Ladybird sources)

Requirements match [Ladybird's build instructions](https://github.com/LadybirdBrowser/ladybird/blob/master/Documentation/BuildInstructionsLadybird.md):
CMake ≥ 3.30, a C++23 compiler (clang-22 / gcc-16 class), Qt 6.10+, Ninja, and enough RAM/disk
for vcpkg (Skia, etc.). Native Windows is experimental; WSL2 or Linux/macOS is the supported
Ladybird build surface today.

```bash
# From the Kakangkung repo root
./engines/ladybird/build.sh                 # clones Ladybird if needed, builds the host
# or point at an existing tree:
LADYBIRD_SOURCE_DIR=/path/to/ladybird ./engines/ladybird/build.sh
```

On Windows (PowerShell), after Ladybird itself builds under WSL2 or clang-cl:

```powershell
.\engines\ladybird\build.ps1
```

The binary lands in `engines/ladybird/out/kakangkung-ladybird-host` (`.exe` on Windows).
Copy it next to `kakangkung.exe`, or set `KAKANGKUNG_LADYBIRD_HOST` to its path.

## Build (stub host, no Ladybird)

Without Ladybird sources the same CMake project builds a **stub host** that speaks the protocol
and shows an explanatory page. Useful to exercise discovery and IPC on a machine that cannot yet
compile LibWeb:

```bash
./engines/ladybird/build.sh --stub
```

## Run with Kakangkung

1. Build Kakangkung as usual (`cargo build --release` on Windows).
2. Place `kakangkung-ladybird-host` beside the exe (or set the env var).
3. Settings → System → **Page engine** = Ladybird (only option).
4. If the host is missing, the native chrome still starts but pages do not load — there is **no WebView2 fallback**.

## Blockers (honest status)

| Gap | Impact |
|---|---|
| No public C embedding API in Ladybird — UI ports must implement `ViewImplementation` in C++ | This folder *is* that port; Rust talks over a small C ABI + IPC |
| Ladybird alpha targets Linux/macOS; native Windows is experimental | Full Windows LibWeb link needs clang-cl + Ladybird Windows work; WSL2 builds the Linux host |
| Kakangkung chrome is Win32/Direct2D; Ladybird Qt UI is separate | We embed LibWebView, not Ladybird's Qt chrome |
| Extensions, WebView2 DevTools docking, TrySuspend, SmartScreen | Ladybird path reimplements what LibWebView exposes; Chromium-only features stay WebView2-only until replaced |
| This CI image cannot build Ladybird (CMake/Qt/compiler/RAM) | Host build is opt-in via `engines/ladybird`; main `cargo` CI stays on WebView2 fallback |

See [docs/LADYBIRD.md](../../docs/LADYBIRD.md) for architecture and the migration plan.
