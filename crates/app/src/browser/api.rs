//! `/api/...` of the built-in pages, token-checked before it gets here.

use std::path::PathBuf;

use serde_json::{json, Value as Json};
use tabs::Lifecycle;
use tao::platform::windows::WindowExtWindows;

use crate::browser::Browser;
use crate::i18n::tr;
use crate::msg::Msg;
use crate::pages::{api_json, ext_json, query_param, Reply};
use crate::prefs::{all_values, mode_from_id, mode_id, reset, valid_url_setting, Prefs, Settings, LANGUAGES, SETTABLE};
use crate::profile::{account, import_from_chrome, keep_browsers_in_sync, run_sync, sign_in, sign_out, sync_status, touch_settings};

/// Component extensions the WebView2 runtime installs itself (clipboard, PDF viewer); not shown.
pub(crate) const BUILTIN_EXTENSIONS: [&str; 2] = ["dgiklkfkllikcanfonkcabmbdfmgleag", "mhjfbmdgcfjbbpaeojofohoefgiehjai"];

fn read_password_csv(file: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    const MAX: u64 = 20 * 1024 * 1024;
    let source = std::fs::File::open(file).map_err(|e| format!("reading CSV: {e}"))?;
    let mut bytes = Vec::new();
    source.take(MAX + 1).read_to_end(&mut bytes).map_err(|e| format!("reading CSV: {e}"))?;
    if bytes.len() as u64 > MAX {
        return Err("The passwords CSV is larger than 20 MiB. Export a smaller file and try again.".into());
    }
    String::from_utf8(bytes).map_err(|_| "The passwords CSV must use UTF-8. Export it again as UTF-8; no passwords were imported.".into())
}

