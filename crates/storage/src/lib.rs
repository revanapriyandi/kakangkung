//! Persistence: history and settings in one SQLite file, Chrome import, folder sync.
//!
//! Every function here may block on disk; the app calls them from its storage worker thread.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use platform::sqlite::{Db, Value};
use serde::{Deserialize, Serialize};

pub mod chrome;
pub mod passwords;
pub mod sync;

/// Schema migrations; index + 1 = `PRAGMA user_version` after applying it.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE history(url TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT '', visit_count INTEGER NOT NULL DEFAULT 0, last_visit INTEGER NOT NULL DEFAULT 0);
     CREATE INDEX history_last ON history(last_visit DESC);
     CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    "CREATE TABLE bookmarks(url TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT '', added INTEGER NOT NULL)",
    "CREATE TABLE downloads(id INTEGER PRIMARY KEY, url TEXT NOT NULL, path TEXT NOT NULL, size INTEGER NOT NULL, done INTEGER NOT NULL, started INTEGER NOT NULL)",
    // Passwords are DPAPI-sealed, hex encoded (see passwords.rs).
    "CREATE TABLE passwords(id INTEGER PRIMARY KEY, origin TEXT NOT NULL, username TEXT NOT NULL, password TEXT NOT NULL, created INTEGER NOT NULL, used INTEGER NOT NULL DEFAULT 0, UNIQUE(origin, username));
     CREATE TABLE password_never(origin TEXT PRIMARY KEY)",
    "ALTER TABLE passwords ADD COLUMN canonical_origin TEXT NOT NULL DEFAULT '';
     CREATE INDEX passwords_canonical ON passwords(canonical_origin, username)",
];

/// A finished download kept for the Downloads page after a restart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PastDownload {
    pub id: i64,
    pub url: String,
    pub path: String,
    /// Bytes received.
    pub size: i64,
    /// Completed (else cancelled or failed).
    pub done: bool,
    /// Unix ms when it started.
    pub started: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
    pub added: i64,
}

/// What an import from another browser brought in.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Imported {
    pub pages: usize,
    pub bookmarks: usize,
    /// New saved passwords.
    pub passwords: usize,
    /// Passwords the browser keeps locked to itself (Chrome's app-bound encryption).
    pub locked: usize,
}

impl Imported {
    /// "Imported 120 pages, 14 bookmarks and 30 passwords from Google Chrome.", plus why some
    /// passwords could not come along.
    pub fn summary(&self, from: &str) -> String {
        let mut s = format!("Imported {} pages, {} bookmarks and {} passwords from {from}.", self.pages, self.bookmarks, self.passwords);
        if self.locked > 0 {
            s.push_str(&format!(" {} passwords are locked to that browser: export them there as a CSV file and import it on the Passwords page.", self.locked));
        }
        s
    }
}

/// One address-bar suggestion.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    pub url: String,
    pub title: String,
    pub bookmarked: bool,
}

/// Address-bar suggestions plus Chrome-style inline completion of what was typed.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct Suggestions {
    pub items: Vec<Suggestion>,
    /// Text the address bar may complete `q` to (starts with `q`, case-insensitively), or "".
    pub inline: String,
}

/// URL without scheme and `www.`, as people type it.
fn typed_form(url: &str) -> &str {
    let u = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).unwrap_or(url);
    u.strip_prefix("www.").unwrap_or(u)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    pub url: String,
    pub title: String,
    pub count: i64,
    /// Unix time in milliseconds.
    pub last: i64,
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Only web pages are stored or merged: never `javascript:`, `data:`, `file:` or built-in pages,
/// so imported or synced data cannot plant script links into the history page.
pub fn storable(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    (l.starts_with("https://") || l.starts_with("http://")) && !l.starts_with("https://kakangkung.internal")
}

pub struct Store {
    db: Db,
}

impl Store {
    /// One SQLite statement observes a coherent set of settings across connections.
    pub fn settings_snapshot(&self) -> Result<std::collections::HashMap<String, String>, String> {
        Ok(self.db.query("SELECT key, value FROM settings", &[])?.into_iter().map(|row| (row[0].text(), row[1].text())).collect())
    }

