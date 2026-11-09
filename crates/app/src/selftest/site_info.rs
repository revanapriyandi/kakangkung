use serde_json::json;
use crate::{browser::Browser, msg::Msg};

pub(super) fn check(browser: &mut Browser) -> (bool, String) {
    let id = browser.active;
    let Some(i) = browser.index(id) else { return (false, "no active fixture tab".into()) };
    let original_url = browser.tabs[i].view.url.clone();
    browser.tabs[i].view.url = "https://fixture.example/app/".into();
    let url = browser.tabs[i].view.url.clone();
    let message = json!({"visibleSecurityState":{"securityState":"secure","certificateSecurityState":{"protocol":"TLS 1.3","issuer":"Fixture CA"}}}).to_string();
    browser.on_msg(Msg::Security { id, url: "https://wrong.example/".into(), data: message.clone() });
    let stale_ignored = browser.frame_state(false,false).chip != platform::frame::Chip::Secure;
    browser.on_msg(Msg::Security { id, url: url.clone(), data: message });
    let secure = browser.frame_state(false,false).chip == platform::frame::Chip::Secure;
    let app = json!({"name":"Fixture App","start_url":url,"scope":url,"manifest_id":url,"manifest_url":"https://fixture.example/manifest.json","icon_url":"https://fixture.example/icon.png"}).to_string();
    browser.on_msg(Msg::WebAppFound { id, url: "https://wrong.example/".into(), data: app.clone() });
    let foreign_app_ignored = !browser.frame_state(false,false).installable;
    browser.on_msg(Msg::WebAppFound { id, url: url.clone(), data: app });
    let installable = browser.frame_state(false,false).installable;
    let fixture_app = browser.tabs[i].web_app.clone().unwrap();
    let app_id = fixture_app.id().unwrap();
    let base = browser.web_app_base().to_path_buf();
    let shortcuts = base.join("test-app-shortcuts");
    let previous = std::env::var_os("KAKANGKUNG_TEST_APP_SHORTCUTS");
    std::env::set_var("KAKANGKUNG_TEST_APP_SHORTCUTS", &shortcuts);
    browser.show_web_app(600.0,88.0);
    let command = browser.menu_main.iter().find_map(|row| row["id"].as_str().filter(|id|id.starts_with("webapp-install:")).map(str::to_string));
    if let Some(command) = command { browser.on_menu(&command); }
    let installed = crate::webapps::load(&base,&app_id).is_ok()
        && std::fs::read_dir(&shortcuts).ok().is_some_and(|entries|entries.flatten().any(|entry|entry.path().extension().is_some_and(|ext|ext=="lnk")));
    let removed = crate::webapps::uninstall(&base,&shortcuts,&app_id).is_ok() && crate::webapps::load(&base,&app_id).is_err();
    let _ = std::fs::remove_dir(shortcuts);
    match previous { Some(value) => std::env::set_var("KAKANGKUNG_TEST_APP_SHORTCUTS",value), None => std::env::remove_var("KAKANGKUNG_TEST_APP_SHORTCUTS") }
    browser.hide_overlay();
    browser.show_site_info(70.0,88.0);
    let interactive = ["connection","privacy","site-data","site-adblock","site-install"].iter()
        .all(|id| browser.menu_main.iter().any(|row|row["id"] == *id));
    if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
        let _ = platform::capture_window_png(browser.popup.hwnd(), &std::path::PathBuf::from(dir).join("site-info-secure.png"));
    }
    browser.hide_overlay();
    browser.on_msg(Msg::Security { id, url: url.clone(), data: json!({"visibleSecurityState":{"securityState":"insecure-broken","certificateSecurityState":{"certificateNetworkError":"net::ERR_CERT_DATE_INVALID"}}}).to_string() });
    let warning = browser.frame_state(false,false).chip == platform::frame::Chip::Insecure && !browser.frame_state(false,false).installable;
    browser.show_site_info(70.0,88.0);
    if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
        let _ = platform::capture_window_png(browser.popup.hwnd(), &std::path::PathBuf::from(dir).join("site-info-insecure.png"));
    }
    if std::env::var_os("KAKANGKUNG_SITE_INFO_UI_HOLD").is_none() { browser.hide_overlay(); }
    browser.tabs[i].web_app = None;
    browser.tabs[i].view.url = original_url;
    browser.tabs[i].connection = Default::default();
    browser.render();
    (stale_ignored && foreign_app_ignored && secure && installable && installed && removed && interactive && warning,
        format!("native IPC fixture: stale TLS ignored={stale_ignored}, foreign app ignored={foreign_app_ignored}, secure icon={secure}, install icon={installable}, native install/shortcut={installed}, removal={removed}, actionable rows={interactive}, certificate warning={warning}"))
}
