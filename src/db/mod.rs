pub mod models;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::path::Path;

use crate::state::DbPool;

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "001_initial",
        include_str!("../../migrations/001_initial.sql"),
    ),
    (
        "002_social_accounts",
        include_str!("../../migrations/002_social_accounts.sql"),
    ),
    (
        "003_services",
        include_str!("../../migrations/003_services.sql"),
    ),
    (
        "004_passkey_json",
        include_str!("../../migrations/004_passkey_json.sql"),
    ),
    (
        "005_mesh_network",
        include_str!("../../migrations/005_mesh_network.sql"),
    ),
];

pub fn create_pool(db_path: &Path) -> anyhow::Result<DbPool> {
    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let manager = SqliteConnectionManager::file(db_path);
    let pool = Pool::builder().max_size(8).build(manager)?;

    // Configure SQLite for performance
    let conn = pool.get()?;
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        ",
    )?;

    Ok(pool)
}

pub fn run_migrations(pool: &DbPool) -> anyhow::Result<()> {
    let conn = pool.get()?;

    // Create migrations tracking table
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

    // Ensure the "owner" user exists (for single-user mode)
    conn.execute(
        "INSERT OR IGNORE INTO users (id, username, is_admin) VALUES ('owner', 'owner', 1)",
        [],
    )?;

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
        // Verify we can get a connection
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

        // Verify schema_version tracks all 3 migrations
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 4);

        // Verify key tables exist
        let tables: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"posts".to_string()));
        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"social_accounts".to_string()));
        assert!(tables.contains(&"services".to_string()));
        assert!(tables.contains(&"config".to_string()));
    }

    #[test]
    fn migrations_are_idempotent() {
        let pool = test_pool();
        run_migrations(&pool).unwrap();
        run_migrations(&pool).unwrap(); // Should not error on second run

        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 4);
    }

    #[test]
    fn users_table_has_correct_schema() {
        let pool = test_pool();
        run_migrations(&pool).unwrap();

        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO users (id, username, is_admin) VALUES (?1, ?2, ?3)",
            params!["test-id", "alice", true],
        )
        .unwrap();

        let (username, is_admin): (String, bool) = conn
            .query_row(
                "SELECT username, is_admin FROM users WHERE id = ?1",
                params!["test-id"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(username, "alice");
        assert!(is_admin);
    }

    #[test]
    fn foreign_keys_enforced() {
        let pool = test_pool();
        run_migrations(&pool).unwrap();

        let conn = pool.get().unwrap();
        // Inserting a post with a non-existent user_id should fail
        let result = conn.execute(
            "INSERT INTO posts (id, user_id, body) VALUES (?1, ?2, ?3)",
            params!["post-1", "nonexistent-user", "hello"],
        );
        assert!(result.is_err());
    }
}
