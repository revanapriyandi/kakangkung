# Building Kakangkung

## Requirements

- Windows 10 (1809 or newer) or Windows 11, x64 or ARM64.
- [Visual Studio 2022 Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with
  the **Desktop development with C++** workload (MSVC and a Windows 10/11 SDK). For ARM64 builds,
  also the *MSVC ARM64 build tools* component.
- [Rust](https://rustup.rs). `rust-toolchain.toml` pins the stable MSVC toolchain; rustup installs
  it the first time you run `cargo`.
- Microsoft Edge WebView2 Runtime (part of Windows 11; on Windows 10 it comes with Edge or from
  [Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/)).

No C compiler is needed for SQLite: Kakangkung uses the copy that ships with Windows
(`winsqlite3.dll`).

## Build and run

```powershell
cargo build --release
target\release\kakangkung.exe                 # the New Tab page (Welcome on the very first start)
target\release\kakangkung.exe example.com     # open an address
```

`cargo build` (debug) works too; third-party crates are optimised even in debug builds, so it
stays usable.

Command-line options:

| Option | What it does |
|---|---|
| `<address>` | Open it in the first tab |
| `--new-window [address]` | Another window on the same profile |
| `--incognito [address]` | An Incognito window |
| `--restore-session` | Reopen the last session's tabs (used after an update) |
| `--self-test <file.json>` | Drive the browser through its features and write the results |
| `--screenshot <file.png> [address]` | Save a screenshot once the page has loaded |

Set `KAKANGKUNG_TIMING=1` to print start-up timings, and `KAKANGKUNG_SHOT_DIR=<folder>` during a
self-test to save screenshots of every built-in page.

## Checks

```powershell
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
target\release\kakangkung.exe --self-test result.json
```

The self-test uses a throwaway profile; it opens the browser, types into the address bar, opens
and closes tabs, freezes and wakes a tab, goes through every built-in page and checks settings,
import, sync, extensions, tab search and the menus. CI runs all three on every push.

## ARM64

```powershell
rustup target add aarch64-pc-windows-msvc
cargo build --release --target aarch64-pc-windows-msvc
```

## Installer

The installer is made with [Inno Setup 6.3+](https://jrsoftware.org/isinfo.php):

```powershell
cargo build --release
iscc /DAppVersion=0.1.0 /DArch=x64 /DBinDir=..\target\release installer\kakangkung.iss
```

The installer lands in `dist\`. For ARM64 pass `/DArch=arm64` and the ARM64 build's folder.
The wizard pictures (`installer\wizard*.bmp`) and the icon (`installer\kakangkung.ico`) are
generated from the logo; `crates/app/build.rs` also embeds the icon and version details in
`kakangkung.exe`.

## Forks

Updates come from the GitHub releases of `revanapriyandi/kakangkung`. A fork that publishes its
own releases builds with `KAKANGKUNG_REPO=<owner>/<name>` set, so its copies update from the fork.

## Pinned crates

| Crate | Version | Why |
|---|---|---|
| wry | 0.57.1 | WebView2 webviews, `default-features = false`, `os-webview` |
| tao | 0.37.2 | window and event loop (the windowing crate wry is built for) |
| webview2-com | 0.39.1 | raw COM for WebView2 APIs wry does not wrap |
| windows | 0.62.2 | Win32, Direct2D, DirectWrite, WinHTTP, CNG; one version shared with wry and tao |
| serde / serde_json | 1.0 | settings and data |