impl Browser {
    /// `/api/...` of the built-in pages (token already checked).
    pub(crate) fn api(&mut self, path: &str, query: &str, body: String, respond: Reply) {
        let ok = |r: Result<(), String>| r.map(|_| json!({})).unwrap_or_else(|e| json!({ "error": e }));
        match path {
            "history" => {
                let q = query_param(query, "q");
                let limit = query_param(query, "limit").parse().unwrap_or(300).clamp(1, 5000);
                self.db_reply(respond, move |s| s.history(&q, limit).map(|v| json!(v)).unwrap_or_else(|e| json!({ "error": e })));
            }
            "topsites" => self.db_reply(respond, |s| json!(crate::native::top_sites(s.top_sites(60).unwrap_or_default()))),
            "history/delete" => self.db_reply(respond, move |s| ok(s.delete_url(&body))),
            "history/clear" => self.db_reply(respond, move |s| ok(s.clear_history())),
            "chrome" => self.job(Box::new(move |_| {
                let profiles = storage::chrome::user_data_dir().map(|d| storage::chrome::profiles(&d)).unwrap_or_default();
                respond.send(json!({ "profiles": profiles }));
            })),
            "browsers" => self.job(Box::new(move |s| {
                let keep = s.and_then(|s| s.setting("import.keep")).unwrap_or_default();
                let list: Vec<Json> = storage::chrome::browsers()
                    .into_iter()
                    .map(|b| {
                        let keep = keep.split(',').any(|k| k == b.id);
                        let default = storage::chrome::default_profile(&b);
                        json!({ "id": b.id, "name": b.name, "chromium": b.chromium, "keep": keep, "default": default, "profiles": b.profiles })
                    })
                    .collect();
                let imported = s.and_then(|s| s.setting("import.summary")).unwrap_or_default();
                respond.send(json!({ "browsers": list, "imported": imported }));
            })),
            "import/browser" => {
                let (id, dir) = body.split_once('\t').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
                self.db_reply(respond, move |s| {
                    let Some(b) = storage::chrome::browser(&id) else { return json!({ "error": "that browser was not found" }) };
                    match s.import_browser(&b, &dir) {
                        Ok(n) => json!({
                            "pages": n.pages,
                            "bookmarks": n.bookmarks,
                            "passwords": n.passwords,
                            "browser": b.name,
                            "summary": n.summary(b.name),
                        }),
                        Err(e) => json!({ "error": e }),
                    }
                })
            }
            "browsers/keep" => {
                let known: Vec<&str> = storage::chrome::browsers().iter().map(|b| b.id).collect();
                let keep: Vec<String> = body.split(',').map(str::trim).filter(|k| known.contains(k)).map(str::to_string).collect();
                let keep = keep.join(",");
                self.db_reply(respond, move |s| match s.set_setting("import.keep", &keep) {
                    Ok(()) => json!({ "keep": keep, "read": keep_browsers_in_sync(s) }),
                    Err(e) => json!({ "error": e }),
                })
            }
            "chrome/history" => {
                let dir = body.trim().to_string();
                let file = storage::chrome::user_data_dir()
                    .filter(|_| !dir.is_empty() && !dir.contains(['/', '\\']) && dir != ".." && dir != ".")
                    .map(|d| d.join(&dir).join("History"));
                match file {
                    Some(f) => self.db_reply(respond, move |s| {
                        s.import_chrome_history(&f).map(|n| json!({ "imported": n })).unwrap_or_else(|e| json!({ "error": e }))
                    }),
                    None => respond.send(json!({ "error": "unknown Chrome profile" })),
                }
            }
            "chrome/extension" => {
                // Only folders inside a Chromium-family browser's own profile data (Chrome, Edge,
                // Brave…) can be imported this way.
                let src = PathBuf::from(body.trim());
                let inside_browser = !body.contains("..")
                    && src.join("manifest.json").is_file()
                    && storage::chrome::browsers().iter().any(|b| b.chromium && src.starts_with(&b.dir));
                if !inside_browser {
                    return respond.send(json!({ "error": "not an extension folder of an installed browser" }));
                }
                let (dest_root, proxy) = (self.paths.extensions.clone(), self.proxy.clone());
                self.job(Box::new(move |_| match storage::chrome::copy_extension(&src, &dest_root) {
                    Ok(folder) => {
                        let _ = proxy.send_event(Msg::ExtAdd { folder, respond });
                    }
                    Err(e) => respond.send(json!({ "error": e })),
                }));
            }
            "extensions" => match self.extension_host() {
                Some(wv) => platform::extensions_list(wv, move |r| respond.send(api_json(r.map(|v| {
                    v.into_iter().filter(|e| !BUILTIN_EXTENSIONS.contains(&e.id.as_str())).map(ext_json).collect::<Vec<_>>()
                })))),
                None => respond.send(json!({ "error": "open a tab first" })),
            },
            "extensions/enable" | "extensions/remove" => {
                let (id, on) = body.split_once('\t').unwrap_or((body.as_str(), ""));
                let change = (path == "extensions/enable").then_some(on == "1");
                match self.extension_host() {
                    Some(wv) => platform::extension_change(wv, id, change, move |r| respond.send(ok(r))),
                    None => respond.send(json!({ "error": "open a tab first" })),
                }
            }
            "extensions/load" => {
                let folder = PathBuf::from(body.trim());
                if !folder.join("manifest.json").is_file() {
                    return respond.send(json!({ "error": "that folder has no manifest.json" }));
                }
                match self.extension_host() {
                    Some(wv) => platform::extension_add(wv, &folder, move |r| respond.send(api_json(r.map(ext_json)))),
                    None => respond.send(json!({ "error": "open a tab first" })),
                }
            }
            "sync" => self.db_reply(respond, sync_status),
            "search" => {
                let url = ui::resolve_input(&query_param(query, "q"), &self.search.0).unwrap_or_else(|| ui::NEWTAB_URL.into());
                respond.send(json!({ "url": url }));
            }
            "update/check" => {
                self.check_updates();
                respond.send(json!({}));
            }
            "update/restart" => {
                if self.restart_to_update() {
                    let _ = self.proxy.send_event(Msg::Ipc("exitforupdate".into()));
                }
                respond.send(match &self.update {
                    crate::update::Status::Failed(e) => json!({ "error": e }),
                    _ => json!({}),
                });
            }
            "settings" => {
                let update = self.update.json();
                let ads_blocked = platform::ads_blocked(); // counted on this (UI) thread
                self.db_reply(respond, move |s| {
                let engines: Vec<Json> = ui::ENGINES.iter().map(|e| json!({ "id": e.id, "name": e.name })).collect();
                let p = Prefs::read(s);
                json!({
                    "engine": s.setting("search.engine").unwrap_or_else(|| ui::ENGINES[0].id.into()),
                    "engines": engines,
                    "memory": mode_id(p.memory),
                    "startup": p.startup,
                    "downloads": p.downloads.display().to_string(),
                    "theme": p.settings.theme,
                    "animations": p.settings.animations,
                    "tracking": p.settings.protection.tracking,
                    "safeBrowsing": p.settings.protection.safe_browsing,
                    "adblock": p.settings.protection.adblock,
                    "adsBlocked": ads_blocked,
                    "httpsFirst": p.settings.https_first,
                    "passwords": p.settings.protection.save_passwords,
                    "autofill": p.settings.protection.autofill,
                    "sync": sync_status(s),
                    "imported": s.setting("import.summary").unwrap_or_default(),
                    "all": all_values(s),
                    "languages": LANGUAGES,
                    "isDefault": platform::is_default_browser(),
                    "version": env!("CARGO_PKG_VERSION"),
                    "webview2": wry::webview_version().unwrap_or_default(),
                    "update": update,
                })
            })
            }
            "settings/memory" => {
                let Some(mode) = mode_from_id(body.trim()) else {
                    return respond.send(json!({ "error": "unknown mode" }));
                };
                let _ = self.proxy.send_event(Msg::MemoryMode(mode));
                self.db_reply(respond, move |s| {
                    touch_settings(s);
                    ok(s.set_setting("memory.mode", mode_id(mode)))
                })
            }
            "memory" => {
                let webviews: Vec<&wry::WebView> = self.tabs.iter().filter_map(|tab| tab.webview.as_ref()).chain(self.ext_bubble.as_ref()).collect();
                let procs: Vec<Json> = platform::processes(&webviews)
                    .into_iter()
                    .map(|p| json!({ "pid": p.pid, "kind": p.kind, "ws": p.private_ws, "commit": p.commit }))
                    .collect();
                // Native pages run no page process: not counted.
                let count = |s: Lifecycle| self.tabs.iter().filter(|t| t.state == s && crate::native::page_of(&t.view.url).is_none()).count();
                let mode = match self.memory_mode {
                    memory::Mode::Moderate => "Moderate",
                    memory::Mode::UltraLite => "Maximum",
                    memory::Mode::Balanced => "Balanced",
                    memory::Mode::Performance => "Off",
                };
                respond.send(json!({
                    "processes": procs,
                    "running": count(Lifecycle::Active) + count(Lifecycle::Idle) + count(Lifecycle::LowMemory),
                    "frozen": count(Lifecycle::Suspended),
                    "unloaded": count(Lifecycle::Discarded),
                    "mode": mode,
                }));
            }
            "settings/set" => {
                let (key, value) = body.split_once('\t').unwrap_or(("", ""));
                let valid = SETTABLE.iter().any(|(k, allowed, _)| *k == key && allowed.contains(&value));
                if !valid {
                    return respond.send(json!({ "error": "unknown setting" }));
                }
                let (key, value, p) = (key.to_string(), value.to_string(), self.proxy.clone());
                self.db_reply(respond, move |s| match s.set_setting(&key, &value) {
                    Ok(()) => {
                        touch_settings(s);
                        let _ = p.send_event(Msg::SettingsChanged(Settings::read(|k| s.setting(k))));
                        json!({})
                    }
                    Err(e) => json!({ "error": e }),
                })
            }
            "settings/url" => {
                let (key, value) = body.split_once('\t').unwrap_or((body.as_str(), ""));
                let Some(value) = valid_url_setting(key, value) else {
                    return respond.send(json!({ "error": "Enter web addresses that start with https:// or http://" }));
                };
                let (key, p) = (key.to_string(), self.proxy.clone());
                self.db_reply(respond, move |s| match s.set_setting(&key, &value) {
                    Ok(()) => {
                        touch_settings(s);
                        let _ = p.send_event(Msg::SettingsChanged(Settings::read(|k| s.setting(k))));
                        json!({})
                    }
                    Err(e) => json!({ "error": e }),
                })
            }
            "settings/reset" => {
                let p = self.proxy.clone();
                self.db_reply(respond, move |s| match reset(s) {
                    Ok(()) => {
                        touch_settings(s);
                        let prefs = Prefs::read(s);
                        let _ = p.send_event(Msg::SettingsChanged(prefs.settings));
                        let _ = p.send_event(Msg::Engine(prefs.engine));
                        let _ = p.send_event(Msg::MemoryMode(prefs.memory));
                        json!({})
                    }
                    Err(e) => json!({ "error": e }),
                })
            }
            "default/make" => {
                let r = std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| {
                    platform::register_as_browser(&exe).map_err(|e| e.message())?;
                    open_windows_settings("ms-settings:defaultapps?registeredAppUser=Kakangkung")
                });
                respond.send(ok(r));
            }
            "downloads/open" => self.db_reply(respond, |s| {
                let folder = Prefs::read(s).downloads;
                let r = std::process::Command::new("explorer.exe").arg(&folder).spawn().map(|_| ()).map_err(|e| e.to_string());
                r.map(|_| json!({})).unwrap_or_else(|e| json!({ "error": e }))
            }),
            "system/proxy" => respond.send(ok(open_windows_settings("ms-settings:network-proxy"))),
            // Windows' certificate manager, the same dialog Chrome opens.
            "system/certs" => respond.send(ok(std::process::Command::new("rundll32.exe")
                .arg("cryptui.dll,CryptUIStartCertMgr")
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string()))),
            "settings/startup" => {
                let v = match body.trim() {
                    "continue" => "continue",
                    "pages" => "pages",
                    _ => "newtab",
                };
                self.db_reply(respond, move |s| {
                    touch_settings(s);
                    ok(s.set_setting("startup", v))
                })
            }
            "settings/downloads" => match platform::pick_folder(self.window.hwnd(), "Choose where downloads are saved") {
                Some(folder) => {
                    if let Some(wv) = self.extension_host() {
                        let _ = platform::set_download_folder(wv, &folder);
                    }
                    self.downloads_folder = Some(folder.clone());
                    let shown = folder.display().to_string();
                    self.db_reply(respond, move |s| {
                        match s.set_setting("downloads.folder", &shown) {
                            Ok(()) => json!({ "downloads": shown }),
                            Err(e) => json!({ "error": e }),
                        }
                    })
                }
                None => respond.send(json!({ "cancelled": true })),
            },
            "sync/pick" => match platform::pick_folder(self.window.hwnd(), "Choose the folder your cloud drive syncs") {
                Some(folder) => {
                    let folder = folder.display().to_string();
                    let (proxy, tabs) = (self.proxy.clone(), self.open_tabs());
                    self.db_reply(respond, move |s| {
                        let r = s.set_setting("sync.folder", &folder).and_then(|_| s.set_setting("sync.enabled", "1"));
                        if let Err(e) = r {
                            return json!({ "error": e });
                        }
                        let _ = proxy.send_event(Msg::Synced(run_sync(s, tabs, &proxy).is_ok()));
                        sync_status(s)
                    })
                }
                None => respond.send(json!({ "cancelled": true })),
            },
            "extensions/pick" => match platform::pick_folder(self.window.hwnd(), "Choose an unpacked extension folder") {
                Some(folder) if folder.join("manifest.json").is_file() => match self.extension_host() {
                    Some(wv) => platform::extension_add(wv, &folder, move |r| respond.send(api_json(r.map(ext_json)))),
                    None => respond.send(json!({ "error": "open a tab first" })),
                },
                Some(_) => respond.send(json!({ "error": "that folder has no manifest.json" })),
                None => respond.send(json!({ "cancelled": true })),
            },
            "permissions" => match self.extension_host() {
                Some(wv) => platform::site_permissions(wv, move |r| {
                    respond.send(match r {
                        Ok(list) => json!(list
                            .iter()
                            .map(|p| json!({ "kind": p.kind, "name": permission_name(p.kind), "origin": p.origin, "allowed": p.allowed }))
                            .collect::<Vec<_>>()),
                        Err(e) => json!({ "error": e }),
                    })
                }),
                None => respond.send(json!([])),
            },
            "permissions/reset" => {
                let (kind, origin) = body.split_once('\t').unwrap_or(("", ""));
                match (kind.parse::<i32>(), self.extension_host()) {
                    (Ok(kind), Some(wv)) => platform::reset_site_permission(wv, kind, origin, move |r| {
                        respond.send(ok(r))
                    }),
                    _ => respond.send(json!({ "error": "no such permission" })),
                }
            }
            "privacy/clear" if body.trim() == "passwords" => {
                self.db(|s| s.delete_password(None));
                match self.extension_host() {
                    Some(wv) => platform::clear_passwords(wv, move |r| respond.send(ok(r))),
                    None => respond.send(json!({})),
                }
            }
            "passwords" => {
                let on = self.settings.protection.save_passwords;
                self.db_reply(respond, move |s| {
                    let browsers: Vec<Json> = storage::chrome::browsers()
                        .iter()
                        .filter(|b| b.chromium)
                        .filter_map(|b| {
                            let export = storage::chrome::password_export_page(b.id).is_some();
                            Some(json!({ "id": b.id, "name": b.name, "profile": storage::chrome::default_profile(b)?, "export": export }))
                        })
                        .collect();
                    match s.logins("") {
                        Ok(logins) => json!({ "on": on, "logins": logins, "browsers": browsers }),
                        Err(e) => json!({ "error": e }),
                    }
                })
            }
            // Showing or copying a password asks Windows Hello first, like Chrome on Windows.
            "passwords/copy" | "passwords/show" => {
                let Ok(id) = body.trim().parse::<i64>() else { return respond.send(json!({ "error": "that password is no longer saved" })) };
                let copy = path == "passwords/copy";
                let jobs = self.jobs.clone();
                platform::verify_user(self.window.hwnd(), &tr("Kakangkung wants to show a saved password."), move |verified| {
                    if !verified {
                        return respond.send(json!({ "error": "Windows did not confirm it's you." }));
                    }
                    let _ = jobs.send(Box::new(move |s| {
                        let Some(s) = s else { return respond.send(json!({ "error": "the history database could not be opened" })) };
                        respond.send(match s.password(id, false) {
                            Ok((l, pw)) => {
                                let who = if l.username.is_empty() { l.origin.clone() } else { l.username.clone() };
                                if copy {
                                    match platform::copy_secret(&pw) {
                                        Ok(()) => json!({ "summary": format!("Password for {who} copied. It leaves the clipboard in 30 seconds.") }),
                                        Err(e) => json!({ "error": e }),
                                    }
                                } else {
                                    json!({ "summary": format!("{who} · {pw}") })
                                }
                            }
                            Err(e) => json!({ "error": e }),
                        })
                    }));
                });
            }
            // Open the other browser on its own "Export passwords" page (`start` finds it).
            "passwords/open-export" => match storage::chrome::password_export_page(body.trim()) {
                Some((exe, page)) => {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    let r = std::process::Command::new("cmd").args(["/c", "start", "", exe, page]).creation_flags(CREATE_NO_WINDOW).spawn();
                    respond.send(match r {
                        Ok(_) => json!({ "summary": "Choose Export passwords there, then import the file here (From a CSV file)." }),
                        Err(e) => json!({ "error": e.to_string() }),
                    })
                }
                None => respond.send(json!({ "error": "that browser was not found" })),
            },
            "passwords/delete" => {
                let id = body.trim().parse::<i64>().ok();
                if id.is_none() && body.trim() != "all" {
                    return respond.send(json!({ "error": "that password is no longer saved" }));
                }
                self.db_reply(respond, move |s| ok(s.delete_password(id)))
            }
            "passwords/import" => match platform::pick_file(self.window.hwnd(), &tr("Choose the passwords file (CSV)"), &tr("Passwords (CSV)"), "*.csv") {
                None => respond.send(json!({ "cancelled": true })),
                Some(file) => self.db_reply(respond, move |s| {
                    let text = match read_password_csv(&file) {
                        Ok(text) => text,
                        Err(e) => return json!({ "error": e }),
                    };
                    let list = storage::passwords::parse_csv(&text);
                    if list.is_empty() {
                        return json!({ "error": "No passwords found in that file. Export a CSV file from your browser or password manager and choose it here." });
                    }
                    match s.import_passwords(&list) {
                        Ok(n) => json!({ "summary": format!("New passwords imported: {n}") }),
                        Err(e) => json!({ "error": e }),
                    }
                }),
            },
            // Export: where to, then Windows Hello, then the file (Chrome's order).
            "passwords/export" => {
                let downloads = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default().join("Downloads").join("Kakangkung Passwords.csv");
                let Some(dest) = platform::save_file(self.window.hwnd(), &downloads.to_string_lossy()) else { return respond.send(json!({ "cancelled": true })) };
                let jobs = self.jobs.clone();
                platform::verify_user(self.window.hwnd(), &tr("Kakangkung wants to export your saved passwords."), move |verified| {
                    if !verified {
                        return respond.send(json!({ "error": "Windows did not confirm it's you." }));
                    }
                    let _ = jobs.send(Box::new(move |s| {
                        let Some(s) = s else { return respond.send(json!({ "error": "the history database could not be opened" })) };
                        let written = s.export_passwords().and_then(|list| {
                            std::fs::write(&dest, storage::passwords::to_csv(&list)).map(|_| list.len()).map_err(|e| format!("writing {}: {e}", dest.display()))
                        });
                        respond.send(match written {
                            Ok(_) => json!({ "summary": format!("Passwords exported to {}", dest.display()) }),
                            Err(e) => json!({ "error": e }),
                        })
                    }));
                });
            }
            "privacy/clear" => {
                // Within the chosen time range; "all" = the data types ticked in Settings.
                let since = self.settings.clear_since(storage::now_ms());
                let all = body.trim() == "all";
                let s = &self.settings;
                if all && (s.clear_cookies || s.clear_cache) {
                    if let Some(wv) = self.extension_host() {
                        platform::clear_browsing_data(wv, s.clear_cookies, s.clear_cache, since.unwrap_or(0), |r| {
                            if let Err(e) = r {
                                eprintln!("clear browsing data: {e}");
                            }
                        });
                    }
                }
                if all && !s.clear_history {
                    return respond.send(json!({}));
                }
                self.db_reply(respond, move |s| {
                    ok(match since {
                        Some(t) => s.clear_history_since(t),
                        None => s.clear_history(),
                    })
                })
            }
            "import/chrome" => self.db_reply(respond, |s| match import_from_chrome(s) {
                Ok(summary) => json!({ "summary": summary }),
                Err(e) => json!({ "error": e }),
            }),
            "settings/engine" => {
                let choice = body.trim().to_string();
                if !ui::ENGINES.iter().any(|e| e.id == choice) && !ui::valid_custom_engine(&choice) {
                    return respond.send(json!({ "error": "Use one of the listed engines, or an https:// address that contains %s where the search words go." }));
                }
                let p = self.proxy.clone();
                self.db_reply(respond, move |s| match s.set_setting("search.engine", &choice) {
                    Ok(()) => {
                        touch_settings(s);
                        let name = ui::search_template(&choice).1;
                        let _ = p.send_event(Msg::Engine(choice));
                        json!({ "name": name })
                    }
                    Err(e) => json!({ "error": e }),
                })
            }
            // Downloads kept from before, shaped like this run's (see `page_host::live_downloads`).
            "downloads" => self.db_reply(respond, |s| match s.downloads(500) {
                Ok(list) => Json::Array(
                    list.into_iter()
                        .map(|d| json!({ "id": format!("s{}", d.id), "url": d.url, "path": d.path, "received": d.size, "total": d.size, "state": if d.done { "Done" } else { "Stopped" }, "started": d.started }))
                        .collect(),
                ),
                Err(e) => json!({ "error": e }),
            }),
            "bookmarks" => self.db_reply(respond, |s| s.bookmarks().map(|b| json!(b)).unwrap_or_else(|e| json!({ "error": e }))),
            "bookmarks/remove" => {
                let p = self.proxy.clone();
                self.db_reply(respond, move |s| {
                    let r = s.set_bookmark(&body, "", false);
                    let _ = p.send_event(Msg::Bookmarked { url: body, on: false });
                    ok(r)
                })
            }
            "sync/config" => {
                let (on, folder) = body.split_once('\t').unwrap_or(("0", ""));
                let (on, folder) = (on == "1", folder.trim().to_string());
                let proxy = self.proxy.clone();
                let tabs = self.open_tabs();
                self.db_reply(respond, move |s| {
                    let r = s.set_setting("sync.enabled", if on { "1" } else { "0" }).and_then(|_| {
                        if folder.is_empty() { Ok(()) } else { s.set_setting("sync.folder", &folder) }
                    });
                    if let Err(e) = r {
                        return json!({ "error": e });
                    }
                    let synced = on && run_sync(s, tabs, &proxy).is_ok();
                    let _ = proxy.send_event(Msg::Synced(synced));
                    sync_status(s)
                });
            }
            "account" => self.db_reply(respond, sync_status),
            "account/signin" => {
                let mut f = body.splitn(3, '\t');
                let (name, email, types) = (f.next().unwrap_or("").to_string(), f.next().unwrap_or("").to_string(), f.next().unwrap_or("").to_string());
                let (proxy, tabs) = (self.proxy.clone(), self.open_tabs());
                self.db_reply(respond, move |s| {
                    if let Err(e) = sign_in(s, &name, &email, &types) {
                        return json!({ "error": e });
                    }
                    let _ = proxy.send_event(Msg::Account(account(s).map(|a| a.0).unwrap_or_default()));
                    let _ = proxy.send_event(Msg::Synced(run_sync(s, tabs, &proxy).is_ok()));
                    sync_status(s)
                })
            }
            // Sign in with Google: consent in the default browser, then sync through Drive. The
            // answer comes when the person has finished there (or after the wait runs out).
            "account/google" => {
                let types = storage::sync::Types::parse(Some(body.trim())).list();
                let (proxy, tabs, jobs) = (self.proxy.clone(), self.open_tabs(), self.jobs.clone());
                std::thread::spawn(move || {
                    let account = match crate::google::sign_in() {
                        Ok(a) => a,
                        Err(e) => return respond.send(json!({ "error": e })),
                    };
                    let job: crate::jobs::Job = Box::new(move |s| {
                        let Some(s) = s else { return respond.send(json!({ "error": "the profile database could not be opened" })) };
                        if let Err(e) = crate::google::save(s, &account).and_then(|_| s.set_setting("sync.types", &types)) {
                            return respond.send(json!({ "error": e }));
                        }
                        let _ = proxy.send_event(Msg::AccountProfile(account.name.clone(), s.setting("account.picture.path").unwrap_or_default()));
                        let _ = proxy.send_event(Msg::Synced(run_sync(s, tabs, &proxy).is_ok()));
                        respond.send(sync_status(s));
                    });
                    let _ = jobs.send(job);
                });
            }
            "account/signout" => {
                let proxy = self.proxy.clone();
                self.db_reply(respond, move |s| {
                    if let Err(e) = sign_out(s) {
                        return json!({ "error": e });
                    }
                    let _ = proxy.send_event(Msg::Account(String::new()));
                    let _ = proxy.send_event(Msg::Synced(false));
                    sync_status(s)
                })
            }
            "account/types" => {
                let types = storage::sync::Types::parse(Some(body.trim())).list();
                self.db_reply(respond, move |s| match s.set_setting("sync.types", &types) {
                    Ok(()) => sync_status(s),
                    Err(e) => json!({ "error": e }),
                })
            }
            "sync/now" => {
                let (proxy, tabs) = (self.proxy.clone(), self.open_tabs());
                self.db_reply(respond, move |s| {
                    let r = run_sync(s, tabs, &proxy);
                    let _ = proxy.send_event(Msg::Synced(r.is_ok()));
                    match r {
                        Ok(()) => sync_status(s),
                        Err(e) => json!({ "error": e }),
                    }
                });
            }
            other => respond.send(json!({ "error": format!("unknown API {other}") })),
        }
    }
}

