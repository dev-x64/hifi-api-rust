//! Track genres kept in their own SQLite file next to the main database, so
//! they never bloat backups or hold locks the account tables need.
//!
//! Track genres do not change, so a row lives as long as it is used: the
//! retention period counts from the last request, not from the fetch. Writes
//! (new rows, use counters) are buffered in memory and flushed in one
//! transaction every 30 seconds. Over the size cap, the rows that are least
//! used and longest idle go first, then the file is compacted.

use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use tokio::sync::Notify;

use crate::settings::AppSettings;

const FLUSH_EVERY: Duration = Duration::from_secs(30);
/// Pending rows or use counters that trigger an early flush.
const FLUSH_AT: usize = 10_000;
/// Rows per INSERT statement (4 bound values each).
const ROWS_PER_INSERT: usize = 200;
const PURGE_EVERY_SECS: i64 = 6 * 3600;
/// Eviction shrinks the cache to this share of the cap, so it does not run
/// again on the next few flushes.
const EVICT_TO: f64 = 0.85;
const DAY_SECS: i64 = 86_400;

/// Genres of one track as the cache stores them: Tidal genre ids.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub track_id: i64,
    pub isrc: Option<String>,
    pub genre_ids: Vec<String>,
}

/// A cached track with genre names resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Cached {
    pub track_id: i64,
    pub isrc: Option<String>,
    pub genres: Vec<String>,
}

#[derive(Default)]
struct Pending {
    /// New or refetched rows with the uses they gathered while pending.
    rows: HashMap<i64, (Entry, i64)>,
    /// Uses of rows already on disk.
    uses: HashMap<i64, i64>,
    names: HashMap<String, String>,
    /// ISRC → track id of `rows`, so a lookup costs O(requested values)
    /// however many rows are pending.
    by_isrc: HashMap<String, i64>,
}

impl Pending {
    fn insert_row(&mut self, entry: Entry, uses: i64) {
        if let Some(isrc) = &entry.isrc {
            self.by_isrc.insert(isrc.clone(), entry.track_id);
        }
        if let Some((old, _)) = self.rows.insert(entry.track_id, (entry, uses)) {
            self.forget_isrc(&old);
        }
    }

    fn remove_row(&mut self, track_id: i64) {
        if let Some((old, _)) = self.rows.remove(&track_id) {
            self.forget_isrc(&old);
        }
    }

    /// Drops the index entry of a row that left `rows`, unless the ISRC now
    /// points at another pending row.
    fn forget_isrc(&mut self, old: &Entry) {
        let Some(isrc) = &old.isrc else { return };
        let still_pending = self.rows.get(&old.track_id).is_some_and(|(e, _)| e.isrc.as_ref() == Some(isrc));
        if !still_pending && self.by_isrc.get(isrc) == Some(&old.track_id) {
            self.by_isrc.remove(isrc);
        }
    }

    /// The pending row for a requested ISRC or track id.
    fn row_mut(&mut self, by_isrc: bool, value: &str) -> Option<&mut (Entry, i64)> {
        let track_id = if by_isrc { *self.by_isrc.get(value)? } else { value.parse().ok()? };
        self.rows.get_mut(&track_id)
    }
}

pub struct GenreCache {
    pool: SqlitePool,
    settings: Arc<AppSettings>,
    location: String,
    names: RwLock<HashMap<String, String>>,
    pending: Mutex<Pending>,
    flush_now: Notify,
    /// Tracks answered from the cache / fetched from Tidal.
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub evicted: AtomicU64,
    pub expired: AtomicU64,
}

fn today() -> i64 {
    Utc::now().timestamp() / DAY_SECS
}