    /// Publish related identity settings together, never a partial account switch.
    pub fn set_settings(&self, settings: &[(&str, &str)]) -> Result<(), String> {
        self.db.batch("BEGIN IMMEDIATE")?;
        let changed = self.db.run_many(
            "INSERT INTO settings(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            settings.iter().map(|(key, value)| vec![(*key).into(), (*value).into()]),
        );
        match changed {
            Ok(_) => self.db.batch("COMMIT"),
            Err(error) => { let _ = self.db.batch("ROLLBACK"); Err(error) }
        }
    }

    /// Apply account metadata only if the authorizing session still matches.
    pub fn set_settings_if_current(&self, expected: &[(&str, &str)], updates: &[(&str, &str)]) -> Result<bool, String> {
        self.db.batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            let current = self.settings_snapshot()?;
            if expected.iter().any(|(key, value)| current.get(*key).map(String::as_str) != Some(*value)) {
                return Ok(false);
            }
            self.db.run_many("INSERT INTO settings(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                updates.iter().map(|(key, value)| vec![(*key).into(), (*value).into()]))?;
            Ok(true)
        })();
        self.db.batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
        result
    }

    pub fn open(path: &Path) -> Result<Store, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }
        let db = Db::open(path, false)?;
        db.batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL")?;
        let version = db.query("PRAGMA user_version", &[])?.first().and_then(|r| r.first()).map(Value::int).unwrap_or(0);
        for (i, m) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            db.batch("BEGIN")?;
            db.batch(m).and_then(|_| db.batch(&format!("PRAGMA user_version={}", i + 1))).inspect_err(|_| {
                let _ = db.batch("ROLLBACK");
            })?;
            db.batch("COMMIT")?;
        }
        let store = Store { db };
        store.migrate_password_origins()?;
        Ok(store)
    }

    pub fn add_visit(&self, url: &str, title: &str, at: i64) -> Result<(), String> {
        if !storable(url) {
            return Ok(());
        }
        self.db.run(
            "INSERT INTO history(url, title, visit_count, last_visit) VALUES(?1, ?2, 1, ?3)
             ON CONFLICT(url) DO UPDATE SET visit_count = visit_count + 1, last_visit = ?3,
               title = CASE WHEN ?2 = '' THEN title ELSE ?2 END",
            &[url.into(), title.into(), at.into()],
        )
    }

    pub fn set_title(&self, url: &str, title: &str) -> Result<(), String> {
        if title.is_empty() {
            return Ok(());
        }
        self.db.run("UPDATE history SET title = ? WHERE url = ?", &[title.into(), url.into()])
    }

    /// Newest first; `q` matches URL or title (case-insensitive substring).
    pub fn history(&self, q: &str, limit: i64) -> Result<Vec<Visit>, String> {
        let like = format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let rows = self.db.query(
            "SELECT url, title, visit_count, last_visit FROM history
             WHERE url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'
             ORDER BY last_visit DESC LIMIT ?2",
            &[like.into(), limit.into()],
        )?;
        Ok(rows
            .into_iter()
            .map(|r| Visit { url: r[0].text(), title: r[1].text(), count: r[2].int(), last: r[3].int() })
            .collect())
    }

    pub fn delete_url(&self, url: &str) -> Result<(), String> {
        self.db.run("DELETE FROM history WHERE url = ?", &[url.into()])
    }

    pub fn clear_history(&self) -> Result<(), String> {
        self.db.run("DELETE FROM history", &[])
    }

    /// Forget pages last visited at or after `since_ms` (Unix ms).
    /// ponytail: only the last visit is kept, so a page visited before and inside the range goes too.
    pub fn clear_history_since(&self, since_ms: i64) -> Result<(), String> {
        self.db.run("DELETE FROM history WHERE last_visit >= ?1", &[since_ms.into()])
    }

    /// Upsert visits from another source: keep the higher count and the newer visit/title.
    pub fn merge(&self, visits: &[Visit]) -> Result<usize, String> {
        self.db.batch("BEGIN")?;
        let r = self.db.run_many(
            "INSERT INTO history(url, title, visit_count, last_visit) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(url) DO UPDATE SET
               visit_count = MAX(visit_count, excluded.visit_count),
               title = CASE WHEN excluded.last_visit > last_visit AND excluded.title <> '' THEN excluded.title ELSE title END,
               last_visit = MAX(last_visit, excluded.last_visit)",
            visits
                .iter()
                .filter(|v| storable(&v.url))
                .map(|v| vec![v.url.as_str().into(), v.title.as_str().into(), v.count.into(), v.last.into()]),
        );
        self.db.batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
        r
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        let rows = self.db.query("SELECT value FROM settings WHERE key = ?", &[key.into()]).ok()?;
        rows.first().and_then(|r| r.first()).map(Value::text)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.db.run(
            "INSERT INTO settings(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            &[key.into(), value.into()],
        )
    }

    pub fn set_bookmark(&self, url: &str, title: &str, on: bool) -> Result<(), String> {
        if !on {
            return self.db.run("DELETE FROM bookmarks WHERE url = ?", &[url.into()]);
        }
        if !storable(url) {
            return Err("only web pages can be bookmarked".into());
        }
        self.db.run(
            "INSERT INTO bookmarks(url, title, added) VALUES(?1, ?2, ?3) ON CONFLICT(url) DO UPDATE SET title = ?2",
            &[url.into(), title.into(), now_ms().into()],
        )
    }

    pub fn is_bookmarked(&self, url: &str) -> bool {
        self.db.query("SELECT 1 FROM bookmarks WHERE url = ?", &[url.into()]).map(|r| !r.is_empty()).unwrap_or(false)
    }

    pub fn bookmarks(&self) -> Result<Vec<Bookmark>, String> {
        let rows = self.db.query("SELECT url, title, added FROM bookmarks ORDER BY added DESC", &[])?;
        Ok(rows.into_iter().map(|r| Bookmark { url: r[0].text(), title: r[1].text(), added: r[2].int() }).collect())
    }

    /// Keep finished downloads (their `id` is ignored; the database gives one).
    pub fn add_downloads(&self, list: &[PastDownload]) -> Result<usize, String> {
        self.db.run_many(
            "INSERT INTO downloads(url, path, size, done, started) VALUES(?1, ?2, ?3, ?4, ?5)",
            list.iter().map(|d| vec![d.url.as_str().into(), d.path.as_str().into(), d.size.into(), i64::from(d.done).into(), d.started.into()]),
        )
    }

    /// Newest first.
    pub fn downloads(&self, limit: i64) -> Result<Vec<PastDownload>, String> {
        let rows = self.db.query("SELECT id, url, path, size, done, started FROM downloads ORDER BY started DESC, id DESC LIMIT ?", &[limit.into()])?;
        Ok(rows
            .into_iter()
            .map(|r| PastDownload { id: r[0].int(), url: r[1].text(), path: r[2].text(), size: r[3].int(), done: r[4].int() != 0, started: r[5].int() })
            .collect())
    }

    /// Take one download off the list (the file stays); `None` takes them all.
    pub fn forget_downloads(&self, id: Option<i64>) -> Result<(), String> {
        match id {
            Some(id) => self.db.run("DELETE FROM downloads WHERE id = ?", &[id.into()]),
            None => self.db.run("DELETE FROM downloads", &[]),
        }
    }

    /// Most visited pages, one per URL, for the new-tab shortcuts.
    pub fn top_sites(&self, limit: i64) -> Result<Vec<Visit>, String> {
        self.history_where("1", &[], "visit_count DESC, last_visit DESC", limit)
    }

    fn history_where(&self, cond: &str, params: &[Value], order: &str, limit: i64) -> Result<Vec<Visit>, String> {
        let mut p = params.to_vec();
        p.push(limit.into());
        let sql = format!(
            "SELECT url, title, visit_count, last_visit FROM history WHERE {cond} ORDER BY {order} LIMIT ?{}",
            p.len()
        );
        Ok(self
            .db
            .query(&sql, &p)?
            .into_iter()
            .map(|r| Visit { url: r[0].text(), title: r[1].text(), count: r[2].int(), last: r[3].int() })
            .collect())
    }

    /// Suggestions for what is typed in the address bar: bookmarks and history whose URL or title
    /// contains `q`. Ranked like Chrome: typed-prefix matches first, then bookmarks, then visits.
    pub fn suggest(&self, q: &str, limit: usize) -> Result<Suggestions, String> {
        let q = q.trim();
        if q.is_empty() {
            return Ok(Suggestions::default());
        }
        let like = format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let marks: Vec<Bookmark> = self
            .db
            .query(
                "SELECT url, title, added FROM bookmarks WHERE url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\' LIMIT 50",
                &[like.clone().into()],
            )?
            .into_iter()
            .map(|r| Bookmark { url: r[0].text(), title: r[1].text(), added: r[2].int() })
            .collect();
        let visits = self.history_where(
            "url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'",
            &[like.into()],
            "visit_count DESC, last_visit DESC",
            200,
        )?;
        let ql = q.to_lowercase();
        let mut scored: Vec<(i64, Suggestion)> = vec![];
        let mut add = |url: &str, title: &str, count: i64, bookmarked: bool| {
            if let Some(s) = scored.iter_mut().find(|(_, s)| s.url == url) {
                s.1.bookmarked |= bookmarked;
                s.0 += if bookmarked { 500 } else { count.min(400) };
                return;
            }
            let prefix = typed_form(url).to_lowercase().starts_with(&ql);
            let score = if prefix { 10_000 } else { 0 } + if bookmarked { 500 } else { 0 } + count.min(400);
            scored.push((score, Suggestion { url: url.into(), title: title.into(), bookmarked }));
        };
        for b in &marks {
            add(&b.url, &b.title, 0, true);
        }
        for v in &visits {
            add(&v.url, &v.title, v.count, false);
        }
        scored.sort_by_key(|(score, s)| (std::cmp::Reverse(*score), s.url.len()));
        let items: Vec<Suggestion> = scored.into_iter().take(limit).map(|(_, s)| s).collect();
        // Inline completion: complete to the host first (like Chrome), or the whole URL once the
        // host has been typed.
        let inline = items
            .iter()
            .map(|s| typed_form(&s.url))
            .find(|t| t.to_lowercase().starts_with(&ql))
            .map(|t| {
                let host_end = t.find('/').unwrap_or(t.len());
                if host_end >= q.len() { &t[..host_end] } else { t.trim_end_matches('/') }
            })
            .filter(|t| t.len() > q.len())
            .unwrap_or("")
            .to_string();
        Ok(Suggestions { items, inline })
    }

    /// Add bookmarks that are not there yet (Chrome import). Returns how many were new.
    pub fn import_bookmarks(&self, marks: &[Bookmark]) -> Result<usize, String> {
        let before = self.bookmarks()?.len();
        self.db.batch("BEGIN")?;
        let r = self.db.run_many(
            "INSERT OR IGNORE INTO bookmarks(url, title, added) VALUES(?, ?, ?)",
            marks.iter().filter(|b| storable(&b.url)).map(|b| vec![b.url.as_str().into(), b.title.as_str().into(), b.added.into()]),
        );
        self.db.batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
        r?;
        Ok(self.bookmarks()?.len() - before)
    }

    /// Import history, bookmarks and saved passwords from another browser's profile.
    pub fn import_browser(&self, b: &chrome::Browser, profile: &str) -> Result<Imported, String> {
        self.import_browser_parts(b, profile, true)
    }

    /// Refresh ordinary browsing data without reading or replacing saved passwords.
    pub fn refresh_browser(&self, b: &chrome::Browser, profile: &str) -> Result<Imported, String> {
        self.import_browser_parts(b, profile, false)
    }

    fn import_browser_parts(&self, b: &chrome::Browser, profile: &str, passwords: bool) -> Result<Imported, String> {
        let p = b.profiles.iter().find(|p| p.dir == profile).ok_or("unknown profile")?;
        if b.chromium {
            let dir = b.dir.join(&p.dir);
            let pages = self.import_chrome_history(&dir.join("History"))?;
            let file = dir.join("Bookmarks");
            let bookmarks = match std::fs::metadata(&file) {
                Ok(_) => self.import_bookmarks(&chrome::read_bookmarks(&file)?)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
                Err(e) => return Err(format!("reading bookmarks: {e}")),
            };
            let (passwords, locked) = match passwords.then(|| chrome::read_logins(&b.dir, &dir)) {
                Some(Ok((logins, locked))) => (self.import_passwords(&logins)?, locked),
                Some(Err(e)) => {
                    eprintln!("import passwords from {}: {e}", b.name);
                    (0, 0)
                }
                None => (0, 0),
            };
            Ok(Imported { pages, bookmarks, passwords, locked })
        } else {
            // ponytail: Firefox seals passwords with its own NSS key; its CSV export imports instead.
            let (visits, marks) = chrome::read_firefox(Path::new(&p.history))?;
            Ok(Imported { pages: self.merge(&visits)?, bookmarks: self.import_bookmarks(&marks)?, ..Default::default() })
        }
    }

    /// Import a Chrome `History` SQLite file (works while Chrome is running: a copy is read).
    pub fn import_chrome_history(&self, history: &Path) -> Result<usize, String> {
        let copy = chrome::Snapshot::read(history)?;
        let visits = chrome::read_history(&copy.path);
        self.merge(&visits?)
    }
}

