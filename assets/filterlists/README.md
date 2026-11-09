# Bundled filter lists

EasyList and EasyPrivacy are copyright The EasyList authors
(https://easylist.to/). These unmodified snapshots are distributed under
Creative Commons Attribution-ShareAlike 3.0 Unported, one of the licenses
offered by the authors: https://easylist.to/pages/licence.html.
See LICENSE-CC-BY-SA-3.0.txt for the complete terms. The header of each list
retains its original version, upstream commit, source and expiration interval.
manifest.json records the exact source URL, retrieval time, size and SHA-256.

The matcher is Brave's adblock-rust 0.13.5 (MPL-2.0):
https://github.com/brave/adblock-rust/ (crate version pinned in Cargo.toml).
See LICENSE-MPL-2.0.txt. Kakangkung's adapter is separate from the dependency;
no upstream source files are modified.

Network and element-hiding (cosmetic) rules are compiled, with the engine's
embedded public suffix resolver and regular-expression support enabled. The
engine handles supported ABP network syntax, `##`/`#@#` hiding rules and
exceptions; unsupported syntax is ignored. `$popup` rules, which adblock-rust
does not support, are rewritten as `$document` rules into a second small engine
that only checks new windows and page-script top-level redirects. Hiding CSS is
injected by the app; procedural (`:-abp-*`, `:has-text`) rules are not applied.
This adapter does not implement scriptlet injection, remote code execution,
redirect-resource replacement, CSP response modification or response-body
rewriting. `kakangkung.txt` holds Kakangkung's own site fixes (MIT).

The app does not currently download list updates. From the app directory, run
`python bench/update-adblock-lists.py` to download and validate both official
sources without changing files, then add `--write` to refresh the local bundle.
The command accepts only the two official easylist.to HTTPS URLs, validates
redirects before following them, limits each response to 8 MiB, requires valid
UTF-8 and an Adblock Plus header, and preserves the exact list bytes and license
records. It stages all files before atomic per-file replacement, installs the
manifest last, and attempts rollback on a replacement failure. Run
`python bench/test-update-adblock-lists.py`, the filter_engine tests and browser
request tests before committing or releasing updated snapshots. This command
does not release the app or provide automatic list updates; the bundled lists
remain at their recorded version until explicitly refreshed and rebuilt. A future
automatic updater must use bounded background downloads, a known-good bundled
fallback and explicit version reporting, outside the request callback.
