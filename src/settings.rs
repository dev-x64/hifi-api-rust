use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::upstash::UpstashStore;

/// Server preferences persisted in SQLite and optionally shared through Redis.
pub struct AppSettings {
    pub auto_heal: AtomicBool,
    /// off (FLAC) | prefer (Atmos) | high (AAC 320 kbps).
    /// Query param `atmos=` overrides per request.
    pub atmos_mode: RwLock<String>,
    /// Secret: never include this in API snapshots or logs.
    discord_webhook_url: RwLock<String>,
    /// Shared cross-instance state (None = single-host mode, skip sync).
    upstash: OnceLock<std::sync::Arc<UpstashStore>>,
}

impl AppSettings {
    pub fn from_env() -> Self {
        Self {
            auto_heal: AtomicBool::new(env_bool("AUTO_HEAL", true)),
            atmos_mode: RwLock::new(default_atmos_mode()),
            discord_webhook_url: RwLock::new(
                std::env::var("DISCORD_WEBHOOK_URL").unwrap_or_default(),
            ),
            upstash: OnceLock::new(),
        }
    }

    /// Attach shared state once at startup (before serving).
    pub fn set_upstash(&self, store: Option<std::sync::Arc<UpstashStore>>) {
        if let Some(s) = store {
            let _ = self.upstash.set(s);
        }
    }

    fn upstash(&self) -> Option<std::sync::Arc<UpstashStore>> {
        self.upstash.get().cloned()
    }

    pub fn snapshot(&self) -> Value {
        json!({
            "auto_heal": self.auto_heal.load(Ordering::Relaxed),
            "atmos_mode": self.atmos_mode.read().map(|v| v.clone()).unwrap_or_else(|_| "prefer".to_string()),
        })
    }

    pub fn discord_webhook_url(&self) -> String {
        self.discord_webhook_url
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Persist first, then activate immediately. Empty string disables alerts.
    pub async fn set_discord_webhook_url(
        &self,
        url: String,
        db: Option<&SqlitePool>,
    ) -> Result<(), sqlx::Error> {
        if let Some(db) = db {
            sqlx::query(
                "INSERT INTO settings (key, value) VALUES ('discord_webhook_url', ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            )
            .bind(&url)
            .execute(db)
            .await?;
        }
        *self
            .discord_webhook_url
            .write()
            .unwrap_or_else(|e| e.into_inner()) = url.clone();
        if let Some(store) = self.upstash() {
            store
                .set(&UpstashStore::k_settings("discord_webhook_url"), &url, None)
                .await;
        }
        Ok(())
    }

    pub fn apply(&self, updates: &Value) -> Result<(), String> {
        if let Some(v) = first_opt_bool(updates, &["auto_heal"])? {
            self.auto_heal.store(v, Ordering::Relaxed);
        }
        if let Some(v) = first_opt_string(updates, &["atmos_mode"])? {
            if let Ok(mut w) = self.atmos_mode.write() {
                *w = normalize_atmos_mode(&v);
            }
        }
        Ok(())
    }

    pub async fn load_from_db(&self, db: &SqlitePool) {
        let rows: Result<Vec<(String, String)>, sqlx::Error> =
            sqlx::query_as("SELECT key, value FROM settings").fetch_all(db).await;
        let rows = match rows {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("Failed to load settings from DB: {}", e);
                return;
            }
        };
        let mut found_webhook = false;
        for (key, value) in rows {
            found_webhook |= key == "discord_webhook_url";
            self.apply_kv(&key, &value);
        }
        if !found_webhook {
            self.apply_kv(
                "discord_webhook_url",
                &std::env::var("DISCORD_WEBHOOK_URL").unwrap_or_default(),
            );
        }
    }

    /// Apply one persisted setting. Shared by the SQLite and Redis loaders.
    fn apply_kv(&self, key: &str, value: &str) {
        match key {
            "auto_heal" => {
                if value == "true" {
                    self.auto_heal.store(true, Ordering::Relaxed);
                } else if value == "false" {
                    self.auto_heal.store(false, Ordering::Relaxed);
                }
            }
            "atmos_mode" => {
                if let Ok(mut w) = self.atmos_mode.write() {
                    *w = normalize_atmos_mode(value);
                }
            }
            "discord_webhook_url" => {
                *self
                    .discord_webhook_url
                    .write()
                    .unwrap_or_else(|e| e.into_inner()) = value.to_string();
            }
            _ => {}
        }
    }

    /// Setting names mirrored to Redis (same keys as the SQLite table).
    const REDIS_SETTING_NAMES: &'static [&'static str] =
        &["auto_heal", "atmos_mode", "discord_webhook_url"];

