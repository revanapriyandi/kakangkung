; Kakangkung installer (Inno Setup 6.5 or newer).
;
; Built by installer\build.ps1 (CI and .github/workflows/release.yml), or by hand:
;   pwsh installer\build.ps1 -Version 0.1.0 -Arch x64 -BinDir target\release
; Checked by installer\smoke-test.ps1 in CI: silent install, what it left, uninstall.
;
; - Installs for the current user only (no administrator prompt), so automatic updates can put a
;   new version in silently.
; - Asks for the language (English or Bahasa Indonesia); Kakangkung's interface starts in it.
; - Installs Microsoft Edge WebView2 Runtime when the PC does not have it (Windows 10).
; - `/RELAUNCH` (used by the updater's "Restart to update") opens Kakangkung again afterwards
;   with the tabs that were open.
; - installer\build.ps1 signs the program file and, with /DSign, Setup and its uninstaller.

#if VER < EncodeVer(6, 5, 0)
  #error Inno Setup 6.5.0 or newer is needed (installer\Indonesian.isl is for 6.5)
#endif

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef BinDir
  #define BinDir "..\target\release"
#endif
#ifndef OutDir
  #define OutDir "..\dist"
#endif

#define AppName "Kakangkung"
#define AppExe "kakangkung.exe"
#define Repo "https://github.com/revanapriyandi/kakangkung"

[Setup]
AppId={{B62D7E15-F7C0-4FC1-9F78-A27C8FC6A39E}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Kakangkung contributors
AppPublisherURL={#Repo}
AppSupportURL={#Repo}/issues
AppUpdatesURL={#Repo}/releases
AppCopyright=Kakangkung contributors, MIT License
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppName} Setup
VersionInfoProductName={#AppName}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
MinVersion=10.0.17763
OutputDir={#OutDir}
OutputBaseFilename=Kakangkung-Setup-{#AppVersion}-{#Arch}
SetupIconFile=kakangkung.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
LicenseFile=..\LICENSE
WizardStyle=modern
WizardSizePercent=110
WizardImageFile=wizard.bmp,wizard-200.bmp
WizardSmallImageFile=wizard-small.bmp,wizard-small-200.bmp
WizardImageStretch=no
Compression=lzma2/ultra64
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
SetupLogging=yes
; The language list always shows, with the Windows display language picked.
ShowLanguageDialog=yes
#ifdef Sign
SignTool=kksign
SignedUninstaller=yes
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "indonesian"; MessagesFile: "Indonesian.isl"

[Messages]
english.WelcomeLabel2=This will install [name/ver] on your computer.%n%nKakangkung is a light, fast browser for Windows with a native interface, a memory saver for idle tabs, a built-in ad blocker and automatic updates.%n%nClose other programs before you continue.
indonesian.WelcomeLabel2=Ini akan memasang [name/ver] di komputer Anda.%n%nKakangkung adalah browser yang ringan dan cepat untuk Windows, dengan antarmuka native, penghemat memori untuk tab yang tidak dipakai, pemblokir iklan bawaan, dan pembaruan otomatis.%n%nTutup program lain sebelum melanjutkan.
english.FinishedLabel=Kakangkung is installed. It keeps itself up to date in the background.
indonesian.FinishedLabel=Kakangkung sudah terpasang. Kakangkung memperbarui dirinya sendiri di latar belakang.

[CustomMessages]
; The interface language Kakangkung starts in (HKCU\Software\Kakangkung, Language).
english.AppLang=en
indonesian.AppLang=id
english.LaunchComment=Browse the web with Kakangkung
indonesian.LaunchComment=Jelajahi web dengan Kakangkung
english.WebView2Title=Getting Microsoft Edge WebView2
indonesian.WebView2Title=Mengunduh Microsoft Edge WebView2
english.WebView2Text=Kakangkung uses the WebView2 Runtime to show web pages. It is being downloaded from Microsoft.
indonesian.WebView2Text=Kakangkung memakai WebView2 Runtime untuk menampilkan halaman web. Runtime ini sedang diunduh dari Microsoft.
english.WebView2Installing=Installing Microsoft Edge WebView2...
indonesian.WebView2Installing=Memasang Microsoft Edge WebView2...
english.WebView2Failed=Microsoft Edge WebView2 could not be installed (%1).%nKakangkung needs it to show web pages; you can get it from microsoft.com/edge/webview2.
indonesian.WebView2Failed=Microsoft Edge WebView2 tidak dapat dipasang (%1).%nKakangkung membutuhkannya untuk menampilkan halaman web; Anda bisa mengunduhnya dari microsoft.com/edge/webview2.
english.RemoveData=Also delete your Kakangkung data (history, bookmarks, cookies, saved passwords, extensions and settings)?%n%nChoose No to keep it for a later install.
indonesian.RemoveData=Hapus juga data Kakangkung Anda (riwayat, bookmark, cookie, kata sandi tersimpan, ekstensi, dan pengaturan)?%n%nPilih Tidak untuk menyimpannya bagi instalasi berikutnya.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#BinDir}\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\filterlists\LICENSE-MPL-2.0.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\filterlists\LICENSE-CC-BY-SA-3.0.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "{cm:LaunchComment}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Kakangkung"; ValueType: string; ValueName: "Language"; ValueData: "{cm:AppLang}"; Flags: uninsdeletekey
; Left by "Make default" in Settings; removed with Kakangkung.
Root: HKCU; Subkey: "Software\Clients\StartMenuInternet\Kakangkung"; Flags: uninsdeletekey dontcreatekey
Root: HKCU; Subkey: "Software\Classes\KakangkungHTML"; Flags: uninsdeletekey dontcreatekey
; PDF files: Kakangkung is offered under "Open with" (its PDF viewer); Windows keeps the choice.
Root: HKCU; Subkey: "Software\Classes\KakangkungPDF"; ValueType: string; ValueData: "Kakangkung PDF Document"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\KakangkungPDF\DefaultIcon"; ValueType: string; ValueData: """{app}\{#AppExe}"",0"
Root: HKCU; Subkey: "Software\Classes\KakangkungPDF\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\.pdf\OpenWithProgids"; ValueType: string; ValueName: "KakangkungPDF"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueType: none; ValueName: "Kakangkung"; Flags: uninsdeletevalue dontcreatekey

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
; After an update started with "Restart to update": open again with the same tabs.
Filename: "{app}\{#AppExe}"; Parameters: "--restore-session"; Flags: nowait; Check: ShouldRelaunch

[UninstallRun]
; A running Kakangkung would keep its program file locked.
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM {#AppExe}"; Flags: runhidden; RunOnceId: "CloseKakangkung"

[Code]
var
  DownloadPage: TDownloadWizardPage;

const
  WebView2Key = 'Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';
  WebView2Bootstrapper = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703';

function HasVersion(Root: Integer; Key: String): Boolean;
var
  V: String;
begin
  Result := RegQueryStringValue(Root, Key, 'pv', V) and (V <> '') and (V <> '0.0.0.0');
end;

{ Microsoft Edge WebView2 Runtime renders the web pages; Windows 11 ships it. }
function WebView2Installed: Boolean;
begin
  Result := HasVersion(HKLM, 'SOFTWARE\WOW6432Node\' + WebView2Key)
    or HasVersion(HKLM, 'SOFTWARE\' + WebView2Key)
    or HasVersion(HKCU, 'Software\' + WebView2Key);
end;

function ShouldRelaunch: Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/RELAUNCH') = 0 then
      Result := True;
end;

procedure InitializeWizard;
begin
  DownloadPage := CreateDownloadPage(CustomMessage('WebView2Title'), CustomMessage('WebView2Text'), nil);
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  Code: Integer;
begin
  Result := True;
  if (CurPageID = wpReady) and not WebView2Installed then begin
    DownloadPage.Clear;
    DownloadPage.Add(WebView2Bootstrapper, 'MicrosoftEdgeWebview2Setup.exe', '');
    DownloadPage.Show;
    try
      try
        DownloadPage.Download;
        DownloadPage.SetText(CustomMessage('WebView2Installing'), '');
        Exec(ExpandConstant('{tmp}\MicrosoftEdgeWebview2Setup.exe'), '/silent /install', '', SW_HIDE, ewWaitUntilTerminated, Code);
      except
        SuppressibleMsgBox(FmtMessage(CustomMessage('WebView2Failed'), [GetExceptionMessage]), mbError, MB_OK, IDOK);
      end;
    finally
      DownloadPage.Hide;
    end;
  end;
end;

{ The person's data stays unless they ask; silent uninstalls (scripts) never delete it. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
    if MsgBox(CustomMessage('RemoveData'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      DelTree(ExpandConstant('{localappdata}\{#AppName}'), True, True, True);
end;
