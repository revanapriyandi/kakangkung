//! Tab right-click actions (pin, mute, groups, split view, windows) and page tools.

use serde_json::json;
use tabs::Event as TabEvent;

use crate::browser::Browser;

/// Reading mode, run in the page: finds the block with the most paragraph text and shows a
/// clean copy of its headings, paragraphs and large images in a full-window reader (closed
/// shadow root and a constructed stylesheet, so page styles and CSP don't interfere). Text is
/// copied with textContent only, never as HTML. Running it again closes the reader.
pub(crate) const READER_JS: &str = include_str!("../../../../assets/reader.js");

impl Browser {
    /// Drop groups no tab belongs to any more.
    pub(crate) fn prune_groups(&mut self) {
        let tabs = &self.tabs;
        self.groups.retain(|g| tabs.iter().any(|t| t.view.group == g.id));
    }

    /// Start another Kakangkung window on the same profile, showing `url` (Incognito if asked).
    pub(crate) fn open_window(&mut self, url: &str, incognito: bool) {
        if let Some(probe) = &mut self.window_probe {
            probe.push((url.to_string(), incognito));
            return;
        }
        let flag = if incognito { "--incognito" } else { "--new-window" };
        let r = std::env::current_exe().and_then(|exe| std::process::Command::new(exe).arg(flag).arg(url).spawn());
        if let Err(e) = r {
            eprintln!("new window: {e}");
        }
    }

    /// Actions of the tab right-click menu and tab search: "<action>:<tab id>".
    pub(crate) fn tab_action(&mut self, what: &str) {
        let (action, id) = what.rsplit_once(':').unwrap_or((what, ""));
        let id: u64 = id.parse().unwrap_or(0);
        let Some(i) = self.index(id) else { return };
        let url = self.tabs[i].view.url.clone();
        match action {
            "goto" => self.activate(id),
            "newright" => self.open_tab(ui::NEWTAB_URL, Some(i + 1)),
            "reload" => {
                if let Some(wv) = &self.tabs[i].webview {
                    let _ = wv.reload();
                }
            }
            "dup" => self.open_tab(&url, Some(i + 1)),
            "pin" => {
                let pinned = !self.tabs[i].view.pinned;
                let mut tab = self.tabs.remove(i);
                tab.view.pinned = pinned;
                if pinned {
                    tab.view.group = 0;
                }
                // Pinned tabs form a block at the start of the strip.
                let at = self.tabs.iter().take_while(|t| t.view.pinned).count();
                self.tabs.insert(at, tab);
                self.prune_groups();
            }
            "mute" => {
                let muted = !self.tabs[i].view.muted;
                self.tabs[i].view.muted = muted;
                if let Some(wv) = &self.tabs[i].webview {
                    if let Err(e) = platform::set_muted(wv, muted) {
                        eprintln!("mute: {e}");
                    }
                }
            }
            "ungroup" => {
                self.tabs[i].view.group = 0;
                self.prune_groups();
            }
            g if g.starts_with("group") => {
                let mut gid: u32 = g["group".len()..].parse().unwrap_or(0);
                if gid == 0 {
                    gid = self.next_group;
                    self.next_group += 1;
                    let color = ui::GROUP_COLORS[(gid as usize - 1) % ui::GROUP_COLORS.len()];
                    self.groups.push(ui::Group { id: gid, name: format!("Group {gid}"), color, collapsed: false });
                }
                let mut tab = self.tabs.remove(i);
                tab.view.group = gid;
                tab.view.pinned = false;
                // A group's tabs stay together: join after its last tab (or stay put if new).
                let at = self.tabs.iter().rposition(|t| t.view.group == gid).map_or(i.min(self.tabs.len()), |p| p + 1);
                let at = at.max(self.tabs.iter().take_while(|t| t.view.pinned).count());
                self.tabs.insert(at, tab);
                self.prune_groups();
            }
            // Split view: the current tab on the left, the other on the right; activating with
            // `split` set shows both halves (see `visible_with`).
            "split" | "splitnew" => {
                let left = self.active;
                let right = if action == "split" {
                    id
                } else {
                    self.open_tab(ui::NEWTAB_URL, Some(i + 1));
                    self.active
                };
                self.split = Some((left, right));
                self.activate(right);
                self.relayout();
            }
            "unsplit" => {
                let keep = self.active;
                if let Some((l, r)) = self.split.take().filter(|&(l, r)| keep == l || keep == r) {
                    if let Some(j) = self.index(if keep == l { r } else { l }) {
                        self.apply(j, TabEvent::Deactivate);
                    }
                }
                self.relayout();
            }
            "window" => {
                self.open_window(&url, self.incognito);
                self.close_tab(id);
            }
            "close" => self.close_tab(id),
            "others" => {
                let keep: Vec<u64> = self.tabs.iter().filter(|t| t.view.id == id || t.view.pinned).map(|t| t.view.id).collect();
                let close: Vec<u64> = self.tabs.iter().map(|t| t.view.id).filter(|t| !keep.contains(t)).collect();
                for c in close {
                    self.close_tab(c);
                }
                self.activate(id);
            }
            "right" => {
                let close: Vec<u64> = self.tabs[i + 1..].iter().map(|t| t.view.id).collect();
                for c in close {
                    self.close_tab(c);
                }
            }
            _ => {}
        }
        self.render();
    }

    /// Picture in picture for the page's playing (else largest) video; again to leave it.
    /// Runs through the DevTools protocol so it counts as a user gesture, which PiP requires.
    /// Simplification: videos inside cross-origin iframes (embedded players) are not reached.
    pub(crate) fn picture_in_picture(&self) {
        const PIP_JS: &str = "(() => { if (document.pictureInPictureElement) return document.exitPictureInPicture(); \
            const vs = [...document.querySelectorAll('video')].filter(v => v.readyState > 0); \
            const v = vs.find(v => !v.paused) || vs.sort((a, b) => b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)[0]; \
            if (v) { v.disablePictureInPicture = false; return v.requestPictureInPicture(); } })()";
        if let Some(wv) = self.active_webview() {
            let params = json!({ "expression": PIP_JS, "userGesture": true, "awaitPromise": false }).to_string();
            if let Err(e) = platform::devtools_call(wv, "Runtime.evaluate", &params) {
                eprintln!("picture in picture: {e}");
            }
        }
    }

    /// Translate the page with Google Translate's web proxy, into the browser's language.
    pub(crate) fn translate(&self) {
        let Some(t) = self.tabs.iter().find(|t| t.view.id == self.active) else { return };
        if !storage::storable(&t.view.url) || t.view.url.contains(".translate.goog") {
            return;
        }
        let url = serde_json::to_string(&t.view.url).unwrap_or_default();
        // Target language from Settings > Languages ("" = the browser's own language).
        let lang = Some(self.settings.translate_to.as_str()).filter(|l| *l != "auto").unwrap_or("");
        let lang = serde_json::to_string(lang).unwrap_or_default();
        let js = format!(
            "location.href = 'https://translate.google.com/translate?sl=auto&tl=' + encodeURIComponent({lang} || (navigator.language || 'en').split('-')[0]) + '&u=' + encodeURIComponent({url})"
        );
        if let Some(wv) = self.active_webview() {
            let _ = wv.evaluate_script(&js);
        }
    }

    /// Reading mode: the page's main text in a calm, full-window reader; again to close it.
    pub(crate) fn reading_mode(&self) {
        if let Some(wv) = self.active_webview() {
            let _ = wv.evaluate_script(READER_JS);
        }
    }
}
