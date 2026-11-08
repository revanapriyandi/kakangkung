//! The password manager in the tabs (assets/passwords.js): saved logins offered under a login
//! field, and "Save password?" after signing in. The vault itself is `storage::passwords`.

use platform::popup::PopupRow;
use serde_json::{json, Value as Json};
use storage::passwords::{origin_of, Credential, Login};

use crate::browser::Browser;
use crate::i18n::tr;
use crate::msg::Msg;

/// The pop-up of saved logins is at least this wide (DIPs).
const FILL_MIN_W: f64 = 280.0;
/// The save dialog's width (DIPs).
const SAVE_W: f64 = 400.0;

/// What the password manager is waiting on.
#[derive(Default)]
pub(crate) struct PasswordState {
    /// The login field that has focus: tab, its page's origin, where it is (DIPs: x, bottom, width).
    focus: Option<(u64, String, [f64; 3])>,
    /// The last login offered for saving and not answered yet: tab, login, update. The key in
    /// the address bar shows it again ("not now" keeps it, like Chrome).
    offer: Option<(u64, Credential, bool)>,
    /// The active page's origin when it has saved logins, and how many.
    pub(crate) saved: Option<(String, usize)>,
}

impl Browser {
    /// `kk:pw:<what>` from the page in tab `tab` at `url`.
    pub(crate) fn on_password_message(&mut self, tab: u64, url: &str, what: &str) {
        let Some(origin) = origin_of(url) else { return };
        if what == "blur" {
            self.passwords.focus = None;
            if self.overlay_open && self.overlay_kind == "pwfill" {
                self.hide_overlay();
            }
        } else if let Some(at) = what.strip_prefix("focus:") {
            let Ok(r) = serde_json::from_str::<Json>(at) else { return };
            if tab != self.active {
                return;
            }
            // The page's CSS pixels are DIPs times its zoom: devicePixelRatio / the window's scale.
            let zoom = r["dpr"].as_f64().unwrap_or(1.0) / self.window.scale_factor();
            let page = self.tab_bounds(tab).position.to_logical::<f64>(self.window.scale_factor());
            let num = |k: &str| r[k].as_f64().unwrap_or(0.0) * zoom;
            self.passwords.focus = Some((tab, origin.clone(), [page.x + num("x"), page.y + num("y") + 2.0, num("w")]));
            let p = self.proxy.clone();
            self.db(move |s| {
                let logins = s.logins_for(&origin)?;
                if !logins.is_empty() {
                    let _ = p.send_event(Msg::Logins { tab, origin, logins });
                }
                Ok(())
            });
        } else if let Some(sent) = what.strip_prefix("save:") {
            let private = self.index(tab).is_none_or(|i| self.tabs[i].view.private);
            if private || !self.settings.protection.save_passwords {
                return;
            }
            let Ok(v) = serde_json::from_str::<Json>(sent) else { return };
            let cred = Credential { origin, username: v["u"].as_str().unwrap_or("").trim().to_string(), password: v["p"].as_str().unwrap_or("").to_string() };
            if cred.password.is_empty() {
                return;
            }
            let p = self.proxy.clone();
            self.db(move |s| {
                if s.is_never_saved(&cred.origin) {
                    return Ok(());
                }
                if let Some(update) = s.password_change(&cred)? {
                    let _ = p.send_event(Msg::PasswordAsk { tab, cred, update });
                }
                Ok(())
            });
        }
    }

    /// Saved logins for the focused field arrived: offer them under it.
    pub(crate) fn offer_logins(&mut self, tab: u64, origin: &str, logins: Vec<Login>) {
        let Some((_, _, [x, y, w])) = self.passwords.focus.clone().filter(|(t, o, _)| *t == tab && o == origin && tab == self.active) else { return };
        if self.overlay_open && self.overlay_kind != "pwfill" {
            return;
        }
        let mut rows: Vec<PopupRow> = logins
            .iter()
            .map(|l| PopupRow { id: format!("fill:{}", l.id), kind: "passwords".into(), title: if l.username.is_empty() { tr("(no username)").into_owned() } else { l.username.clone() }, ..Default::default() })
            .collect();
        rows.push(PopupRow { kind: "sep".into(), ..Default::default() });
        rows.push(PopupRow { id: "manage".into(), kind: "settings".into(), title: tr("Manage passwords…").into_owned(), ..Default::default() });
        self.overlay_open = true;
        self.overlay_kind = "pwfill".into();
        let dark = self.dark_ui();
        self.popup.show("pwfill", x as f32, y as f32, w.max(FILL_MIN_W) as f32, rows, None, false, dark);
    }

