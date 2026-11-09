use std::{fs, path::{Path, PathBuf}};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct WebApp {
    pub name: String,
    pub start_url: String,
    pub scope: String,
    pub manifest_url: String,
    pub manifest_id: String,
    pub icon_url: String,
}

fn web_url(text: &str) -> Result<Url, String> {
    if text.len() > 8_192 { return Err("Web app URL is too long".into()); }
    let url = Url::parse(text).map_err(|_| "Invalid web app URL")?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
        || !url.username().is_empty() || url.password().is_some() || url.host_str().is_none() {
        return Err("Web apps require HTTPS or a local development server, without URL credentials".into());
    }
    Ok(url)
}

impl WebApp {
    pub(crate) fn from_message(page: &str, text: &str) -> Result<Self, String> {
        if text.len() > 32_768 { return Err("Web app description is too large".into()); }
        let app: Self = serde_json::from_str(text).map_err(|_| "Invalid web app description")?;
        app.validate(page)?;
        Ok(app)
    }

    fn validate(&self, page: &str) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.chars().count() > 80 || self.name.chars().any(char::is_control) {
            return Err("Invalid web app name".into());
        }
        let page = web_url(page)?;
        let start = web_url(&self.start_url)?;
        let scope = web_url(&self.scope)?;
        let identity = web_url(&self.manifest_id)?;
        web_url(&self.manifest_url)?;
        web_url(&self.icon_url)?;
        if start.origin() != page.origin() || scope.origin() != page.origin() || identity.origin() != page.origin()
            || !start.path().starts_with(scope.path()) || !page.path().starts_with(scope.path()) {
            return Err("Web app start URL, identity and scope must belong to this site".into());
        }
        Ok(())
    }

    pub(crate) fn contains(&self, text: &str) -> bool {
        let (Ok(url), Ok(scope)) = (Url::parse(text), Url::parse(&self.scope)) else { return false };
        url.origin() == scope.origin() && url.path().starts_with(scope.path()) && url.username().is_empty() && url.password().is_none()
    }

    pub(crate) fn id(&self) -> Result<String, String> { platform::sha256_hex(self.manifest_id.as_bytes()) }

    fn shortcut_name(&self) -> Result<String, String> {
        let name: String = self.name.chars().filter(|c| !"<>:\"/\\|?*".contains(*c)
            && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).take(60).collect();
        let name = name.trim().trim_matches('.');
        Ok(format!("{} - {}.lnk", if name.is_empty() { "Web app" } else { name }, &self.id()?[..8]))
    }
}

fn valid_id(id: &str) -> bool { id.len() == 64 && id.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) }

pub(crate) fn shortcut_directory() -> Result<PathBuf, String> {
    if let Some(test) = std::env::var_os("KAKANGKUNG_TEST_APP_SHORTCUTS") { return Ok(PathBuf::from(test)); }
    std::env::var_os("APPDATA").map(PathBuf::from)
        .map(|path| path.join("Microsoft/Windows/Start Menu/Programs/Kakangkung Apps"))
        .ok_or_else(|| "Windows Start menu is unavailable".into())
}

pub(crate) fn load(base: &Path, id: &str) -> Result<WebApp, String> {
    if !valid_id(id) { return Err("Invalid web app identifier".into()); }
    let path = base.join("Web Apps").join(id).join("app.json");
    let metadata = fs::metadata(&path).map_err(|_| "This web app is not installed")?;
    if metadata.len() > 32_768 { return Err("Invalid installed web app".into()); }
    let json = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let app: WebApp = serde_json::from_str(&json).map_err(|_| "Invalid installed web app")?;
    app.validate(&app.start_url)?;
    if app.id()? != id { return Err("Web app identity does not match its installation".into()); }
    Ok(app)
}

pub(crate) fn install(base: &Path, shortcuts: &Path, exe: &Path, app: &WebApp) -> Result<(), String> {
    app.validate(&app.start_url)?;
    let id = app.id()?;
    if load(base, &id).is_ok() { return Ok(()); }
    let directory = base.join("Web Apps").join(&id);
    fs::create_dir_all(&directory).and_then(|_| fs::create_dir_all(shortcuts)).map_err(|error| error.to_string())?;
    let json = serde_json::to_vec_pretty(app).map_err(|error| error.to_string())?;
    let temporary = directory.join("app.json.tmp");
    fs::write(&temporary, json).map_err(|error| error.to_string())?;
    platform::create_app_shortcut(&shortcuts.join(app.shortcut_name()?), exe, &id, base, &app.name).map_err(|error| error.to_string())?;
    if let Err(error) = fs::rename(&temporary, directory.join("app.json")) {
        let _ = fs::remove_file(shortcuts.join(app.shortcut_name()?));
        let _ = fs::remove_file(temporary);
        return Err(error.to_string());
    }
    Ok(())
}

pub(crate) fn uninstall(base: &Path, shortcuts: &Path, id: &str) -> Result<(), String> {
    let app = load(base, id)?;
    let link = shortcuts.join(app.shortcut_name()?);
    if link.exists() { fs::remove_file(link).map_err(|error| error.to_string())?; }
    let directory = base.join("Web Apps").join(id);
    fs::remove_file(directory.join("app.json")).map_err(|error| error.to_string())?;
    let _ = fs::remove_dir(directory);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> WebApp { WebApp { name: "Fixture App".into(), start_url: "https://example.test/app/start".into(), scope: "https://example.test/app/".into(), manifest_url: "https://example.test/manifest.json".into(), manifest_id: "https://example.test/app/".into(), icon_url: "https://example.test/icon.png".into() } }

    #[test]
    fn validates_origin_scope_and_launch_urls() {
        let mut app = app();
        assert!(app.validate("https://example.test/app/page").is_ok());
        assert!(app.validate("https://other.test/app/page").is_err());
        app.start_url = "https://other.test/app/start".into();
        assert!(app.validate("https://example.test/app/").is_err());
        for url in ["javascript:alert(1)", "file:///C:/Windows/notepad.exe", "http://example.test/", "https://user:secret@example.test/app/", "data:text/html,bad"] { assert!(web_url(url).is_err(), "{url}"); }
        assert!(web_url("http://127.0.0.1:3000/").is_ok());
    }

    #[test]
    fn install_reopen_and_remove_leave_profile_data_intact() {
        let base = std::env::temp_dir().join(format!("kk-web-app-{}-{}", std::process::id(), storage::now_ms()));
        let shortcuts = base.join("shortcuts");
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("keep-profile.txt"), b"retained").unwrap();
        let app = app(); let id = app.id().unwrap();
        install(&base, &shortcuts, &std::env::current_exe().unwrap(), &app).unwrap();
        assert_eq!(load(&base, &id).unwrap(), app);
        assert!(shortcuts.join(app.shortcut_name().unwrap()).metadata().unwrap().len() > 76);
        assert!(load(&base, "../escape").is_err());
        uninstall(&base, &shortcuts, &id).unwrap();
        assert!(load(&base, &id).is_err());
        assert_eq!(fs::read(base.join("keep-profile.txt")).unwrap(), b"retained");
        let _ = fs::remove_file(base.join("keep-profile.txt"));
        let _ = fs::remove_dir(shortcuts); let _ = fs::remove_dir(base.join("Web Apps")); let _ = fs::remove_dir(base);
    }
}