    /// Canonical (key, value) snapshot, shared by the SQLite and Redis writers.
    fn settings_entries(&self) -> Vec<(String, String)> {
        vec![
            (
                "auto_heal",
                self.auto_heal.load(Ordering::Relaxed).to_string(),
            ),
            (
                "atmos_mode",
                self.atmos_mode
                    .read()
                    .map(|v| v.clone())
                    .unwrap_or_else(|_| "prefer".to_string()),
            ),
            ("discord_webhook_url", self.discord_webhook_url()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    }

    pub async fn save_to_db(&self, db: &SqlitePool) {
        let entries = self.settings_entries();
        for (key, value) in entries {
            let _ = sqlx::query(
                "INSERT INTO settings (key, value) VALUES (?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            )
            .bind(key)
            .bind(value)
            .execute(db)
            .await;
        }
    }

    /// Write-through of all settings to Redis (best-effort, no-op when
    /// sync is disabled).
    pub async fn save_to_redis(&self) {
        let store = match self.upstash() {
            Some(s) => s,
            None => return,
        };
        for (key, value) in self.settings_entries() {
            store.set(&UpstashStore::k_settings(&key), &value, None).await;
        }
        tracing::debug!("Synced {} settings to Redis", Self::REDIS_SETTING_NAMES.len());
    }

    /// Load shared settings into memory. Returns the number of keys applied.
    pub async fn load_from_redis(&self) -> usize {
        let store = match self.upstash() {
            Some(s) => s,
            None => return 0,
        };
        let keys: Vec<String> =
            Self::REDIS_SETTING_NAMES.iter().map(|n| UpstashStore::k_settings(n)).collect();
        let values = store.mget(&keys).await;
        let mut applied = 0;
        for (name, value) in Self::REDIS_SETTING_NAMES.iter().zip(values.iter()) {
            if let Some(v) = value {
                self.apply_kv(name, v);
                applied += 1;
            }
        }
        if applied > 0 {
            tracing::info!("Loaded {} settings from Redis", applied);
        }
        applied
    }

    /// First-boot convergence: when Redis holds no settings, seed it from
    /// this host (first writer wins via NX across the fleet), then load.
    pub async fn seed_and_load(&self) {
        let store = match self.upstash() {
            Some(s) => s,
            None => return,
        };
        let keys: Vec<String> = Self::REDIS_SETTING_NAMES
            .iter()
            .map(|n| UpstashStore::k_settings(n))
            .collect();
        let present = store.mget(&keys).await;
        let mut seeded = 0;
        for (index, (key, value)) in self.settings_entries().into_iter().enumerate() {
            if present.get(index).and_then(|value| value.as_ref()).is_none()
                && store
                    .set_nx(&UpstashStore::k_settings(&key), &value, None)
                    .await
            {
                seeded += 1;
            }
        }
        let applied = self.load_from_redis().await;
        tracing::info!("Seeded {} settings to Redis, converged on {}", seeded, applied);
    }

    /// Periodic pull for the 30s tick.
    pub async fn refresh_from_redis(&self) {
        self.load_from_redis().await;
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "true" || v == "1" || v == "yes"
        })
        .unwrap_or(default)
}

fn normalize_atmos_mode(s: &str) -> String {
    match s.trim().to_lowercase().as_str() {
        "prefer" => "prefer".to_string(),
        "high" => "high".to_string(),
        _ => "off".to_string(),
    }
}

/// Server default: Atmos preferred unless explicitly turned off.
fn default_atmos_mode() -> String {
    match std::env::var("ATMOS_MODE") {
        Ok(v) if !v.trim().is_empty() => normalize_atmos_mode(&v),
        _ => "prefer".to_string(),
    }
}

fn first_opt_string(obj: &Value, keys: &[&str]) -> Result<Option<String>, String> {
    for key in keys {
        match obj.get(key) {
            None => continue,
            Some(v) if v.is_null() => return Ok(None),
            Some(v) => {
                return v
                    .as_str()
                    .map(|s| Some(s.to_string()))
                    .ok_or_else(|| format!("{} must be a string", key));
            }
        }
    }
    Ok(None)
}

fn first_opt_bool(obj: &Value, keys: &[&str]) -> Result<Option<bool>, String> {
    for key in keys {
        match obj.get(key) {
            None => continue,
            Some(v) if v.is_null() => return Ok(None),
            Some(v) => {
                return v
                    .as_bool()
                    .map(Some)
                    .ok_or_else(|| format!("{} must be a boolean", key));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::{default_atmos_mode, normalize_atmos_mode, AppSettings};

    #[tokio::test]
    async fn discord_webhook_persists_without_entering_public_snapshot() {
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .execute(&db)
            .await
            .unwrap();
        let url = "https://discord.com/api/webhooks/123/secret-token";
        let settings = AppSettings::from_env();
        settings
            .set_discord_webhook_url(url.to_string(), Some(&db))
            .await
            .unwrap();
        assert!(!settings.snapshot().to_string().contains("secret-token"));

        let reloaded = AppSettings::from_env();
        reloaded.load_from_db(&db).await;
        assert_eq!(reloaded.discord_webhook_url(), url);

        reloaded
            .set_discord_webhook_url(String::new(), Some(&db))
            .await
            .unwrap();
        settings.load_from_db(&db).await;
        assert!(settings.discord_webhook_url().is_empty());
    }

    #[test]
    fn atmos_explicit_values_honored() {
        assert_eq!(normalize_atmos_mode("prefer"), "prefer");
        assert_eq!(normalize_atmos_mode("off"), "off");
        assert_eq!(normalize_atmos_mode("HIGH"), "high");
        assert_eq!(normalize_atmos_mode("banana"), "off");
    }

    #[test]
    fn atmos_defaults_to_prefer_when_unset() {
        let saved = std::env::var("ATMOS_MODE").ok();
        unsafe {
            std::env::remove_var("ATMOS_MODE");
        }
        assert_eq!(default_atmos_mode(), "prefer");
        unsafe {
            std::env::set_var("ATMOS_MODE", "");
        }
        assert_eq!(default_atmos_mode(), "prefer");
        unsafe {
            std::env::set_var("ATMOS_MODE", "off");
        }
        assert_eq!(default_atmos_mode(), "off");
        unsafe {
            match saved {
                Some(v) => std::env::set_var("ATMOS_MODE", v),
                None => std::env::remove_var("ATMOS_MODE"),
            }
        }
    }
}