/// `hifi.db` → `hifi-genres.db` in the same folder. In-memory or no main
/// database → None (the genre cache then lives in memory too).
pub fn file_for(database_url: &str) -> Option<String> {
    if database_url.is_empty() || database_url == "ephemeral" {
        return None;
    }
    let mut path = database_url;
    for prefix in ["sqlite://", "sqlite:"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            path = rest;
            break;
        }
    }
    let path = path.split('?').next().unwrap_or("");
    if path.is_empty() || path == ":memory:" {
        return None;
    }
    let path = Path::new(path);
    let stem = path.file_stem()?.to_string_lossy();
    Some(path.with_file_name(format!("{stem}-genres.db")).to_string_lossy().into_owned())
}

impl GenreCache {
    /// Opens (or creates) the cache file and starts its flush loop.
    pub async fn open(
        file: Option<&str>,
        settings: Arc<AppSettings>,
    ) -> Result<Arc<Self>, sqlx::Error> {
        let pool = match file {
            Some(file) => {
                let options = SqliteConnectOptions::new()
                    .filename(file)
                    .create_if_missing(true)
                    // A cache: losing the last flush on power loss is fine.
                    .journal_mode(SqliteJournalMode::Wal)
                    .synchronous(SqliteSynchronous::Normal)
                    .pragma("journal_size_limit", (8 * 1024 * 1024).to_string());
                SqlitePoolOptions::new().max_connections(4).connect_with(options).await?
            }
            // One connection that never closes keeps the in-memory database.
            None => {
                SqlitePoolOptions::new()
                    .max_connections(1)
                    .min_connections(1)
                    .idle_timeout(None)
                    .max_lifetime(None)
                    .connect_with(SqliteConnectOptions::from_str("sqlite::memory:")?)
                    .await?
            }
        };
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS genre_names (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            ) WITHOUT ROWID",
        )
        .execute(&pool)
        .await?;
        // last_used is a day number; hits counts requests for the track.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS track_genres (
                track_id INTEGER PRIMARY KEY,
                isrc TEXT,
                genre_ids TEXT NOT NULL,
                hits INTEGER NOT NULL DEFAULT 0,
                last_used INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS track_genres_isrc ON track_genres(isrc)")
            .execute(&pool)
            .await?;
        let names: Vec<(String, String)> = sqlx::query_as("SELECT id, name FROM genre_names")
            .fetch_all(&pool)
            .await?;
        let cache = Arc::new(Self {
            pool,
            settings,
            location: file.unwrap_or("memory").to_string(),
            names: RwLock::new(names.into_iter().collect()),
            pending: Mutex::new(Pending::default()),
            flush_now: Notify::new(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evicted: AtomicU64::new(0),
            expired: AtomicU64::new(0),
        });
        tokio::spawn(Self::run(Arc::downgrade(&cache)));
        Ok(cache)
    }

    pub fn enabled(&self) -> bool {
        self.settings.genre_cache_days() > 0
    }

    /// Cached tracks for the given ISRCs (`by_isrc`) or track ids, keyed by
    /// the requested value. Counts a use for every track found.
    pub async fn lookup(&self, by_isrc: bool, values: &[String]) -> HashMap<String, Cached> {
        let mut found: HashMap<String, Entry> = HashMap::new();
        if !self.enabled() || values.is_empty() {
            return HashMap::new();
        }
        {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            for value in values {
                if found.contains_key(value) {
                    continue;
                }
                if let Some((entry, uses)) = pending.row_mut(by_isrc, value) {
                    *uses += 1;
                    found.insert(value.clone(), entry.clone());
                }
            }
        }
        let rest: Vec<&String> = values.iter().filter(|v| !found.contains_key(*v)).collect();
        if !rest.is_empty() {
            let cutoff = today() - i64::from(self.settings.genre_cache_days());
            let column = if by_isrc { "isrc" } else { "track_id" };
            let mut query = QueryBuilder::<Sqlite>::new(
                "SELECT track_id, isrc, genre_ids FROM track_genres WHERE last_used > ",
            );
            query.push_bind(cutoff).push(format!(" AND {column} IN ("));
            let mut list = query.separated(", ");
            for value in &rest {
                if by_isrc {
                    list.push_bind(value.as_str());
                } else {
                    list.push_bind(value.parse::<i64>().unwrap_or(-1));
                }
            }
            query.push(")");
            match query
                .build_query_as::<(i64, Option<String>, String)>()
                .fetch_all(&self.pool)
                .await
            {
                Ok(rows) => {
                    let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
                    for (track_id, isrc, genre_ids) in rows {
                        let key = if by_isrc { isrc.clone() } else { Some(track_id.to_string()) };
                        let Some(key) = key else { continue };
                        if found.contains_key(&key) {
                            continue;
                        }
                        *pending.uses.entry(track_id).or_insert(0) += 1;
                        found.insert(
                            key,
                            Entry { track_id, isrc, genre_ids: split_ids(&genre_ids) },
                        );
                    }
                    if pending.uses.len() >= FLUSH_AT {
                        self.flush_now.notify_one();
                    }
                }
                Err(e) => tracing::warn!("Genre cache lookup failed: {}", e),
            }
        }
        let names = self.names.read().unwrap_or_else(|e| e.into_inner());
        found
            .into_iter()
            .map(|(key, entry)| {
                let genres = entry
                    .genre_ids
                    .iter()
                    .filter_map(|id| names.get(id).cloned())
                    .collect();
                (key, Cached { track_id: entry.track_id, isrc: entry.isrc, genres })
            })
            .collect()
    }

