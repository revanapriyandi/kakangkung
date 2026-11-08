//! `/api/...` of the built-in pages, token-checked before it gets here.

use std::path::PathBuf;

use serde_json::{json, Value as Json};
use tabs::Lifecycle;
use tao::platform::windows::WindowExtWindows;

use crate::browser::Browser;
use crate::msg::Msg;
use crate::pages::{api_json, ext_json, query_param, Reply};
use crate::prefs::{all_values, mode_from_id, mode_id, reset, valid_url_setting, Prefs, Settings, LANGUAGES, SETTABLE};
use crate::profile::{account, import_from_chrome, keep_browsers_in_sync, run_sync, sign_in, sign_out, sync_status, touch_settings};

/// Component extensions the WebView2 runtime installs itself (clipboard, PDF viewer); not shown.
pub(crate) const BUILTIN_EXTENSIONS: [&str; 2] = ["dgiklkfkllikcanfonkcabmbdfmgleag", "mhjfbmdgcfjbbpaeojofohoefgiehjai"];

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
                        Ok((pages, marks)) => json!({
                            "pages": pages,
                            "bookmarks": marks,
                            "browser": b.name,
                            "summary": format!("Imported {pages} pages and {marks} bookmarks from {}.", b.name),
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
                let procs: Vec<Json> = self
                    .extension_host()
                    .map(platform::processes)
                    .unwrap_or_default()
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
            "privacy/clear" if body.trim() == "passwords" => match self.extension_host() {
                Some(wv) => platform::clear_passwords(wv, move |r| respond.send(ok(r))),
                None => respond.send(json!({ "error": "open a tab first" })),
            },
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
                        let _ = proxy.send_event(Msg::Account(account.name.clone()));
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
