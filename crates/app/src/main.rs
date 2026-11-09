//! Kakangkung entry point: a frameless window with a UI webview (tab strip + toolbar)
//! and one child webview per tab.
//!
//! `--screenshot <png> [url]`: start, let the page settle, save a PNG of the window, exit.
//! `--self-test <json>`: drive the browser like a user (address bar, back, new tab, switch,
//! suspend/resume, close), record what worked, exit.
//! Any other arguments are opened as URLs, one tab each (all loaded, the first selected).
//! Like Chrome: `--user-data-dir=<folder>` uses that data folder, `--no-first-run` skips the
//! first-run welcome tab and import (the benchmark in bench/ uses both).
// A GUI program: no console window opens with it. Debug builds keep the console for eprintln!
// output; release output still reaches a pipe or file when redirected.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod browser;
mod google;
mod i18n;
mod jobs;
mod msg;
mod memory_trace;
mod native;
mod pages;
mod prefs;
mod profile;
mod selftest;
mod update;
mod webstore;

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use storage::Store;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};
use tao::window::{Icon, WindowBuilder};
use wry::dpi::LogicalSize;

use crate::browser::{Browser, Paths};
use crate::jobs::spawn_storage;
use crate::msg::Msg;
use crate::prefs::Prefs;
use crate::profile::{import_from_chrome, keep_browsers_in_sync, run_sync, sync_conf};
use crate::selftest::{Observed, SelfTest};

const APP: &str = ui::APP_NAME;
/// A built-in web page (not native) the self-test loads: the API script runs in it.
const ABOUT: &str = "https://kakangkung.internal/selftest";
const WELCOME: &str = "https://kakangkung.internal/welcome";

/// First automatic sync after start, so start-up stays quick (estimated).
const FIRST_SYNC_AFTER: Duration = Duration::from_secs(20);

/// Start-up timing log, printed when KAKANGKUNG_TIMING is set.
fn timing(what: &str) {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    if std::env::var_os("KAKANGKUNG_TIMING").is_some() {
        eprintln!("[{:>5} ms] {what}", start.elapsed().as_millis());
    }
}

/// Automatic sync interval while the browser runs (estimated; plus at start and exit).
const SYNC_EVERY: Duration = Duration::from_secs(300);

/// How often the open tabs are written for session restore, when they changed (estimated).
const SESSION_EVERY: Duration = Duration::from_secs(10);
/// ~120 frames at 60 Hz after the page reports loaded.
const SETTLE: Duration = Duration::from_millis(2000);
const TEST_TIMEOUT: Duration = Duration::from_secs(90);

enum Run {
    Normal(String),
    /// A further window (Ctrl+N, "Move tab to new window"): same profile, but it leaves session
    /// restore, sync and first-run work to the main window.
    /// Simplification: its tabs are not restored after a restart; one-process multi-window would fix that.
    Window { url: String, incognito: bool },
    Screenshot(PathBuf, String),
    SelfTest(PathBuf),
}

fn absolute(p: PathBuf) -> PathBuf {
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p)
    }
}

/// `--restore-session`: reopen the last session's tabs whatever "On startup" says (the installer
/// passes it when it opens Kakangkung again after an update).
fn restore_requested() -> bool {
    std::env::args().any(|a| a == "--restore-session")
}

/// Switches that may come with any run; `parse_args` skips them.
fn is_global(a: &str) -> bool {
    a == "--restore-session" || a == "--no-first-run" || a.starts_with("--user-data-dir=")
}

