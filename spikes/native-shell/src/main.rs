//! Spike for the native UI: a Slint window draws the browser frame (tab, address bar, ⋮), a
//! WebView2 child fills the page area, and the ⋮ menu is its own borderless Slint window owned by
//! the frame, so it appears *over* the web page (a child HWND would otherwise cover it).
//!
//! `native-shell-spike [url] [--menu]`: `--menu` opens the menu once after start (for screenshots).

use std::cell::RefCell;
use std::rc::Rc;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{Rect, WebView, WebViewBuilder};

slint::slint! {
    import { LineEdit } from "std-widgets.slint";

    component IconButton inherits Rectangle {
        in property <string> glyph;
        callback clicked;
        width: 34px; height: 34px; border-radius: 17px;
        background: touch.has-hover ? #ffffff1a : transparent;
        Text { text: root.glyph; color: #e3e1ea; font-size: 18px; horizontal-alignment: center; vertical-alignment: center; }
        touch := TouchArea { clicked => { root.clicked(); } }
    }

    export component Shell inherits Window {
        title: "Kakangkung (native shell spike)";
        preferred-width: 1280px; preferred-height: 820px;
        background: #26232d;
        in-out property <string> url;
        in property <string> tab-title: "New Tab";
        callback navigate(string);
        callback open-menu(length, length);
        callback resized();
        changed width => { root.resized(); }
        changed height => { root.resized(); }

        VerticalLayout {
            // Tab strip
            Rectangle {
                height: 41px;
                HorizontalLayout {
                    padding-left: 8px; alignment: start;
                    Rectangle {
                        y: 7px; width: 240px; height: 34px; border-radius: 8px;
                        background: #35323c;
                        Text { x: 14px; text: root.tab-title; color: #ebe8f2; font-size: 12px; vertical-alignment: center; }
                    }
                }
            }
            // Toolbar
            Rectangle {
                height: 46px; background: #35323c;
                HorizontalLayout {
                    padding: 6px; spacing: 4px;
                    IconButton { glyph: "←"; }
                    IconButton { glyph: "→"; }
                    IconButton { glyph: "⟳"; }
                    Rectangle {
                        border-radius: 17px; background: #24222a;
                        HorizontalLayout {
                            padding-left: 12px; padding-right: 12px;
                            address := LineEdit { text <=> root.url; accepted(t) => { root.navigate(t); } }
                        }
                    }
                    menu-button := IconButton {
                        glyph: "⋮";
                        clicked => { root.open-menu(self.absolute-position.x + self.width, self.absolute-position.y + self.height + 2px); }
                    }
                }
            }
            // Page area: the WebView2 child window sits here.
            Rectangle { }
        }
    }

}

/// Height of the tab strip + toolbar in logical pixels (matches the layout above).
const FRAME_HEIGHT: f64 = 87.0;

fn hwnd(w: &slint::Window) -> isize {
    match w.window_handle().window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        _ => 0,
    }
}

fn page_rect(w: &slint::Window) -> Rect {
    let size = w.size().to_logical(w.scale_factor());
    Rect {
        position: LogicalPosition::new(0.0, FRAME_HEIGHT).into(),
        size: LogicalSize::new(size.width as f64, (size.height as f64 - FRAME_HEIGHT).max(0.0)).into(),
    }
}

/// Client-area point of `hwnd` in screen coordinates (physical pixels).
fn client_to_screen(hwnd: isize, x: i32, y: i32) -> (i32, i32) {
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    let mut pt = POINT { x, y };
    // SAFETY: `pt` is a writable local; `hwnd` is the live frame window.
    unsafe {
        let _ = ClientToScreen(HWND(hwnd as _), &mut pt);
    }
    (pt.x, pt.y)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let start = args.iter().find(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| "https://example.com".into());
    let menu_at_start = args.iter().any(|a| a == "--menu");

    let shell = Shell::new()?;
    shell.set_url(start.clone().into());
    shell.show()?;
    // The native window exists once the event loop runs: create the WebView2 child then.
    let page: Rc<RefCell<Option<WebView>>> = Rc::default();
    {
        let (weak, cell, start) = (shell.as_weak(), page.clone(), start.clone());
        slint::Timer::single_shot(std::time::Duration::from_millis(300), move || {
            let Some(s) = weak.upgrade() else { return };
            match WebViewBuilder::new().with_url(&start).with_bounds(page_rect(s.window())).build_as_child(&s.window().window_handle()) {
                Ok(wv) => *cell.borrow_mut() = Some(wv),
                Err(e) => eprintln!("page: {e}"),
            }
        });
    }

    let weak = shell.as_weak();
    let p = page.clone();
    shell.on_resized(move || {
        if let (Some(s), Some(wv)) = (weak.upgrade(), p.borrow().as_ref()) {
            let _ = wv.set_bounds(page_rect(s.window()));
        }
    });
    let p = page.clone();
    let weak = shell.as_weak();
    shell.on_navigate(move |text| {
        let url = if text.contains("://") { text.to_string() } else { format!("https://{text}") };
        if let Some(wv) = p.borrow().as_ref() {
            let _ = wv.load_url(&url);
        }
        if let Some(s) = weak.upgrade() {
            s.set_tab_title(url.into());
        }
    });

    // The ⋮ menu is a native Windows menu: always above the page, light/dark like Windows.
    platform::menu::set_menu_theme(None);
    {
        let (weak, page2) = (shell.as_weak(), page.clone());
        shell.on_open_menu(move |right, bottom| {
            let Some(s) = weak.upgrade() else { return };
            let owner = hwnd(s.window());
            let scale = s.window().scale_factor();
            let (x, y) = client_to_screen(owner, (right * scale) as i32, (bottom * scale) as i32);
            use platform::menu::MenuItem;
            let items = vec![
                MenuItem::new(1, "New tab", "Ctrl+T"),
                MenuItem::new(2, "New window", "Ctrl+N"),
                MenuItem::new(3, "New Incognito window", "Ctrl+Shift+N"),
                MenuItem::Separator,
                MenuItem::new(4, "History", "Ctrl+H"),
                MenuItem::new(5, "Downloads", "Ctrl+J"),
                MenuItem::new(6, "Bookmarks", "Ctrl+Shift+O"),
                MenuItem::Separator,
                MenuItem::Submenu {
                    label: "More tools".into(),
                    items: vec![MenuItem::new(20, "Task manager", "Shift+Esc"), MenuItem::new(21, "Developer tools", "Ctrl+Shift+I")],
                },
                MenuItem::Separator,
                MenuItem::new(8, "Settings", ""),
                MenuItem::new(9, "Exit", ""),
            ];
            let t = std::time::Instant::now();
            eprintln!("menu: popup at screen {x},{y} owner={owner}");
            let picked = platform::menu::popup(owner, x, y, true, &items);
            eprintln!("menu: returned {picked:?} after {} ms (err {:?})", t.elapsed().as_millis(), std::io::Error::last_os_error());
            match picked {
                Some(9) => {
                    let _ = slint::quit_event_loop();
                }
                Some(id) => {
                    if let Some(wv) = page2.borrow().as_ref() {
                        let _ = wv.evaluate_script(&format!("document.title = 'native menu picked {id}'"));
                    }
                }
                None => {}
            }
        });
    }
    if menu_at_start {
        let s = shell.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(4000), move || {
            eprintln!("menu: start timer fired");
            if let Some(s) = s.upgrade() {
                let w = s.window().size().to_logical(s.window().scale_factor()).width;
                s.invoke_open_menu(w - 6.0, 82.0);
            }
        });
    }
    slint::run_event_loop()?;
    Ok(())
}
