//! Kakangkung's own password manager: logins kept in the profile database, each password sealed
//! with Windows DPAPI (only this Windows user on this PC can open it, like Chrome's key). Imports
//! CSV exports from other browsers and password managers, and exports the same format.

use serde::Serialize;

use crate::Store;

/// A saved login, without its password.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Login {
    pub id: i64,
    /// `https://host[:port]`.
    pub origin: String,
    pub username: String,
    /// Unix ms when it was last filled in (0 = never).
    pub used: i64,
}

/// A login with its password in clear, as imported or exported.
#[derive(Debug, Clone, PartialEq)]
pub struct Credential {
    pub origin: String,
    pub username: String,
    pub password: String,
}

/// `scheme://host[:port]` of a web address, lowercase; `None` for anything but http(s).
pub fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    // Drop "user:pass@" in front of the host.
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h).to_ascii_lowercase();
    (!host.is_empty() && !host.contains(char::is_whitespace)).then(|| format!("{scheme}://{host}"))
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    s.len().is_multiple_of(2).then_some(())?;
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn seal(password: &str) -> Result<String, String> {
    platform::protect(password.as_bytes()).map(|b| hex(&b))
}

fn open(sealed: &str) -> Result<String, String> {
    let bytes = unhex(sealed).ok_or("damaged password entry")?;
    String::from_utf8(platform::unprotect(&bytes)?).map_err(|_| "damaged password entry".into())
}

/// The CSV a password manager exported: logins found by their column names, which every
/// manager spells its own way (Chrome/Edge/Google: url,username,password; Firefox the same;
/// Bitwarden login_uri,login_username,login_password; 1Password, LastPass, Proton Pass,
/// Dashlane, NordPass, Safari: url/website, username/login/email, password).
pub fn parse_csv(text: &str) -> Vec<Credential> {
    let rows = csv_rows(text.trim_start_matches('\u{feff}'));
    let Some((head, body)) = rows.split_first() else { return vec![] };
    let head: Vec<String> = head.iter().map(|h| h.trim().to_ascii_lowercase()).collect();
    let col = |names: &[&str]| names.iter().find_map(|n| head.iter().position(|h| h == n));
    let (Some(url), Some(user), Some(pass)) = (
        col(&["url", "login_uri", "website", "web site", "login url", "uri", "urls"]),
        col(&["username", "login_username", "login", "user name", "email", "user"]),
        col(&["password", "login_password"]),
    ) else {
        return vec![];
    };
    body.iter()
        .filter_map(|r| {
            // A cell may hold several addresses (1Password, Proton Pass): the first one counts.
            let first = r.get(url)?.split([',', '\n']).next().unwrap_or("").trim();
            let address = if first.contains("://") { first.to_string() } else { format!("https://{first}") };
            let password = r.get(pass)?.to_string();
            (!password.is_empty()).then_some(())?;
            Some(Credential { origin: origin_of(&address)?, username: r.get(user).cloned().unwrap_or_default(), password })
        })
        .collect()
}

/// RFC 4180 rows: commas, quoted cells with doubled quotes and line breaks inside.
fn csv_rows(text: &str) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut cell) = (vec![], vec![], String::new());
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => cell.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut cell)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows.retain(|r| r.iter().any(|c| !c.is_empty()));
    rows
}

/// Chrome's export format (name,url,username,password,note), which every password manager reads.
pub fn to_csv(list: &[Credential]) -> String {
    let q = |s: &str| if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
    let mut out = String::from("name,url,username,password,note\r\n");
    for c in list {
        let host = c.origin.split_once("://").map_or(c.origin.as_str(), |(_, h)| h);
        out.push_str(&format!("{},{},{},{},\r\n", q(host), q(&format!("{}/", c.origin)), q(&c.username), q(&c.password)));
    }
    out
}

impl Store {
    /// Logins that fill in on a page of `origin`: its own, and on https also those saved for the
    /// same host over http. Most recently used first.
    pub fn logins_for(&self, origin: &str) -> Result<Vec<Login>, String> {
        let http = origin.strip_prefix("https://").map(|h| format!("http://{h}")).unwrap_or_else(|| origin.to_string());
        self.login_rows("origin = ?1 OR origin = ?2", &[origin.into(), http.into()])
    }