/// Settings label of a `COREWEBVIEW2_PERMISSION_KIND` value.
pub(crate) fn permission_name(kind: i32) -> &'static str {
    match kind {
        1 => "Microphone",
        2 => "Camera",
        3 => "Location",
        4 => "Notifications",
        5 => "Motion sensors",
        6 => "Clipboard",
        7 => "Automatic downloads",
        8 => "File editing",
        9 => "Autoplay",
        10 => "Fonts on your device",
        11 => "MIDI devices",
        12 => "Window management",
        _ => "Other",
    }
}

/// Open a page of the Windows Settings app (default apps, proxy).
fn open_windows_settings(uri: &str) -> Result<(), String> {
    std::process::Command::new("explorer.exe").arg(uri).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod csv_tests {
    use super::read_password_csv;

    #[test]
    fn password_csv_rejects_invalid_utf8_and_oversized_files() {
        let path = std::env::temp_dir().join(format!("kakangkung-csv-input-{}.csv", std::process::id()));
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(read_password_csv(&path).unwrap_err().contains("UTF-8"));
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(20 * 1024 * 1024 + 1).unwrap();
        drop(f);
        assert!(read_password_csv(&path).unwrap_err().contains("20 MiB"));
        std::fs::write(&path, "url,username,password\nhttps://example.invalid,é,synthetic").unwrap();
        assert!(read_password_csv(&path).unwrap().contains('é'));
        let _ = std::fs::remove_file(path);
    }
}
