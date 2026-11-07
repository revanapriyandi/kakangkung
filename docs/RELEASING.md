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
   git tag v0.2.0
   git push origin main v0.2.0
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

The files are not code-signed yet, so Windows SmartScreen may warn on the first download of a new
version ("More info" > "Run anyway"). Automatic updates are not affected.