    /// Every login whose address or username contains `q`.
    pub fn logins(&self, q: &str) -> Result<Vec<Login>, String> {
        let like = format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        self.login_rows("origin LIKE ?1 ESCAPE '\\' OR username LIKE ?1 ESCAPE '\\'", &[like.into()])
    }

    fn login_rows(&self, cond: &str, params: &[platform::sqlite::Value]) -> Result<Vec<Login>, String> {
        let sql = format!("SELECT id, origin, username, used FROM passwords WHERE {cond} ORDER BY used DESC, origin, username");
        Ok(self.db.query(&sql, params)?.into_iter().map(|r| Login { id: r[0].int(), origin: r[1].text(), username: r[2].text(), used: r[3].int() }).collect())
    }

    /// The login `id` with its password (and it counts as used when `fill`).
    pub fn password(&self, id: i64, fill: bool) -> Result<(Login, String), String> {
        let rows = self.db.query("SELECT id, origin, username, used, password FROM passwords WHERE id = ?", &[id.into()])?;
        let r = rows.first().ok_or("that password is no longer saved")?;
        if fill {
            self.db.run("UPDATE passwords SET used = ? WHERE id = ?", &[crate::now_ms().into(), id.into()])?;
        }
        Ok((Login { id: r[0].int(), origin: r[1].text(), username: r[2].text(), used: r[3].int() }, open(&r[4].text())?))
    }

    /// What saving this login would do: `None` when it is saved already, `Some(false)` new,
    /// `Some(true)` a new password for a saved username.
    pub fn password_change(&self, c: &Credential) -> Result<Option<bool>, String> {
        let rows = self.db.query("SELECT password FROM passwords WHERE origin = ? AND username = ?", &[c.origin.as_str().into(), c.username.as_str().into()])?;
        Ok(match rows.first() {
            None => Some(false),
            Some(r) if open(&r[0].text()).ok().as_deref() == Some(c.password.as_str()) => None,
            Some(_) => Some(true),
        })
    }

    /// Save a login (a saved username gets the new password).
    pub fn save_password(&self, c: &Credential) -> Result<(), String> {
        self.db.run(
            "INSERT INTO passwords(origin, username, password, created, used) VALUES(?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(origin, username) DO UPDATE SET password = ?3, used = ?4",
            &[c.origin.as_str().into(), c.username.as_str().into(), seal(&c.password)?.into(), crate::now_ms().into()],
        )
    }

    /// Add imported logins; ones saved already (same address and username) are kept as they are.
    /// Returns how many were new.
    pub fn import_passwords(&self, list: &[Credential]) -> Result<usize, String> {
        let before = self.password_count()?;
        let sealed: Vec<(&Credential, String)> = list.iter().map(|c| seal(&c.password).map(|s| (c, s))).collect::<Result<_, _>>()?;
        self.db.batch("BEGIN")?;
        let r = self.db.run_many(
            "INSERT OR IGNORE INTO passwords(origin, username, password, created, used) VALUES(?, ?, ?, ?, 0)",
            sealed.iter().map(|(c, s)| vec![c.origin.as_str().into(), c.username.as_str().into(), s.as_str().into(), crate::now_ms().into()]),
        );
        self.db.batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
        r?;
        Ok(self.password_count()? - before)
    }

    pub fn password_count(&self) -> Result<usize, String> {
        Ok(self.db.query("SELECT COUNT(*) FROM passwords", &[])?.first().map_or(0, |r| r[0].int() as usize))
    }

    /// Every login with its password, for exporting.
    pub fn export_passwords(&self) -> Result<Vec<Credential>, String> {
        let rows = self.db.query("SELECT origin, username, password FROM passwords ORDER BY origin, username", &[])?;
        rows.into_iter().map(|r| Ok(Credential { origin: r[0].text(), username: r[1].text(), password: open(&r[2].text())? })).collect()
    }

    /// Forget one login, or all of them (`None`, the never-save list too).
    pub fn delete_password(&self, id: Option<i64>) -> Result<(), String> {
        match id {
            Some(id) => self.db.run("DELETE FROM passwords WHERE id = ?", &[id.into()]),
            None => self.db.batch("DELETE FROM passwords; DELETE FROM password_never"),
        }
    }