    /// A row of the saved-logins pop-up was picked.
    pub(crate) fn pick_login(&mut self, id: &str) {
        if id == "manage" {
            return self.open_after_active(&format!("{}passwords", ui::INTERNAL));
        }
        let (Some(n), Some((tab, origin, _))) = (id.strip_prefix("fill:").and_then(|n| n.parse::<i64>().ok()), self.passwords.focus.clone()) else { return };
        let p = self.proxy.clone();
        self.db(move |s| {
            let (login, pass) = s.password(n, true)?;
            let _ = p.send_event(Msg::PasswordFill { tab, origin, user: login.username, pass });
            Ok(())
        });
    }

    /// Fill a login into the page of tab `tab`, only while it still shows `origin`.
    pub(crate) fn fill_login(&mut self, tab: u64, origin: &str, user: &str, pass: &str) {
        let Some(wv) = self.index(tab).and_then(|i| self.tabs[i].webview.as_ref()) else { return };
        // `location.origin` cannot be faked by the page: a page that navigated away meanwhile
        // gets nothing.
        let js = format!("location.origin === {} && window.__kkPwFill && window.__kkPwFill({}, {})", json!(origin), json!(user), json!(pass));
        let _ = wv.evaluate_script(&js);
    }

    /// "Save password for example.com?": Save / Never; closing it is "not now" (the key in the
    /// address bar brings it back).
    pub(crate) fn ask_save_password(&mut self, tab: u64, cred: Credential, update: bool) {
        self.passwords.offer = Some((tab, cred.clone(), update));
        self.render();
        if tab != self.active || (self.overlay_open && self.overlay_kind != "pwsave") {
            return;
        }
        let host = cred.origin.split_once("://").map_or(cred.origin.as_str(), |(_, h)| h).to_string();
        let title = if update { tr("Update password for {}?") } else { tr("Save password for {}?") }.replace("{}", &host);
        let who = if cred.username.is_empty() { tr("(no username)").into_owned() } else { cred.username.clone() };
        let rows = vec![
            json!({ "header": true, "id": "", "label": title }),
            json!({ "note": true, "label": format!("{who}  ·  ••••••••") }),
            json!({ "note": true, "label": tr("Saved on this PC and sealed with your Windows account.") }),
            json!({ "buttons": true, "id": "save", "label": tr(if update { "Update" } else { "Save" }), "key": tr("Never"), "close": "never" }),
        ];
        let width = self.window.inner_size().width as f64 / self.window.scale_factor();
        let data = json!({ "rows": rows }).to_string();
        self.show_overlay("pwsave", [(width - SAVE_W - 8.0).max(0.0), platform::frame::FRAME_H as f64 - 4.0, SAVE_W, 0.0], &data);
    }

    /// The save dialog was answered: "save" or "never".
    pub(crate) fn answer_save_password(&mut self, answer: &str) {
        self.hide_overlay();
        let Some((_, cred, _)) = self.passwords.offer.take() else { return };
        match answer {
            "save" => {
                let n = self.passwords.saved.as_ref().filter(|(o, _)| *o == cred.origin).map_or(0, |(_, n)| *n);
                self.passwords.saved = Some((cred.origin.clone(), n + 1));
                self.db(move |s| s.save_password(&cred));
            }
            "never" => self.db(move |s| s.never_save(&cred.origin)),
            _ => {}
        }
        self.render();
    }

    /// Show the key in the address bar? The active page has saved logins or a login to save.
    pub(crate) fn key_shown(&self) -> bool {
        let Some(origin) = self.index(self.active).and_then(|i| origin_of(&self.tabs[i].view.url)) else { return false };
        let offered = self.passwords.offer.as_ref().is_some_and(|(t, c, _)| *t == self.active && c.origin == origin);
        offered || self.passwords.saved.as_ref().is_some_and(|(o, _)| *o == origin)
    }

    /// The key was clicked (its pop-up hangs from `right, top`): the save dialog again, else
    /// what is saved for the site.
    pub(crate) fn key_clicked(&mut self, right: f64, top: f64) {
        if self.toggles_closed("pwsave") || self.toggles_closed(&format!("menu@{right},{top}")) {
            return;
        }
        if let Some((tab, cred, update)) = self.passwords.offer.clone().filter(|(t, _, _)| *t == self.active) {
            return self.ask_save_password(tab, cred, update);
        }
        let n = self.passwords.saved.as_ref().map_or(0, |(_, n)| *n);
        let rows = vec![
            json!({ "id": "", "label": tr("Saved passwords for this site: {}. Click a login field to fill one in.").replace("{}", &n.to_string()) }),
            json!({ "sep": true }),
            json!({ "id": "passwords", "label": "Manage passwords…", "key": "" }),
        ];
        let _ = self.native_popup(rows, right, top, true);
    }

    /// How many logins are saved for `origin` (the active page), for the key.
    pub(crate) fn on_saved_logins(&mut self, origin: String, n: usize) {
        let active = self.index(self.active).and_then(|i| origin_of(&self.tabs[i].view.url));
        if active.as_deref() == Some(origin.as_str()) {
            self.passwords.saved = (n > 0).then_some((origin, n));
            self.render();
        }
    }
}
