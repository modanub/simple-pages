use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::error::AppError;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub is_admin: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InviteCode {
    pub code: String,
    pub created_at: String,
    pub used_by: Option<String>,
    pub used_at: Option<String>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let conn = Connection::open(path)
            .map_err(|e| AppError::Internal(format!("Failed to open database: {e}")))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| AppError::Internal(format!("Failed to set pragmas: {e}")))?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<(), AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                is_admin INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS invite_codes (
                code TEXT PRIMARY KEY,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                used_by TEXT REFERENCES users(username),
                used_at TEXT
            );",
        )?;
        Ok(())
    }

    pub fn get_user_by_username(&self, username: &str) -> Result<Option<User>, AppError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, username, password_hash, is_admin, created_at FROM users WHERE username = ?1",
        )?;
        let user = stmt
            .query_row(params![username], |row| {
                Ok(User {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    is_admin: row.get::<_, i32>(3)? != 0,
                    created_at: row.get(4)?,
                })
            })
            .optional()?;
        Ok(user)
    }

    pub fn create_invite_code(&self, code: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO invite_codes (code) VALUES (?1)", params![code])?;
        Ok(())
    }

    pub fn register_user(
        &self,
        username: &str,
        password_hash: &str,
        invite_code: &str,
    ) -> Result<i64, AppError> {
        let conn = self.conn.lock().unwrap();

        // Check invite code exists and is unused
        let code_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM invite_codes WHERE code = ?1 AND used_by IS NULL",
                params![invite_code],
                |row| row.get::<_, i32>(0),
            )
            .map(|c| c > 0)?;

        if !code_exists {
            return Err(AppError::BadRequest(
                "Invalid or already used invite code".to_string(),
            ));
        }

        // Create user first (so FK is satisfied)
        conn.execute(
            "INSERT INTO users (username, password_hash) VALUES (?1, ?2)",
            params![username, password_hash],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                AppError::Conflict("Username already taken".to_string())
            }
            other => AppError::Internal(other.to_string()),
        })?;

        let user_id = conn.last_insert_rowid();

        // Now mark the invite code as used
        conn.execute(
            "UPDATE invite_codes SET used_by = ?1, used_at = datetime('now') WHERE code = ?2 AND used_by IS NULL",
            params![username, invite_code],
        )?;

        Ok(user_id)
    }

    pub fn list_invite_codes(&self) -> Result<Vec<InviteCode>, AppError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT code, created_at, used_by, used_at FROM invite_codes ORDER BY created_at DESC")?;
        let codes = stmt
            .query_map([], |row| {
                Ok(InviteCode {
                    code: row.get(0)?,
                    created_at: row.get(1)?,
                    used_by: row.get(2)?,
                    used_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(codes)
    }

    pub fn delete_invite_code(&self, code: &str) -> Result<bool, AppError> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute(
            "DELETE FROM invite_codes WHERE code = ?1 AND used_by IS NULL",
            params![code],
        )?;
        Ok(rows > 0)
    }
}

trait OptionalRow {
    fn optional(self) -> Result<Option<User>, rusqlite::Error>;
}

impl OptionalRow for Result<User, rusqlite::Error> {
    fn optional(self) -> Result<Option<User>, rusqlite::Error> {
        match self {
            Ok(user) => Ok(Some(user)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
