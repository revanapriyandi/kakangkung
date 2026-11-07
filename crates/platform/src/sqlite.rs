//! Minimal SQLite binding to the copy that ships with Windows 10/11 (`winsqlite3.dll`).
//!
//! Using the operating system's copy means no C compiler is needed to build Kakangkung and no
//! SQLite is shipped: it is linked with raw-dylib.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;

#[link(name = "winsqlite3", kind = "raw-dylib")]
extern "system" {
    fn sqlite3_open_v2(file: *const c_char, db: *mut *mut c_void, flags: c_int, vfs: *const c_char) -> c_int;
    fn sqlite3_close_v2(db: *mut c_void) -> c_int;
    fn sqlite3_errmsg(db: *mut c_void) -> *const c_char;
    fn sqlite3_busy_timeout(db: *mut c_void, ms: c_int) -> c_int;
    fn sqlite3_prepare_v2(db: *mut c_void, sql: *const c_char, n: c_int, stmt: *mut *mut c_void, tail: *mut *const c_char) -> c_int;
    fn sqlite3_step(stmt: *mut c_void) -> c_int;
    fn sqlite3_reset(stmt: *mut c_void) -> c_int;
    fn sqlite3_finalize(stmt: *mut c_void) -> c_int;
    fn sqlite3_bind_text(stmt: *mut c_void, i: c_int, text: *const c_char, n: c_int, destructor: isize) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut c_void, i: c_int, v: i64) -> c_int;
    fn sqlite3_bind_null(stmt: *mut c_void, i: c_int) -> c_int;
    fn sqlite3_column_count(stmt: *mut c_void) -> c_int;
    fn sqlite3_column_type(stmt: *mut c_void, i: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut c_void, i: c_int) -> i64;
    fn sqlite3_column_text(stmt: *mut c_void, i: c_int) -> *const u8;
    fn sqlite3_column_bytes(stmt: *mut c_void, i: c_int) -> c_int;
}

const OK: c_int = 0;
const ROW: c_int = 100;
const DONE: c_int = 101;
const OPEN_READONLY: c_int = 0x1;
const OPEN_READWRITE: c_int = 0x2;
const OPEN_CREATE: c_int = 0x4;
const OPEN_FULLMUTEX: c_int = 0x10000;
const TYPE_INTEGER: c_int = 1;
const TYPE_NULL: c_int = 5;
/// SQLITE_TRANSIENT: SQLite copies the bound text before the call returns.
const TRANSIENT: isize = -1;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Text(String),
}

