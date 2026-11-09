//! Windows shell and registry: folder picker, theme, account, dialogs, default browser.

use std::path::Path;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::HWND;
use crate::Result;

pub fn create_app_shortcut(path: &Path, exe: &Path, id: &str, profile: &Path, title: &str) -> Result<()> {
    use windows::core::Interface;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    fn quote(value: &str) -> String {
        let mut result = String::from("\"");
        let mut slashes = 0;
        for c in value.chars() {
            if c == '\\' { slashes += 1; continue; }
            result.extend(std::iter::repeat_n('\\', slashes * if c == '"' { 2 } else { 1 }));
            if c == '"' { result.push('\\'); }
            result.push(c); slashes = 0;
        }
        result.extend(std::iter::repeat_n('\\', slashes * 2));
        result.push('"'); result
    }
    // SAFETY: interfaces stay on this thread. Balance only successful COM initialization;
    // an existing apartment may also create the shell link.
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(&HSTRING::from(exe.as_os_str()))?;
            link.SetArguments(&HSTRING::from(format!("--app-id={id} {}", quote(&format!("--user-data-dir={}", profile.display())))))?;
            link.SetDescription(&HSTRING::from(title))?;
            link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
            link.cast::<IPersistFile>()?.Save(&HSTRING::from(path.as_os_str()), true)
        })();
        if initialized { CoUninitialize(); }
        result
    }
}

/// Windows' own folder picker. `None` when the person cancels.
pub fn pick_folder(owner: isize, title: &str) -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH};
    let title = HSTRING::from(title);
    // SAFETY: COM is initialised on this (UI) thread by WebView2; the dialog, the shell item and
    // the CoTaskMem string are released here; `owner` is our top-level window.
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dialog.GetOptions().ok()?;
        dialog.SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
        let _ = dialog.SetTitle(&title);
        dialog.Show(Some(HWND(owner as *mut _))).ok()?;
        let item = dialog.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = p.to_string().ok().map(std::path::PathBuf::from);
        CoTaskMemFree(Some(p.0 as *const _));
        path
    }
}

/// Windows' Open dialog for one file matching `pattern` ("*.csv"), described as `kind`.
/// `None` when the person cancels.
pub fn pick_file(owner: isize, title: &str, kind: &str, pattern: &str) -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{Common::COMDLG_FILTERSPEC, FileOpenDialog, IFileOpenDialog, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, SIGDN_FILESYSPATH};
    let (title, kind, pattern) = (HSTRING::from(title), HSTRING::from(kind), HSTRING::from(pattern));
    // SAFETY: as in `pick_folder`; the filter strings outlive the dialog.
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dialog.GetOptions().ok()?;
        dialog.SetOptions(opts | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM).ok()?;
        let _ = dialog.SetTitle(&title);
        let _ = dialog.SetFileTypes(&[COMDLG_FILTERSPEC { pszName: windows::core::PCWSTR(kind.as_ptr()), pszSpec: windows::core::PCWSTR(pattern.as_ptr()) }]);
        dialog.Show(Some(HWND(owner as *mut _))).ok()?;
        let item = dialog.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = p.to_string().ok().map(std::path::PathBuf::from);
        CoTaskMemFree(Some(p.0 as *const _));
        path
    }
}

/// How long a copied password stays on the clipboard.
const SECRET_ON_CLIPBOARD: std::time::Duration = std::time::Duration::from_secs(30);

/// Put a secret (a password) on the clipboard, kept out of Windows' clipboard history and cloud
/// clipboard, and take it off again after 30 seconds unless something else was copied since.
pub fn copy_secret(text: &str) -> std::result::Result<(), String> {
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_UNICODETEXT;
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: the clipboard is opened and closed here; the memory handed to SetClipboardData is
    // owned by the clipboard afterwards (freed by us only when handing it over failed).
    let seq = unsafe {
        OpenClipboard(None).map_err(|e| format!("clipboard: {e}"))?;
        let put = (|| -> windows::core::Result<()> {
            EmptyClipboard()?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2)?;
            let p = GlobalLock(mem) as *mut u16;
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(mem);
            if let Err(e) = SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(mem.0))) {
                let _ = GlobalFree(Some(mem));
                return Err(e);
            }
            for name in ["ExcludeClipboardContentFromMonitorProcessing", "CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"] {
                let format = RegisterClipboardFormatW(&HSTRING::from(name));
                let zero = GlobalAlloc(GMEM_MOVEABLE, 4)?;
                *(GlobalLock(zero) as *mut u32) = 0;
                let _ = GlobalUnlock(zero);
                if SetClipboardData(format, Some(HANDLE(zero.0))).is_err() {
                    let _ = GlobalFree(Some(HGLOBAL(zero.0)));
                }
            }
            Ok(())
        })();
        let _ = CloseClipboard();
        put.map_err(|e| format!("clipboard: {e}"))?;
        GetClipboardSequenceNumber()
    };
    std::thread::spawn(move || {
        std::thread::sleep(SECRET_ON_CLIPBOARD);
        // SAFETY: as above; only emptied when the clipboard still holds what was put there.
        unsafe {
            if GetClipboardSequenceNumber() == seq && OpenClipboard(None).is_ok() {
                let _ = EmptyClipboard();
                let _ = CloseClipboard();
            }
        }
    });
    Ok(())
}