    /// Queues fetched tracks for the next flush. `names` maps genre id → name.
    pub fn store(&self, entries: Vec<Entry>, names: HashMap<String, String>) {
        if !self.enabled() || (entries.is_empty() && names.is_empty()) {
            return;
        }
        {
            let mut known = self.names.write().unwrap_or_else(|e| e.into_inner());
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            for (id, name) in names {
                if known.get(&id) != Some(&name) {
                    known.insert(id.clone(), name.clone());
                    pending.names.insert(id, name);
                }
            }
            for entry in entries {
                // The fetch itself is a use.
                let uses = pending.uses.remove(&entry.track_id).unwrap_or(0) + 1;
                pending.insert_row(entry, uses);
            }
            if pending.rows.len() < FLUSH_AT {
                return;
            }
        }
        self.flush_now.notify_one();
    }

    async fn run(cache: std::sync::Weak<Self>) {
        let mut last_purge = 0;
        loop {
            let Some(notify) = cache.upgrade() else { return };
            let wait = async {
                tokio::select! {
                    _ = notify.flush_now.notified() => {}
                    _ = tokio::time::sleep(FLUSH_EVERY) => {}
                }
            };
            wait.await;
            drop(notify);
            let Some(cache) = cache.upgrade() else { return };
            match cache.flush().await {
                Ok(true) => {
                    if let Err(e) = cache.enforce_size().await {
                        tracing::warn!("Genre cache size check failed: {}", e);
                    }
                }
                Ok(false) => {}
                Err(e) => tracing::warn!("Genre cache flush failed: {}", e),
            }
            let now = Utc::now().timestamp();
            if now - last_purge >= PURGE_EVERY_SECS {
                last_purge = now;
                if let Err(e) = cache.purge_expired().await {
                    tracing::warn!("Genre cache purge failed: {}", e);
                }
            }
        }
    }

