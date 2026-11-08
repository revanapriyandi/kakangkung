# Contributing to Kakangkung

Thanks for helping! Every kind of contribution counts: reporting a bug, suggesting a feature,
improving the docs, testing on your PC, or writing code.

## Ways to help

- **Report a bug** or **suggest a feature** with the
  [issue forms](https://github.com/revanapriyandi/kakangkung/issues/new/choose).
- **Ask or discuss** in [Discussions](https://github.com/revanapriyandi/kakangkung/discussions).
- **Pick an issue**: [good first issue](https://github.com/revanapriyandi/kakangkung/labels/good%20first%20issue)
  and [help wanted](https://github.com/revanapriyandi/kakangkung/labels/help%20wanted) are ready to
  take. Leave a comment so nobody does the same work twice.

## Getting started

1. Install the tools in [docs/BUILDING.md](docs/BUILDING.md) and fork the repository.
2. Build and run: `cargo run --release`.
3. Make your change on a branch (`fix-tab-drag`, `feature-reading-list`, …).
4. Check it:

   ```powershell
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo build --release; target\release\kakangkung.exe --self-test result.json
   ```

5. Open a pull request and fill in the template. CI runs the same checks.

## How the code is organised

| Crate | Holds |
|---|---|
| `crates/app` | The browser: window, tabs, built-in pages (`native/`), API behind the pages, updater, self-test |
| `crates/platform` | Every Windows call — the only crate allowed `unsafe`. Native frame, pop-ups and pages (Direct2D), menus, WebView2, downloads, HTTP, SQLite, hashing |
| `crates/ui` | Pure logic of the address bar, shortcuts, security state, the logo |
| `crates/tabs` | Tab lifecycle state machine |
| `crates/memory` | Memory Saver modes and protection rules |
| `crates/storage` | History, bookmarks, settings, import, sync |

Design notes: [docs/NATIVE_UI.md](docs/NATIVE_UI.md) (the native interface),
[docs/API_MAP.md](docs/API_MAP.md) (WebView2 features), [docs/SETTINGS.md](docs/SETTINGS.md)
(every setting).

## Translations

Interface text is English in the code and translated when shown, from one table per language
in `crates/app/src/i18n/` (`id.rs` is Bahasa Indonesia). To add a language, copy `id.rs`,
translate the right-hand side, add it to `TABLES` in `i18n/mod.rs`, to the `languages.ui`
values in `crates/app/src/prefs.rs` and to `UI_LANGS` in `crates/app/src/native/settings.rs`.
To fix a translation, edit its line. Text with no entry stays in English.

## Guidelines

- **Keep it light.** Measure memory and start-up when a change could affect them; no new background
  processes or services.
- **Every feature has a setting** in `crates/app/src/prefs.rs`, a control on the Settings page
  (`crates/app/src/native/settings.rs`) and a row in `docs/SETTINGS.md`. A test checks the first two.
- **`unsafe` only in `crates/platform`**, each block with a `// SAFETY:` comment saying why it is
  sound.
- **Small files and clear names.** Split a module before it grows past a few hundred lines.
- **Comments say why**, not what. Docs are plain English, short sentences.
- **No new dependency** for something a few lines can do; pin versions with `=` in `Cargo.toml`.
- **Tests** for logic (pure functions, parsers, layout); the self-test for behaviour that needs
  the real browser.

## Commit messages

Short summary in the imperative ("Add reading list"), a blank line, then what and why if it is not
obvious. Reference issues with `Fixes #123`.

## Code of conduct

Be kind and constructive. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

By contributing you agree that your work is released under the [MIT License](LICENSE).
