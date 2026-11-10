use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::RustmanError;
use crate::models::collection::Collection;
use crate::models::cookie::Cookie;
use crate::cookies::CookieJar;
use crate::models::environment::Environment;
use crate::models::http::{HttpRequest, HttpResponse};

/// A logged request/response exchange.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub request_id: Option<Uuid>,
    pub request_name: Option<String>,
    pub url: String,
    pub method: String,
    pub status: u16,
    pub elapsed_ms: u64,
    pub request: HttpRequest,
    pub response: HttpResponse,
    pub created_at: DateTime<Utc>,
}

/// SQLite-backed storage for collections, environments and request history.
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS collections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    data TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS environments (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    data TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS history (
    id TEXT PRIMARY KEY,
    request_id TEXT,
    request_name TEXT,
    url TEXT NOT NULL,
    method TEXT NOT NULL,
    status INTEGER NOT NULL,
    elapsed_ms INTEGER NOT NULL,
    request_data TEXT NOT NULL,
    response_data TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS cookies (
    domain TEXT NOT NULL,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    expires TEXT,
    http_only INTEGER NOT NULL,
    secure INTEGER NOT NULL,
    PRIMARY KEY (domain, path, name)
);
CREATE INDEX IF NOT EXISTS idx_history_created_at ON history(created_at DESC);
";

fn parse_uuid(s: &str) -> Result<Uuid, RustmanError> {
    Uuid::parse_str(s).map_err(|e| RustmanError::StorageError(format!("invalid uuid: {e}")))
}

fn parse_time(s: &str) -> Result<DateTime<Utc>, RustmanError> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| RustmanError::StorageError(format!("invalid timestamp: {e}")))
}

impl SqliteStorage {
    fn from_connection(conn: Connection) -> Result<Self, RustmanError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn new_in_memory() -> Result<Self, RustmanError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, RustmanError> {
        Self::from_connection(Connection::open(path)?)
    }

    fn conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, RustmanError> {
        self.conn
            .lock()
            .map_err(|_| RustmanError::StorageError("connection lock poisoned".into()))
    }

    // ---- generic helpers for the collections / environments tables ----

    fn save_doc(
        &self,
        table: &str,
        id: &Uuid,
        name: &str,
        data: String,
    ) -> Result<(), RustmanError> {
        let sql = format!(
            "INSERT INTO {table} (id, name, data, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, data = excluded.data,
             updated_at = excluded.updated_at"
        );
        self.conn()?
            .execute(&sql, params![id.to_string(), name, data, Utc::now().to_rfc3339()])?;
        Ok(())
    }

    fn get_doc(&self, table: &str, id: &Uuid) -> Result<Option<String>, RustmanError> {
        let sql = format!("SELECT data FROM {table} WHERE id = ?1");
        Ok(self
            .conn()?
            .query_row(&sql, params![id.to_string()], |r| r.get::<_, String>(0))
            .optional()?)
    }

