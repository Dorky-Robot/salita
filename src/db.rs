use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::path::Path;

pub type DbPool = Pool<SqliteConnectionManager>;

pub const MIGRATIONS: &[(&str, &str)] = &[(
    "001_init",
    include_str!("../migrations/001_init.sql"),
)];

pub fn create_pool(db_path: &Path) -> anyhow::Result<DbPool> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let manager = SqliteConnectionManager::file(db_path);
    let pool = Pool::builder().max_size(8).build(manager)?;

    let conn = pool.get()?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;

    Ok(pool)
}

pub fn run_migrations(pool: &DbPool) -> anyhow::Result<()> {
    let conn = pool.get()?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    for (name, sql) in MIGRATIONS {
        let already_applied: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM schema_version WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )?;

        if !already_applied {
            tracing::info!("Applying migration: {}", name);
            conn.execute_batch(sql)?;
            conn.execute(
                "INSERT INTO schema_version (name) VALUES (?1)",
                params![name],
            )?;
        }
    }

    tracing::info!("Database migrations complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pool() -> DbPool {
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::builder().max_size(1).build(manager).unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;",
        )
        .unwrap();
        pool
    }

    #[test]
    fn create_pool_creates_db_file() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("sub/dir/test.db");
        let pool = create_pool(&db_path).unwrap();
        assert!(db_path.exists());
        let conn = pool.get().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn migrations_run_successfully() {
        let pool = test_pool();
        run_migrations(&pool).unwrap();

        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let tables: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        assert!(tables.contains(&"devices".to_string()));
        assert!(tables.contains(&"current_node".to_string()));
    }

    #[test]
    fn migrations_are_idempotent() {
        let pool = test_pool();
        run_migrations(&pool).unwrap();
        run_migrations(&pool).unwrap();

        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