/// Where Kakangkung keeps its data: `KAKANGKUNG_USER_DATA_DIR` (set from `--user-data-dir=` and
/// inherited by further windows), else `%LOCALAPPDATA%\Kakangkung`.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("KAKANGKUNG_USER_DATA_DIR") {
        return d.into();
    }
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Kakangkung")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_metadata_cannot_overwrite_a_switched_account() {
        let (store, path) = temp_store("conditional-profile");
        store.set_settings(&[("account.email", "first@example.invalid"), ("account.name", "First")]).unwrap();
        let other = Store::open(&path).unwrap();
        other.set_settings(&[("account.email", "second@example.invalid"), ("account.name", "Second")]).unwrap();
        assert!(!store.set_settings_if_current(&[("account.email", "first@example.invalid")], &[("account.name", "Stale name")]).unwrap());
        assert_eq!(store.setting("account.name").as_deref(), Some("Second"));
        assert!(store.set_settings_if_current(&[("account.email", "second@example.invalid")], &[("account.name", "Updated")]).unwrap());
        assert_eq!(other.setting("account.name").as_deref(), Some("Updated"));
    }

    #[test]
    fn chrome_import_reads_committed_wal() {
        let (store, out) = temp_store("wal-import");
        let root = std::env::temp_dir().join(format!("kakangkung-wal-import-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let history = root.join("History");
        let source = Db::open(&history, false).unwrap();
        source.batch("CREATE TABLE IF NOT EXISTS urls(url TEXT, title TEXT, visit_count INTEGER, last_visit_time INTEGER, hidden INTEGER); DELETE FROM urls; PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0").unwrap();
        source.run("INSERT INTO urls VALUES(?, ?, 1, ?, 0)", &["https://wal.example/".into(), "Committed in WAL".into(), 13_000_000_000_000_000i64.into()]).unwrap();
        assert!(root.join("History-wal").metadata().unwrap().len() > 0);
        store.import_chrome_history(&history).unwrap();
        assert_eq!(store.history("wal.example", 10).unwrap().len(), 1);
        drop(source);
        drop(store);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(out);
    }

    fn temp_store(tag: &str) -> (Store, PathBuf) {
        let p = std::env::temp_dir().join(format!("kakangkung-store-{tag}-{}.db", std::process::id()));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", p.display()));
        }
        (Store::open(&p).expect("open"), p)
    }

    #[test]
    fn downloads_are_kept_newest_first_and_forgotten() {
        let (s, _) = temp_store("d");
        let d = |url: &str, started| PastDownload { id: 0, url: url.into(), path: format!("C:/dl/{started}.zip"), size: 10, done: true, started };
        assert_eq!(s.add_downloads(&[d("https://a.example/", 1000), d("https://b.example/", 2000)]).unwrap(), 2);
        let list = s.downloads(10).unwrap();
        assert_eq!(list.iter().map(|d| d.url.as_str()).collect::<Vec<_>>(), ["https://b.example/", "https://a.example/"]);
        assert!(list[0].done && list[0].size == 10);
        s.forget_downloads(Some(list[0].id)).unwrap();
        assert_eq!(s.downloads(10).unwrap().len(), 1);
        s.forget_downloads(None).unwrap();
        assert!(s.downloads(10).unwrap().is_empty());
    }

    #[test]
    fn history_visits_search_and_merge() {
        let (s, _) = temp_store("h");
        s.add_visit("https://example.com/", "Example", 1000).unwrap();
        s.add_visit("https://example.com/", "", 2000).unwrap();
        s.add_visit("javascript:alert(1)", "evil", 3000).unwrap();
        s.add_visit("https://kakangkung.internal/history", "History", 3000).unwrap();
        s.set_title("https://example.com/", "Example Domain").unwrap();
        let h = s.history("", 10).unwrap();
        assert_eq!(h, vec![Visit { url: "https://example.com/".into(), title: "Example Domain".into(), count: 2, last: 2000 }]);
        assert_eq!(s.history("DOMAIN", 10).unwrap().len(), 1);
        assert_eq!(s.history("100%", 10).unwrap().len(), 0);

        let n = s
            .merge(&[
                Visit { url: "https://example.com/".into(), title: "Newer".into(), count: 7, last: 5000 },
                Visit { url: "https://rust-lang.org/".into(), title: "Rust".into(), count: 1, last: 4000 },
                Visit { url: "data:text/html,x".into(), title: "x".into(), count: 1, last: 9000 },
            ])
            .unwrap();
        assert_eq!(n, 2);
        let h = s.history("", 10).unwrap();
        assert_eq!(h[0], Visit { url: "https://example.com/".into(), title: "Newer".into(), count: 7, last: 5000 });
        assert_eq!(h.len(), 2);
        s.delete_url("https://rust-lang.org/").unwrap();
        assert_eq!(s.history("", 10).unwrap().len(), 1);
        s.clear_history().unwrap();
        assert!(s.history("", 10).unwrap().is_empty());
    }

    #[test]
    fn bookmarks_and_suggestions() {
        let (s, _) = temp_store("b");
        s.add_visit("https://www.github.com/", "GitHub", 1).unwrap();
        s.add_visit("https://www.github.com/", "GitHub", 2).unwrap();
        s.add_visit("https://docs.github.com/en", "GitHub Docs", 3).unwrap();
        s.add_visit("https://example.com/git-guide", "Git guide", 4).unwrap();
        s.set_bookmark("https://gitlab.com/", "GitLab", true).unwrap();
        assert!(s.is_bookmarked("https://gitlab.com/"));
        assert!(s.set_bookmark("javascript:x", "x", true).is_err());

        let r = s.suggest("git", 5).unwrap();
        let urls: Vec<&str> = r.items.iter().map(|i| i.url.as_str()).collect();
        // Typed-prefix matches first (bookmark, then most visited), substring matches after.
        assert_eq!(urls, ["https://gitlab.com/", "https://www.github.com/", "https://docs.github.com/en", "https://example.com/git-guide"]);
        assert!(r.items[0].bookmarked);
        assert_eq!(r.inline, "gitlab.com");
        assert_eq!(s.suggest("github.com/", 5).unwrap().inline, "");
        assert_eq!(s.suggest("docs.github.com/e", 5).unwrap().inline, "docs.github.com/en");
        assert!(s.suggest("  ", 5).unwrap().items.is_empty());

        assert_eq!(s.top_sites(1).unwrap()[0].url, "https://www.github.com/");
        s.set_bookmark("https://gitlab.com/", "", false).unwrap();
        assert!(s.bookmarks().unwrap().is_empty());
    }

    #[test]
    fn settings_and_reopen_keep_schema() {
        let (s, p) = temp_store("s");
        s.set_setting("a", "1").unwrap();
        s.set_setting("a", "2").unwrap();
        drop(s);
        let s = Store::open(&p).unwrap();
        assert_eq!(s.setting("a").as_deref(), Some("2"));
        assert_eq!(s.setting("missing"), None);
    }
}