/// Ask Windows Hello (PIN, face, fingerprint) that the person at the PC is this Windows user,
/// over window `owner`, saying `message`. `done(true)` when confirmed, or when this PC has no
/// Windows Hello set up (the profile is this Windows user's already). Runs `done` on another
/// thread.
pub fn verify_user(owner: isize, message: &str, done: impl FnOnce(bool) + Send + 'static) {
    use windows::Security::Credentials::UI::{UserConsentVerificationResult as R, UserConsentVerifier};
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    let message = HSTRING::from(message);
    // SAFETY: plain WinRT activation and an interop call with our live top-level window.
    let op = unsafe {
        windows::core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>()
            .and_then(|f| f.RequestVerificationForWindowAsync::<windows_future::IAsyncOperation<R>>(HWND(owner as *mut _), &message))
    };
    std::thread::spawn(move || {
        let ok = match op.and_then(|op| op.join()) {
            Ok(R::Verified | R::DeviceNotPresent | R::NotConfiguredForUser | R::DisabledByPolicy) => true,
            Ok(_) => false,
            Err(e) => {
                eprintln!("Windows Hello: {e}");
                false
            }
        };
        done(ok);
    });
}

/// Windows' Save As dialog, starting at `suggested` (folder and file name). `None` when cancelled.
pub fn save_file(owner: isize, suggested: &str) -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{FileSaveDialog, IFileSaveDialog, IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH};
    let suggested = Path::new(suggested);
    // SAFETY: as in `pick_folder`; the strings outlive the calls that borrow them.
    unsafe {
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        if let Some(name) = suggested.file_name() {
            let _ = dialog.SetFileName(&HSTRING::from(name));
        }
        if let Some(ext) = suggested.extension() {
            let _ = dialog.SetDefaultExtension(&HSTRING::from(ext));
        }
        if let Some(dir) = suggested.parent().filter(|d| d.is_dir()) {
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None) {
                let _ = dialog.SetFolder(&item);
            }
        }
        dialog.Show(Some(HWND(owner as *mut _))).ok()?;
        let item = dialog.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = p.to_string().ok().map(std::path::PathBuf::from);
        CoTaskMemFree(Some(p.0 as *const _));
        path
    }
}

/// Two-letter code of the interface language to start in: the one picked in the installer
/// (HKCU\Software\Kakangkung, Language), else the Windows display language ("en" when not known).
pub fn ui_language() -> String {
    if let Some(lang) = reg_get("Software\\Kakangkung", "Language").filter(|l| matches!(l.as_str(), "en" | "id")) {
        return lang;
    }
    // SAFETY: no arguments.
    let lang = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() } & 0x3ff;
    // ponytail: only the languages Kakangkung has tables for; a full LCID map when there are more.
    match lang {
        0x21 => "id",
        _ => "en",
    }
    .to_string()
}

/// Is Windows set to dark mode for apps (HKCU ...\Themes\Personalize\AppsUseLightTheme = 0)?
pub fn dark_mode() -> bool {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let (key, value) = (
        HSTRING::from("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        HSTRING::from("AppsUseLightTheme"),
    );
    let (mut data, mut size) = (1u32, 4u32);
    // SAFETY: `data` is a 4-byte buffer matching `size`; strings outlive the call.
    let rc = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, &key, &value, RRF_RT_REG_DWORD, None, Some((&mut data as *mut u32).cast()), Some(&mut size))
    };
    rc.is_ok() && data == 0
}

/// The Microsoft account email Windows is signed in with, if any (suggested on the Kakangkung
/// sign-in page). Read from HKCU\Software\Microsoft\IdentityCRL\UserExtendedProperties, whose
/// sub-keys are the signed-in account names.
pub fn windows_account_email() -> Option<String> {
    use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ};
    let path = HSTRING::from("Software\\Microsoft\\IdentityCRL\\UserExtendedProperties");
    let mut key = HKEY::default();
    // SAFETY: `key` is a writable out-param and closed below; the name buffer length is passed in.
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, &path, None, KEY_READ, &mut key).is_err() {
            return None;
        }
        let mut found = None;
        for i in 0..16 {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            if RegEnumKeyExW(key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                break;
            }
            let name = String::from_utf16_lossy(&buf[..len as usize]);
            if name.contains('@') && name.contains('.') {
                found = Some(name);
                break;
            }
        }
        let _ = RegCloseKey(key);
        found
    }
}

