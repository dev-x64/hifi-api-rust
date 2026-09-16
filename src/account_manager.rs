use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use chrono::Utc;
use serde::Deserialize;
use sqlx::FromRow;
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::AppError;
use crate::upstash::UpstashStore;

#[derive(Clone, Debug)]
pub struct SwitchingWeights {
    pub balance: f64,
    pub recency: f64,
    pub error: f64,
}

impl Default for SwitchingWeights {
    fn default() -> Self {
        Self {
            balance: 0.4,
            recency: 0.3,
            error: 0.3,
        }
    }
}

pub struct AccountState {
    pub id: String,
    pub label: String,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub user_id: RwLock<Option<String>>,
    pub access_token: RwLock<Option<String>>,
    pub token_expires_at: AtomicI64,
    pub is_active: AtomicBool,
    /// Dedicated metadata credential (upstream CATALOG_* / token.json role).
    /// Never leased to playback: excluded from selection and pool counts.
    pub is_catalog: AtomicBool,
    /// True when the system (not the owner) deactivated the account.
    /// Only these are eligible for auto-heal; manual OFF is never touched.
    pub auto_disabled: AtomicBool,
    pub heal_failures: AtomicU64,
    pub heal_next_retry: AtomicI64,
    pub notes: RwLock<String>,
    pub last_used: AtomicI64,
    pub request_count: AtomicU64,
    pub error_count: AtomicU64,
    /// Last mutation unix timestamp (local admin ops AND Redis merges).
    /// Drives newest-wins convergence across instances.
    pub updated_at: AtomicI64,
}

