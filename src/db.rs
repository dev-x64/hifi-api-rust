use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub async fn init_pool(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS accounts (
            id TEXT PRIMARY KEY,
            label TEXT NOT NULL DEFAULT '',
            client_id TEXT NOT NULL,
            client_secret TEXT NOT NULL,
            refresh_token TEXT NOT NULL,
            user_id TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            auto_disabled INTEGER NOT NULL DEFAULT 0,
            disabled_at INTEGER NOT NULL DEFAULT 0,
            is_catalog INTEGER NOT NULL DEFAULT 0,
            notes TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    // Migrate pre-existing databases that lack the column.
    let cols: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(accounts)")
            .fetch_all(&pool)
            .await?;
    if !cols.iter().any(|c| c.1 == "auto_disabled") {
        sqlx::query("ALTER TABLE accounts ADD COLUMN auto_disabled INTEGER NOT NULL DEFAULT 0")
            .execute(&pool)
            .await?;
        tracing::info!("Migrated accounts table: added auto_disabled");
    }
    if !cols.iter().any(|c| c.1 == "disabled_at") {
        sqlx::query("ALTER TABLE accounts ADD COLUMN disabled_at INTEGER NOT NULL DEFAULT 0")
            .execute(&pool)
            .await?;
        tracing::info!("Migrated accounts table: added disabled_at");
    }
    if !cols.iter().any(|c| c.1 == "is_catalog") {
        sqlx::query("ALTER TABLE accounts ADD COLUMN is_catalog INTEGER NOT NULL DEFAULT 0")
            .execute(&pool)
            .await?;
        tracing::info!("Migrated accounts table: added is_catalog");
    }
    // Recovery deadlines are absolute timestamps so restarts keep the
    // remaining cooldown, rather than starting a new retry cycle.
    for (name, definition) in [
        ("heal_failures", "INTEGER NOT NULL DEFAULT 0"),
        ("heal_next_retry", "INTEGER NOT NULL DEFAULT 0"),
        ("rejected_access_token", "TEXT"),
        ("last_refresh_error", "TEXT"),
    ] {
        if !cols.iter().any(|c| c.1 == name) {
            sqlx::query(&format!("ALTER TABLE accounts ADD COLUMN {name} {definition}"))
                .execute(&pool)
                .await?;
        }
    }

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tokens (
            account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
            access_token TEXT NOT NULL,
            expires_at INTEGER NOT NULL,
            refreshed_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS account_metrics (
            account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
            request_count INTEGER NOT NULL DEFAULT 0,
            error_count INTEGER NOT NULL DEFAULT 0,
            rate_limit_hits INTEGER NOT NULL DEFAULT 0,
            last_used_at INTEGER,
            last_error_at INTEGER,
            last_error_message TEXT
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(include_str!("../migrations/002_account_uptime.sql"))
        .execute(&pool)
        .await?;

    let uptime_cols: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(account_uptime_events)")
            .fetch_all(&pool)
            .await?;
    if !uptime_cols.iter().any(|c| c.1 == "token_ready") {
        // Existing events described pool state only; preserve their history.
        sqlx::query("ALTER TABLE account_uptime_events ADD COLUMN token_ready INTEGER NOT NULL DEFAULT 1 CHECK (token_ready IN (0, 1))")
            .execute(&pool).await?;
    }
    if !uptime_cols.iter().any(|c| c.1 == "token_expires_at") {
        sqlx::query("ALTER TABLE account_uptime_events ADD COLUMN token_expires_at INTEGER")
            .execute(&pool).await?;
    }

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS daily_usage (
            account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
            day INTEGER NOT NULL,
            count INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS api_keys (
            id TEXT PRIMARY KEY,
            label TEXT NOT NULL DEFAULT '',
            key_hash TEXT NOT NULL UNIQUE,
            key_prefix TEXT NOT NULL DEFAULT '',
            quota INTEGER NOT NULL DEFAULT 0,
            used INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS proxy_assignments (
            account_id TEXT PRIMARY KEY,
            proxy_url TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn legacy_database_gains_refresh_state_without_changing_credentials() {
        let path = std::env::temp_dir().join(format!("hifi-migrate-{}.db", uuid::Uuid::new_v4()));
        let url = format!("sqlite://{}", path.display());
        let db = SqlitePool::connect_with(
            SqliteConnectOptions::from_str(&url)
                .unwrap()
                .create_if_missing(true),
        )
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE accounts (
                id TEXT PRIMARY KEY, label TEXT NOT NULL DEFAULT '', client_id TEXT NOT NULL,
                client_secret TEXT NOT NULL, refresh_token TEXT NOT NULL, user_id TEXT,
                is_active INTEGER NOT NULL DEFAULT 1, notes TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO accounts (id, client_id, client_secret, refresh_token, created_at, updated_at)
            VALUES ('a', 'c', 's', 'r', 1, 1)").execute(&db).await.unwrap();
        db.close().await;
        let db = init_pool(&url).await.unwrap();
        let state: (String, i64, i64, Option<String>) = sqlx::query_as(
            "SELECT refresh_token, heal_failures, heal_next_retry, rejected_access_token FROM accounts WHERE id = 'a'",
        ).fetch_one(&db).await.unwrap();
        assert_eq!(state, ("r".into(), 0, 0, None));
        sqlx::query("UPDATE accounts SET heal_failures = 3, heal_next_retry = 123456, rejected_access_token = 'bad'")
            .execute(&db).await.unwrap();
        db.close().await;
        let db = init_pool(&url).await.unwrap();
        let state: (i64, i64, Option<String>) = sqlx::query_as(
            "SELECT heal_failures, heal_next_retry, rejected_access_token FROM accounts WHERE id = 'a'",
        ).fetch_one(&db).await.unwrap();
        assert_eq!(state, (3, 123456, Some("bad".into())));
        db.close().await;
        std::fs::remove_file(path).unwrap();
    }
}
