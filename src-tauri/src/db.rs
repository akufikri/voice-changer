use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use std::path::PathBuf;

fn db_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::env::temp_dir())
        .join("voice-changer")
        .join("credentials.db")
}

fn open() -> Result<Connection> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS credentials (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;
    Ok(conn)
}

pub fn set(key: &str, value: &str) -> Result<()> {
    let conn = open()?;
    conn.execute(
        "INSERT OR REPLACE INTO credentials (key, value) VALUES (?1, ?2)",
        [key, value],
    )?;
    Ok(())
}

pub fn get(key: &str) -> Result<Option<String>> {
    let conn = open()?;
    let mut stmt = conn.prepare("SELECT value FROM credentials WHERE key = ?1")?;
    let result = stmt
        .query_row([key], |row| row.get::<_, String>(0))
        .optional()?;
    Ok(result)
}