/// The run, plus further URLs for a normal run (opened as extra tabs).
fn parse_args() -> (Run, Vec<String>) {
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !is_global(a)).collect();
    let home = ui::NEWTAB_URL.to_string();
    if let [url, more @ ..] = args.as_slice() {
        if !url.starts_with("--") {
            let resolve = |u: &String| ui::resolve_input(u, ui::DEFAULT_SEARCH);
            return (Run::Normal(resolve(url).unwrap_or(home)), more.iter().filter_map(resolve).collect());
        }
    }
    let run = match args.as_slice() {
        [flag, path, url, ..] if flag == "--screenshot" => Run::Screenshot(path.into(), url.clone()),
        [flag, path] if flag == "--screenshot" => Run::Screenshot(path.into(), home),
        [flag, path, ..] if flag == "--self-test" => Run::SelfTest(path.into()),
        [flag, rest @ ..] if flag == "--new-window" || flag == "--incognito" => {
            let url = rest.first().and_then(|u| ui::resolve_input(u, ui::DEFAULT_SEARCH)).unwrap_or(home);
            Run::Window { url, incognito: flag == "--incognito" }
        }
        _ => Run::Normal(home),
    };
    (run, Vec::new())
}

/// Window frame colour (matches --frame in chrome.html).
fn frame_color(dark: bool) -> (u8, u8, u8, u8) {
    if dark {
        (0x26, 0x23, 0x2d, 255)
    } else {
        (0xe7, 0xe3, 0xf1, 255)
    }
}

fn icon(size: u32) -> Option<Icon> {
    Icon::from_rgba(ui::brand_icon_rgba(size), size, size).map_err(|e| eprintln!("window icon: {e}")).ok()
}