impl Value {
    pub fn int(&self) -> i64 {
        match self {
            Value::Int(v) => *v,
            Value::Text(t) => t.parse().unwrap_or(0),
            Value::Null => 0,
        }
    }
    pub fn text(&self) -> String {
        match self {
            Value::Text(t) => t.clone(),
            Value::Int(v) => v.to_string(),
            Value::Null => String::new(),
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Text(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Text(s)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

pub type Rows = Vec<Vec<Value>>;

/// One connection. Opened with SQLITE_OPEN_FULLMUTEX, so moving it to a worker thread is sound.
pub struct Db(*mut c_void);

// SAFETY: the connection is opened in serialized (FULLMUTEX) mode; SQLite itself guards it.
unsafe impl Send for Db {}

struct Stmt(*mut c_void);

impl Drop for Stmt {
    fn drop(&mut self) {
        // SAFETY: the statement came from sqlite3_prepare_v2 and is finalized exactly once.
        unsafe { sqlite3_finalize(self.0) };
    }
}

impl Db {
    pub fn open(path: &Path, read_only: bool) -> Result<Db, String> {
        let file = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let flags = OPEN_FULLMUTEX | if read_only { OPEN_READONLY } else { OPEN_READWRITE | OPEN_CREATE };
        let mut db = std::ptr::null_mut();
        // SAFETY: `file` is NUL-terminated UTF-8 (SQLite's expected encoding); `db` is a valid out-pointer.
        let rc = unsafe { sqlite3_open_v2(file.as_ptr(), &mut db, flags, std::ptr::null()) };
        let db = Db(db);
        if rc != OK {
            return Err(format!("opening {}: {}", path.display(), db.error()));
        }
        // SAFETY: db.0 is an open connection.
        unsafe { sqlite3_busy_timeout(db.0, 3000) };
        Ok(db)
    }

    fn error(&self) -> String {
        if self.0.is_null() {
            return "out of memory".into();
        }
        // SAFETY: errmsg returns a NUL-terminated string owned by the connection.
        unsafe { CStr::from_ptr(sqlite3_errmsg(self.0)) }.to_string_lossy().into_owned()
    }

    fn prepare(&self, sql: &str) -> Result<Stmt, String> {
        let c = CString::new(sql).map_err(|e| e.to_string())?;
        let mut stmt = std::ptr::null_mut();
        // SAFETY: `c` is NUL-terminated; -1 = read up to the terminator; out-pointer valid.
        let rc = unsafe { sqlite3_prepare_v2(self.0, c.as_ptr(), -1, &mut stmt, std::ptr::null_mut()) };
        if rc != OK || stmt.is_null() {
            return Err(format!("{}: {sql}", self.error()));
        }
        Ok(Stmt(stmt))
    }

    fn bind(&self, st: &Stmt, params: &[Value]) -> Result<(), String> {
        for (i, p) in params.iter().enumerate() {
            let idx = i as c_int + 1;
            // SAFETY: st.0 is a live statement; TRANSIENT makes SQLite copy text before returning.
            let rc = unsafe {
                match p {
                    Value::Null => sqlite3_bind_null(st.0, idx),
                    Value::Int(v) => sqlite3_bind_int64(st.0, idx, *v),
                    Value::Text(t) => {
                        sqlite3_bind_text(st.0, idx, t.as_ptr().cast(), t.len() as c_int, TRANSIENT)
                    }
                }
            };
            if rc != OK {
                return Err(self.error());
            }
        }
        Ok(())
    }

    fn step_all(&self, st: &Stmt, mut row: impl FnMut(&Stmt)) -> Result<(), String> {
        loop {
            // SAFETY: st.0 is a live, bound statement.
            match unsafe { sqlite3_step(st.0) } {
                ROW => row(st),
                DONE => return Ok(()),
                _ => return Err(self.error()),
            }
        }
    }

    /// Run one statement and collect every row.
    pub fn query(&self, sql: &str, params: &[Value]) -> Result<Rows, String> {
        let st = self.prepare(sql)?;
        self.bind(&st, params)?;
        let mut rows = Vec::new();
        self.step_all(&st, |st| {
            // SAFETY: called only while the statement points at a row; column indexes are < count;
            // column_text's buffer is valid for column_bytes bytes until the next step.
            let row = unsafe {
                (0..sqlite3_column_count(st.0))
                    .map(|i| match sqlite3_column_type(st.0, i) {
                        TYPE_NULL => Value::Null,
                        TYPE_INTEGER => Value::Int(sqlite3_column_int64(st.0, i)),
                        _ => {
                            let p = sqlite3_column_text(st.0, i);
                            let n = sqlite3_column_bytes(st.0, i) as usize;
                            if p.is_null() {
                                Value::Null
                            } else {
                                Value::Text(String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).into_owned())
                            }
                        }
                    })
                    .collect()
            };
            rows.push(row);
        })?;
        Ok(rows)
    }

    /// Run one statement that returns no rows.
    pub fn run(&self, sql: &str, params: &[Value]) -> Result<(), String> {
        self.query(sql, params).map(|_| ())
    }

    /// Run one statement once per parameter row, reusing the prepared statement.
    pub fn run_many(&self, sql: &str, rows: impl IntoIterator<Item = Vec<Value>>) -> Result<usize, String> {
        let st = self.prepare(sql)?;
        let mut n = 0;
        for params in rows {
            // SAFETY: resetting a live statement before re-binding it.
            unsafe { sqlite3_reset(st.0) };
            self.bind(&st, &params)?;
            self.step_all(&st, |_| {})?;
            n += 1;
        }
        Ok(n)
    }

    /// Run a script of `;`-separated statements without parameters.
    pub fn batch(&self, sql: &str) -> Result<(), String> {
        for stmt in sql.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            self.run(stmt, &[])?;
        }
        Ok(())
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        // SAFETY: closes the connection once; close_v2 defers if statements are still open.
        unsafe { sqlite3_close_v2(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let path = std::env::temp_dir().join(format!("kakangkung-sqlite-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Db::open(&path, false).expect("open");
        db.batch("CREATE TABLE t(a INTEGER, b TEXT); CREATE INDEX t_a ON t(a)").expect("schema");
        let n = db
            .run_many("INSERT INTO t VALUES(?, ?)", (0..3).map(|i| vec![Value::Int(i), Value::from(format!("v{i}"))]))
            .expect("insert");
        assert_eq!(n, 3);
        db.run("INSERT INTO t VALUES(?, ?)", &[Value::Int(9), Value::from("ünïcode ✓")]).expect("insert");
        let rows = db.query("SELECT a, b FROM t WHERE a >= ? ORDER BY a", &[Value::Int(2)]).expect("query");
        assert_eq!(rows, vec![vec![Value::Int(2), Value::from("v2")], vec![Value::Int(9), Value::from("ünïcode ✓")]]);
        assert!(db.run("SELECT nope FROM t", &[]).is_err());
        drop(db);
        let _ = std::fs::remove_file(&path);
    }
}
