//! Tab lifecycle: open, show, hide, freeze (Memory Saver), split view and close.

use std::time::Instant;

use tabs::{transition, Command, Event as TabEvent, Lifecycle};
use ui::TabView;
use wry::{MemoryUsageLevel, Rect, WebViewExtWindows};
use wry::dpi::{PhysicalPosition, PhysicalSize};

use crate::timing;
use crate::browser::{chrome_height, page_bounds, set_default_zoom, Browser, Tab};
use crate::msg::Msg;

impl Browser {
    pub(crate) fn open_tab(&mut self, url: &str, at: Option<usize>) {
        let id = self.next_id;
        self.next_id += 1;
        let muted = self.settings.mute_sites;
        let view = TabView { id, url: url.into(), title: String::new(), loading: true, private: self.incognito, muted, ..Default::default() };
        // Created hidden and in the DISCARDED state; activation runs CreateWebview + Show.
        let tab = Tab::unloaded(view);
        let at = at.unwrap_or(self.tabs.len()).min(self.tabs.len());
        self.tabs.insert(at, tab);
        self.activate(id);
        if url == ui::NEWTAB_URL {
            self.focus_omnibox();
        }
    }

    /// Show pop-up webview `id` (built by the new-window handler of tab `opener`) as a tab next
    /// to its opener. It keeps `window.opener` and closes when its page calls `window.close()`.
    pub(crate) fn adopt_popup(&mut self, id: u64, opener: u64, url: String) {
        let Some(wv) = super::take_popup(id) else { return };
        let at = self.index(opener).map(|i| i + 1);
        let private = at.map_or(self.incognito, |i| self.tabs[i - 1].view.private);
        let mut tab = Tab::unloaded(TabView { id, url, title: String::new(), loading: true, private, muted: self.settings.mute_sites, ..Default::default() });
        for e in platform::apply_protection(&wv, self.settings.protection) {
            eprintln!("pop-up {id}: {e}");
        }
        tab.hooks = self.hook_tab(id, &wv);
        set_default_zoom(&wv, self.settings.zoom);
        let p = self.proxy.clone();
        let _ = platform::on_close_requested(&wv, move || {
            let _ = p.send_event(Msg::CloseTab(id));
        });
        tab.webview = Some(wv);
        tab.state = Lifecycle::Idle;
        let at = at.unwrap_or(self.tabs.len()).min(self.tabs.len());
        self.tabs.insert(at, tab);
        self.activate(id);
    }

    /// Open a page the way Chrome opens its own pages: switch to a tab already showing it, else
    /// use the current tab if it is an empty new tab, else open a tab next to the current one.
    pub(crate) fn open_after_active(&mut self, url: &str) {
        // A native page counts as one page whatever section it shows.
        let base = |u: &str| match crate::native::page_of(u) {
            Some(page) => format!("{}{page}", ui::INTERNAL),
            None => u.split('#').next().unwrap_or(u).trim_end_matches('/').to_string(),
        };
        if let Some(id) = self.tabs.iter().find(|t| base(&t.view.url) == base(url)).map(|t| t.view.id) {
            self.activate(id);
            if crate::native::anchor(url).is_some() {
                self.navigate(url);
            }
            return;
        }
        let blank = self.tabs.iter().any(|t| t.view.id == self.active && (t.view.url == ui::NEWTAB_URL || t.view.url == "about:blank"));
        if blank {
            self.navigate(url);
        } else {
            let at = self.index(self.active).map(|i| i + 1);
            self.open_tab(url, at);
        }
    }

    /// A restored tab: listed in the strip, webview created only when it is first selected.
    pub(crate) fn open_tab_lazy(&mut self, url: &str) -> u64 {
        self.insert_lazy(url, self.tabs.len())
    }

    /// A tab opened behind the current one (link with "open in background"): loaded when selected.
    pub(crate) fn open_tab_background(&mut self, url: &str, at: usize) {
        self.insert_lazy(url, at);
        self.render();
    }

