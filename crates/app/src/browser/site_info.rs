use serde_json::{json, Value};
use tao::platform::windows::WindowExtWindows;
use super::Browser;

#[derive(Default)]
pub(crate) struct Connection {
    state: String,
    protocol: String,
    issuer: String,
    error: String,
}

impl Connection {
    fn parse(data: &str) -> Option<Self> {
        if data.len() > 65_536 { return None; }
        let data: Value = serde_json::from_str(data).ok()?;
        let value = &data["visibleSecurityState"];
        let state = value["securityState"].as_str()?;
        let certificate = &value["certificateSecurityState"];
        let text = |key: &str| certificate[key].as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(180).collect();
        Some(Self { state: state.into(), protocol: text("protocol"), issuer: text("issuer"), error: text("certificateNetworkError") })
    }

    pub(crate) fn security(&self, url: &str) -> ui::Security {
        let scheme = ui::Security::of(url);
        if scheme == ui::Security::Internal || !url.starts_with("https://") { return scheme; }
        if !self.error.is_empty() || matches!(self.state.as_str(), "insecure" | "insecure-broken" | "dangerous") { return ui::Security::Insecure; }
        if self.state == "secure" { ui::Security::Secure } else { ui::Security::Local }
    }

    pub(crate) fn description(&self, url: &str) -> (&'static str, &'static str, &'static str) {
        match self.security(url) {
            ui::Security::Internal => ("info", "Kakangkung page", "Built into your browser"),
            ui::Security::Secure => ("lock", "Connection is secure", "Your connection to this site is encrypted"),
            ui::Security::Insecure => ("warn", "Connection is not secure", "Avoid entering passwords or payment details"),
            ui::Security::Local if url.starts_with("https://") => ("info", "HTTPS connection", "Certificate status has not been reported yet"),
            _ => ("info", "Local page", "A file or a page on your local network"),
        }
    }
}

impl Browser {
    pub(crate) fn web_app_base(&self) -> &std::path::Path {
        self.ctx.data_directory().and_then(std::path::Path::parent).expect("browser profile has a parent directory")
    }

    pub(crate) fn security_changed(&mut self, id: u64, url: &str, data: &str) {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.view.id == id && t.view.url == url) else { return };
        let Some(connection) = Connection::parse(data) else { return };
        tab.connection = connection;
        if tab.connection.security(url) == ui::Security::Insecure { tab.web_app = None; }
        if id == self.active { self.render(); }
    }

    pub(crate) fn web_app_found(&mut self, id: u64, url: &str, data: &str) {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.view.id == id && t.view.url == url && !t.view.private && t.connection.security(url) != ui::Security::Insecure) else { return };
        let Ok(app) = crate::webapps::WebApp::from_message(url, data) else { return };
        tab.web_app = Some(app);
        if id == self.active { self.render(); }
    }

    pub(crate) fn show_connection(&mut self) {
        let Some(tab) = self.tabs.iter().find(|t| t.view.id == self.active) else { return };
        let (icon, title, detail) = tab.connection.description(&tab.view.url);
        let mut rows = vec![json!({"label": ui::host(&tab.view.url)}), json!({"id":"siteinfo-back","label":title,"sub":detail,"icon":icon})];
        if !tab.connection.protocol.is_empty() { rows.push(json!({"label":format!("Protocol: {}", tab.connection.protocol)})); }
        if !tab.connection.issuer.is_empty() { rows.push(json!({"label":format!("Certificate issuer: {}", tab.connection.issuer)})); }
        if !tab.connection.error.is_empty() { rows.push(json!({"label":format!("Certificate error: {}", tab.connection.error)})); }
        if storage::storable(&tab.view.url) { rows.push(json!({"label":"An encrypted connection does not guarantee that a site is trustworthy."})); }
        rows.push(json!({"id":"siteinfo-back","label":"Back to site information","icon":"back"}));
        self.native_popup(rows, self.menu_place.0, self.menu_place.1, false);
    }

    pub(crate) fn show_web_app(&mut self, x: f64, y: f64) {
        let Some(tab) = self.tabs.iter().find(|t| t.view.id == self.active && !t.view.private) else { return };
        let Some(app) = &tab.web_app else { return };
        let Ok(id) = app.id() else { return };
        let installed = crate::webapps::load(self.web_app_base(), &id).is_ok();
        let command = format!("webapp-{}:{}:{}", if installed { "open" } else { "install" }, tab.view.id, tab.revision);
        let mut rows = vec![json!({"label":app.name}), json!({"label":ui::host(&app.start_url)}),
            json!({"id":command,"label":if installed {"Open app"} else {"Install app"},"sub":if installed {"Open in its own window"} else {"Add to the Windows Start menu"},"icon":"install"})];
        if installed { rows.push(json!({"id":format!("webapp-remove:{}:{}",tab.view.id,tab.revision),"label":"Uninstall app…","icon":"delete"})); }
        else { rows.push(json!({"label":"Uses this browser profile. Install only apps from sites you trust."})); }
        rows.push(json!({"id":"dismiss","label":"Cancel","icon":"close"}));
        self.native_popup(rows, x, y, true);
    }

    pub(crate) fn web_app_action(&mut self, command: &str) {
        let parts: Vec<_> = command.split(':').collect();
        if parts.len() != 3 { return; }
        let Some(tab) = self.tabs.iter().find(|t| t.view.id == self.active && Some(t.view.id) == parts[1].parse().ok() && Some(t.revision) == parts[2].parse().ok() && !t.view.private && t.connection.security(&t.view.url) != ui::Security::Insecure) else { return };
        let Some(app) = tab.web_app.clone() else { return };
        let result = (|| {
            let base = self.web_app_base().to_path_buf(); let id = app.id()?;
            match parts[0] {
                "webapp-install" => {
                    let shortcuts = crate::webapps::shortcut_directory()?;
                    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                    crate::webapps::install(&base, &shortcuts, &exe, &app)?;
                    self.show_web_app(self.menu_place.0, self.menu_place.1);
                }
                "webapp-open" => {
                    crate::webapps::load(&base, &id)?;
                    std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                        .arg(format!("--app-id={id}")).arg(format!("--user-data-dir={}",base.display()))
                        .spawn().map_err(|e| e.to_string())?;
                }
                "webapp-remove" if platform::confirm(self.window.hwnd(), "Uninstall app", &format!("Remove {} from the Start menu? Your browsing data will be kept.", app.name)) => {
                    crate::webapps::uninstall(&base, &crate::webapps::shortcut_directory()?, &id)?;
                }
                _ => {}
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result { self.native_popup(vec![json!({"label":error}),json!({"id":"dismiss","label":"Close","icon":"close"})], self.menu_place.0, self.menu_place.1, true); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn certificate_errors_override_https_and_unreported_is_not_secure() {
        let url = "https://example.test/";
        assert_eq!(Connection::default().security(url), ui::Security::Local);
        let secure = Connection::parse(r#"{"visibleSecurityState":{"securityState":"secure","certificateSecurityState":{"protocol":"TLS 1.3"}}}"#).unwrap();
        assert_eq!(secure.security(url),ui::Security::Secure);
        let broken = Connection::parse(r#"{"visibleSecurityState":{"securityState":"insecure-broken","certificateSecurityState":{"certificateNetworkError":"net::ERR_CERT_DATE_INVALID"}}}"#).unwrap();
        assert_eq!(broken.security(url),ui::Security::Insecure);
        assert_eq!(secure.security("http://example.test/"),ui::Security::Insecure);
        assert_eq!(secure.security(ui::NEWTAB_URL),ui::Security::Internal);
    }
}
