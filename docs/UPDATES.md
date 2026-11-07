# Automatic updates

Kakangkung updates itself from this project's
[GitHub releases](https://github.com/revanapriyandi/kakangkung/releases). Nothing has to be
downloaded by hand.

## What happens

1. **Check.** About 20 seconds after Kakangkung starts, and every 6 hours while it runs, it asks
   GitHub for the latest release (`api.github.com/repos/<owner>/<repo>/releases/latest`). Nothing
   about you or your browsing is sent; GitHub sees an ordinary request from your IP address.
2. **Compare.** The release's tag (`v1.2.3`) is compared with the running version, number by
   number. A pre-release (`1.2.3-beta.1`) never replaces the release it leads up to.
3. **Download.** When the release is newer, the installer for your processor
   (`Kakangkung-Setup-<version>-x64.exe` or `-arm64.exe`) is downloaded in the background to
   `%LOCALAPPDATA%\Kakangkung\Updates`, together with the release's `SHA256SUMS.txt`.
4. **Verify.** The installer's SHA-256 (computed with Windows' own cryptography) must match the
   line for it in `SHA256SUMS.txt`, else the download is thrown away.
5. **Install.**
   - When you close Kakangkung, the installer runs silently and replaces the program. The next
     start is the new version.
   - Or choose ⋮ > **Restart to update Kakangkung**, or **Restart to update** in Settings > About:
     Kakangkung closes, updates, and opens again with the same tabs.

The installer installs per user (no administrator prompt), so the update never needs your
permission. Your profile — history, bookmarks, settings, logins — is not touched.

## Settings

- **Settings > System > Update Kakangkung automatically** (on by default) switches checking,
  downloading and installing on or off.
- **Settings > About Kakangkung > Updates** shows where the updater is and has
  **Check for updates**.

## Portable and development copies

Only copies installed with the installer update themselves (the updater looks for the
uninstaller next to `kakangkung.exe`). A portable copy or one built from source only shows that a
newer version exists, with a link to it.

## For maintainers

The updater expects every release to contain:

| File | Used for |
|---|---|
| `Kakangkung-Setup-<version>-x64.exe` | Updating x64 PCs |
| `Kakangkung-Setup-<version>-arm64.exe` | Updating ARM64 PCs |
| `SHA256SUMS.txt` | Verifying the download (`sha256sum` format) |

`.github/workflows/release.yml` makes all of them from a version tag; see
[RELEASING.md](RELEASING.md). Code: `crates/app/src/update.rs` (check, download, verify, install)
and `crates/app/src/browser/updater.rs` (when, menu, Settings).