    /// Writes everything pending in one transaction. Rows stay readable from
    /// memory until they are committed. Returns whether anything was written.
    pub async fn flush(&self) -> Result<bool, sqlx::Error> {
        let (rows, uses, names) = {
            let pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            if pending.rows.is_empty() && pending.uses.is_empty() && pending.names.is_empty() {
                return Ok(false);
            }
            (pending.rows.clone(), pending.uses.clone(), pending.names.clone())
        };
        let day = today();
        let mut tx = self.pool.begin().await?;
        if !names.is_empty() {
            let mut query = QueryBuilder::<Sqlite>::new("INSERT INTO genre_names (id, name) ");
            query.push_values(&names, |mut row, (id, name)| {
                row.push_bind(id).push_bind(name);
            });
            query.push(" ON CONFLICT(id) DO UPDATE SET name = excluded.name");
            query.build().execute(&mut *tx).await?;
        }
        let row_list: Vec<&(Entry, i64)> = rows.values().collect();
        for chunk in row_list.chunks(ROWS_PER_INSERT) {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO track_genres (track_id, isrc, genre_ids, hits, last_used) ",
            );
            query.push_values(chunk, |mut row, (entry, uses)| {
                row.push_bind(entry.track_id)
                    .push_bind(entry.isrc.as_deref())
                    .push_bind(entry.genre_ids.join(","))
                    .push_bind(*uses)
                    .push_bind(day);
            });
            query.push(
                " ON CONFLICT(track_id) DO UPDATE SET isrc = excluded.isrc,
                  genre_ids = excluded.genre_ids, hits = hits + excluded.hits,
                  last_used = excluded.last_used",
            );
            query.build().execute(&mut *tx).await?;
        }
        for (track_id, count) in &uses {
            sqlx::query("UPDATE track_genres SET hits = hits + ?, last_used = ? WHERE track_id = ?")
                .bind(count)
                .bind(day)
                .bind(track_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        // Keep whatever changed while the transaction ran.
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        for (track_id, written) in rows {
            if let Some(current) = pending.rows.get_mut(&track_id) {
                if current.0 == written.0 {
                    let left = current.1 - written.1;
                    pending.remove_row(track_id);
                    if left > 0 {
                        *pending.uses.entry(track_id).or_insert(0) += left;
                    }
                }
            }
        }
        for (track_id, written) in uses {
            if let Some(current) = pending.uses.get_mut(&track_id) {
                *current -= written;
                if *current <= 0 {
                    pending.uses.remove(&track_id);
                }
            }
        }
        for (id, name) in names {
            if pending.names.get(&id) == Some(&name) {
                pending.names.remove(&id);
            }
        }
        Ok(true)
    }

    /// Bytes the database pages hold (free pages excluded).
    async fn used_bytes(&self) -> Result<i64, sqlx::Error> {
        let pages: i64 = sqlx::query_scalar("PRAGMA page_count").fetch_one(&self.pool).await?;
        let free: i64 = sqlx::query_scalar("PRAGMA freelist_count").fetch_one(&self.pool).await?;
        let size: i64 = sqlx::query_scalar("PRAGMA page_size").fetch_one(&self.pool).await?;
        Ok((pages - free) * size)
    }

    fn limit_bytes(&self) -> i64 {
        i64::from(self.settings.genre_cache_max_mb()) * 1024 * 1024
    }

    /// Over the cap: drop the rows with the lowest uses-per-idle-day score
    /// (rarely requested and long unused first), then compact the file.
    pub async fn enforce_size(&self) -> Result<(), sqlx::Error> {
        let used = self.used_bytes().await?;
        let limit = self.limit_bytes();
        if used <= limit {
            return Ok(());
        }
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM track_genres")
            .fetch_one(&self.pool)
            .await?;
        if rows == 0 {
            return Ok(());
        }
        let keep = ((rows as f64) * (limit as f64) * EVICT_TO / (used as f64)) as i64;
        let drop = (rows - keep).max(1);
        let removed = sqlx::query(
            "DELETE FROM track_genres WHERE track_id IN (
                SELECT track_id FROM track_genres
                ORDER BY (hits + 1.0) / (? - last_used + 1) ASC, last_used ASC
                LIMIT ?
            )",
        )
        .bind(today())
        .bind(drop)
        .execute(&self.pool)
        .await?
        .rows_affected();
        self.evicted.fetch_add(removed, Ordering::Relaxed);
        self.compact().await?;
        tracing::info!(
            "Genre cache over {} MB: evicted {} of {} tracks",
            self.settings.genre_cache_max_mb(),
            removed,
            rows
        );
        Ok(())
    }

