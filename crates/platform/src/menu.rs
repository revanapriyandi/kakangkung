//! Native Windows pop-up menus (`TrackPopupMenuEx`): the ⋮ menu, tab and profile menus.
//! They are real Windows 11 menus, so they always open above the page (a WebView2 child window
//! covers anything the frame draws itself), follow light/dark mode and work with screen readers.

use windows::core::{w, HSTRING, PCSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, PostMessageW, SetForegroundWindow, TrackPopupMenuEx, HMENU, MF_CHECKED,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTALIGN, TPM_RIGHTBUTTON,
    TPM_TOPALIGN, WM_NULL,
};

/// One entry of a pop-up menu. Command ids must be non-zero (0 means "nothing chosen").
#[derive(Debug, Clone, PartialEq)]
pub enum MenuItem {
    Item { id: u32, label: String, shortcut: String, enabled: bool, checked: bool },
    Separator,
    Submenu { label: String, items: Vec<MenuItem> },
}

impl MenuItem {
    /// An enabled, unchecked command.
    pub fn new(id: u32, label: &str, shortcut: &str) -> Self {
        MenuItem::Item { id, label: label.into(), shortcut: shortcut.into(), enabled: true, checked: false }
    }
}

/// Menu text as Windows wants it: `&` doubled (it marks access keys), shortcut after a tab.
fn menu_text(label: &str, shortcut: &str) -> HSTRING {
    let label = label.replace('&', "&&");
    if shortcut.is_empty() { HSTRING::from(label) } else { HSTRING::from(format!("{label}\t{shortcut}")) }
}

fn build(items: &[MenuItem]) -> windows::core::Result<HMENU> {
    // SAFETY: plain Win32 calls; every created menu is attached to its parent or returned to the
    // caller, who destroys the top one (DestroyMenu destroys sub-menus too).
    unsafe {
        let menu = CreatePopupMenu()?;
        for item in items {
            match item {
                MenuItem::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, None)?,
                MenuItem::Item { id, label, shortcut, enabled, checked } => {
                    let mut flags = MF_STRING;
                    if !enabled {
                        flags |= MF_GRAYED;
                    }
                    if *checked {
                        flags |= MF_CHECKED;
                    }
                    AppendMenuW(menu, flags, *id as usize, &menu_text(label, shortcut))?
                }
                MenuItem::Submenu { label, items } => {
                    let sub = build(items)?;
                    AppendMenuW(menu, MF_STRING | MF_POPUP, sub.0 as usize, &menu_text(label, ""))?
                }
            }
        }
        Ok(menu)
    }
}

/// Show `items` at screen point `x, y` (physical pixels) and wait for a choice. With
/// `align_right` the menu's right edge is at `x` (for the ⋮ button). Returns the chosen id.
pub fn popup(owner: isize, x: i32, y: i32, align_right: bool, items: &[MenuItem]) -> Option<u32> {
    let menu = build(items).ok()?;
    let owner = HWND(owner as _);
    let align = if align_right { TPM_RIGHTALIGN } else { TPM_LEFTALIGN };
    // SAFETY: `menu` is a live menu destroyed below; `owner` is the browser window. The owner must
    // be foreground and get a WM_NULL afterwards so the menu closes on outside clicks (MSDN).
    let chosen = unsafe {
        let _ = SetForegroundWindow(owner);
        let id = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_TOPALIGN | align).0, x, y, owner, None);
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        id.0 as u32
    };
    (chosen != 0).then_some(chosen)
}

/// Light or dark menus. `None` follows Windows' app mode. Uses uxtheme's SetPreferredAppMode
/// (ordinal 135) and FlushMenuThemes (ordinal 136): undocumented but stable since Windows 10
/// 1903 and what File Explorer and Notepad use; without them Win32 menus are always light.
pub fn set_menu_theme(dark: Option<bool>) {
    let mode: i32 = match dark {
        None => 1,        // AllowDark: follow the system setting
        Some(true) => 2,  // ForceDark
        Some(false) => 3, // ForceLight
    };
    // SAFETY: uxtheme.dll is a system library; the exports are looked up by ordinal and called
    // with the signatures they have had since 1903. If either is missing nothing happens.
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        let ordinal = |n: usize| GetProcAddress(lib, PCSTR(n as *const u8));
        if let Some(set_mode) = ordinal(135) {
            let set_mode: extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
            set_mode(mode);
        }
        if let Some(flush) = ordinal(136) {
            let flush: extern "system" fn() = std::mem::transmute(flush);
            flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_text_escapes_access_keys_and_adds_shortcut() {
        assert_eq!(menu_text("Bookmarks & lists", "Ctrl+B").to_string(), "Bookmarks && lists\tCtrl+B");
        assert_eq!(menu_text("Exit", "").to_string(), "Exit");
    }

    #[test]
    fn builds_nested_menus() {
        let items = vec![
            MenuItem::new(1, "New tab", "Ctrl+T"),
            MenuItem::Separator,
            MenuItem::Submenu { label: "More tools".into(), items: vec![MenuItem::new(2, "Task manager", "Shift+Esc")] },
        ];
        let menu = build(&items).expect("menu");
        // SAFETY: destroying the menu just built.
        unsafe { DestroyMenu(menu).expect("destroy") };
    }
}