fn main() {
    timing("start");
    // Output files named on the command line are relative to where we were started from.
    let (run, more_urls) = parse_args();
    let run = match run {
        Run::Screenshot(p, url) => Run::Screenshot(absolute(p), url),
        Run::SelfTest(p) => Run::SelfTest(absolute(p)),
        other => other,
    };
    // An environment variable, so windows this one starts use the same folder.
    if let Some(d) = std::env::args().find_map(|a| a.strip_prefix("--user-data-dir=").map(PathBuf::from)) {
        std::env::set_var("KAKANGKUNG_USER_DATA_DIR", absolute(d));
    }
    // Run from the program's own folder, so relative paths are stable and Windows never searches
    // the launch folder for DLLs.
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        if let Err(e) = std::env::set_current_dir(&dir) {
            eprintln!("setting the working folder to {}: {e}", dir.display());
        }
    }
    let event_loop = EventLoopBuilder::<Msg>::with_user_event().build();
    timing("event loop");
    let proxy = event_loop.create_proxy();
    let window = match WindowBuilder::new()
        .with_title(APP)
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(500.0, 300.0))
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_background_color(frame_color(platform::dark_mode()))
        .with_window_icon(icon(32))
        .with_taskbar_icon(icon(64))
        .build(&event_loop)
    {
        Ok(w) => w,
        Err(e) => {
            eprintln!("creating the main window failed: {e}");
            std::process::exit(1);
        }
    };

    timing("window built");
    // Tests get a throwaway profile so they always start fresh.
    let base = match run {
        Run::Normal(_) | Run::Window { .. } => storage::data_dir(),
        // Test runs get a fresh profile; KAKANGKUNG_TEST_DIR reuses one (start-up timing checks).
        _ => std::env::var_os("KAKANGKUNG_TEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join(format!("kakangkung-test-{}", std::process::id()))),
    };
    let first_run_marker = base.join("first-run-done");
    let first_run = !first_run_marker.exists() && !std::env::args().any(|a| a == "--no-first-run");
    if let Err(e) = std::fs::create_dir_all(&base).and_then(|_| std::fs::write(&first_run_marker, b"")) {
        eprintln!("creating {}: {e}", base.display());
    }
    let store = Store::open(&base.join("kakangkung.db")).ok();
    // The self-test checks English text whatever the Windows display language.
    if let (Run::SelfTest(_), Some(s)) = (&run, &store) {
        let _ = s.set_setting("languages.ui", "en");
    }
    timing("database opened");
    let prefs = store.map(|s| Prefs::read(&s));
    timing("settings read");
    let (jobs, storage_thread) = spawn_storage(base.join("kakangkung.db"));
    let paths = Paths { extensions: base.join("Extensions") };
    // Hardware acceleration is a browser-process switch: read before the first webview exists.
    // So are the language sites are asked for (Accept-Language follows `--lang`) and images.
    let mut args = String::new();
    if let Some(s) = prefs.as_ref().map(|p| &p.settings) {
        if !s.hardware_acceleration {
            args += " --disable-gpu";
        }
        if s.preferred_language != "auto" {
            args += &format!(" --lang={}", s.preferred_language);
        }
        // ponytail: start-up switch (restart); live per-site blocking needs a WebResourceRequested filter.
        if s.block_images {
            args += " --blink-settings=imagesEnabled=false";
        }
    }
    let mut browser = match Browser::new(window, base.join("Profile"), proxy.clone(), jobs, paths, &args) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("creating the WebView2 UI failed (is the WebView2 Runtime installed?): {e}");
            std::process::exit(1);
        }
    };
    let start_url = match &run {
        // Like Chrome: the very first start opens the welcome tab, later starts a new tab.
        Run::Normal(url) if first_run && url == ui::NEWTAB_URL => WELCOME.into(),
        // An Incognito window starts on the Incognito page.
        Run::Window { url, incognito: true } if url == ui::NEWTAB_URL => format!("{}incognito", ui::INTERNAL),
        Run::Normal(url) | Run::Window { url, .. } | Run::Screenshot(_, url) => url.clone(),
        Run::SelfTest(_) => ui::NEWTAB_URL.into(),
    };
    browser.incognito = matches!(run, Run::Window { incognito: true, .. });
    if let Some(p) = &prefs {
        browser.search = ui::search_template(&p.engine);
        browser.engine_setting = p.engine.clone();
        browser.memory_mode = p.memory;
        browser.settings = p.settings.clone();
        browser.account_name = p.account.clone();
        browser.account_picture = platform::load_avatar_file(Path::new(&p.account_picture)).unwrap_or_default();
        browser.downloads_folder = Some(p.downloads.clone()).filter(|f| f.is_dir());
    }
    // "Clear browsing data when you exit" also runs at the next start (WebView2 clears
    // asynchronously, so the exit may beat it); done when the first webview exists.
    platform::configure_adblock_sites(&browser.settings.adblock_allowlist);
    browser.clear_at_start = browser.settings.clear_on_exit && matches!(run, Run::Normal(_));
    browser.update_dir = base.join("Updates");
    // The native frame is ready as soon as it exists: show the window now, before any webview
    // (WebView2 needs most of a second to start; the page area fills in when it is ready).
    browser.on_ipc("ready");
    // The logo hops in the page area until the first page is on screen.
    browser.show_launch();
    // "On startup": continue where you left off, or open specific pages. Tabs other than the
    // selected one come back lazily (no page loads until selected).
    let restore = match (&run, &prefs) {
        (Run::Normal(url), Some(p)) if url == ui::NEWTAB_URL && (p.startup == "continue" || restore_requested()) && !p.session.is_empty() => {
            Some((p.session.clone(), p.session_active))
        }
        (Run::Normal(url), Some(p)) if url == ui::NEWTAB_URL && p.startup == "pages" && !p.startup_pages.is_empty() => {
            Some((p.startup_pages.clone(), 0))
        }
        _ => None,
    };
    match restore {
        Some((urls, active)) => {
            let ids: Vec<u64> = urls.iter().map(|u| browser.open_tab_lazy(u)).collect();
            browser.activate(ids[active.min(ids.len() - 1)]);
        }
        None => {
            browser.open_tab(&start_url, None);
            // Further URLs on the command line: each loads now, like Chrome; the first stays selected.
            let first = browser.active;
            for url in &more_urls {
                browser.open_tab(url, None);
            }
            if !more_urls.is_empty() {
                browser.activate(first);
            }
        }
    }
    if first_run && matches!(run, Run::Normal(_)) {
        // Like Chrome's first run: bring history and bookmarks over from Chrome automatically,
        // on its own thread and connection so the new tab page and suggestions are not held up.
        // Signing in is the person's choice now (Sign in with Google): nothing signs in by itself.
        let db = base.join("kakangkung.db");
        std::thread::spawn(move || {
            let Ok(s) = Store::open(&db) else { return };
            match import_from_chrome(&s) {
                Ok(summary) => timing(&summary),
                Err(e) => eprintln!("first-run import: {e}"),
            }
            keep_browsers_in_sync(&s);
        });
    }
    // Keeps the Google Drive copy of sync data and the local sync folder equal.
    if let Run::Normal(_) = run {
        crate::google::start_mirror(base.join("kakangkung.db"), proxy.clone());
        crate::google::kick();
    }
    timing("first tab requested");
    let mut storage_thread = Some(storage_thread);
    let mut next_sync = Instant::now() + FIRST_SYNC_AFTER;
    let mut next_update = Instant::now() + update::FIRST_CHECK_AFTER;
    let mut next_session = Instant::now() + SESSION_EVERY;
    let mut saved_session = (String::new(), String::new());

    let started = Instant::now();
    let mut memory_trace = memory_trace::MemoryTrace::from_environment();
    let mut shot_at: Option<Instant> = None;
    let mut test = match &run {
        Run::SelfTest(out) => Some(SelfTest::new(out.clone())),
        _ => None,
    };

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if browser.confirm_quit() {
                    *flow = ControlFlow::Exit;
                }
            }
            Event::LoopDestroyed => {
                // A downloaded update goes in now, silently (the installer waits for us to close).
                if let Run::Normal(_) = run {
                    browser.install_pending_update();
                }
                // "Clear browsing data when you exit": history, cookies, cache and site data.
                if browser.settings.clear_on_exit && matches!(run, Run::Normal(_) | Run::Window { incognito: false, .. }) {
                    if let Some(wv) = browser.extension_host() {
                        let _ = wv.clear_all_browsing_data();
                    }
                    browser.db(|s| s.clear_history());
                }
                // Publish the open tabs, then let the storage thread finish its queue (max 5 s).
                if let Run::Normal(_) = run {
                    let p = browser.proxy.clone();
                    let tabs = browser.open_tabs();
                    browser.save_session(browser.session());
                    browser.job(Box::new(move |s| {
                        if let Some(s) = s {
                            if sync_conf(s).0 {
                                if let Err(e) = run_sync(s, tabs, &p) {
                                    eprintln!("sync on exit: {e}");
                                }
                            }
                        }
                    }));
                }
                let (done_tx, done_rx) = mpsc::channel();
                browser.job(Box::new(move |_| {
                    let _ = done_tx.send(());
                }));
                let _ = done_rx.recv_timeout(Duration::from_secs(5));
                drop(storage_thread.take());
            }
            Event::WindowEvent { event: WindowEvent::Moved(_), .. } => browser.hide_overlay(),
            // Tab search takes the keyboard into its own pop-up window and closes itself when it
            // loses it; every other pop-up closes when the browser window loses focus.
            Event::WindowEvent { event: WindowEvent::Focused(false), .. } if !matches!(run,Run::SelfTest(_)) && browser.overlay_kind != "tabsearch" && !browser.overlay_kind.starts_with("menu@") => browser.hide_overlay(),
            Event::WindowEvent { event: WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }, .. } => {
                browser.relayout()
            }
            Event::UserEvent(msg) => {
                // Native fixtures generate their own keyboard/mouse scenarios; desktop focus
                // changes must not dismiss a popup halfway through its assertion.
                if matches!(run, Run::SelfTest(_)) && matches!(&msg, Msg::Ipc(command)
                    if matches!(command.as_str(), "omniblur" | "overlayhide" | "tabhover\t0")) {
                    return;
                }
                if let Run::Screenshot(..) = run {
                    if shot_at.is_none() && browser.ui_ready && matches!(msg, Msg::Load { done: true, .. }) {
                        shot_at = Some(Instant::now() + SETTLE);
                    }
                }
                let script = match &msg {
                    Msg::Script(s) => Some(s.clone()),
                    _ => None,
                };
                let observed = test.as_ref().map(|_| Observed::of(&msg));
                if !browser.on_msg(msg) {
                    *flow = ControlFlow::Exit;
                }
                if let (Some(t), Some(o)) = (&mut test, observed) {
                    if t.step(o, script, &mut browser) {
                        *flow = ControlFlow::Exit;
                    }
                }
            }
            _ => {}
        }

        // Background clocks: window fallback, idle preparation, memory saver, automatic sync.
        let mut wake = browser.memory_tick();
        if let Some(at) = browser.preview_tick() {
            wake = Some(wake.map_or(at, |w| w.min(at)));
        }
        if let Some(trace) = memory_trace.as_mut() {
            let at = trace.tick(&browser);
            wake = Some(wake.map_or(at, |w| w.min(at)));
        }
        if let Some(at) = browser.devtools_tick() {
            wake = Some(wake.map_or(at, |w| w.min(at)));
        }
        if let Some(at) = browser.native_tick() {
            wake = Some(wake.map_or(at, |w| w.min(at)));
        }
        if let Run::Normal(_) = run {
            if Instant::now() >= next_sync {
                next_sync = Instant::now() + SYNC_EVERY;
                let (p, tabs) = (browser.proxy.clone(), browser.open_tabs());
                browser.job(Box::new(move |s| {
                    if let Some(s) = s {
                        keep_browsers_in_sync(s);
                        if sync_conf(s).0 {
                            let _ = p.send_event(Msg::Synced(run_sync(s, tabs, &p).is_ok()));
                        }
                    }
                }));
            }
            wake = Some(wake.map_or(next_sync, |w| w.min(next_sync)));
            // Automatic updates (Settings > System): look for a new version now and then.
            if Instant::now() >= next_update {
                next_update = Instant::now() + update::CHECK_EVERY;
                if browser.settings.auto_update {
                    browser.check_updates();
                }
            }
            wake = Some(wake.map_or(next_update, |w| w.min(next_update)));
            // Keep the stored session current so a crash or power cut loses at most a few seconds.
            if Instant::now() >= next_session {
                next_session = Instant::now() + SESSION_EVERY;
                let session = browser.session();
                if session != saved_session {
                    saved_session = session.clone();
                    browser.save_session(session);
                }
            }
            wake = Some(wake.map_or(next_session, |w| w.min(next_session)));
        }
        if let (Some(w), ControlFlow::Wait) = (wake, *flow) {
            *flow = ControlFlow::WaitUntil(w);
        }

        if let Run::Screenshot(path, _) = &run {
            let due = shot_at.unwrap_or(started + Duration::from_secs(20));
            if Instant::now() >= due {
                if let Err(e) = platform::capture_window_png(browser.window.hwnd(), path) {
                    eprintln!("screenshot failed: {e}");
                }
                *flow = ControlFlow::Exit;
            } else if *flow != ControlFlow::Exit {
                *flow = ControlFlow::WaitUntil(due);
            }
        }

        if let Some(t) = &mut test {
            let poll = t.tick(&browser);
            let timeout = if std::env::var_os("KAKANGKUNG_YOUTUBE_PROBE").is_some() { Duration::from_secs(240) } else { TEST_TIMEOUT };
            if started.elapsed() >= timeout {
                t.fail_rest("timed out after 90 s");
                *flow = ControlFlow::Exit;
            } else if *flow != ControlFlow::Exit {
                let end = started + TEST_TIMEOUT;
                *flow = ControlFlow::WaitUntil(poll.map_or(end, |p| p.min(end)));
            }
        }
    });
}