    /// Drops rows nobody requested within the retention period.
    pub async fn purge_expired(&self) -> Result<(), sqlx::Error> {
        let days = i64::from(self.settings.genre_cache_days());
        if days == 0 {
            return Ok(());
        }
        // Uses waiting in memory would otherwise not save their rows.
        self.flush().await?;
        let removed = sqlx::query("DELETE FROM track_genres WHERE last_used <= ?")
            .bind(today() - days)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if removed > 0 {
            self.expired.fetch_add(removed, Ordering::Relaxed);
            self.compact().await?;
        }
        Ok(())
    }

    /// Rewrites the file without free or half-empty pages.
    async fn compact(&self) -> Result<(), sqlx::Error> {
        sqlx::query("VACUUM").execute(&self.pool).await?;
        if self.location != "memory" {
            sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn clear(&self) -> Result<u64, sqlx::Error> {
        {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.rows.clear();
            pending.by_isrc.clear();
            pending.uses.clear();
        }
        let removed = sqlx::query("DELETE FROM track_genres")
            .execute(&self.pool)
            .await?
            .rows_affected();
        self.compact().await?;
        Ok(removed)
    }

    pub async fn stats(&self) -> Value {
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM track_genres")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        let pending = {
            let pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.rows.len()
        };
        json!({
            "file": self.location,
            "tracks": rows,
            "pending": pending,
            "bytes": self.used_bytes().await.unwrap_or(0),
            "limit_bytes": self.limit_bytes(),
            "days": self.settings.genre_cache_days(),
            "hits": self.hits.load(Ordering::Relaxed),
            "misses": self.misses.load(Ordering::Relaxed),
            "evicted": self.evicted.load(Ordering::Relaxed),
            "expired": self.expired.load(Ordering::Relaxed),
        })
    }
}

fn split_ids(text: &str) -> Vec<String> {
    text.split(',').filter(|s| !s.is_empty()).map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn cache(file: Option<&str>) -> Arc<GenreCache> {
        GenreCache::open(file, Arc::new(AppSettings::from_env())).await.unwrap()
    }

    fn entry(id: i64, isrc: &str, genres: &[&str]) -> Entry {
        Entry {
            track_id: id,
            isrc: Some(isrc.into()),
            genre_ids: genres.iter().map(|g| g.to_string()).collect(),
        }
    }

    fn names() -> HashMap<String, String> {
        [("1", "Pop"), ("7", "Alternative")].map(|(a, b)| (a.into(), b.into())).into()
    }

    #[test]
    fn pending_isrc_index_follows_rows() {
        let mut pending = Pending::default();
        pending.insert_row(entry(1, "AAAAA0000001", &["1"]), 1);
        assert_eq!(pending.row_mut(true, "AAAAA0000001").unwrap().0.track_id, 1);
        assert_eq!(pending.row_mut(false, "1").unwrap().1, 1);
        // Refetched under a new ISRC: the old one no longer resolves.
        pending.insert_row(entry(1, "BBBBB0000001", &["1"]), 2);
        assert!(pending.row_mut(true, "AAAAA0000001").is_none());
        assert!(pending.row_mut(true, "BBBBB0000001").is_some());
        // Another track takes the ISRC; removing the first keeps the index.
        pending.insert_row(entry(2, "BBBBB0000001", &["1"]), 1);
        pending.remove_row(1);
        assert_eq!(pending.row_mut(true, "BBBBB0000001").unwrap().0.track_id, 2);
        pending.remove_row(2);
        assert!(pending.by_isrc.is_empty() && pending.rows.is_empty());
    }

    #[test]
    fn genre_file_sits_next_to_the_main_database() {
        assert_eq!(file_for("hifi.db").as_deref(), Some("hifi-genres.db"));
        assert_eq!(file_for("/data/hifi.db").as_deref(), Some("/data/hifi-genres.db"));
        assert_eq!(file_for("sqlite:///data/hifi.db?mode=rwc").as_deref(), Some("/data/hifi-genres.db"));
        assert_eq!(file_for("sqlite:data/x").as_deref(), Some("data/x-genres.db"));
        for memory in ["", "ephemeral", "sqlite::memory:", ":memory:"] {
            assert_eq!(file_for(memory), None, "{memory}");
        }
    }

    #[tokio::test]
    async fn serves_pending_rows_then_flushed_rows_with_names() {
        let cache = cache(None).await;
        cache.store(vec![entry(10, "GBAYE9200070", &["7"]), entry(11, "USSM15900113", &[])], names());
        let found = cache.lookup(true, &["GBAYE9200070".into(), "ZZ0000000000".into()]).await;
        assert_eq!(found.len(), 1);
        assert_eq!(found["GBAYE9200070"].genres, ["Alternative"]);
        assert!(cache.flush().await.unwrap());
        assert!(!cache.flush().await.unwrap());
        let found = cache.lookup(false, &["10".into(), "11".into()]).await;
        assert_eq!(found["10"].isrc.as_deref(), Some("GBAYE9200070"));
        assert_eq!(found["11"].genres, Vec::<String>::new());
        cache.flush().await.unwrap();
        // fetch + pending lookup, then the lookup from disk.
        let hits: i64 = sqlx::query_scalar("SELECT hits FROM track_genres WHERE track_id = 10")
            .fetch_one(&cache.pool).await.unwrap();
        assert_eq!(hits, 3);
        let names: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM genre_names")
            .fetch_one(&cache.pool).await.unwrap();
        assert_eq!(names, 2);
    }

    #[tokio::test]
    async fn idle_rows_expire_and_used_rows_stay() {
        let cache = cache(None).await;
        cache.store(vec![entry(1, "AAAAA0000001", &["1"]), entry(2, "AAAAA0000002", &["1"])], names());
        cache.flush().await.unwrap();
        sqlx::query("UPDATE track_genres SET last_used = last_used - 40 WHERE track_id = 1")
            .execute(&cache.pool).await.unwrap();
        assert!(cache.lookup(false, &["1".into()]).await.is_empty());
        cache.purge_expired().await.unwrap();
        let left: Vec<i64> = sqlx::query_scalar("SELECT track_id FROM track_genres")
            .fetch_all(&cache.pool).await.unwrap();
        assert_eq!(left, [2]);
    }

    #[tokio::test]
    async fn over_the_cap_the_least_used_and_idle_rows_go_first() {
        let path = std::env::temp_dir().join(format!("hifi-genres-{}.db", uuid::Uuid::new_v4()));
        let file = path.to_string_lossy().into_owned();
        let cache = cache(Some(&file)).await;
        cache.settings.apply(&json!({"genre_cache_max_mb": 1})).unwrap();
        // Written directly: store() would wake the flush loop, which enforces
        // the cap on its own before the test sets up its rows.
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < 29999)
             INSERT INTO track_genres (track_id, isrc, genre_ids, hits, last_used)
             SELECT i, printf('QZ%010d', i), '1,7', 1, ? FROM n",
        )
        .bind(today())
        .execute(&cache.pool)
        .await
        .unwrap();
        // Track 0 is popular but idle for a week, 1 is fresh and unused,
        // 2 was used once long ago.
        sqlx::query("UPDATE track_genres SET hits = 500, last_used = last_used - 7 WHERE track_id = 0")
            .execute(&cache.pool).await.unwrap();
        sqlx::query("UPDATE track_genres SET hits = 1, last_used = last_used - 20 WHERE track_id = 2")
            .execute(&cache.pool).await.unwrap();
        assert!(cache.used_bytes().await.unwrap() > cache.limit_bytes());
        cache.enforce_size().await.unwrap();
        assert!(cache.used_bytes().await.unwrap() <= cache.limit_bytes());
        let kept = |id: i64| {
            let pool = cache.pool.clone();
            async move {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM track_genres WHERE track_id = ?")
                    .bind(id).fetch_one(&pool).await.unwrap() == 1
            }
        };
        assert!(kept(0).await);
        assert!(!kept(2).await);
        assert!(cache.evicted.load(Ordering::Relaxed) > 0);
        drop(cache);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{file}{suffix}"));
        }
    }
}