    /// "Never" was chosen for this site: it is not offered again.
    pub fn never_save(&self, origin: &str) -> Result<(), String> {
        self.db.run("INSERT OR IGNORE INTO password_never(origin) VALUES(?)", &[origin.into()])
    }

    pub fn is_never_saved(&self, origin: &str) -> bool {
        self.db.query("SELECT 1 FROM password_never WHERE origin = ?", &[origin.into()]).map(|r| !r.is_empty()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        assert_eq!(origin_of("https://Accounts.Example.com:8443/login?x=1").as_deref(), Some("https://accounts.example.com:8443"));
        assert_eq!(origin_of("http://user:pw@a.example/").as_deref(), Some("http://a.example"));
        assert_eq!(origin_of("android://hash@com.example.app/"), None);
        assert_eq!(origin_of("javascript:alert(1)"), None);
    }

    #[test]
    fn csv_from_any_manager() {
        let chrome = "\u{feff}name,url,username,password,note\r\nexample.com,https://example.com/login,ana,\"p,a\"\"ss\",\r\n,android://x@com.app/,b,c,\n";
        assert_eq!(parse_csv(chrome), vec![Credential { origin: "https://example.com".into(), username: "ana".into(), password: "p,a\"ss".into() }]);
        let bitwarden = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n,,login,Git,\"multi\nline\",,0,https://github.com,dev,secret,\n";
        assert_eq!(parse_csv(bitwarden)[0], Credential { origin: "https://github.com".into(), username: "dev".into(), password: "secret".into() });
        let onepassword = "Title,Website,Username,Password\nMail,mail.example.org,me@x.id,pw\nEmpty,a.example,me,\n";
        assert_eq!(parse_csv(onepassword), vec![Credential { origin: "https://mail.example.org".into(), username: "me@x.id".into(), password: "pw".into() }]);
        assert!(parse_csv("a,b\n1,2\n").is_empty());
        let back = parse_csv(&to_csv(&parse_csv(chrome)));
        assert_eq!(back, parse_csv(chrome));
    }

    #[test]
    fn vault_saves_fills_imports_and_forgets() {
        let p = std::env::temp_dir().join(format!("kakangkung-pw-{}.db", std::process::id()));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", p.display()));
        }
        let s = Store::open(&p).unwrap();
        let c = |o: &str, u: &str, pw: &str| Credential { origin: o.into(), username: u.into(), password: pw.into() };
        assert_eq!(s.password_change(&c("https://a.example", "ana", "1")).unwrap(), Some(false));
        s.save_password(&c("https://a.example", "ana", "1")).unwrap();
        assert_eq!(s.password_change(&c("https://a.example", "ana", "1")).unwrap(), None);
        assert_eq!(s.password_change(&c("https://a.example", "ana", "2")).unwrap(), Some(true));
        // Sealed at rest: the database never holds the password in clear.
        let raw = s.db.query("SELECT password FROM passwords", &[]).unwrap()[0][0].text();
        assert!(raw.len() > 100 && unhex(&raw).is_some());
        assert_eq!(s.import_passwords(&[c("https://a.example", "ana", "old"), c("http://a.example", "bo", "3")]).unwrap(), 1);
        let found = s.logins_for("https://a.example").unwrap();
        assert_eq!(found.len(), 2, "the http login fills on https too");
        assert!(s.logins_for("http://a.example").unwrap().iter().all(|l| l.origin == "http://a.example"));
        let ana = found.iter().find(|l| l.username == "ana").unwrap().id;
        assert_eq!(s.password(ana, true).unwrap().1, "1", "an import never overwrites");
        assert!(s.password(ana, false).unwrap().0.used > 0);
        assert_eq!(s.logins("BO").unwrap().len(), 1);
        assert_eq!(s.export_passwords().unwrap().len(), 2);
        s.never_save("https://b.example").unwrap();
        assert!(s.is_never_saved("https://b.example"));
        s.delete_password(Some(ana)).unwrap();
        assert_eq!(s.password_count().unwrap(), 1);
        s.delete_password(None).unwrap();
        assert_eq!(s.password_count().unwrap(), 0);
        assert!(!s.is_never_saved("https://b.example"));
    }
}