impl AccountState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        label: String,
        client_id: String,
        client_secret: String,
        refresh_token: String,
        user_id: Option<String>,
        is_active: bool,
        notes: String,
    ) -> Self {
        Self {
            id,
            label,
            client_id,
            client_secret,
            refresh_token,
            user_id: RwLock::new(user_id),
            access_token: RwLock::new(None),
            token_expires_at: AtomicI64::new(0),
            is_active: AtomicBool::new(is_active),
            is_catalog: AtomicBool::new(false),
            auto_disabled: AtomicBool::new(false),
            heal_failures: AtomicU64::new(0),
            heal_next_retry: AtomicI64::new(0),
            notes: RwLock::new(notes),
            last_used: AtomicI64::new(0),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            updated_at: AtomicI64::new(0),
        }
    }

    /// Copy live counters from `old` into a rebuilt state, so admin
    /// edits and cross-instance merges never wipe stats or tokens.
    pub fn carry_over(new: &AccountState, old: &AccountState) {
        new.last_used.store(old.last_used.load(Ordering::Relaxed), Ordering::Relaxed);
        new.request_count.store(old.request_count.load(Ordering::Relaxed), Ordering::Relaxed);
        new.error_count.store(old.error_count.load(Ordering::Relaxed), Ordering::Relaxed);
        new.token_expires_at.store(old.token_expires_at.load(Ordering::Relaxed), Ordering::Relaxed);
        new.is_catalog.store(old.is_catalog.load(Ordering::Relaxed), Ordering::Relaxed);
        new.auto_disabled.store(old.auto_disabled.load(Ordering::Relaxed), Ordering::Relaxed);
        new.heal_failures.store(old.heal_failures.load(Ordering::Relaxed), Ordering::Relaxed);
        new.heal_next_retry.store(old.heal_next_retry.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

/// UTC day number for a unix timestamp. Pure function — unit tested.
#[allow(dead_code)]
pub fn utc_day(ts: i64) -> i64 {
    ts.div_euclid(86400)
}

#[derive(Debug, Deserialize, FromRow)]
pub struct DbAccountRow {
    pub id: String,
    pub label: String,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub user_id: Option<String>,
    pub is_active: i32,
    pub auto_disabled: Option<i32>,
    pub is_catalog: Option<i32>,
    pub notes: String,
    pub access_token: Option<String>,
    pub expires_at: Option<i64>,
    pub updated_at: Option<i64>,
}

/// Extract a numeric user id from an auto-generated "Tidal Account (<id>)" label.
fn user_id_from_label(label: &str) -> Option<&str> {
    let inner = label
        .strip_prefix("Tidal Account (")?
        .strip_suffix(')')?;
    if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
        Some(inner)
    } else {
        None
    }
}

pub struct AccountManager {
    accounts: RwLock<Vec<Arc<AccountState>>>,
    weights: SwitchingWeights,
    db: Option<SqlitePool>,
    /// Shared cross-instance state (None = single-host mode, skip sync).
    upstash: OnceLock<Arc<UpstashStore>>,
}

impl AccountManager {
    pub fn new(db: Option<SqlitePool>, weights: SwitchingWeights) -> Self {
        Self {
            accounts: RwLock::new(Vec::new()),
            weights,
            db,
            upstash: OnceLock::new(),
        }
    }

    /// Attach shared state once at startup (before serving).
    pub fn set_upstash(&self, store: Option<Arc<UpstashStore>>) {
        if let Some(s) = store {
            let _ = self.upstash.set(s);
        }
    }

    fn upstash(&self) -> Option<Arc<UpstashStore>> {
        self.upstash.get().cloned()
    }

    pub async fn load_from_db(&self) -> Result<(), AppError> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok(()),
        };

        let rows: Vec<DbAccountRow> = sqlx::query_as::<_, DbAccountRow>(
            "SELECT a.id, a.label, a.client_id, a.client_secret, a.refresh_token,
             a.user_id, a.is_active, a.auto_disabled, a.is_catalog, a.notes,
             t.access_token, t.expires_at, a.updated_at
             FROM accounts a
             LEFT JOIN tokens t ON t.account_id = a.id
             ORDER BY a.created_at ASC",
        )
        .fetch_all(db)
        .await?;

        let mut accounts = self.accounts.write().await;
        for row in rows {
            // Backfill user_id for accounts created before it was persisted
            // (label was "Tidal Account (<id>)").
            let user_id = match row.user_id {
                Some(uid) => Some(uid),
                None => user_id_from_label(&row.label).map(|s| s.to_string()),
            };
            if user_id.is_some() && self.db.is_some() {
                if let Some(db) = &self.db {
                    let _ = sqlx::query("UPDATE accounts SET user_id = ? WHERE id = ? AND user_id IS NULL")
                        .bind(&user_id)
                        .bind(&row.id)
                        .execute(db)
                        .await;
                }
            }
            let state = Arc::new(AccountState::new(
                row.id,
                row.label,
                row.client_id,
                row.client_secret,
                row.refresh_token,
                user_id,
                row.is_active != 0,
                row.notes,
            ));
            if row.auto_disabled.unwrap_or(0) != 0 {
                state.auto_disabled.store(true, Ordering::Relaxed);
            }
            if row.is_catalog.unwrap_or(0) != 0 {
                state.is_catalog.store(true, Ordering::Relaxed);
            }
            if let (Some(token), Some(expires)) = (row.access_token, row.expires_at) {
                if !token.is_empty() && expires > 0 {
                    *state.access_token.write().await = Some(token);
                    state.token_expires_at.store(expires, Ordering::Relaxed);
                }
            }
            state.updated_at.store(row.updated_at.unwrap_or(0), Ordering::Relaxed);
            accounts.push(state);
        }

        tracing::info!("Loaded {} accounts from database", accounts.len());
        Ok(())
    }

    /// Drop all in-memory state and reload from the database (used after restore).
    pub async fn reload_from_db(&self) -> Result<(), AppError> {
        self.accounts.write().await.clear();
        self.load_from_db().await?;
        // Converge with the fleet (backup restores only touch SQLite).
        self.merge_accounts_from_redis().await;
        Ok(())
    }

    pub async fn add_account(
        &self,
        label: String,
        client_id: String,
        client_secret: String,
        refresh_token: String,
        user_id: Option<String>,
    ) -> Result<Arc<AccountState>, AppError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();
        let state = Arc::new(AccountState::new(
            id.clone(),
            label.clone(),
            client_id.clone(),
            client_secret.clone(),
            refresh_token.clone(),
            user_id.clone(),
            true,
            String::new(),
        ));

        if let Some(db) = &self.db {
            sqlx::query(
                "INSERT INTO accounts (id, label, client_id, client_secret, refresh_token, user_id, is_active, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?)",
            )
            .bind(&id)
            .bind(&label)
            .bind(&client_id)
            .bind(&client_secret)
            .bind(&refresh_token)
            .bind(&user_id)
            .bind(now)
            .bind(now)
            .execute(db)
            .await?;

            sqlx::query("INSERT INTO account_metrics (account_id) VALUES (?)")
                .bind(&id)
                .execute(db)
                .await?;
        }

        state.updated_at.store(now, Ordering::Relaxed);
        self.accounts.write().await.push(state.clone());
        self.push_account_to_redis(&state).await;
        Ok(state)
    }

    pub async fn remove_account(&self, id: &str) -> Result<(), AppError> {
        {
            let mut accounts = self.accounts.write().await;
            accounts.retain(|a| a.id != id);
        }

        if let Some(db) = &self.db {
            sqlx::query("DELETE FROM accounts WHERE id = ?")
                .bind(id)
                .execute(db)
                .await?;
        }
        // Remove from the shared backup too (or the next merge resurrects it).
        if let Some(store) = self.upstash() {
            store.del_many(&[
                UpstashStore::k_account(id),
                UpstashStore::k_token(id),
            ]).await;
            store.srem(&UpstashStore::k_accounts_set(), id).await;
        }
        Ok(())
    }

    pub async fn get_account_by_id(&self, id: &str) -> Option<Arc<AccountState>> {
        let accounts = self.accounts.read().await;
        accounts.iter().find(|a| a.id == id).cloned()
    }

    pub async fn select_account_excluding(&self, exclude_ids: &[String]) -> Result<Arc<AccountState>, AppError> {
        let accounts = self.accounts.read().await;
        if accounts.is_empty() {
            return Err(AppError::Internal(
                "No Tidal credentials available; add an account via the admin panel".into(),
            ));
        }

        let now = Utc::now().timestamp();
        let mut scored: Vec<(f64, usize)> = Vec::new();

        for (i, account) in accounts.iter().enumerate() {
            if exclude_ids.contains(&account.id) {
                continue;
            }
            if !account.is_active.load(Ordering::Relaxed) {
                continue;
            }
            // Catalog-only credentials never serve playback.
            if account.is_catalog.load(Ordering::Relaxed) {
                continue;
            }

            let usage = account.request_count.load(Ordering::Relaxed).max(1) as f64;
            let last_used = account.last_used.load(Ordering::Relaxed);
            let recency = if last_used > 0 {
                (now - last_used) as f64
            } else {
                3600.0
            };
            let errors = account.error_count.load(Ordering::Relaxed).max(1) as f64;
            let total = account.request_count.load(Ordering::Relaxed).max(1) as f64;
            let error_rate = errors / total;

            let usage_score = self.weights.balance / usage;
            let recency_score = self.weights.recency * (recency / 3600.0).min(1.0).max(0.0);
            let error_score = self.weights.error * (1.0 - error_rate);

            let score = usage_score + recency_score + error_score;

            scored.push((score, i));
        }

        if scored.is_empty() {
            return Err(AppError::ServiceUnavailable(
                "All accounts are inactive or have expired tokens".into(),
            ));
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let best = &accounts[scored[0].1];
        best.last_used.store(now, Ordering::Relaxed);
        best.request_count.fetch_add(1, Ordering::Relaxed);
        Ok(best.clone())
    }

    pub async fn select_account(&self) -> Result<Arc<AccountState>, AppError> {
        self.select_account_excluding(&[]).await
    }

    /// Playback pool size (upstream: PlaybackCredentialPool.size).
    /// Catalog-only credentials are excluded. Minimum 1 so callers
    /// without playback accounts still run once and surface the real error.
    pub async fn playback_slots(&self) -> usize {
        self.accounts
            .read()
            .await
            .iter()
            .filter(|a| !a.is_catalog.load(Ordering::Relaxed))
            .count()
            .max(1)
    }

    /// Number of playback (non-catalog) accounts.
    pub async fn playback_count(&self) -> usize {
        self.accounts
            .read()
            .await
            .iter()
            .filter(|a| !a.is_catalog.load(Ordering::Relaxed))
            .count()
    }

    /// Dedicated metadata credential (upstream _catalog_cred). Prefers an
    /// active catalog account; returns None when none is flagged.
    pub async fn find_catalog_account(&self) -> Option<Arc<AccountState>> {
        let accounts = self.accounts.read().await;
        accounts
            .iter()
            .find(|a| {
                a.is_catalog.load(Ordering::Relaxed) && a.is_active.load(Ordering::Relaxed)
            })
            .cloned()
            .or_else(|| {
                accounts
                    .iter()
                    .find(|a| a.is_catalog.load(Ordering::Relaxed))
                    .cloned()
            })
    }

    /// Metadata account selection (upstream catalog=True): the catalog
    /// credential when one is active, otherwise the normal playback pool.
    pub async fn select_catalog_account(&self) -> Result<Arc<AccountState>, AppError> {
        if let Some(acc) = self.find_catalog_account().await {
            if acc.is_active.load(Ordering::Relaxed) {
                return Ok(acc);
            }
        }
        self.select_account_excluding(&[]).await
    }

    /// Flag or unflag an account as catalog-only (kept out of playback).
    pub async fn set_account_catalog(&self, id: &str, catalog: bool) -> Result<(), AppError> {
        if let Some(account) = self.get_account_by_id(id).await {
            account.is_catalog.store(catalog, Ordering::Relaxed);
            let now = Utc::now().timestamp();
            account.updated_at.store(now, Ordering::Relaxed);
            if let Some(db) = &self.db {
                sqlx::query(
                    "UPDATE accounts SET is_catalog = ?, updated_at = ? WHERE id = ?",
                )
                .bind(catalog as i32)
                .bind(now)
                .bind(id)
                .execute(db)
                .await?;
            }
            self.push_account_to_redis(&account).await;
            Ok(())
        } else {
            Err(AppError::NotFound(format!("Account {} not found", id)))
        }
    }

    pub async fn mark_account_error(&self, id: &str, message: &str) {
        let now = Utc::now().timestamp();
        if let Some(account) = self.get_account_by_id(id).await {
            account.error_count.fetch_add(1, Ordering::Relaxed);
            if let Some(db) = &self.db {
                let _ = sqlx::query(
                    "UPDATE account_metrics SET error_count = error_count + 1, last_error_at = ?, last_error_message = ? WHERE account_id = ?",
                )
                .bind(now)
                .bind(message)
                .bind(id)
                .execute(db)
                .await;
            }
        }
    }

    pub async fn update_account(
        &self,
        id: &str,
        label: Option<String>,
        client_id: Option<String>,
        client_secret: Option<String>,
        refresh_token: Option<String>,
        user_id: Option<String>,
    ) -> Result<(), AppError> {
        let mut accounts = self.accounts.write().await;
        let idx = accounts.iter().position(|a| a.id == id).ok_or_else(|| {
            AppError::NotFound(format!("Account {} not found", id))
        })?;

        let old = &accounts[idx];
        let new_label = label.unwrap_or_else(|| old.label.clone());
        let new_client_id = client_id.unwrap_or_else(|| old.client_id.clone());
        let new_client_secret = client_secret.unwrap_or_else(|| old.client_secret.clone());
        let new_refresh_token = refresh_token.unwrap_or_else(|| old.refresh_token.clone());
        let current_user_id = old.user_id.read().await.clone();
        let new_user_id = user_id.or(current_user_id);
        let new_notes = old.notes.read().await.clone();

        let updated = Arc::new(AccountState::new(
            old.id.clone(),
            new_label,
            new_client_id,
            new_client_secret,
            new_refresh_token,
            new_user_id.clone(),
            old.is_active.load(Ordering::Relaxed),
            new_notes,
        ));
        // Preserve live counters (an edit must not wipe stats
        // or tokens) and stamp the mutation for fleet merges.
        AccountState::carry_over(&updated, old);
        let now_mut = Utc::now().timestamp();
        updated.updated_at.store(now_mut, Ordering::Relaxed);

        if let Some(db) = &self.db {
            let now = Utc::now().timestamp();
            sqlx::query(
                "UPDATE accounts SET label = ?, client_id = ?, client_secret = ?, refresh_token = ?, user_id = ?, updated_at = ? WHERE id = ?",
            )
            .bind(&updated.label)
            .bind(&updated.client_id)
            .bind(&updated.client_secret)
            .bind(&updated.refresh_token)
            .bind(&new_user_id)
            .bind(now)
            .bind(&old.id)
            .execute(db)
            .await?;
        }

        accounts[idx] = updated.clone();
        drop(accounts);
        self.push_account_to_redis(&updated).await;
        Ok(())
    }

    /// Mark an account as system-disabled (eligible for auto-heal) or clear it
    /// (owner intent / successful recovery — never auto-healed while clear).
    pub async fn set_auto_disabled(&self, id: &str, disabled: bool) -> Result<(), AppError> {
        if let Some(account) = self.get_account_by_id(id).await {
            account.auto_disabled.store(disabled, Ordering::Relaxed);
            if disabled {
                account.heal_failures.store(0, Ordering::Relaxed);
                account.heal_next_retry.store(0, Ordering::Relaxed);
            }
            let now = Utc::now().timestamp();
            account.updated_at.store(now, Ordering::Relaxed);
            if let Some(db) = &self.db {
                sqlx::query(
                    "UPDATE accounts SET auto_disabled = ?, updated_at = ? WHERE id = ?",
                )
                .bind(disabled as i32)
                .bind(now)
                .bind(id)
                .execute(db)
                .await?;
            }
            self.push_account_to_redis(&account).await;
            Ok(())
        } else {
            Err(AppError::NotFound(format!("Account {} not found", id)))
        }
    }

    pub async fn set_account_active(&self, id: &str, active: bool) -> Result<(), AppError> {
        if let Some(account) = self.get_account_by_id(id).await {
            account.is_active.store(active, Ordering::Relaxed);
            let now = Utc::now().timestamp();
            account.updated_at.store(now, Ordering::Relaxed);
            if let Some(db) = &self.db {
                sqlx::query(
                    "UPDATE accounts SET is_active = ?, updated_at = ? WHERE id = ?",
                )
                .bind(active as i32)
                .bind(now)
                .bind(id)
                .execute(db)
                .await?;
            }
            self.push_account_to_redis(&account).await;
            Ok(())
        } else {
            Err(AppError::NotFound(format!("Account {} not found", id)))
        }
    }

    // --- Redis credential sync ---

    /// Serialize one account for Redis. Contains Tidal secrets — the
    /// payload is never logged, only key names at debug level.
    async fn account_to_json(acc: &AccountState) -> String {
        serde_json::json!({
            "id": acc.id,
            "label": acc.label,
            "client_id": acc.client_id,
            "client_secret": acc.client_secret,
            "refresh_token": acc.refresh_token,
            "user_id": acc.user_id.read().await.clone(),
            "is_active": acc.is_active.load(Ordering::Relaxed),
            "auto_disabled": acc.auto_disabled.load(Ordering::Relaxed),
            "is_catalog": acc.is_catalog.load(Ordering::Relaxed),
            "notes": acc.notes.read().await.clone(),
            "updated_at": acc.updated_at.load(Ordering::Relaxed),
        })
        .to_string()
    }

    /// Publish one account record + index entry (no expiry: backup
    /// semantics). Best-effort; called after every local mutation.
    async fn push_account_to_redis(&self, acc: &AccountState) {
        let store = match self.upstash() {
            Some(s) => s,
            None => return,
        };
        let payload = Self::account_to_json(acc).await;
        store.set(&UpstashStore::k_account(&acc.id), &payload, None).await;
        store.sadd(&UpstashStore::k_accounts_set(), &acc.id).await;
    }

    /// Union-merge accounts from Redis: add ids missing locally (this is
    /// what restores a wiped host), and adopt newer records for known ids
    /// (newest `updated_at` wins; live counters/leases are preserved, never
    /// overwritten). Ids absent from the Redis index are never resurrected,
    /// so explicit deletes stay deleted. Called at startup and on the 60s
    /// tick; also pushes local-only accounts Redis never saw.
    pub async fn merge_accounts_from_redis(&self) {
        let store = match self.upstash() {
            Some(s) => s,
            None => return,
        };
        let remote_ids = match store.smembers(&UpstashStore::k_accounts_set()).await {
            Some(ids) => ids,
            None => return,
        };
        if !remote_ids.is_empty() {
            let keys: Vec<String> =
                remote_ids.iter().map(|id| UpstashStore::k_account(id)).collect();
            let values = store.mget(&keys).await;
            let mut accounts = self.accounts.write().await;
            for (id, payload) in remote_ids.iter().zip(values.iter()) {
                let raw = match payload {
                    Some(r) => r,
                    None => continue,
                };
                let v: serde_json::Value = match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let str_field = |k: &str| {
                    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
                };
                let remote_updated = v.get("updated_at").and_then(|x| x.as_i64()).unwrap_or(0);
                let user_id =
                    v.get("user_id").and_then(|x| x.as_str()).map(|s| s.to_string());
                let is_active = v.get("is_active").and_then(|x| x.as_bool()).unwrap_or(true);
                let auto_disabled =
                    v.get("auto_disabled").and_then(|x| x.as_bool()).unwrap_or(false);
                let is_catalog =
                    v.get("is_catalog").and_then(|x| x.as_bool()).unwrap_or(false);
                if let Some(pos) = accounts.iter().position(|a| &a.id == id) {
                    if remote_updated <= accounts[pos].updated_at.load(Ordering::Relaxed) {
                        continue;
                    }
                    let old = &accounts[pos];
                    let notes = str_field("notes");
                    let rebuilt = Arc::new(AccountState::new(
                        old.id.clone(),
                        str_field("label"),
                        str_field("client_id"),
                        str_field("client_secret"),
                        str_field("refresh_token"),
                        user_id.clone(),
                        is_active,
                        notes.clone(),
                    ));
                    AccountState::carry_over(&rebuilt, old);
                    if auto_disabled {
                        rebuilt.auto_disabled.store(true, Ordering::Relaxed);
                    }
                    if is_catalog {
                        rebuilt.is_catalog.store(true, Ordering::Relaxed);
                    }
                    rebuilt.updated_at.store(remote_updated, Ordering::Relaxed);
                    if let Some(db) = &self.db {
                        let _ = sqlx::query(
                            "UPDATE accounts SET label = ?, client_id = ?, client_secret = ?,
                             refresh_token = ?, user_id = ?, is_active = ?, auto_disabled = ?,
                             is_catalog = ?, notes = ?, updated_at = ? WHERE id = ?",
                        )
                        .bind(&rebuilt.label)
                        .bind(&rebuilt.client_id)
                        .bind(&rebuilt.client_secret)
                        .bind(&rebuilt.refresh_token)
                        .bind(&user_id)
                        .bind(is_active as i32)
                        .bind(auto_disabled as i32)
                        .bind(is_catalog as i32)
                        .bind(&notes)
                        .bind(remote_updated)
                        .bind(&rebuilt.id)
                        .execute(db)
                        .await;
                    }
                    accounts[pos] = rebuilt;
                } else {
                    // Unknown locally: restore into SQLite (survives the next
                    // Redis outage) and memory.
                    let notes = str_field("notes");
                    let state = Arc::new(AccountState::new(
                        id.clone(),
                        str_field("label"),
                        str_field("client_id"),
                        str_field("client_secret"),
                        str_field("refresh_token"),
                        user_id.clone(),
                        is_active,
                        notes.clone(),
                    ));
                    if auto_disabled {
                        state.auto_disabled.store(true, Ordering::Relaxed);
                    }
                    if is_catalog {
                        state.is_catalog.store(true, Ordering::Relaxed);
                    }
                    state.updated_at.store(remote_updated, Ordering::Relaxed);
                    if let Some(db) = &self.db {
                        let now = Utc::now().timestamp();
                        let _ = sqlx::query(
                            "INSERT INTO accounts (id, label, client_id, client_secret, refresh_token,
                             user_id, is_active, auto_disabled, is_catalog, notes, created_at, updated_at)
                             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                             ON CONFLICT(id) DO NOTHING",
                        )
                        .bind(id)
                        .bind(&state.label)
                        .bind(&state.client_id)
                        .bind(&state.client_secret)
                        .bind(&state.refresh_token)
                        .bind(&user_id)
                        .bind(is_active as i32)
                        .bind(auto_disabled as i32)
                        .bind(is_catalog as i32)
                        .bind(&notes)
                        .bind(now)
                        .bind(remote_updated)
                        .execute(db)
                        .await;
                        let _ = sqlx::query(
                            "INSERT INTO account_metrics (account_id) VALUES (?)
                             ON CONFLICT(account_id) DO NOTHING",
                        )
                        .bind(id)
                        .execute(db)
                        .await;
                    }
                    tracing::info!("Restored account {} from Redis backup", state.label);
                    accounts.push(state);
                }
            }
            // Push anything local that Redis never saw (added while Redis
            // was unreachable, or on this host first).
            let mut to_push = Vec::new();
            for a in accounts.iter() {
                if !remote_ids.contains(&a.id) {
                    to_push.push(a.clone());
                }
            }
            drop(accounts);
            for acc in to_push {
                self.push_account_to_redis(&acc).await;
            }
        }
    }

    /// Publish the full local roster to Redis and delete remote orphans, so
    /// a backup restore wins fleet-wide instead of being re-merged away.
    pub async fn publish_all_to_redis(&self) {
        let store = match self.upstash() {
            Some(s) => s,
            None => return,
        };
        let accounts = self.accounts.read().await;
        for a in accounts.iter() {
            self.push_account_to_redis(a).await;
        }
        let local_ids: Vec<String> = accounts.iter().map(|a| a.id.clone()).collect();
        drop(accounts);
        if let Some(remote_ids) = store.smembers(&UpstashStore::k_accounts_set()).await {
            let mut orphan_keys = Vec::new();
            for id in &remote_ids {
                if !local_ids.contains(id) {
                    orphan_keys.push(UpstashStore::k_account(id));
                    store.srem(&UpstashStore::k_accounts_set(), id).await;
                }
            }
            store.del_many(&orphan_keys).await;
        }
    }

    pub async fn active_count(&self) -> usize {
        self.accounts
            .read()
            .await
            .iter()
            .filter(|a| a.is_active.load(Ordering::Relaxed))
            .count()
    }

    pub async fn healthy_count(&self) -> (usize, usize) {
        let accounts = self.accounts.read().await;
        let total = accounts.len();
        let healthy = accounts
            .iter()
            .filter(|a| a.is_active.load(Ordering::Relaxed))
            .count();
        (healthy, total)
    }

    pub async fn list_accounts(&self) -> Vec<Arc<AccountState>> {
        self.accounts.read().await.clone()
    }

    pub async fn account_count(&self) -> usize {
        self.accounts.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{utc_day, AccountState};
    use std::sync::atomic::Ordering;

    #[test]
    fn utc_day_boundaries() {
        // 1970-01-01T00:00:00Z == day 0; 86399 still day 0; 86400 day 1.
        assert_eq!(utc_day(0), 0);
        assert_eq!(utc_day(86399), 0);
        assert_eq!(utc_day(86400), 1);
        // Negative timestamps (pre-1970) still bucket consistently.
        assert_eq!(utc_day(-1), -1);
        assert_eq!(utc_day(-86400), -1);
    }

    #[test]
    fn utc_day_rollover_detected() {
        let monday_2359 = 86400 * 100 + 86399;
        let tuesday_0001 = 86400 * 101 + 60;
        assert_ne!(utc_day(monday_2359), utc_day(tuesday_0001));
    }

    #[tokio::test]
    async fn account_json_round_trip() {
        let acc = AccountState::new(
            "id-1".into(),
            "Label".into(),
            "cid".into(),
            "csec".into(),
            "rt".into(),
            Some("977".into()),
            true,
            "note".into(),
        );
        acc.updated_at.store(12345, Ordering::Relaxed);
        let raw = super::AccountManager::account_to_json(&acc).await;
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["id"], "id-1");
        assert_eq!(v["label"], "Label");
        assert_eq!(v["client_id"], "cid");
        assert_eq!(v["client_secret"], "csec");
        assert_eq!(v["refresh_token"], "rt");
        assert_eq!(v["user_id"], "977");
        assert_eq!(v["is_active"], true);
        assert_eq!(v["notes"], "note");
        assert_eq!(v["updated_at"], 12345);
    }

    #[test]
    fn carry_over_preserves_live_state() {
        let old = AccountState::new(
            "id-1".into(),
            "Old".into(),
            "a".into(),
            "b".into(),
            "c".into(),
            None,
            true,
            String::new(),
        );
        old.request_count.store(500, Ordering::Relaxed);
        old.token_expires_at.store(8_888_888, Ordering::Relaxed);
        old.is_catalog.store(true, Ordering::Relaxed);
        let new = AccountState::new(
            "id-1".into(),
            "New".into(),
            "x".into(),
            "y".into(),
            "z".into(),
            None,
            true,
            String::new(),
        );
        AccountState::carry_over(&new, &old);
        assert_eq!(new.label, "New");
        assert_eq!(new.request_count.load(Ordering::Relaxed), 500);
        assert_eq!(new.token_expires_at.load(Ordering::Relaxed), 8_888_888);
        assert!(new.is_catalog.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn catalog_accounts_stay_out_of_playback() {
        use super::{AccountManager, SwitchingWeights};
        use std::sync::Arc;
        let am = Arc::new(AccountManager::new(None, SwitchingWeights::default()));
        let playback = am
            .add_account("play".into(), "c".into(), "s".into(), "rt-play".into(), None)
            .await
            .unwrap();
        let catalog = am
            .add_account("cat".into(), "c".into(), "s".into(), "rt-cat".into(), None)
            .await
            .unwrap();
        am.set_account_catalog(&catalog.id, true).await.unwrap();
        assert_eq!(am.playback_count().await, 1);
        assert_eq!(am.playback_slots().await, 1);
        // Playback selection never returns the catalog account.
        for _ in 0..3 {
            let picked = am.select_account_excluding(&[]).await.unwrap();
            assert_eq!(picked.id, playback.id);
        }
        // Catalog resolution finds the flagged account.
        let found = am.find_catalog_account().await.unwrap();
        assert_eq!(found.id, catalog.id);
        let via_helper = am.select_catalog_account().await.unwrap();
        assert_eq!(via_helper.id, catalog.id);
    }

    #[tokio::test]
    async fn catalog_selection_falls_back_to_pool() {
        use super::{AccountManager, SwitchingWeights};
        use std::sync::Arc;
        let am = Arc::new(AccountManager::new(None, SwitchingWeights::default()));
        let playback = am
            .add_account("play".into(), "c".into(), "s".into(), "rt-play".into(), None)
            .await
            .unwrap();
        assert!(am.find_catalog_account().await.is_none());
        let picked = am.select_catalog_account().await.unwrap();
        assert_eq!(picked.id, playback.id);
    }
}
