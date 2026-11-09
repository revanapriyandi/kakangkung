# Releasing

Releases are built and published by GitHub Actions
(`.github/workflows/release.yml`) for every platform Kakangkung supports: Windows x64 and Windows
ARM64.

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
| `SHA256SUMS.txt` | Checksums of all of the above (the updater verifies against it) |

Both installers and portable ZIPs include `THIRD_PARTY_NOTICES.md` and the full
MPL-2.0 and CC BY-SA 3.0 license texts for the bundled ad-blocking components.

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
