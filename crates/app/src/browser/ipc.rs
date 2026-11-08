//! Commands from the native frame (tab strip, toolbar, address bar) and the native pop-up.

use serde_json::Value as Json;
use tao::window::ResizeDirection;
use ui::Action;

use crate::browser::Browser;

/// Query suggestions taken from the search engine's reply (estimated).
pub(crate) const MAX_ENGINE_SUGGESTIONS: usize = 4;
/// Give up on the engine's suggestions after this long (estimated).
pub(crate) const SUGGEST_TIMEOUT_MS: i32 = 1500;

impl Browser {
    /// Handle a command from the UI page. Returns false when the window should close.
    pub(crate) fn on_ipc(&mut self, msg: &str) -> bool {
        let mut parts = msg.split('\t');
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("");
        let id = arg.parse::<u64>().unwrap_or(0);
        match cmd {
            "ready" => {
                self.ui_ready = true;
                self.show_window();
                let settings = self.settings.clone();
                self.apply_settings(settings);
                self.render();
                if self.tabs.iter().any(|t| t.view.id == self.active && t.view.url == ui::NEWTAB_URL) {
                    self.focus_omnibox();
                }
            }
            "select" => self.activate(id),
            "close" => self.close_tab(id),
            "move" => {
                if let (Some(from), Ok(to)) = (self.index(id), parts.next().unwrap_or("").parse::<usize>()) {
                    let tab = self.tabs.remove(from);
                    self.tabs.insert(to.min(self.tabs.len()), tab);
                }
            }
            "newtab" => self.open_new_tab(),
            "home" => return self.act(Action::Home),
            "tabmenu" => {
                let left: f64 = parts.next().unwrap_or("").parse().unwrap_or(0.0);
                let top: f64 = parts.next().unwrap_or("").parse().unwrap_or(0.0);
                if let Some(pick) = self.show_tab_menu(id, left, top) {
                    return self.on_menu(&pick) && !self.tabs.is_empty();
                }
            }
            "tabsearch" => {
                if !self.toggles_closed("tabsearch") {
                    self.show_tab_search();
                }
            }
            "mutetab" if self.settings.speaker_mutes => self.tab_action(&format!("mute:{id}")),
            "mutetab" => {}
            "groupname" => {
                let name: String = parts.next().unwrap_or("").trim().chars().take(40).collect();
                if let (false, Some(g)) = (name.is_empty(), self.groups.iter_mut().find(|g| g.id as u64 == id)) {
                    g.name = name;
                }
                self.render();
            }
            "groupmenu" => {
                let x: f64 = parts.next().unwrap_or("").parse().unwrap_or(0.0);
                let y: f64 = parts.next().unwrap_or("").parse().unwrap_or(0.0);
                if let Some(pick) = self.show_group_menu(id as u32, x, y) {
                    return self.on_menu(&pick) && !self.tabs.is_empty();
                }
            }
            // A one-line field of the pop-up was confirmed: input, kind, text.
            "input" => {
                let text = msg.splitn(3, '\t').nth(2).unwrap_or("").to_string();
                self.hide_overlay();
                if let Some(gid) = arg.strip_prefix("grouprename:") {
                    return self.on_ipc(&format!("groupname\t{gid}\t{text}"));
                }
            }
            "groupcollapse" => {
                if let Some(g) = self.groups.iter_mut().find(|g| g.id as u64 == id) {
                    g.collapsed = !g.collapsed;
                }
                self.render();
            }
            "bookmark" => return self.act(Action::Bookmark),
            // The native address bar.
            "omnichange" => {
                let text = msg.split_once('\t').map_or("", |(_, t)| t).to_string();
                self.on_omnibox_change(&text);
            }
            "omnikey" => return self.on_omnibox_key(arg),
            "omnifocus" => self.on_omnibox_focus(),
            "omniblur" => self.on_omnibox_blur(),
            // A browser shortcut typed while the address bar has the keyboard.
            "key" => {
                let flag = |p: Option<&str>| p == Some("1");
                let (ctrl, shift, alt) = (flag(parts.next()), flag(parts.next()), flag(parts.next()));
                if let Some(action) = ui::shortcut(id as u32, ctrl, shift, alt) {
                    return self.act(action);
                }
            }
            "overlay" => {
                let nums: Vec<f64> = parts.by_ref().take(4).map(|v| v.parse().unwrap_or(0.0)).collect();
                let data = parts.next().unwrap_or("{}");
                // The pop-up gets validated JSON only.
                if let (true, Ok(v)) = (nums.len() == 4, serde_json::from_str::<Json>(data)) {
                    self.show_overlay(arg, [nums[0], nums[1], nums[2], nums[3]], &v.to_string());
                }
            }
            "overlaysel" => self.popup.highlight(arg.parse().ok()),
            // "overlayhide	suggest" from the address bar only closes the suggestions, not a menu.
            "overlayhide" if arg.is_empty() || arg == self.overlay_kind => self.hide_overlay(),
            "overlayhide" => {}
            "overlayclose" => {
                self.hide_overlay();
                self.focus_omnibox();
            }
            "profile" => {
                self.hide_overlay();
                let right: f64 = arg.parse().unwrap_or(0.0);
                let top: f64 = parts.next().unwrap_or("").parse().unwrap_or(0.0);
                if let Some(pick) = self.show_profile(right, top) {
                    return self.on_menu(&pick) && !self.tabs.is_empty();
                }
            }
            "siteinfo" => {
                self.hide_overlay();
                let left: f64 = arg.parse().unwrap_or(0.0);
                let top: f64 = parts.next().unwrap_or("").parse().unwrap_or(0.0);
                if let Some(pick) = self.show_site_info(left, top) {
                    return self.on_menu(&pick) && !self.tabs.is_empty();
                }
            }
            "menu" => {
                self.hide_overlay();
                let right: f64 = arg.parse().unwrap_or(0.0);
                let top: f64 = parts.next().unwrap_or("").parse().unwrap_or(0.0);
                if let Some(pick) = self.show_menu(right, top) {
                    return self.on_menu(&pick) && !self.tabs.is_empty();
                }
            }
            "pick" => {
                let value = parts.next().unwrap_or("").to_string();
                // A tab closed with its × in tab search: the list stays open without it.
                if arg == "menu" && value.starts_with("tab:close:") && self.overlay_kind == "tabsearch" {
                    self.on_menu(&value);
                    if !self.tabs.is_empty() {
                        self.show_tab_search();
                    }
                    return !self.tabs.is_empty();
                }
                if !(arg == "menu" && matches!(value.as_str(), "zoomin" | "zoomout" | "zoomreset")) {
                    self.hide_overlay();
                }
                match arg {
                    "suggest" => self.navigate(&value),
                    "menu" if value.is_empty() => {}
                    "menu" => return self.on_menu(&value) && !self.tabs.is_empty(),
                    _ => {}
                }
            }
            "open" => {
                let url = ui::resolve_input(arg, &self.search.0).unwrap_or_else(|| ui::NEWTAB_URL.into());
                self.open_after_active(&url);
            }
            "nav" => self.navigate(arg),
            // A control of a native page: page, kind, id, value (the value may hold tabs).
            "page" => {
                let mut f = msg.splitn(4, '\t').skip(1);
                let (kind, id, value) = (f.next().unwrap_or(""), f.next().unwrap_or(""), f.next().unwrap_or(""));
                self.on_page_command(kind, id, value);
            }
            "back" => return self.act(Action::Back),
            "forward" => return self.act(Action::Forward),
            "reload" => return self.act(Action::Reload),
            "stop" => {
                if let Some(wv) = self.active_webview() {
                    let _ = platform::stop(wv);
                }
            }
            "focuspage" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.focus();
                }
            }
            "drag" => {
                let _ = self.window.drag_window();
            }
            "resize" => {
                let dir = match arg {
                    "nw" => ResizeDirection::NorthWest,
                    "ne" => ResizeDirection::NorthEast,
                    _ => ResizeDirection::North,
                };
                let _ = self.window.drag_resize_window(dir);
            }
            "min" => self.window.set_minimized(true),
            "max" => {
                self.window.set_maximized(!self.window.is_maximized());
            }
            "quit" => return !self.confirm_quit(),
            "exitforupdate" => return false,
            other => eprintln!("ui: unknown command {other:?}"),
        }
        !self.tabs.is_empty()
    }
}