/// OK / Cancel question owned by the browser window. True when the person chose OK.
pub fn confirm(owner: isize, title: &str, text: &str) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDOK, MB_ICONQUESTION, MB_OKCANCEL};
    let (title, text) = (HSTRING::from(title), HSTRING::from(text));
    // SAFETY: both strings outlive the call; `owner` is the browser's own window handle.
    unsafe { MessageBoxW(Some(HWND(owner as _)), &text, &title, MB_OKCANCEL | MB_ICONQUESTION) == IDOK }
}

/// ProgID, client key and display name under which the browser registers itself.
const PROG_ID: &str = "KakangkungHTML";
/// PDF files opened in Kakangkung (also offered by the installer under "Open with").
const PDF_PROG_ID: &str = "KakangkungPDF";

const CLIENT_KEY: &str = "Software\\Clients\\StartMenuInternet\\Kakangkung";

fn reg_set(key: &str, name: Option<&str>, value: &str) -> Result<()> {
    use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let data: Vec<u16> = value.encode_utf16().chain([0]).collect();
    let (key, name) = (HSTRING::from(key), name.map(HSTRING::from));
    let name_ptr = name.as_ref().map_or(windows::core::PCWSTR::null(), |n| windows::core::PCWSTR(n.as_ptr()));
    // SAFETY: `data` is a NUL-terminated UTF-16 buffer whose byte length is passed; strings outlive the call.
    unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, &key, name_ptr, REG_SZ.0, Some(data.as_ptr().cast()), (data.len() * 2) as u32).ok()
    }
}

fn reg_get(key: &str, name: &str) -> Option<String> {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let (key, name) = (HSTRING::from(key), HSTRING::from(name));
    let mut buf = [0u16; 260];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: `buf` is writable and `size` is its byte length; strings outlive the call.
    let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, &key, &name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size)) };
    rc.is_ok().then(|| String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]))
}

/// Register as a web browser for the current user (HKCU only, no admin rights): ProgID for
/// http/https/.htm/.html and a StartMenuInternet client with Capabilities. Windows then lists
/// Kakangkung in Settings > Default apps, where the person makes the choice (Windows does not
/// let programs set the default themselves).
pub fn register_as_browser(exe: &Path) -> Result<()> {
    let exe = exe.display().to_string();
    let open = format!("\"{exe}\" \"%1\"");
    let icon = format!("\"{exe}\",0");
    for (prog_id, name) in [(PROG_ID, "Kakangkung HTML Document"), (PDF_PROG_ID, "Kakangkung PDF Document")] {
        let classes = format!("Software\\Classes\\{prog_id}");
        reg_set(&classes, None, name)?;
        reg_set(&format!("{classes}\\DefaultIcon"), None, &icon)?;
        reg_set(&format!("{classes}\\shell\\open\\command"), None, &open)?;
    }
    reg_set(CLIENT_KEY, None, "Kakangkung")?;
    reg_set(&format!("{CLIENT_KEY}\\DefaultIcon"), None, &icon)?;
    reg_set(&format!("{CLIENT_KEY}\\shell\\open\\command"), None, &format!("\"{exe}\""))?;
    let caps = format!("{CLIENT_KEY}\\Capabilities");
    reg_set(&caps, Some("ApplicationName"), "Kakangkung")?;
    reg_set(&caps, Some("ApplicationDescription"), "A light web browser")?;
    reg_set(&caps, Some("ApplicationIcon"), &icon)?;
    for scheme in ["http", "https"] {
        reg_set(&format!("{caps}\\URLAssociations"), Some(scheme), PROG_ID)?;
    }
    // The files Chrome registers for: pages, PDF (Kakangkung's PDF viewer), SVG and WebP.
    for ext in [".htm", ".html", ".shtml", ".xht", ".xhtml", ".svg", ".webp"] {
        reg_set(&format!("{caps}\\FileAssociations"), Some(ext), PROG_ID)?;
    }
    reg_set(&format!("{caps}\\FileAssociations"), Some(".pdf"), PDF_PROG_ID)?;
    reg_set("Software\\RegisteredApplications", Some("Kakangkung"), &caps)
}

/// Is Kakangkung the browser Windows opens https links with?
pub fn is_default_browser() -> bool {
    let key = "Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\https\\UserChoice";
    reg_get(key, "ProgId").is_some_and(|p| p == PROG_ID)
}
