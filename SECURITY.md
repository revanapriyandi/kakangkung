# Security policy

## Supported versions

Only the [latest release](https://github.com/revanapriyandi/kakangkung/releases/latest) gets
security fixes. Installed copies update themselves (see [docs/UPDATES.md](docs/UPDATES.md)).

## Reporting a vulnerability

Please **do not** open a public issue. Use
[GitHub's private vulnerability reporting](https://github.com/revanapriyandi/kakangkung/security/advisories/new)
and include what is affected, how to reproduce it, and what an attacker could do.

You will get an answer within a week. Once fixed, the fix is released and the advisory is
published with credit to you, unless you prefer otherwise.

## Scope

In scope: Kakangkung's own code — the native interface, built-in pages and their API, the updater
(download and verification), the installer, import and sync.

Out of scope: bugs in Microsoft Edge WebView2 / Chromium itself (report those to
[Microsoft](https://msrc.microsoft.com)) and websites you visit.

## Built-in protections

- Built-in pages are native, not web pages; the internal API answers only Kakangkung's own pages
  (a per-session token).
- Updates come only from this repository's releases over HTTPS and must match the release's
  SHA-256 checksums before they run.
- Microsoft Defender SmartScreen, tracking prevention and HTTPS-First are on by default.
