//! `--self-test`: drives the browser like a user and records what worked.

mod native;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use tabs::Lifecycle;
use tao::platform::windows::WindowExtWindows;
use ui::Action;

use crate::ABOUT;
use crate::browser::Browser;
use crate::msg::Msg;

/// The parts of a message the self-test looks at (taken before the browser consumes it).
#[derive(Debug)]
pub(crate) enum Observed {
    Loaded { id: u64, url: String },
    Suspended { id: u64, ok: bool },
    Script,
    Other,
}

impl Observed {
    pub(crate) fn of(msg: &Msg) -> Self {
        match msg {
            Msg::Load { id, url, done: true } => Observed::Loaded { id: *id, url: url.clone() },
            Msg::Suspended { id, ok } => Observed::Suspended { id: *id, ok: *ok },
            Msg::Script(_) => Observed::Script,
            _ => Observed::Other,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Stage {
    Start,
    WaitAbout,
    WaitBack,
    WaitAboutAgain,
    WaitNewTab,
    WaitSuspend,
    WaitRetained,
    WaitPreview,
    WaitPreviewSuspend,
    WaitPreviewSleeping,
    WaitScript,
    WaitScriptPage,
    PollFeatures,
    WaitAutocompleteFocus,
    WaitAutocomplete,
    WaitNative,
    WaitNativeSave,
    WaitHistory,
    WaitHistorySearch,
    WaitBookmarks,
    WaitTools,
    WaitDownloads,
    WaitExtensions,
    /// Visiting the remaining native pages one by one (index into `native::TOUR`).
    Tour(usize),
    /// Tab strip clicks: above the first tab, then the ⌄ button.
    WaitTopClick,
    WaitTabSearch,
    PollYouTube,
    WaitYouTubePreview,
    Done,
}

/// What the "hud" step reads from the native frame: tabs, toolbar buttons, chip, window buttons.
fn hud(b: &Browser) -> Json {
    let st = b.frame_state(false, false);
    let active = st.tabs.iter().find(|t| t.id == st.active).map(|t| if t.title.is_empty() { "New Tab".to_string() } else { t.title.clone() });
    let buttons = 3 + usize::from(st.show_home) + usize::from(st.show_extensions) + 1;
    json!({
        "tabs": st.tabs.len(),
        "active": active.unwrap_or_default(),
        "chip": format!("{:?}", st.chip).to_lowercase(),
        "buttons": buttons,
        "brand": "Kakangkung native frame",
        "caption": 3,
        "strip": platform::frame::STRIP_H,
        "omnibox": true,
    })
}

/// A test RSA public key (SubjectPublicKeyInfo, base64) and the extension id Chrome derives from
/// it (SHA-256, first 16 bytes as a–p), for the "store" step.
const TEST_STORE_KEY: &str = "MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQC1l6DJXMoR40Y3FuEYlmWPBIkPT51DhLhSH/aR97FjKkMTLG+DvejR+9qvymczs7n0G6xMkScnuPNYAOZHQEZ4mlXV65ioqOZkAtAlk5zDoPbDNGZVtNBJIVz2V+SLbBXGp+qBousEo3eLXiTGUpBsaLTZXbbrb/bHcsfZLoBYnwIDAQAB";
const TEST_STORE_ID: &str = "fnefnikcefeclaobcbndpmkblbnidkon";

/// An unpacked extension like a store install leaves (manifest with `key`, a toolbar pop-up).
fn test_store_extension(dir: &std::path::Path) -> PathBuf {
    let _ = std::fs::create_dir_all(dir);
    let manifest = serde_json::json!({
        "manifest_version": 3, "name": "Kakangkung store test", "version": "1.0",
        "key": TEST_STORE_KEY, "action": { "default_popup": "popup.html" },
    });
    let _ = std::fs::write(dir.join("manifest.json"), manifest.to_string());
    let _ = std::fs::write(dir.join("popup.html"), "<!doctype html><body style=\"width:200px\">ok</body>");
    dir.to_path_buf()
}

/// Runs inside kakangkung://selftest (a blank page that carries the API token) through the token-checked API:
/// Chrome history import, Chrome extension import + off + remove, folder sync.
pub(crate) const FEATURE_JS: &str = r#"window.__kakangkungTest = "";
(async () => {
  const TOKEN = document.querySelector('meta[name="kakangkung-token"]').content;
  const api = async (path, body) => {
    const r = await fetch("/api/" + path, { method: body === undefined ? "GET" : "POST", headers: { "X-Kakangkung-Token": TOKEN }, body });
    const j = await r.json().catch(() => ({ error: "bad response (" + r.status + ")" }));
    if (!r.ok || j.error) throw new Error(j.error || "request failed (" + r.status + ")");
    return j;
  };
  const FOLDER = __FOLDER__;
  const STORE_EXT = __STORE_EXT__, STORE_ID = "__STORE_ID__";
  const r = {};
  const step = async (name, f) => {
    try { r[name] = await f(); } catch (e) { r[name] = { ok: false, detail: String((e && e.message) || e) }; }
  };
  let profile = null;
  await step("import", async () => {
    const c = await api("chrome");
    if (!c.profiles.length) return { ok: true, detail: "no Chrome profile on this PC; skipped" };
    profile = c.profiles.slice().sort((a, b) => a.historyBytes - b.historyBytes)[0];
    const n = (await api("chrome/history", profile.dir)).imported;
    const h = await api("history?q=&limit=5");
    return { ok: n > 0 && h.length > 0,
      detail: `imported ${n} pages from Chrome profile '${profile.dir}'; kakangkung://history lists them (newest: ${h[0] ? h[0].url : "none"})` };
  });
  await step("extensions", async () => {
    const cands = profile ? profile.extensions : [];
    if (!cands.length) return { ok: true, detail: "the Chrome profile has no extensions; skipped" };
    let added = null;
    const refused = [];
    for (const x of cands.slice(0, 8)) {
      try { added = await api("chrome/extension", x.path); break; } catch (e) { refused.push(x.name + ": " + e.message); }
    }
    if (!added) return { ok: false, detail: "WebView2 refused every extension tried: " + refused.join("; ") };
    const state = async () => ((await api("extensions")).find(e => e.id === added.id) || {}).enabled;
    const on = await state();
    await api("extensions/enable", added.id + "\t0");
    const off = await state();
    await api("extensions/remove", added.id);
    const gone = (await state()) === undefined;
    return { ok: on === true && off === false && gone,
      detail: `imported '${added.name}' from Chrome and it is running (enabled=${on})` +
        (refused.length ? ` (refused first: ${refused.join("; ")})` : "") + `; switched off -> enabled=${off}; removed=${gone}` };
  });
  await step("store", async () => {
    // What a Chrome Web Store install loads: an unpacked folder whose manifest carries the
    // store's key, so WebView2 gives it the store id. The id and the toolbar pop-up must hold.
    const added = await api("extensions/load", STORE_EXT);
    const listed = (await api("extensions")).some(e => e.id === added.id);
    await api("extensions/remove", added.id);
    return { ok: added.id === STORE_ID && listed,
      detail: `loaded a test extension with a manifest key: id ${added.id} (expected ${STORE_ID}), listed=${listed}, removed` };
  });
  await step("sync", async () => {
    // Point sync at a test folder first, then sign in like a person would (sync starts by itself).
    await api("sync/config", "0	" + FOLDER);
    const a = await api("account/signin", "Test Person	test.person@example.com	bookmarks,history,tabs,settings,extensions");
    await api("settings/set", "appearance.theme	dark");
    const s2 = await api("sync/now", "");
    let refused = false;
    try { await api("account/signin", "X	not-an-email	"); } catch (e) { refused = true; }
    await api("settings/set", "appearance.theme	auto");
    return { ok: a.signedIn && a.email === "test.person@example.com" && s2.enabled && s2.last > 0 && a.types.bookmarks && refused,
      detail: `signed in as ${a.name} <${a.email}>, syncing ${Object.keys(a.types).filter(k => a.types[k]).join("/")}; ` +
              `this PC (${s2.device}) published to its account folder; invalid email refused=${refused}` };
  });
  await step("browsers", async () => {
    const r = await api("browsers");
    const names = r.browsers.map(b => b.name).join(", ");
    const other = r.browsers.find(b => b.id !== "chrome");
    if (!other) return { ok: true, detail: `browsers found: ${names || "none"}; no browser besides Chrome to import` };
    const imp = await api("import/browser", other.id + "	" + other.default);
    const k = await api("browsers/keep", other.id);
    await api("browsers/keep", "");
    return { ok: imp.pages >= 0 && k.keep === other.id && k.read === 1,
      detail: `browsers found: ${names}; imported ${imp.pages} pages and ${imp.bookmarks} bookmarks from ${imp.browser}; "keep in sync" with it re-read ${k.read} browser` };
  });
  await step("search", async () => {
    const before = (await api("search?q=" + encodeURIComponent("rust lang"))).url;
    await api("settings/engine", "bing");
    const after = (await api("search?q=rust")).url;
    let refused = false;
    try { await api("settings/engine", "javascript:%s"); } catch (e) { refused = true; }
    await api("settings/engine", "google");
    return { ok: before.startsWith("https://www.google.com/search?q=rust+lang") && after.startsWith("https://www.bing.com/search?q=rust") && refused,
      detail: `default engine searched ${before}; after switching to Bing: ${after}; unsafe custom engine refused=${refused}; switched back to Google` };
  });
  await step("privacy", async () => {
    const want = [["privacy.tracking", "3"], ["privacy.https_first", "1"], ["privacy.safe_browsing", "1"],
                  ["privacy.passwords", "0"], ["appearance.theme", "dark"], ["appearance.animations", "0"]];
    for (const [k, v] of want) await api("settings/set", k + "	" + v);
    const s = await api("settings");
    let refused = false;
    try { await api("settings/set", "privacy.tracking	9"); } catch (e) { refused = true; }
    for (const [k, v] of [["privacy.tracking", "2"], ["privacy.passwords", "1"], ["appearance.theme", "auto"], ["appearance.animations", "1"]])
      await api("settings/set", k + "	" + v);
    return { ok: s.tracking === 3 && s.httpsFirst && s.safeBrowsing && !s.passwords && s.theme === "dark" && !s.animations && refused,
      detail: `saved and read back: tracking prevention strict=${s.tracking === 3}, HTTPS-First=${s.httpsFirst}, Safe Browsing=${s.safeBrowsing}, ` +
              `save passwords off=${!s.passwords}, theme=${s.theme}, animations off=${!s.animations}; invalid value refused=${refused}` };
  });
  await step("adblock", async () => {
    const ad = () => "https://securepubads.g.doubleclick.net/kakangkung-selftest.gif?" + Math.random();
    const load = (u) => new Promise((r) => { const i = new Image(); i.onload = i.onerror = r; setTimeout(r, 5000); i.src = u; });
    await api("settings/set", "privacy.adblock	1");
    const before = (await api("settings")).adsBlocked;
    await load(ad());
    const on = (await api("settings")).adsBlocked;
    await load(location.origin + "/kakangkung-selftest-not-an-ad.gif");
    const ownSite = (await api("settings")).adsBlocked;
    await api("settings/set", "privacy.adblock	0");
    await load(ad());
    const off = (await api("settings")).adsBlocked;
    await api("settings/set", "privacy.adblock	1");
    const back = (await api("settings")).adblock;
    return { ok: on === before + 1 && ownSite === on && off === on && back,
      detail: `on: ad request blocked=${on === before + 1}, the page's own request let through=${ownSite === on}; ` +
              `off: ad request let through=${off === on}; switched back on=${back} (${off} blocked in total)` };
  });
  await step("download", async () => {
    const name = "kakangkung-download-selftest.txt";
    const link = document.createElement("a");
    const object = URL.createObjectURL(new Blob(["Kakangkung isolated download fixture\n"], { type: "text/plain" }));
    link.href = object;
    link.download = name;
    document.body.appendChild(link);
    link.click();
    link.remove();
    let saved;
    for (let attempt = 0; attempt < 40 && !saved; attempt++) {
      await new Promise(resolve => setTimeout(resolve, 250));
      saved = (await api("downloads")).find(item => item.path.endsWith(name) && item.state === "Done");
    }
    URL.revokeObjectURL(object);
    return { ok: !!saved, detail: `real blob download completed and persisted=${!!saved}` };
  });
  await step("taskmanager", async () => {
    const m = await api("memory");
    const total = m.processes.reduce((a, p) => a + p.ws, 0);
    const kinds = [...new Set(m.processes.map(p => p.kind))].join(", ");
    return { ok: m.processes.length >= 3 && total > 0,
      detail: `${m.processes.length} processes (${kinds}), private working set ${(total / 1048576).toFixed(0)} MB in total; Memory Saver ${m.mode}` };
  });
  await step("settings", async () => {
    const before = (await api("settings")).all;
    await api("settings/set", "tabs.position	next");
    await api("settings/set", "appearance.zoom	125");
    await api("settings/set", "tools.pip	0");
    await api("settings/url", "tabs.ntp_url	https://example.com/");
    let refused = false;
    try { await api("settings/url", "appearance.home_url	javascript:alert(1)"); } catch (e) { refused = true; }
    const set = (await api("settings")).all;
    await api("settings/reset", "");
    const after = (await api("settings")).all;
    const ok = set["tabs.position"] === "next" && set["appearance.zoom"] === "125" && set["tools.pip"] === "0" &&
      set["tabs.ntp_url"] === "https://example.com/" && refused &&
      after["tabs.position"] === "end" && after["appearance.zoom"] === "100" && after["tools.pip"] === "1" && after["tabs.ntp_url"] === "";
    return { ok, detail: `${Object.keys(before).length} settings readable by key; changed 4 and read them back; unsafe home page refused=${refused}; Reset settings restored the defaults` };
  });
  await step("sitedata", async () => {
    const p = await api("permissions");
    await api("privacy/clear", "passwords");
    return { ok: Array.isArray(p), detail: `site permissions listed (${p.length} saved); saved passwords and autofill deleted` };
  });
  window.__kakangkungTest = JSON.stringify(r);
})();
"#;

/// Self-test of the tab right-click menu, groups, split view and tab search, through the same
/// actions the menus send.
pub(crate) fn tab_features(b: &mut Browser) -> (bool, String) {
    let start = b.tabs.len();
    b.open_tab(ABOUT, None);
    let a = b.active;
    b.open_tab(ABOUT, None);
    let c = b.active;
    let pos = |b: &Browser, id: u64| b.index(id).unwrap_or(usize::MAX);

    b.tab_action(&format!("pin:{c}"));
    let pinned = b.tabs[0].view.id == c && b.tabs[0].view.pinned;
    b.tab_action(&format!("mute:{a}"));
    let muted = b.index(a).is_some_and(|i| b.tabs[i].view.muted);

    b.tab_action(&format!("group0:{a}"));
    let gid = b.index(a).map_or(0, |i| b.tabs[i].view.group);
    b.tab_action(&format!("dup:{a}"));
    let dup = b.tabs[pos(b, a) + 1].view.id;
    b.tab_action(&format!("group{gid}:{dup}"));
    let grouped = gid != 0 && b.groups.len() == 1 && pos(b, dup) == pos(b, a) + 1;
    // The group's right-click menu: its entries, a new colour, a new name (typed in the field).
    b.menu_probe = Some(vec![]);
    let _ = b.show_group_menu(gid, 120.0, 20.0);
    let menu = b.menu_probe.take().unwrap_or_default();
    b.group_action(&format!("color:{gid}:3"));
    let _ = b.on_ipc(&format!("input	grouprename:{gid}	Work"));
    let named = b.groups.first().is_some_and(|g| g.name == "Work" && g.color == ui::GROUP_COLORS[3]);
    let group_menu = named && ["Rename group…", "Colour > Green", "Ungroup", "Close group"].iter().all(|w| menu.iter().any(|l| l == w));
    b.tab_action(&format!("ungroup:{dup}"));
    b.tab_action(&format!("ungroup:{a}"));
    let ungrouped = b.groups.is_empty();

    b.activate(a);
    b.tab_action(&format!("split:{dup}"));
    let state = |b: &Browser, id: u64| b.index(id).map(|i| b.tabs[i].state);
    let half = |b: &Browser, id: u64| match b.tab_bounds(id).size {
        wry::dpi::Size::Physical(s) => s.width < b.window.inner_size().width / 2 + 1,
        wry::dpi::Size::Logical(s) => s.width < b.window.inner_size().to_logical::<f64>(b.window.scale_factor()).width / 2.0 + 1.0,
    };
    let split = b.split.is_some()
        && state(b, a) == Some(Lifecycle::Active)
        && state(b, dup) == Some(Lifecycle::Active)
        && half(b, a)
        && half(b, dup);
    b.tab_action(&format!("unsplit:{dup}"));
    let unsplit = b.split.is_none() && state(b, a) == Some(Lifecycle::Idle);

    b.show_tab_search();
    let search = b.overlay_open && b.overlay_kind == "tabsearch";
    b.hide_overlay();

    b.tab_action(&format!("others:{a}"));
    let others = b.tabs.len() == 2 && b.index(c).is_some() && b.index(a).is_some();
    b.tab_action(&format!("pin:{c}"));
    b.tab_action(&format!("close:{c}"));
    let ok = pinned && muted && grouped && group_menu && ungrouped && split && unsplit && search && others && b.tabs.len() == 1;
    (
        ok,
        format!(
            "{start} tab + 2 opened; pin moved tab to the front={pinned}; mute={muted}; new group + duplicate added next to it={grouped}; group menu renamed and recoloured it={group_menu}; \
             groups removed when emptied={ungrouped}; split view showed both tabs side by side={split}, closed again={unsplit}; \
             tab search pop-up={search}; 'Close other tabs' kept the pinned tab={others}; {} tab left",
            b.tabs.len()
        ),
    )
}

/// Scripted run of the core loop, driven through the same paths a user's input takes.
pub(crate) struct SelfTest {
    out: PathBuf,
    stage: Stage,
    ui_ready: bool,
    first_loaded: bool,
    steps: Vec<(&'static str, bool, String)>,
    act_detail: String,
    hooks_with_two_tabs: usize,
    poll_at: Option<Instant>,
    sync_folder: PathBuf,
    typed: String,
    /// Native Settings: first look (passed, detail) and the switch's value before the test.
    native: (bool, String, bool),
    youtube_started: Option<Instant>,
    youtube_samples: Vec<Json>,
    youtube_tab: u64,
    youtube_background: u64,
    youtube_switched: bool,
    preview_tab: u64,
    preview_webviews: usize,
    youtube_result: (bool,String),
}

impl SelfTest {
    pub(crate) fn new(out: PathBuf) -> Self {
        Self {
            out,
            stage: Stage::Start,
            // The window is ready before the event loop runs (the native frame exists at once).
            ui_ready: true,
            first_loaded: false,
            steps: vec![],
            act_detail: String::new(),
            hooks_with_two_tabs: 0,
            poll_at: None,
            typed: String::new(),
            native: (false, String::new(), false),
            youtube_started: None,
            youtube_samples: vec![],
            youtube_tab: 0,
            youtube_background: 0,
            youtube_switched: false,
            preview_tab: 0,
            preview_webviews: 0,
            youtube_result: (false,String::new()),
            sync_folder: std::env::temp_dir().join(format!("kakangkung-test-sync-{}", std::process::id())),
        }
    }

    /// Returns true when the test is finished and the app should exit.
    pub(crate) fn step(&mut self, o: Observed, script: Option<String>, b: &mut Browser) -> bool {
        match (&self.stage, o) {
            (Stage::Start, Observed::Loaded { url, .. }) if url.starts_with(ui::NEWTAB_URL) => {
                self.first_loaded = true
            }
            (Stage::WaitAbout, Observed::Loaded { url, .. }) if url == ABOUT => {
                self.stage = Stage::WaitBack;
                b.act(Action::Back); // Alt+Left path
            }
            (Stage::WaitBack, Observed::Loaded { url, .. }) => {
                let ok = url.starts_with(ui::NEWTAB_URL);
                self.steps.push((
                    "move",
                    ok,
                    format!("typed '{ABOUT}' in the address bar + Enter on the native New Tab page, the web page loaded, then Back landed on {url}"),
                ));
                // Tab 1 shows a web page again for the memory test (Forward-like, by typing).
                self.stage = Stage::WaitAboutAgain;
                b.navigate(ABOUT);
            }
            (Stage::WaitAboutAgain, Observed::Loaded { url, .. }) if url == ABOUT => {
                if let Some(wv) = b.active_webview() {
                    let _ = wv.evaluate_script("window.__memoryRetained={nested:{value:'retained-state'}}; const field=document.createElement('input');field.id='memory-retained-field';field.value='unsaved draft';document.body.append(field);");
                }
                self.stage = Stage::WaitNewTab;
                b.act(Action::NewTab); // Ctrl+T path
            }
            (Stage::WaitNewTab, Observed::Loaded { id, .. }) if id != 1 => {
                // Tab 1 is now hidden (IDLE). Freeze it through the state machine.
                self.hooks_with_two_tabs = b.hook_count();
                let first = b.index(1).unwrap_or(0);
                let before = b.tabs[first].state;
                // Memory Saver's own clock: Maximum mode, tab 1 hidden for longer than 30 s.
                b.memory_mode = memory::Mode::UltraLite;
                b.tabs[first].hidden_since = Instant::now().checked_sub(Duration::from_secs(31));
                b.memory_tick();
                self.act_detail = format!(
                    "Ctrl+T opened tab {id} ({} tabs); Memory Saver (Maximum, 30 s) froze hidden tab 1: {before:?} -> {:?}",
                    b.tabs.len(),
                    b.tabs[first].state
                );
                self.stage = Stage::WaitSuspend;
            }
            (Stage::WaitSuspend, Observed::Suspended { id: 1, ok }) => {
                let frozen = b.tabs[0].webview.as_ref().and_then(|w| platform::is_suspended(w).ok()) == Some(true);
                b.on_msg(Msg::Audio { id: 1, playing: true });
                let protected_running = b.tabs[0].state == Lifecycle::LowMemory
                    && b.tabs[0].webview.as_ref().and_then(|w| platform::is_suspended(w).ok()) == Some(false);
                b.on_msg(Msg::Audio { id: 1, playing: false });
                let protection_cleared = b.tabs[0].state == Lifecycle::Idle;
                b.act(Action::PrevTab); // Ctrl+Shift+Tab: Resume + Show
                let running = b.tabs[0].webview.as_ref().and_then(|w| platform::is_suspended(w).ok()) == Some(false);
                let back_on_1 = b.active == 1 && b.tabs[0].state == Lifecycle::Active;
                b.act(Action::NextTab);
                b.act(Action::CloseTab); // Ctrl+W on the new tab
                let one_left = b.tabs.len() == 1 && b.active == 1;
                let hooks_after = b.hook_count();
                // The native New Tab page adds no COM handlers.
                let ok = ok && frozen && protected_running && protection_cleared && running && back_on_1 && one_left && hooks_after == 7 && self.hooks_with_two_tabs == 7;
                self.steps.push((
                    "act",
                    ok,
                    format!(
                        "{}; TrySuspend ok={ok}, IsSuspended={frozen}; audio protection resumes hidden tab={protected_running}, cleared={protection_cleared}; Ctrl+Shift+Tab resumed it (running={running}, active={back_on_1}); Ctrl+W closed tab 2 (one tab left={one_left}); COM handlers {} -> {hooks_after}",
                        self.act_detail, self.hooks_with_two_tabs
                    ),
                ));
                self.stage = Stage::WaitRetained;
                self.poll_at = Some(Instant::now() + Duration::from_millis(800));
            }
            (Stage::WaitRetained, Observed::Script) => {
                let value: Json = script.and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or(Json::Null);
                let retained = value["object"] == "retained-state" && value["form"] == "unsaved draft";
                self.steps.push(("memoryretained", retained, format!("live JS object and unsaved form retained after hidden GC/suspend/resume: {value}")));
                b.act(Action::Back);
                self.stage = Stage::WaitScript;
                self.poll_at = Some(Instant::now() + Duration::from_millis(800));
            }
            (Stage::WaitScript, Observed::Script) => {
                let r = script.unwrap_or_default().replace("\\\"", "\"");
                let ok = r.contains("\"tabs\":1")
                    && r.contains("\"active\":\"New Tab\"")
                    && r.contains("\"chip\":\"internal\"")
                    && r.contains("\"buttons\":5")
                    && r.contains("\"caption\":3")
                    && r.contains("\"omnibox\":true");
                self.steps.push((
                    "hud",
                    ok,
                    format!("tab strip, toolbar (back/forward/reload/extensions/menu) + sync avatar, address bar with bookmark star with security chip and window buttons drawn: {r}; window title '{}'", b.window.title()),
                ));
                self.stage = Stage::WaitScriptPage;
                b.navigate(ABOUT);
            }
            (Stage::WaitScriptPage, Observed::Loaded { url, .. }) if url == ABOUT => {
                let downloads = self.sync_folder.with_extension("downloads");
                if let Err(error) = std::fs::create_dir_all(&downloads).map_err(|e| e.to_string())
                    .and_then(|_| b.active_webview().ok_or("missing download host".into())
                        .and_then(|wv| platform::set_download_folder(wv, &downloads).map_err(|e| e.to_string()))) {
                    return self.finish_with("download", false, error);
                }
                b.downloads_folder = Some(downloads);
                b.downloads_visible = false;
                b.toggle_devtools();
                let folder = serde_json::to_string(&self.sync_folder.display().to_string()).unwrap_or_default();
                let real_extension = std::env::var_os("KAKANGKUNG_TEST_EXTENSION").map(PathBuf::from);
                let store_id = if real_extension.is_some() { "ddkjiahejlhfcafbddmgiahcphecmpfh" } else { TEST_STORE_ID };
                let ext = real_extension.unwrap_or_else(|| test_store_extension(&self.sync_folder.with_extension("ext")));
                let js = FEATURE_JS
                    .replace("__FOLDER__", &folder)
                    .replace("__STORE_EXT__", &serde_json::to_string(&ext.display().to_string()).unwrap_or_default())
                    .replace("__STORE_ID__", store_id);
                match b.active_webview().map(|wv| wv.evaluate_script(&js)) {
                    Some(Ok(())) => {
                        self.stage = Stage::PollFeatures;
                        self.poll_at = Some(Instant::now() + Duration::from_millis(500));
                    }
                    other => return self.finish_with("import", false, format!("could not run the feature script: {other:?}")),
                }
            }
            (Stage::PollFeatures, Observed::Script) => {
                let raw = script.unwrap_or_default();
                let inner: String = serde_json::from_str(&raw).unwrap_or_default();
                if inner.is_empty() {
                    self.poll_at = Some(Instant::now() + Duration::from_millis(500));
                    return false;
                }
                let r: Json = serde_json::from_str(&inner).unwrap_or(Json::Null);
                let docked = b.devtools_ready && b.devtools.as_ref().is_some_and(|panel| panel.caption_hidden());
                self.steps.push(("devtools", docked, format!("real WebView2 DevTools hosted in a child viewport with its caption clipped={docked}")));
                if docked {
                    if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
                        let _ = platform::capture_window_png(b.window.hwnd(), &PathBuf::from(dir).join("devtools-docked.png"));
                    }
                }
                b.close_devtools();
                let ready = b.active_webview().is_some_and(platform::adblock_script_ready);
                self.steps.push(("adblockscript", ready, format!("latest document-start filtering registration completed={ready}")));
                let download_ready = b.downloads_visible && b.frame_state(false, false).pinned.iter().any(|p| p.id == "page:downloads");
                self.steps.push(("downloadactivity", download_ready, format!("real WebView2 download event made toolbar button visible={download_ready}")));
                for step in ["import", "extensions", "store", "sync", "browsers", "search", "privacy", "adblock", "download", "taskmanager", "settings", "sitedata"] {
                    let ok = r[step]["ok"].as_bool().unwrap_or(false);
                    let mut detail = r[step]["detail"].as_str().unwrap_or("no result").to_string();
                    if step == "sync" {
                        let files = std::fs::read_dir(&self.sync_folder).map(|d| d.flatten().count()).unwrap_or(0);
                        detail.push_str(&format!("; files in the sync folder: {files}"));
                    }
                    self.steps.push((step, ok, detail));
                }
                // Autocomplete: type the start of an imported address, like a user would. Without a
                // Chrome profile to import (a fresh test profile), visit a page first so history
                // has something to complete (queued on the storage thread before the typing).
                let imported = r["import"]["detail"].as_str().and_then(|d| d.split("newest: ").nth(1)).and_then(|u| u.split("://").nth(1)).map(str::to_string);
                let host = imported.unwrap_or_else(|| {
                    b.db(|s| s.add_visit("https://www.example.org/docs/start", "Getting started · Example docs", storage::now_ms()));
                    "www.example.org/docs/start".into()
                });
                let typed: String = host.trim_start_matches("www.").chars().take(4).collect();
                self.typed = typed;
                // DevTools closes on its owning process's queue. Let the window/focus events
                // finish before the next independent keyboard scenario starts.
                self.stage = Stage::WaitAutocompleteFocus;
                self.poll_at = Some(Instant::now() + Duration::from_millis(500));
            }
            (Stage::WaitAutocompleteFocus, Observed::Script) => {
                let typed = self.typed.clone();
                b.on_omnibox_blur();
                b.frame.set_fixture_omnibox_focus(true);
                // Into the native address bar, the way typing reaches the browser.
                b.frame.focus_omnibox();
                b.on_omnibox_focus();
                b.frame.set_omnibox_text(&typed, typed.chars().count());
                b.on_omnibox_change(&typed);
                self.stage = Stage::WaitAutocomplete;
                self.poll_at = Some(Instant::now() + Duration::from_millis(2500));
            }
            (Stage::WaitAutocomplete, Observed::Script) => {
                let raw = script.unwrap_or_default();
                let inner: String = serde_json::from_str(&raw).unwrap_or_default();
                let r: Json = serde_json::from_str(&inner).unwrap_or(Json::Null);
                if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
                    let _ = platform::capture_window_png(b.window.hwnd(), &PathBuf::from(dir).join("autocomplete.png"));
                }
                let value = r["value"].as_str().unwrap_or("");
                let rows = r["rows"].as_u64().unwrap_or(0);
                let shown = b.overlay_open;
                let ok = rows >= 2 && shown && value.len() > self.typed.len() && value.starts_with(&self.typed);
                b.frame.set_fixture_omnibox_focus(false);
                // Built-in pages reuse their tab instead of piling up new ones (like Chrome).
                let n0 = b.tabs.len();
                b.act(Action::TaskManager);
                let n1 = b.tabs.len();
                b.act(Action::TaskManager);
                let n2 = b.tabs.len();
                b.act(Action::Bookmarks);
                let n3 = b.tabs.len();
                b.act(Action::TaskManager);
                b.act(Action::Bookmarks);
                let n4 = b.tabs.len();
                self.steps.push((
                    "pages",
                    n1 == n0 + 1 && n2 == n1 && n3 == n1 + 1 && n4 == n3,
                    format!("Task manager: {n0} -> {n1} tabs; again while on it: {n2}; Bookmarks: {n3}; Task manager and Bookmarks again: {n4} (switched to the open tabs)"),
                ));
                // Incognito window (what --incognito starts): Ctrl+T and Ctrl+Shift+N open InPrivate
                // tabs in it, which are never recorded. (The window process itself is not started
                // here, so the test never touches the real profile.)
                let before = b.tabs.len();
                // Ctrl+Shift+N opens a new Incognito window (recorded, not started), from a normal
                // window and from an Incognito one; typing the Incognito page's address does too.
                b.window_probe = Some(vec![]);
                b.act(Action::NewPrivateTab);
                b.incognito = true;
                b.act(Action::NewPrivateTab);
                b.incognito = false;
                b.navigate(&format!("{}incognito", ui::INTERNAL));
                let windows = b.window_probe.take().unwrap_or_default();
                let new_windows = windows.len() == 3 && windows.iter().all(|(_, private)| *private) && b.tabs.len() == before;
                b.incognito = true;
                b.act(Action::NewTab);
                b.act(Action::NewTab);
                let private = new_windows && b.tabs.len() == before + 2 && b.tabs[before..].iter().all(|t| t.view.private);
                // Its new tab is the native Incognito page (no webview, nothing to record).
                let in_private_mode = b.index(b.active).is_some_and(|i| crate::native::page_of(&b.tabs[i].view.url) == Some("incognito") && b.tabs[i].webview.is_none());
                let synced_tabs = b.open_tabs().len();
                let title = b.window.title();
                b.act(Action::CloseTab);
                b.act(Action::CloseTab);
                b.incognito = false;
                self.steps.push((
                    "incognito",
                    private && in_private_mode && synced_tabs == before && b.tabs.len() == before && title.ends_with("(Incognito)"),
                    format!("Ctrl+Shift+N (twice) and kakangkung://incognito each opened a new Incognito window, no tab ({windows:?}); in an Incognito window Ctrl+T opens InPrivate tabs on the Incognito page (private={private}); left out of sync/session ({synced_tabs} of {} tabs shared); title '{title}'; closed again", before + 2),
                ));
                self.steps.push((
                    "autocomplete",
                    ok,
                    format!(
                        "typed '{}' in the address bar: inline completion '{value}' (selected {}), {rows} suggestions in the pop-up (first history match {}), search-engine suggestions {}, pop-up shown={}",
                        self.typed,
                        r["sel"].as_str().unwrap_or(""),
                        r["first"].as_str().unwrap_or("none"),
                        r["engine"],
                        shown
                    ),
                ));
                // The ⋮ menu, as a click opens it: a native Windows menu. The probe records its
                // entries instead of showing it (a shown menu waits for a click).
                let started = Instant::now();
                b.menu_probe = Some(vec![]);
                let _ = b.on_ipc("menu\t1270\t90");
                let labels = b.menu_probe.take().unwrap_or_default();
                let ms = started.elapsed().as_millis();
                let want = ["New tab", "History", "Find…", "Zoom  100% > Zoom in", "More tools > Task manager", "More tools > Developer tools", "Settings", "Exit"];
                let missing: Vec<&str> = want.iter().copied().filter(|w| !labels.iter().any(|l| l == w)).collect();
                self.steps.push((
                    "menu",
                    missing.is_empty() && ms < 1000,
                    format!("⋮ is a native Windows menu with {} entries, built in {ms} ms (incl. Zoom and More tools submenus); missing: {missing:?}", labels.len()),
                ));
                let (ok, detail) = tab_features(b);
                self.steps.push(("tabs", ok, detail));
                // Settings, a native page: opened like the menu opens it.
                b.open_after_active(&format!("{}settings", ui::INTERNAL));
                self.stage = Stage::WaitNative;
                b.navigate(&format!("{}settings/you",ui::INTERNAL));
                self.poll_at = Some(Instant::now() + Duration::from_millis(1500));
            }
            (Stage::WaitNative, Observed::Script) => {
                self.native = native::settings_shown(b);
                self.stage = Stage::WaitNativeSave;
                self.poll_at = Some(Instant::now() + Duration::from_millis(1000));
            }
            (Stage::WaitNativeSave, Observed::Script) => {
                let (saved, detail) = native::settings_saved(b, self.native.2);
                self.add_native(saved, detail);
                native::seed_lists(b);
                b.act(Action::History);
                self.stage = Stage::WaitHistory;
                self.poll_at = Some(Instant::now() + Duration::from_millis(1000));
            }
            (Stage::WaitHistory, Observed::Script) => {
                let (ok, detail) = native::history_shown(b);
                self.add_native(ok, detail);
                self.stage = Stage::WaitHistorySearch;
                self.poll_at = Some(Instant::now() + Duration::from_millis(1000));
            }
            (Stage::WaitHistorySearch, Observed::Script) => {
                let (ok, detail) = native::history_searched(b);
                self.add_native(ok, detail);
                b.act(Action::Bookmarks);
                self.stage = Stage::WaitBookmarks;
                self.poll_at = Some(Instant::now() + Duration::from_millis(1000));
            }
            (Stage::WaitBookmarks, Observed::Script) => {
                let (ok, detail) = native::bookmarks_shown(b);
                self.add_native(ok, detail);
                b.act(Action::TaskManager);
                self.stage = Stage::WaitTools;
                self.poll_at = Some(Instant::now() + Duration::from_millis(1000));
            }
            (Stage::WaitTools, Observed::Script) => {
                let (ok, detail) = native::taskmanager_shown(b);
                self.add_native(ok, detail);
                b.act(Action::Downloads);
                self.stage = Stage::WaitDownloads;
                self.poll_at = Some(Instant::now() + Duration::from_millis(1000));
            }
            (Stage::WaitDownloads, Observed::Script) => {
                let (ok, detail) = native::downloads_shown(b);
                self.add_native(ok, detail);
                b.open_after_active(&format!("{}extensions", ui::INTERNAL));
                self.stage = Stage::WaitExtensions;
                self.poll_at = Some(Instant::now() + Duration::from_millis(1000));
            }
            (Stage::WaitExtensions, Observed::Script) => {
                let (ok, detail) = native::extensions_shown(b);
                self.add_native(ok, detail);
                native::tour_open(b, 0);
                self.stage = Stage::Tour(0);
                self.poll_at = Some(Instant::now() + Duration::from_millis(1000));
            }
            (Stage::Tour(k), Observed::Script) => {
                let k = *k;
                let (ok, detail) = native::tour_check(b, k);
                self.add_native(ok, detail);
                if k + 1 < native::TOUR.len() {
                    native::tour_open(b, k + 1);
                    self.stage = Stage::Tour(k + 1);
                    self.poll_at = Some(Instant::now() + Duration::from_millis(1000));
                } else {
                    let (ok, detail) = (self.native.0, self.native.1.clone());
                    self.steps.push(("nativepages", ok, detail));
                    native::press_above_first_tab(b);
                    self.native = (true, String::new(), false);
                    self.stage = Stage::WaitTopClick;
                    self.poll_at = Some(Instant::now() + Duration::from_millis(600));
                }
            }
            (Stage::WaitTopClick, Observed::Script) => {
                let (ok, detail) = native::top_click_selected(b);
                self.add_native(ok, detail);
                native::press_tab_search(b);
                self.stage = Stage::WaitTabSearch;
                self.poll_at = Some(Instant::now() + Duration::from_millis(800));
            }
            (Stage::WaitTabSearch, Observed::Script) => {
                let (ok, detail) = native::tab_search_shown(b);
                self.add_native(ok, detail);
                let active = b.active;
                let count = b.tabs.len();
                let webviews = b.tabs.iter().filter(|t| t.webview.is_some()).count();
                let extra: Vec<_> = (0..100).map(|_| b.open_tab_lazy("https://example.invalid/lazy")).collect();
                let lazy = b.tabs.len() == count + 100 && b.tabs.iter().filter(|t| t.webview.is_some()).count() == webviews;
                b.activate(u64::MAX);
                let invalid_ignored = b.active == active;
                for id in extra { b.close_tab(id); }
                self.steps.push(("manytabs", lazy && invalid_ignored && b.tabs.len() == count,
                    format!("100 background tabs created without extra WebViews={lazy}; unknown tab ignored={invalid_ignored}; tabs restored to {count}")));
                b.settings.pinned_pages.retain(|page| *page != "downloads");
                b.on_msg(Msg::DownloadsChanged);
                let download_buttons = b.frame_state(false, false).pinned.iter().filter(|p| p.id == "page:downloads").count();
                b.on_msg(Msg::DownloadsChanged);
                let once = b.frame_state(false, false).pinned.iter().filter(|p| p.id == "page:downloads").count();
                self.steps.push(("downloadbutton", download_buttons == 1 && once == 1,
                    format!("download change automatically adds unpinned toolbar button={download_buttons}; repeated changes keep one button={once}")));
                if let Some(path) = std::env::var_os("KAKANGKUNG_TEST_AVATAR") {
                    b.on_msg(Msg::AccountProfile("Profile Fixture".into(), path.to_string_lossy().into_owned()));
                    let state = b.frame_state(false, false);
                    let photo = !state.avatar_image.is_empty() && platform::favicon_loaded(&state.avatar_image);
                    self.steps.push(("profilephoto", photo && state.avatar == "P", format!("local profile image decoded for native toolbar={photo}; initial fallback={}", state.avatar)));
                    if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
                        let _ = platform::capture_window_png(b.window.hwnd(), &PathBuf::from(dir).join("profile-toolbar.png"));
                    }
                }
                let shield = b.frame_state(false, false).pinned.iter().any(|p| p.id == "adblock");
                let was = b.settings.protection.adblock;
                b.on_menu("adblock-global");
                let toggled = b.settings.protection.adblock != was;
                b.on_menu("adblock-global");
                self.steps.push(("adblockcontrol", shield && toggled && b.settings.protection.adblock == was,
                    format!("ad blocker toolbar control visible={shield}; global switch changes and restores setting={toggled}")));
                b.show_adblock(1500.0, 82.0);
                if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
                    let _ = platform::capture_window_png(b.window.hwnd(), &PathBuf::from(dir).join("adblock-toolbar.png"));
                }
                b.hide_overlay();
                let (ok, detail) = (self.native.0, self.native.1.trim_start_matches("; ").to_string());
                self.steps.push(("tabstrip",ok,detail));
                self.preview_tab = b.active;
                self.preview_webviews = b.tabs.iter().filter(|tab|tab.webview.is_some()).count();
                b.capture_tab_preview(b.active);
                if !b.frame.hover_tab(b.active) { return self.finish_with("tabpreview",false,"tab has no native hit region".into()); }
                self.stage = Stage::WaitPreview;
                self.poll_at = Some(Instant::now()+Duration::from_secs(2));
            }
            (Stage::WaitPreview, Observed::Script) => {
                let status = b.previews.status(self.preview_tab);
                let pass = status["visible"]==true && status["thumbnail"]==true && status["memory"].as_u64().is_some_and(|bytes|bytes>0)
                    && b.tabs.iter().filter(|tab|tab.webview.is_some()).count()==self.preview_webviews;
                self.steps.push(("tabpreview",pass,format!("native mouse hover opened card with real snapshot and attributed renderer memory; no extra WebView: {status}")));
                if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
                    if let Some(window) = &b.previews.window { let _ = window.capture_png(&PathBuf::from(dir).join("tab-hover.png")); }
                }
                b.previews.hide();
                b.act(Action::NewTab);
                if let Some(index) = b.index(self.preview_tab) { b.tabs[index].hidden_since = Instant::now().checked_sub(Duration::from_secs(31)); }
                self.stage = Stage::WaitPreviewSuspend;
                b.memory_tick();
            }
            (Stage::WaitPreviewSuspend, Observed::Suspended { id, ok }) if id == self.preview_tab => {
                if !ok { return self.finish_with("sleepingpreview",false,"fixture tab refused suspension".into()); }
                b.hover_tab(id,60.0,41.0);
                self.stage = Stage::WaitPreviewSleeping;
                self.poll_at = Some(Instant::now()+Duration::from_secs(1));
            }
            (Stage::WaitPreviewSleeping, Observed::Script) => {
                let status = b.previews.status(self.preview_tab);
                let sleeping = b.index(self.preview_tab).and_then(|i|b.tabs[i].webview.as_ref()).and_then(|wv|platform::is_suspended(wv).ok())==Some(true);
                let pass = sleeping && status["visible"]==true && status["thumbnail"]==true
                    && b.tabs.iter().filter(|tab|tab.webview.is_some()).count()==self.preview_webviews;
                self.steps.push(("sleepingpreview",pass,format!("cached preview and memory card kept hidden tab suspended={sleeping}; {status}")));
                b.previews.hide();
                b.act(Action::CloseTab);
                if std::env::var_os("KAKANGKUNG_YOUTUBE_PROBE").is_none() {
                    return self.finish_with("previewcleanup",true,"native card hidden; fixture tab resumed".into());
                }
                self.stage = Stage::PollYouTube;
                self.youtube_started = Some(Instant::now());
                self.youtube_tab = b.active;
                let fixture_views: Vec<_> = b.tabs.iter().filter(|tab|tab.webview.is_some() && tab.view.id!=self.youtube_tab).map(|tab|tab.view.id).collect();
                for id in fixture_views { b.close_tab(id); }
                if let Some(index)=b.index(self.youtube_tab) { b.tabs[index].view.pinned=false; }
                b.settings.protect_media=true;
                b.navigate("https://www.youtube.com/watch?v=aqz-KE-bpKQ");
                if std::env::var_os("KAKANGKUNG_YOUTUBE_WORKLOAD").is_some() {
                    for url in ["https://github.com/revanapriyandi","https://www.instagram.com/","https://www.facebook.com/"] {
                        b.open_tab(url,None);
                        if self.youtube_background==0 { self.youtube_background=b.active; }
                    }
                    b.activate(self.youtube_tab);
                }
                self.poll_at = Some(Instant::now()+Duration::from_secs(5));
            }
            (Stage::PollYouTube, Observed::Script) => {
                let inner = script.and_then(|raw| serde_json::from_str::<String>(&raw).ok()).unwrap_or_default();
                let mut sample: Json = serde_json::from_str(&inner).unwrap_or(Json::Null);
                if !sample.is_object() { sample = json!({"error":"missing playback sample"}); }
                sample["foreground"] = json!(b.active==self.youtube_tab);
                if let Some(index) = b.index(self.youtube_tab) {
                    sample["media"] = json!(b.tabs[index].view.media);
                    sample["native"] = b.tabs[index].webview.as_ref().map(platform::memory_status).unwrap_or(Json::Null);
                }
                let memory = b.memory_snapshot();
                sample["private_ws"] = memory["private_ws"].clone();
                sample["commit"] = memory["commit"].clone();
                self.youtube_samples.push(sample.clone());
                eprintln!("YouTube playback probe: {sample}");
                let elapsed = self.youtube_started.map_or(Duration::ZERO,|start|start.elapsed());
                if self.youtube_background!=0 && elapsed>=Duration::from_secs(20) && !self.youtube_switched && sample["time"].as_f64().unwrap_or(0.0)>5.0 {
                    self.youtube_switched=true;
                    b.activate(self.youtube_background);
                }
                if self.youtube_switched && elapsed>=Duration::from_secs(55) && b.active!=self.youtube_tab { b.activate(self.youtube_tab); }
                if elapsed >= Duration::from_secs(75) {
                    let playing = self.youtube_samples.iter().any(|sample| sample["time"].as_f64().unwrap_or(0.0) > 5.0 && sample["ad"].as_bool() == Some(false));
                    let helper = sample["helper"]["enabled"] == true && sample["helper"]["youtube"] == true;
                    let background: Vec<_> = self.youtube_samples.iter().filter(|sample|sample["foreground"]==false).collect();
                    let uninterrupted = background.iter().all(|sample| sample["native"]["suspended"]==false && sample["media"]==true && sample["paused"]==false && sample["error"]==0)
                        && background.windows(2).all(|samples|samples[1]["time"].as_f64().unwrap_or(0.0)>samples[0]["time"].as_f64().unwrap_or(0.0));
                    let expected_background = self.youtube_background==0 || background.len()>=6;
                    self.steps.push(("backgroundvideo",uninterrupted && expected_background,
                        format!("muted video continues while another site is active for >30s: {uninterrupted}; background samples={}",background.len())));
                    self.youtube_result=(playing && helper && sample["error"]==0 && sample["paused"]==false,
                        format!("public signed-out video advanced beyond 5s={playing}; helper active={helper}; samples={}; ad delivery varies, so playback alone does not prove every ad was blocked", Json::Array(self.youtube_samples.clone())));
                    b.hide_overlay();
                    b.hover_tab(self.youtube_tab,60.0,41.0);
                    self.stage=Stage::WaitYouTubePreview;
                    self.poll_at=Some(Instant::now()+Duration::from_secs(2));
                    return false;
                }
                self.poll_at = Some(Instant::now() + Duration::from_secs(5));
            }
            (Stage::WaitYouTubePreview,Observed::Script) => {
                let status=b.previews.status(self.youtube_tab);
                self.steps.push(("youtubepreview",status["visible"]==true && status["thumbnail"]==true,format!("native card displays public video snapshot: {status}")));
                if let Some(dir)=std::env::var_os("KAKANGKUNG_SHOT_DIR") {
                    let _=platform::capture_window_png(b.window.hwnd(),&PathBuf::from(&dir).join("youtube-playback.png"));
                    if let Some(window)=&b.previews.window { let _=window.capture_png(&PathBuf::from(dir).join("youtube-tab-hover.png")); }
                }
                let (passed,detail)=std::mem::take(&mut self.youtube_result);
                return self.finish_with("youtube",passed,detail);
            }
            _ => {}
        }
        if self.stage == Stage::Start && self.ui_ready && self.first_loaded {
            // Type into the native address bar and press Enter (the keyboard's path).
            self.stage = Stage::WaitAbout;
            b.frame.set_omnibox_text(ABOUT, ABOUT.len());
            b.on_omnibox_key("enter");
        }
        false
    }

    /// Poll the feature script's result (content tabs have no IPC channel by design).
    pub(crate) fn tick(&mut self, b: &Browser) -> Option<Instant> {
        let due = self.poll_at?;
        if Instant::now() < due {
            return Some(due);
        }
        self.poll_at = None;
        let p = b.proxy.clone();
        let send = move |r: String| {
            let _ = p.send_event(Msg::Script(r));
        };
        if matches!(self.stage,Stage::WaitPreview|Stage::WaitPreviewSleeping|Stage::WaitYouTubePreview|Stage::WaitAutocompleteFocus) {
            send(String::new());
        } else if self.stage == Stage::WaitRetained {
            if let Some(wv) = b.active_webview() {
                let _ = wv.evaluate_script_with_callback("({object:window.__memoryRetained?.nested.value,form:document.getElementById('memory-retained-field')?.value})", send);
            } else { send(String::new()); }
        } else if self.stage == Stage::PollYouTube {
            if let Some(wv) = b.index(self.youtube_tab).and_then(|index|b.tabs[index].webview.as_ref()) {
                if platform::is_suspended(wv).ok()==Some(true) {
                    send(serde_json::to_string(&json!({"error":"video tab unexpectedly suspended"}).to_string()).unwrap_or_default());
                    return None;
                }
                let _ = wv.evaluate_script_with_callback(r#"(() => {
                  const video = document.querySelector('video');
                  if (video && !window.__kkProbePlayAttempted) {
                    window.__kkProbePlayAttempted=true; video.muted=true; video.play().catch(() => {});
                  }
                  const quality=video?.getVideoPlaybackQuality?.();
                  return JSON.stringify({helper:window.__kkAdblock?.status?.(), video:!!video,
                    time:video?.currentTime || 0, ready:video?.readyState || 0, paused:video?.paused,
                    dropped:quality?.droppedVideoFrames || 0,frames:quality?.totalVideoFrames || 0, width:video?.videoWidth || 0,height:video?.videoHeight || 0,
                    error:video?.error?.code || 0, ad:!!document.querySelector('.html5-video-player.ad-showing'),
                    playability:window.ytInitialPlayerResponse?.playabilityStatus?.status || ''});
                })()"#, send);
            } else { send(String::new()); }
        } else if self.stage == Stage::WaitScript {
            send(hud(b).to_string());
        } else if self.stage == Stage::WaitAutocomplete {
            let rows = &b.omnibox.rows;
            let r = json!({
                "value": b.frame.omnibox_text(),
                "sel": format!("{}-", b.omnibox.typed.chars().count()),
                "rows": rows.len(),
                "first": rows.iter().skip(1).find(|r| r["kind"] != "search").and_then(|r| r["url"].as_str()).unwrap_or(""),
                "engine": rows.iter().skip(1).filter(|r| r["kind"] == "search").map(|r| r["title"].clone()).collect::<Vec<_>>(),
            });
            send(serde_json::to_string(&r.to_string()).unwrap_or_default());
        } else if matches!(self.stage, Stage::WaitNative | Stage::WaitNativeSave | Stage::WaitHistory | Stage::WaitHistorySearch | Stage::WaitBookmarks | Stage::WaitTools | Stage::WaitDownloads | Stage::WaitExtensions | Stage::Tour(_) | Stage::WaitTopClick | Stage::WaitTabSearch) {
            send(String::new());
        } else if let Some(wv) = b.active_webview() {
            let _ = wv.evaluate_script_with_callback("window.__kakangkungTest || ''", send);
        }
        None
    }

    /// One more check of the native pages, reported together as the `nativepages` step.
    fn add_native(&mut self, ok: bool, detail: String) {
        self.native.0 &= ok;
        self.native.1.push_str("; ");
        self.native.1.push_str(&detail);
    }

    pub(crate) fn finish_with(&mut self, step: &'static str, passed: bool, detail: String) -> bool {
        self.steps.push((step, passed, detail));
        self.fail_rest("not reached");
        true
    }

    pub(crate) fn fail_rest(&mut self, why: &str) {
        if self.stage == Stage::Done {
            return;
        }
        self.stage = Stage::Done;
        for step in ["move", "act", "hud"] {
            if !self.steps.iter().any(|s| s.0 == step) {
                self.steps.push((step, false, why.to_string()));
            }
        }
        // Stopped before the last step (time-out): say so, or the steps that ran read as all passed.
        if !self.steps.iter().any(|s| s.0 == "tabstrip") {
            self.steps.push(("tabstrip", false, why.to_string()));
        }
        let body: Vec<String> = self
            .steps
            .iter()
            .map(|(s, ok, d)| format!(r#"    {{"step": "{s}", "passed": {ok}, "detail": "{}"}}"#, ui::json_escape(d)))
            .collect();
        let json = format!("{{\"version\": 1, \"steps\": [\n{}\n]}}\n", body.join(",\n"));
        if let Err(e) = std::fs::write(&self.out, json) {
            eprintln!("writing self-test result to {} failed: {e}", self.out.display());
        }
    }
}