    fn list_docs(&self, table: &str) -> Result<Vec<String>, RustmanError> {
        let sql = format!("SELECT data FROM {table} ORDER BY name COLLATE NOCASE, id");
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn delete_doc(&self, table: &str, id: &Uuid) -> Result<bool, RustmanError> {
        let sql = format!("DELETE FROM {table} WHERE id = ?1");
        Ok(self.conn()?.execute(&sql, params![id.to_string()])? > 0)
    }

    // ---- collections ----

    pub fn save_collection(&self, collection: &Collection) -> Result<(), RustmanError> {
        let data = serde_json::to_string(collection)?;
        self.save_doc("collections", &collection.id, &collection.name, data)
    }

    pub fn get_collection(&self, id: &Uuid) -> Result<Option<Collection>, RustmanError> {
        self.get_doc("collections", id)?
            .map(|d| serde_json::from_str(&d).map_err(Into::into))
            .transpose()
    }

    pub fn list_collections(&self) -> Result<Vec<Collection>, RustmanError> {
        self.list_docs("collections")?
            .iter()
            .map(|d| serde_json::from_str(d).map_err(Into::into))
            .collect()
    }

    pub fn delete_collection(&self, id: &Uuid) -> Result<bool, RustmanError> {
        self.delete_doc("collections", id)
    }

    // ---- environments ----

    pub fn save_environment(&self, env: &Environment) -> Result<(), RustmanError> {
        let data = serde_json::to_string(env)?;
        self.save_doc("environments", &env.id, &env.name, data)
    }

    pub fn get_environment(&self, id: &Uuid) -> Result<Option<Environment>, RustmanError> {
        self.get_doc("environments", id)?
            .map(|d| serde_json::from_str(&d).map_err(Into::into))
            .transpose()
    }

    pub fn list_environments(&self) -> Result<Vec<Environment>, RustmanError> {
        self.list_docs("environments")?
            .iter()
            .map(|d| serde_json::from_str(d).map_err(Into::into))
            .collect()
    }

    pub fn delete_environment(&self, id: &Uuid) -> Result<bool, RustmanError> {
        self.delete_doc("environments", id)
    }

    // ---- history ----

    /// Replace all persisted cookies with the (non-expired) contents of `jar`.
    pub fn save_cookies(&self, jar: &CookieJar) -> Result<(), RustmanError> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM cookies", [])?;
        for c in jar.get_cookies() {
            tx.execute(
                "INSERT OR REPLACE INTO cookies (domain, path, name, value, expires, http_only, secure)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    c.domain,
                    c.path,
                    c.name,
                    c.value,
                    c.expires.map(|e| e.to_rfc3339()),
                    c.http_only,
                    c.secure
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Load persisted cookies, skipping expired ones.
    pub fn load_cookies(&self) -> Result<Vec<Cookie>, RustmanError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT domain, path, name, value, expires, http_only, secure FROM cookies")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, bool>(6)?,
            ))
        })?;
        let now = Utc::now();
        let mut out = Vec::new();
        for row in rows {
            let (domain, path, name, value, expires, http_only, secure) = row?;
            let expires = expires.as_deref().map(parse_time).transpose()?;
            if expires.is_some_and(|e| e <= now) {
                continue;
            }
            out.push(Cookie { name, value, domain, path, expires, http_only, secure });
        }
        Ok(out)
    }

    /// Build a [`CookieJar`] pre-populated from persisted cookies.
    pub fn load_cookie_jar(&self) -> Result<CookieJar, RustmanError> {
        let jar = CookieJar::new();
        for c in self.load_cookies()? {
            jar.add_cookie(c);
        }
        Ok(jar)
    }

    pub fn save_history(
        &self,
        req: &HttpRequest,
        res: &HttpResponse,
    ) -> Result<Uuid, RustmanError> {
        let id = Uuid::new_v4();
        let method = serde_json::to_value(req.method)?
            .as_str()
            .unwrap_or_default()
            .to_string();
        let request_data = serde_json::to_string(req)?;
        let response_data = serde_json::to_string(res)?;
        self.conn()?.execute(
            "INSERT INTO history (id, request_id, request_name, url, method, status, elapsed_ms,
             request_data, response_data, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id.to_string(),
                req.id.to_string(),
                req.name,
                req.url,
                method,
                res.status as i64,
                res.elapsed_ms as i64,
                request_data,
                response_data,
                Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
            ],
        )?;
        Ok(id)
    }

    /// Returns the most recent entries first.
    pub fn list_history(&self, limit: usize) -> Result<Vec<HistoryEntry>, RustmanError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, request_id, request_name, url, method, status, elapsed_ms,
                    request_data, response_data, created_at
             FROM history ORDER BY created_at DESC, rowid DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, rid, rname, url, method, status, elapsed, req, res, created) = row?;
            out.push(HistoryEntry {
                id: parse_uuid(&id)?,
                request_id: rid.as_deref().map(parse_uuid).transpose()?,
                request_name: rname,
                url,
                method,
                status: status as u16,
                elapsed_ms: elapsed as u64,
                request: serde_json::from_str(&req)?,
                response: serde_json::from_str(&res)?,
                created_at: parse_time(&created)?,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::collection::{Folder, Item};
    use crate::models::environment::Variable;
    use crate::models::http::{HeaderPair, HttpMethod};

    fn sample_collection(name: &str) -> Collection {
        let req = HttpRequest::new("Get users", HttpMethod::GET, "https://example.com/users");
        Collection {
            id: Uuid::new_v4(),
            name: name.into(),
            description: Some("desc".into()),
            items: vec![
                Item::Request(req.clone()),
                Item::Folder(Folder {
                    id: Uuid::new_v4(),
                    name: "sub".into(),
                    description: None,
                    items: vec![Item::Request(req)],
                }),
            ],
        }
    }

    fn sample_env(name: &str) -> Environment {
        Environment {
            id: Uuid::new_v4(),
            name: name.into(),
            variables: vec![Variable {
                key: "host".into(),
                value: "localhost".into(),
                enabled: true,
                secret: false,
            }],
        }
    }

    fn sample_response(status: u16) -> HttpResponse {
        HttpResponse {
            status,
            headers: vec![HeaderPair {
                key: "content-type".into(),
                value: "application/json".into(),
                enabled: true,
            }],
            body: "{}".into(),
            elapsed_ms: 42,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn collection_save_get_roundtrip() {
        let s = SqliteStorage::new_in_memory().unwrap();
        let c = sample_collection("API");
        s.save_collection(&c).unwrap();
        assert_eq!(s.get_collection(&c.id).unwrap(), Some(c));
        assert_eq!(s.get_collection(&Uuid::new_v4()).unwrap(), None);
    }

    #[test]
    fn collection_upsert_list_delete() {
        let s = SqliteStorage::new_in_memory().unwrap();
        let mut a = sample_collection("beta");
        let b = sample_collection("Alpha");
        s.save_collection(&a).unwrap();
        s.save_collection(&b).unwrap();
        a.name = "gamma".into();
        s.save_collection(&a).unwrap();

        let list = s.list_collections().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Alpha");
        assert_eq!(list[1].name, "gamma");

        assert!(s.delete_collection(&a.id).unwrap());
        assert!(!s.delete_collection(&a.id).unwrap());
        assert_eq!(s.list_collections().unwrap().len(), 1);
    }

    #[test]
    fn environment_crud() {
        let s = SqliteStorage::new_in_memory().unwrap();
        let mut e = sample_env("dev");
        s.save_environment(&e).unwrap();
        assert_eq!(s.get_environment(&e.id).unwrap(), Some(e.clone()));

        e.variables[0].value = "prod.example.com".into();
        s.save_environment(&e).unwrap();
        assert_eq!(s.get_environment(&e.id).unwrap().unwrap(), e);
        assert_eq!(s.list_environments().unwrap().len(), 1);

        assert!(s.delete_environment(&e.id).unwrap());
        assert!(!s.delete_environment(&e.id).unwrap());
        assert_eq!(s.get_environment(&e.id).unwrap(), None);
    }

    #[test]
    fn environment_list_sorted_by_name() {
        let s = SqliteStorage::new_in_memory().unwrap();
        s.save_environment(&sample_env("staging")).unwrap();
        s.save_environment(&sample_env("dev")).unwrap();
        let names: Vec<_> = s.list_environments().unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["dev", "staging"]);
    }

    #[test]
    fn history_save_and_list() {
        let s = SqliteStorage::new_in_memory().unwrap();
        let req = HttpRequest::new("Ping", HttpMethod::POST, "https://example.com/ping");
        let res = sample_response(201);
        let id = s.save_history(&req, &res).unwrap();

        let h = s.list_history(10).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].id, id);
        assert_eq!(h[0].request_id, Some(req.id));
        assert_eq!(h[0].request_name.as_deref(), Some("Ping"));
        assert_eq!(h[0].method, "POST");
        assert_eq!(h[0].url, "https://example.com/ping");
        assert_eq!(h[0].status, 201);
        assert_eq!(h[0].elapsed_ms, 42);
        assert_eq!(h[0].request, req);
        assert_eq!(h[0].response, res);
    }

    #[test]
    fn history_limit_and_order() {
        let s = SqliteStorage::new_in_memory().unwrap();
        let mut ids = Vec::new();
        for i in 0..5 {
            let req = HttpRequest::new(format!("r{i}"), HttpMethod::GET, "https://example.com");
            ids.push(s.save_history(&req, &sample_response(200)).unwrap());
        }
        let h = s.list_history(3).unwrap();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].id, ids[4]);
        assert_eq!(h[2].id, ids[2]);
        assert!(s.list_history(0).unwrap().is_empty());
    }

    #[test]
    fn file_backed_persistence() {
        let dir = std::env::temp_dir().join(format!("rustman-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("db.sqlite");
        let c = sample_collection("persist");
        {
            let s = SqliteStorage::new(&path).unwrap();
            s.save_collection(&c).unwrap();
        }
        let s = SqliteStorage::new(&path).unwrap();
        assert_eq!(s.get_collection(&c.id).unwrap(), Some(c));
        drop(s);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cookies_roundtrip() {
        let st = SqliteStorage::new_in_memory().unwrap();
        let jar = CookieJar::new();
        jar.parse_and_store("https://h.com/a", &["s=1; Secure; HttpOnly; Max-Age=3600".to_string()]);
        st.save_cookies(&jar).unwrap();
        let loaded = st.load_cookie_jar().unwrap();
        assert_eq!(loaded.get_cookies().len(), 1);
        let c = &loaded.get_cookies()[0];
        assert!(c.secure && c.http_only && c.expires.is_some());
    }
}
