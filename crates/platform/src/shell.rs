//! Windows shell and registry: folder picker, theme, account, dialogs, default browser.

use std::path::Path;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::HWND;
use crate::Result;

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
    let classes = format!("Software\\Classes\\{PROG_ID}");
    reg_set(&classes, None, "Kakangkung HTML Document")?;
    reg_set(&format!("{classes}\\DefaultIcon"), None, &icon)?;
    reg_set(&format!("{classes}\\shell\\open\\command"), None, &open)?;
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
    for ext in [".htm", ".html"] {
        reg_set(&format!("{caps}\\FileAssociations"), Some(ext), PROG_ID)?;
    }
    reg_set("Software\\RegisteredApplications", Some("Kakangkung"), &caps)
}

/// Is Kakangkung the browser Windows opens https links with?
pub fn is_default_browser() -> bool {
    let key = "Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\https\\UserChoice";
    reg_get(key, "ProgId").is_some_and(|p| p == PROG_ID)
}
