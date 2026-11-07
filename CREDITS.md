# Credits

Kakangkung is original work. No files, images or code from other browsers are included.

- Logo: a white "K" monogram on a violet-to-indigo disc, `assets/logo.svg`; the window, taskbar,
  program and installer icons are drawn from the same shapes in code (`ui::brand_icon_rgba`).
- Interface icons: the Segoe Fluent Icons / Segoe MDL2 Assets fonts that ship with Windows,
  used through DirectWrite (not redistributed).
- Illustrations: drawn in code with Direct2D (`crates/platform/src/page/art.rs`).

Third-party software it runs on: Microsoft Edge WebView2 Runtime (renders web pages, installed
with Windows 11 or by the installer) and the SQLite library that ships with Windows
(`winsqlite3.dll`). Rust crates and their licences are listed in `Cargo.lock`.
