# Installing Kakangkung

## With the installer (recommended)

1. Open the [latest release](https://github.com/revanapriyandi/kakangkung/releases/latest).
2. Download the installer for your PC:
   - most PCs (Intel or AMD): `Kakangkung-Setup-<version>-x64.exe`
   - Windows on ARM (for example Snapdragon laptops): `Kakangkung-Setup-<version>-arm64.exe`

   Not sure? *Settings > System > About* in Windows shows the "System type".
3. Run it. If Windows SmartScreen says it protected your PC, choose **More info > Run anyway**
   (the installer is not code-signed yet).
4. Follow the wizard. It installs for your user account only — no administrator rights — into
   `%LOCALAPPDATA%\Programs\Kakangkung`, adds Kakangkung to the Start menu (and, if you want, the
   desktop), and installs Microsoft Edge WebView2 when your PC does not have it.

From then on Kakangkung updates itself ([UPDATES.md](UPDATES.md)).

To make it your default browser: *Settings > Default browser > Make default* in Kakangkung, then
choose Kakangkung in the Windows window that opens.

## Portable

Download `Kakangkung-<version>-x64.zip` (or `-arm64.zip`), unpack it anywhere and run
`kakangkung.exe`. A portable copy does not update itself; it tells you when a new version exists.
It needs Microsoft Edge WebView2, which Windows 11 already has.

## Your data

Profile data (history, bookmarks, settings, cookies, extensions) lives in
`%LOCALAPPDATA%\Kakangkung`, whichever way you installed.

## Uninstalling

*Windows Settings > Apps > Installed apps > Kakangkung > Uninstall*. Your profile data stays in
`%LOCALAPPDATA%\Kakangkung`; delete that folder too if you want everything gone.

## Requirements

Windows 10 version 1809 or newer, or Windows 11; x64 or ARM64. macOS and Linux are not supported.
