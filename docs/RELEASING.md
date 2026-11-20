# Releasing

Releases are built and published by GitHub Actions
(`.github/workflows/release.yml`) for platforms Kakangkung supports today:

- **Windows x64 / ARM64** — full app (installer + portable zip)
- **Linux x64** — `kakangkung-ladybird-host` tarball (page engine only; Win32 UI is Windows-only)
- **macOS** — **not yet** (no native chrome port; tracked as B5 blocker — no fake `.app`)

## Steps

1. Update the version in `Cargo.toml` (`[workspace.package] version`) and run `cargo build` so
   `Cargo.lock` follows.
2. Add the changes to `CHANGELOG.md`.
3. Commit, then tag and push:

   ```powershell
   git tag -a v0.4.0 -m "Kakangkung 0.4.0"
   git push origin main v0.4.0
   ```

4. The workflow builds both architectures, makes the installers and portable zips, writes
   `SHA256SUMS.txt` and publishes the release with generated notes. The tag must match the version
   in `Cargo.toml`, else the build stops.

Copies installed with the installer find the release within six hours (or at their next start),
download it and update themselves — see [UPDATES.md](UPDATES.md).

## Dry run

*Actions > Release > Run workflow* builds the same files without publishing anything; they are kept
as the run's artifacts (`kakangkung-release-dry-run`).

## What a release contains

| File | |
|---|---|
| `Kakangkung-Setup-<version>-x64.exe` | Installer, Windows x64 |
| `Kakangkung-Setup-<version>-arm64.exe` | Installer, Windows ARM64 |
| `Kakangkung-<version>-x64.zip` | Portable, Windows x64 |
| `Kakangkung-<version>-arm64.zip` | Portable, Windows ARM64 |
| `kakangkung-ladybird-host-<version>-linux-x64.tgz` | Linux Ladybird host (B5; not the full UI) |
| `SHA256SUMS.txt` | Checksums of all of the above (the updater verifies against it) |

Attach a Boat-built Lagom runtime (`engines/ladybird/package-runtime.sh`) as an extra
release asset when publishing a full Linux LibWeb stack. macOS remains unpublished.

Both installers and portable ZIPs include `kakangkung-ladybird-host.exe` next to
`kakangkung.exe` (**KB-38**), `THIRD_PARTY_NOTICES.md`, and the full MPL-2.0 and
CC BY-SA 3.0 license texts for the bundled ad-blocking components.

### KB-38 — Ladybird / Lagom in the package

v0.4.4 installers were ~4.7–4.9 MB because they shipped **only** `kakangkung.exe`.
Without the host, tabs spun forever under the purple‑K splash.

| Layout | Typical size | Contents |
|--------|-------------:|----------|
| Pre–KB-38 (v0.4.4) | ~5 MB | `kakangkung.exe` only |
| Host bundled | ~6–15 MB | + `kakangkung-ladybird-host.exe` (required) |
| + full Lagom runtime | **100s MB** | + `ladybird-runtime\` (DLLs / `.so` + `share\Lagom`) |

`release.yml` always builds `-p ladybird-host` and runs `installer/bundle-ladybird.ps1`.
Optional secret **`LADYBIRD_RUNTIME_URL`**: zip/tarball of a `ladybird-runtime` tree
(real LibWeb/Lagom). Build that tree on Boat with
`engines/ladybird/package-runtime.sh` (Linux ELF today). Windows Lagom DLLs land the
same way once a Windows Ladybird build exists — until then the packaged host unblocks
load-complete; full paint still needs the runtime archive.

Smoke test refuses an installer that omits `kakangkung-ladybird-host.exe`.

### Code signing

`installer/build.ps1` signs `kakangkung.exe`, the installer and its uninstaller (SHA-256,
timestamped) when the repository has two secrets:

- `SIGN_PFX_BASE64`: the code-signing certificate as a base64 `.pfx`
  (`[Convert]::ToBase64String([IO.File]::ReadAllBytes('cert.pfx'))`);
- `SIGN_PFX_PASSWORD`: its password.

Without them the files are unsigned and Windows SmartScreen warns on the first download of a new
version ("More info" > "Run anyway"); automatic updates are not affected. An OV or EV code-signing
certificate (or Azure Trusted Signing) removes the warning.

### Checks

Every CI run builds the x64 installer and runs `installer\smoke-test.ps1`: a silent install in
Bahasa Indonesia, checks of the program, Start menu entry, Installed apps entry and language, then a
silent uninstall that must leave nothing behind but the data folder. The release workflow runs the
same test on the x64 installer it publishes.