    fn insert_lazy(&mut self, url: &str, at: usize) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let title = Some(ui::host(url)).filter(|h| !h.is_empty()).unwrap_or(url).to_string();
        let view = TabView { id, url: url.into(), title, discarded: true, private: self.incognito, muted: self.settings.mute_sites, ..Default::default() };
        self.tabs.insert(at.min(self.tabs.len()), Tab::unloaded(view));
        id
    }

    /// Memory saver: freeze tabs hidden longer than the mode allows. Returns when to look again.
    pub(crate) fn memory_tick(&mut self) -> Option<Instant> {
        let t = self.memory_mode.default_thresholds();
        let after = t.suspend_after?;
        let now = Instant::now();
        let mut next: Option<Instant> = None;
        for i in 0..self.tabs.len() {
            let (state, since) = (self.tabs[i].state, self.tabs[i].hidden_since);
            if state != Lifecycle::Idle || self.tabs[i].view.id == self.active {
                continue;
            }
            let Some(since) = since else { continue };
            if now >= since + after {
                self.apply(i, TabEvent::SuspendDue);
            } else {
                let due = since + after;
                next = Some(next.map_or(due, |n| n.min(due)));
            }
        }
        // Simplification: discarding waits for unsaved-input detection; suspended
        // tabs keep their page and form state, discarded ones would not.
        next
    }

    /// Tabs on screen when `id` is selected: both halves of a split view, else just `id`.
    /// The selected tab comes last so it gets the keyboard focus.
    pub(crate) fn visible_with(&self, id: u64) -> Vec<u64> {
        match self.split {
            Some((l, r)) if id == l => vec![r, l],
            Some((l, r)) if id == r => vec![l, r],
            _ => vec![id],
        }
    }

    pub(crate) fn activate(&mut self, id: u64) {
        self.hide_overlay();
        let before = self.visible_with(self.active);
        let after = self.visible_with(id);
        for old in before {
            if let (false, Some(i)) = (after.contains(&old), self.index(old)) {
                self.apply(i, TabEvent::Deactivate);
            }
        }
        self.active = id;
        for &new in &after {
            if let Some(i) = self.index(new) {
                self.apply(i, TabEvent::Activate);
            }
        }
        if after.len() > 1 {
            if let Some(wv) = self.active_webview() {
                let _ = wv.focus();
            }
        }
        self.refresh_bookmark();
        self.render();
    }

    /// Where tab `id` is drawn: the whole page area, or its half of the split view.
    pub(crate) fn tab_bounds(&self, id: u64) -> Rect {
        let full = page_bounds(&self.window);
        let Some((l, _)) = self.split.filter(|&(l, r)| id == l || id == r) else { return full };
        let size = self.window.inner_size();
        let top = chrome_height(&self.window).min(size.height);
        let gap = (4.0 * self.window.scale_factor()).round() as u32;
        let half = size.width.saturating_sub(gap) / 2;
        let (x, w) = if id == l { (0, half) } else { (half + gap, size.width.saturating_sub(half + gap)) };
        Rect { position: PhysicalPosition::new(x as i32, top as i32).into(), size: PhysicalSize::new(w, size.height - top).into() }
    }

    /// Run the lifecycle state machine for tab `i` and execute its commands.
    pub(crate) fn apply(&mut self, i: usize, ev: TabEvent) {
        let tab = &self.tabs[i];
        // Downloads need no signal: they run in WebView2's browser process, not in the page.
        // Simplification: unsaved-input detection is still missing; it only matters once discarding is
        // switched on (a suspended tab keeps its forms).
        let s = &self.settings;
        // A native page has nothing to freeze.
        let native = crate::native::page_of(&tab.view.url).is_some();
        let protected = native || tab.view.audio || (s.protect_media && tab.view.media) || (s.protect_pinned && tab.view.pinned) || s.kept_active(&tab.view.url);
        let (next, cmds) = match transition(tab.state, ev, protected) {
            Ok(step) => step,
            Err(e) => {
                eprintln!("tab {}: {ev:?} refused in {:?}: {e:?}", tab.view.id, tab.state);
                return;
            }
        };
        self.tabs[i].state = next;
        self.tabs[i].hidden_since = match next {
            Lifecycle::Active => None,
            _ => self.tabs[i].hidden_since.or(Some(Instant::now())),
        };
        for cmd in cmds {
            if let Err(e) = self.exec(i, cmd) {
                eprintln!("tab {}: {cmd:?} failed: {e}", self.tabs[i].view.id);
            }
        }
    }

    pub(crate) fn exec(&mut self, i: usize, cmd: Command) -> Result<(), String> {
        let id = self.tabs[i].view.id;
        if let Some(page) = crate::native::page_of(&self.tabs[i].view.url) {
            self.exec_native(i, page, cmd);
            return Ok(());
        }
        if cmd == Command::CreateWebview {
            let url = self.tabs[i].view.url.clone();
            let private = self.tabs[i].view.private;
            timing("tab webview: start");
            let first = self.env.is_none();
            let wv = self.build_tab_webview(id, &url, false, private).map_err(|e| e.to_string())?;
            timing("tab webview: created");
            // The profile's own settings, once its first webview exists.
            if first {
                if let Some(folder) = &self.downloads_folder {
                    let _ = platform::set_download_folder(&wv, folder);
                }
                if std::mem::take(&mut self.clear_at_start) {
                    let _ = wv.clear_all_browsing_data();
                }
            }
            for e in platform::apply_protection(&wv, self.settings.protection) {
                eprintln!("tab {id}: {e}");
            }
            self.tabs[i].hooks = self.hook_tab(id, &wv);
            set_default_zoom(&wv, self.settings.zoom);
            if self.tabs[i].view.muted {
                let _ = platform::set_muted(&wv, true);
            }
            self.tabs[i].webview = Some(wv);
            self.tabs[i].view.discarded = false;
            return Ok(());
        }
        if cmd == Command::DropWebview {
            let tab = &mut self.tabs[i];
            if let Some(wv) = tab.webview.take() {
                platform::unhook(&wv, &mut tab.hooks);
            }
            tab.view.discarded = true;
            return Ok(());
        }
        let Some(wv) = self.tabs[i].webview.as_ref() else {
            return Err("no webview".into());
        };
        let e = |e: wry::Error| e.to_string();
        match cmd {
            Command::Show => {
                // The first web page is ready: the hopping logo makes way.
                if self.native.tab == 0 && self.page.is_visible() {
                    self.page.set_visible(false);
                }
                wv.set_bounds(self.tab_bounds(id)).map_err(e)?;
                wv.set_visible(true).map_err(e)?;
                wv.focus().map_err(e)
            }
            Command::Hide => wv.set_visible(false).map_err(e),
            Command::TrySuspend => {
                let p = self.proxy.clone();
                platform::try_suspend(wv, move |r| {
                    let _ = p.send_event(Msg::Suspended { id, ok: matches!(r, Ok(true)) });
                })
                .map_err(|e| e.to_string())
            }
            Command::Resume => platform::resume(wv).map_err(|e| e.to_string()),
            Command::SetLevelLow => wv.set_memory_usage_level(MemoryUsageLevel::Low).map_err(e),
            Command::SetLevelNormal => wv.set_memory_usage_level(MemoryUsageLevel::Normal).map_err(e),
            Command::CreateWebview | Command::DropWebview => Ok(()),
        }
    }

    /// A tab of a native page: no webview, the page window shows instead.
    fn exec_native(&mut self, i: usize, page: &str, cmd: Command) {
        let id = self.tabs[i].view.id;
        match cmd {
            Command::CreateWebview => {
                let v = &mut self.tabs[i].view;
                v.discarded = false;
                v.loading = false;
                v.title = crate::native::title(page);
                // Counts as a finished load (bookmark star, self-test).
                let _ = self.proxy.send_event(Msg::Load { id, url: v.url.clone(), done: true });
            }
            Command::DropWebview => self.tabs[i].view.discarded = true,
            Command::Show => self.show_native(id),
            Command::Hide => self.hide_native(id),
            _ => {}
        }
    }

    /// The tab's address switched between a native page and a web page (or to another section
    /// of the same native page): rebuild what shows it.
    pub(crate) fn reload_kind(&mut self, i: usize) {
        let id = self.tabs[i].view.id;
        let url = self.tabs[i].view.url.clone();
        let native = crate::native::page_of(&url).is_some();
        if native && self.native.tab == id && self.tabs[i].webview.is_none() {
            // Still a native page: show it again (anchor, fresh data) under its own title.
            self.tabs[i].view.title = crate::native::page_of(&url).map(crate::native::title).unwrap_or_default();
            self.show_native(id);
            return;
        }
        self.hide_native(id);
        let tab = &mut self.tabs[i];
        if let Some(wv) = tab.webview.take() {
            platform::unhook(&wv, &mut tab.hooks);
        }
        tab.state = Lifecycle::Discarded;
        tab.view.loading = !native;
        self.apply(i, TabEvent::Activate);
    }

    pub(crate) fn close_tab(&mut self, id: u64) {
        let Some(i) = self.index(id) else { return };
        self.hide_native(id);
        let mut partner = None;
        if let Some((l, r)) = self.split.filter(|&(l, r)| id == l || id == r) {
            // The other half fills the page again (and is selected if the closed one was).
            self.split = None;
            let other = if id == l { r } else { l };
            partner = Some(other);
            if let Some(wv) = self.index(other).and_then(|j| self.tabs[j].webview.as_ref()) {
                let _ = wv.set_bounds(page_bounds(&self.window));
            }
        }
        let mut tab = self.tabs.remove(i);
        self.prune_groups();
        if let Some(wv) = tab.webview.take() {
            platform::unhook(&wv, &mut tab.hooks);
        }
        if !tab.view.url.is_empty() {
            self.closed.push(super::tab_search::ClosedTab { url: tab.view.url, title: tab.view.title, at: Instant::now() });
        }
        if self.tabs.is_empty() {
            // "When the last tab is closed": keep the window with a new tab, or let it close.
            if self.settings.keep_window {
                self.active = 0;
                self.open_new_tab();
            }
            return;
        }
        if id == self.active {
            // The tab to the right, else the one to the left.
            let next = partner.unwrap_or(self.tabs[i.min(self.tabs.len() - 1)].view.id);
            self.active = 0;
            self.activate(next);
        } else {
            self.render();
        }
    }
}
