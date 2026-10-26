use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use chrono::Utc;
use serde::Deserialize;
use sqlx::FromRow;
use sqlx::SqlitePool;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::error::AppError;
use crate::proxy_manager::ProxyManager;
use crate::upstash::UpstashStore;

/// Consecutive automatic FULL/PREVIEW checks that must return PREVIEW before
/// a playback account is moved to catalog-only.
pub const PREVIEW_CHECKS_BEFORE_CATALOG: i64 = 3;
/// PREVIEW answers in a row, each for a track another account then got as
/// FULL, after which ordinary traffic marks an account PREVIEW-only.
pub const TRAFFIC_PREVIEWS_BEFORE_PREVIEW_ONLY: u64 = 10;

/// How account selection weighs recent errors against time since last use.
#[derive(Clone, Debug)]
pub struct SwitchingWeights {
    /// An account whose recent requests all failed counts as used this many
    /// milliseconds later than it really was; half the errors, half as much.
    pub error_penalty_ms: i64,
}

impl Default for SwitchingWeights {
    fn default() -> Self {
        Self {
            error_penalty_ms: 10 * 60 * 1000,
        }
    }
}

/// A FULL/PREVIEW result this recent decides playback priority.
pub const FULL_CHECK_MAX_AGE_SECS: i64 = 8 * 60 * 60;
/// Recent errors fade with this half-life.
const RECENT_HALF_LIFE_MS: f64 = 15.0 * 60.0 * 1000.0;

/// Limits a pool lease must respect; 0 turns each one off.
#[derive(Clone, Copy, Debug, Default)]
pub struct LeaseRules {
    /// Requests one account sends to Tidal at once.
    pub concurrency: u32,
    /// Milliseconds an account rests after each request.
    pub rest_ms: u32,
    /// Requests one account may start within an hour.
    pub per_hour: u32,
}

/// Requests and errors with exponential decay, so the error rate reflects
/// roughly the last 15 minutes rather than the whole uptime.
#[derive(Clone, Default)]
struct RecentOutcomes {
    requests: f64,
    errors: f64,
    at_ms: i64,
}

impl RecentOutcomes {
    fn decay(&mut self, now_ms: i64) {
        if self.at_ms > 0 && now_ms > self.at_ms {
            let factor = 0.5f64.powf((now_ms - self.at_ms) as f64 / RECENT_HALF_LIFE_MS);
            self.requests *= factor;
            self.errors *= factor;
        }
        self.at_ms = self.at_ms.max(now_ms);
    }

    fn error_rate(&self) -> f64 {
        if self.requests < 1e-6 {
            0.0
        } else {
            (self.errors / self.requests).clamp(0.0, 1.0)
        }
    }
}

pub struct AccountState {
    uptime: OnceLock<Arc<Mutex<crate::account_uptime::AccountUptime>>>,
    db: OnceLock<SqlitePool>,
    refresh_state_lock: Mutex<()>,
    pub id: String,
    pub label: String,
    pub client_id: String,
    pub client_secret: String,
    refresh_token: std::sync::RwLock<String>,
    pub user_id: RwLock<Option<String>>,
    pub access_token: RwLock<Option<String>>,
    pub token_expires_at: AtomicI64,
    pub is_active: AtomicBool,
    /// Unix time when the account last became inactive; zero means unknown or active.
    pub disabled_at: AtomicI64,
    /// Dedicated metadata credential (upstream CATALOG_* / token.json role).
    /// Never leased to playback: excluded from selection and pool counts.
    pub is_catalog: AtomicBool,
    /// True when the system (not the owner) deactivated the account.
    /// Only these are eligible for auto-heal; manual OFF is never touched.
    pub auto_disabled: AtomicBool,
    pub heal_failures: AtomicU64,
    pub heal_next_retry: AtomicI64,
    /// Last real refresh failure; cooldown checks must never replace its cause.
    last_refresh_error: std::sync::RwLock<Option<String>>,
    /// Preferred OAuth client-authentication shape for the next attempt.
    pub auth_use_basic: AtomicBool,
    /// Consecutive auth-endpoint 403 responses on the account's current proxy.
    /// Four failures arm a proxy change for the next recovery cycle.
    pub auth_forbidden_streak: AtomicU64,
    /// Unix time of the first 403 from Tidal auth since the last successful
    /// token refresh (0 = none). After `auth_failure_disable_hours` the
    /// account is switched off. Persisted, so restarts keep counting.
    pub auth_failing_since: AtomicI64,
    /// PREVIEW answers in a row from ordinary track requests that another
    /// account then served FULL. Reset by any FULL result.
    pub traffic_preview_streak: AtomicU64,
    /// Tidal account country (ISO 3166-1 alpha-2), learned from auth or
    /// session responses. None until Tidal has told us.
    country_code: std::sync::RwLock<Option<String>>,
    /// Latest Tidal subscription answer (plan, status, validUntil, …) and
    /// when it was fetched. Refreshed daily and after token refreshes by
    /// auto-heal, or on demand.
    subscription: std::sync::RwLock<Option<serde_json::Value>>,
    pub subscription_checked_at: AtomicI64,
    /// Why the latest automatic check failed, and when; cleared by the next
    /// answer. A failure never replaces the stored answer.
    subscription_error: std::sync::RwLock<Option<String>>,
    pub subscription_error_at: AtomicI64,
    pub rejected_access_token: RwLock<Option<String>>,
    pub notes: RwLock<String>,
    pub last_used: AtomicI64,
    pub request_count: AtomicU64,
    pub error_count: AtomicU64,
    /// Metadata requests this catalog account is serving right now.
    pub catalog_inflight: AtomicUsize,
    /// Playback requests this account is serving right now.
    pub playback_inflight: AtomicUsize,
    /// Metadata requests this playback account is serving right now.
    pub metadata_inflight: AtomicUsize,
    /// Unix milliseconds until which this playback account rests after its
    /// last request (`track_request_delay_ms`).
    pub rest_until_ms: AtomicI64,
    /// Unix milliseconds of the latest selection; the least recently used
    /// account goes first.
    pub last_pick_ms: AtomicI64,
    recent: std::sync::Mutex<RecentOutcomes>,
    /// Start times (Unix ms) of requests within the last hour, for the
    /// per-hour limit.
    hour_picks: std::sync::Mutex<std::collections::VecDeque<i64>>,
    /// Temporary upstream 429 pause. Kept separate from account health so
    /// rate limiting never disables a valid credential.
    pub rate_limited_until: AtomicI64,
    /// 429 pauses in a row, each starting within `RATE_LIMIT_FORGIVE_SECS`
    /// of the previous one's end. Every strike doubles the next pause.
    pub rate_limit_strikes: AtomicU32,
    /// Result of the latest automatic or manual FULL/PREVIEW probe; informational only.
    pub premium_status: RwLock<String>,
    pub premium_checked_at: AtomicI64,
    /// Automatic checks in a row that returned PREVIEW. A FULL result resets
    /// it; inconclusive checks leave it unchanged.
    pub preview_streak: AtomicI64,
    /// Original insertion time. Used to keep the admin account list stable.
    pub created_at: AtomicI64,
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
            uptime: OnceLock::new(),
            db: OnceLock::new(),
            refresh_state_lock: Mutex::new(()),
            id,
            label,
            client_id,
            client_secret: normalize_client_secret(&client_secret),
            refresh_token: std::sync::RwLock::new(refresh_token),
            user_id: RwLock::new(user_id),
            access_token: RwLock::new(None),
            token_expires_at: AtomicI64::new(0),
            is_active: AtomicBool::new(is_active),
            disabled_at: AtomicI64::new(0),
            is_catalog: AtomicBool::new(false),
            auto_disabled: AtomicBool::new(false),
            heal_failures: AtomicU64::new(0),
            heal_next_retry: AtomicI64::new(0),
            last_refresh_error: std::sync::RwLock::new(None),
            auth_use_basic: AtomicBool::new(false),
            auth_forbidden_streak: AtomicU64::new(0),
            auth_failing_since: AtomicI64::new(0),
            traffic_preview_streak: AtomicU64::new(0),
            country_code: std::sync::RwLock::new(None),
            subscription: std::sync::RwLock::new(None),
            subscription_checked_at: AtomicI64::new(0),
            subscription_error: std::sync::RwLock::new(None),
            subscription_error_at: AtomicI64::new(0),
            rejected_access_token: RwLock::new(None),
            notes: RwLock::new(notes),
            last_used: AtomicI64::new(0),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            catalog_inflight: AtomicUsize::new(0),
            playback_inflight: AtomicUsize::new(0),
            metadata_inflight: AtomicUsize::new(0),
            rest_until_ms: AtomicI64::new(0),
            last_pick_ms: AtomicI64::new(0),
            recent: std::sync::Mutex::new(RecentOutcomes::default()),
            hour_picks: std::sync::Mutex::new(std::collections::VecDeque::new()),
            rate_limited_until: AtomicI64::new(0),
            rate_limit_strikes: AtomicU32::new(0),
            premium_status: RwLock::new("unknown".to_string()),
            premium_checked_at: AtomicI64::new(0),
            preview_streak: AtomicI64::new(0),
            created_at: AtomicI64::new(0),
            updated_at: AtomicI64::new(0),
        }
    }

    /// Copy live counters from `old` into a rebuilt state, so admin
    /// edits and cross-instance merges never wipe stats or tokens.
    pub fn carry_over(new: &AccountState, old: &AccountState) {
        let _ = new.uptime.set(old.uptime_history().clone());
        if let Some(db) = old.db.get() {
            new.attach_db(db);
        }
        new.last_used
            .store(old.last_used.load(Ordering::Relaxed), Ordering::Relaxed);
        new.request_count
            .store(old.request_count.load(Ordering::Relaxed), Ordering::Relaxed);
        new.error_count
            .store(old.error_count.load(Ordering::Relaxed), Ordering::Relaxed);
        new.last_pick_ms
            .store(old.last_pick_ms.load(Ordering::Relaxed), Ordering::Relaxed);
        if let (Ok(src), Ok(mut dst)) = (old.recent.lock(), new.recent.lock()) {
            *dst = src.clone();
        }
        if let (Ok(src), Ok(mut dst)) = (old.hour_picks.lock(), new.hour_picks.lock()) {
            *dst = src.clone();
        }
        new.rate_limited_until.store(
            old.rate_limited_until.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        new.rate_limit_strikes.store(
            old.rate_limit_strikes.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        new.created_at.store(
            old.created_at.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        new.token_expires_at.store(
            old.token_expires_at.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        new.is_catalog
            .store(old.is_catalog.load(Ordering::Relaxed), Ordering::Relaxed);
        new.auto_disabled
            .store(old.auto_disabled.load(Ordering::Relaxed), Ordering::Relaxed);
        new.disabled_at
            .store(old.disabled_at.load(Ordering::Relaxed), Ordering::Relaxed);
        new.heal_failures
            .store(old.heal_failures.load(Ordering::Relaxed), Ordering::Relaxed);
        new.set_country(old.country());
        *new.subscription.write().unwrap_or_else(|e| e.into_inner()) = old.subscription();
        new.subscription_checked_at.store(
            old.subscription_checked_at.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        *new.subscription_error.write().unwrap_or_else(|e| e.into_inner()) = old.subscription_error();
        new.subscription_error_at.store(
            old.subscription_error_at.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        new.heal_next_retry.store(
            old.heal_next_retry.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        if new.client_id == old.client_id
            && new.client_secret == old.client_secret
            && new.refresh_token() == old.refresh_token()
        {
            new.set_last_refresh_error(old.last_refresh_error());
            new.auth_use_basic.store(
                old.auth_use_basic.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            new.auth_forbidden_streak.store(
                old.auth_forbidden_streak.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            new.auth_failing_since.store(
                old.auth_failing_since.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            if let (Ok(src), Ok(mut dst)) = (old.access_token.try_read(), new.access_token.try_write()) {
                *dst = src.clone();
            }
            if let (Ok(src), Ok(mut dst)) = (old.rejected_access_token.try_read(), new.rejected_access_token.try_write()) {
                *dst = src.clone();
            }
            if let (Ok(src), Ok(mut dst)) = (
                old.premium_status.try_read(),
                new.premium_status.try_write(),
            ) {
                *dst = src.clone();
            }
            new.premium_checked_at.store(
                old.premium_checked_at.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            new.preview_streak
                .store(old.preview_streak.load(Ordering::Relaxed), Ordering::Relaxed);
        } else {
            new.token_expires_at.store(0, Ordering::Relaxed);
            new.heal_failures.store(0, Ordering::Relaxed);
            new.heal_next_retry.store(0, Ordering::Relaxed);
            new.auth_forbidden_streak.store(0, Ordering::Relaxed);
        }
    }

    fn uptime_history(&self) -> &Arc<Mutex<crate::account_uptime::AccountUptime>> {
        self.uptime.get_or_init(|| Arc::new(Mutex::new(Default::default())))
    }

    pub async fn uptime_summary(&self, now: i64) -> crate::account_uptime::UptimeSummary {
        self.observe_uptime(now).await;
        self.uptime_history().lock().await.summary(now)
    }

    /// Claim one concurrent metadata slot. `limit` 0 means unlimited.
    fn try_claim_catalog_slot(&self, limit: u32) -> bool {
        let limit = limit as usize;
        let mut current = self.catalog_inflight.load(Ordering::Relaxed);
        loop {
            if limit > 0 && current >= limit {
                return false;
            }
            match self.catalog_inflight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }

    /// A token Tidal accepts right now: present, unexpired and not rejected.
    pub(crate) async fn has_usable_token(&self, now: i64) -> bool {
        let token = self.access_token.read().await;
        let Some(token) = token.as_deref().filter(|token| !token.is_empty()) else {
            return false;
        };
        let rejected = self.rejected_access_token.read().await;
        self.token_expires_at.load(Ordering::Relaxed) > now && rejected.as_deref() != Some(token)
    }

    pub(crate) async fn observe_uptime(&self, now: i64) {
        // Serialize the token snapshot with history updates, including rejected-token events.
        let mut history = self.uptime_history().lock().await;
        self.record_uptime(&mut history, now).await;
    }

    /// Drop recorded history and restart it from the current state, without letting a
    /// concurrent observation land between the two.
    pub(crate) async fn reset_uptime(&self, now: i64) -> Result<(), sqlx::Error> {
        let mut history = self.uptime_history().lock().await;
        history.reset(self.db.get(), &self.id).await?;
        self.record_uptime(&mut history, now).await;
        Ok(())
    }

    async fn record_uptime(&self, history: &mut crate::account_uptime::AccountUptime, now: i64) {
        let active = self.is_active.load(Ordering::Relaxed);
        let expires = self.token_expires_at.load(Ordering::Relaxed);
        let token = self.access_token.read().await;
        let token_ready = active && expires > now
            && token.as_deref().is_some_and(|token| !token.is_empty())
            && self.rejected_access_token.read().await.as_deref() != token.as_deref();
        drop(token);
        if let Err(error) = history.observe(
            self.db.get(), &self.id, active, token_ready,
            token_ready.then_some(expires), now,
        ).await {
            tracing::warn!(account = %self.id, "Could not persist account uptime: {error}");
        }
    }

    pub fn subscription(&self) -> Option<serde_json::Value> {
        self.subscription.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn subscription_error(&self) -> Option<String> {
        self.subscription_error.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Keep the latest subscription answer, in memory and in SQLite.
    pub(crate) async fn store_subscription(&self, subscription: serde_json::Value, checked_at: i64) {
        *self.subscription.write().unwrap_or_else(|e| e.into_inner()) = Some(subscription.clone());
        *self.subscription_error.write().unwrap_or_else(|e| e.into_inner()) = None;
        self.subscription_error_at.store(0, Ordering::Relaxed);
        self.subscription_checked_at.store(checked_at, Ordering::Relaxed);
        if let Some(db) = self.db.get() {
            if let Err(error) = sqlx::query(
                "UPDATE accounts SET subscription_json = ?, subscription_checked_at = ?, subscription_error = NULL, subscription_error_at = 0 WHERE id = ?",
            )
            .bind(subscription.to_string())
            .bind(checked_at)
            .bind(&self.id)
            .execute(db)
            .await
            {
                tracing::warn!(account = %self.id, "Could not save subscription: {error}");
            }
        }
    }

    /// Keep why an automatic check failed, so the panel shows it and the
    /// next attempt comes sooner; the last good answer and its time stay.
    pub(crate) async fn store_subscription_error(&self, error: String, failed_at: i64) {
        *self.subscription_error.write().unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
        self.subscription_error_at.store(failed_at, Ordering::Relaxed);
        if let Some(db) = self.db.get() {
            if let Err(error) = sqlx::query(
                "UPDATE accounts SET subscription_error = ?, subscription_error_at = ? WHERE id = ?",
            )
            .bind(error)
            .bind(failed_at)
            .bind(&self.id)
            .execute(db)
            .await
            {
                tracing::warn!(account = %self.id, "Could not save subscription error: {error}");
            }
        }
    }

    pub fn country(&self) -> Option<String> {
        self.country_code.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Keep a valid two-letter code; anything else is ignored.
    pub(crate) fn set_country(&self, code: Option<String>) {
        let Some(code) = code
            .map(|c| c.trim().to_ascii_uppercase())
            .filter(|c| c.len() == 2 && c.chars().all(|ch| ch.is_ascii_alphabetic()))
        else {
            return;
        };
        *self.country_code.write().unwrap_or_else(|e| e.into_inner()) = Some(code);
    }

    pub fn last_refresh_error(&self) -> Option<String> {
        self.last_refresh_error.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn set_last_refresh_error(&self, error: Option<String>) {
        *self.last_refresh_error.write().unwrap_or_else(|e| e.into_inner()) = error;
    }

    pub fn refresh_token(&self) -> String {
        self.refresh_token.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn replace_refresh_token(&self, token: String) {
        *self.refresh_token.write().unwrap_or_else(|e| e.into_inner()) = token;
    }

    pub(crate) fn attach_db(&self, db: &SqlitePool) {
        let _ = self.db.set(db.clone());
    }

    pub(crate) async fn persist_refresh_state(&self) -> Result<(), sqlx::Error> {
        let Some(db) = self.db.get() else {
            return Ok(());
        };
        let _guard = self.refresh_state_lock.lock().await;
        // A late result from credentials replaced by an admin must not put
        // the replacement credentials into the old cooldown.
        sqlx::query(
            "UPDATE accounts SET heal_failures = ?, heal_next_retry = ?, rejected_access_token = ?, last_refresh_error = ?,
             auth_failing_since = ?, country_code = COALESCE(?, country_code)
             WHERE id = ? AND client_id = ? AND client_secret = ? AND refresh_token = ?",
        )
        .bind(i64::try_from(self.heal_failures.load(Ordering::Relaxed)).unwrap_or(i64::MAX))
        .bind(self.heal_next_retry.load(Ordering::Relaxed))
        .bind(self.rejected_access_token.read().await.clone())
        .bind(self.last_refresh_error())
        .bind(self.auth_failing_since.load(Ordering::Relaxed))
        .bind(self.country())
        .bind(&self.id)
        .bind(&self.client_id)
        .bind(&self.client_secret)
        .bind(self.refresh_token())
        .execute(db)
        .await?;
        Ok(())
    }
}

/// One claimed metadata slot on a catalog account; released on drop.
pub struct CatalogSlot(Arc<AccountState>);

impl CatalogSlot {
    pub fn account(&self) -> &Arc<AccountState> {
        &self.0
    }
}

impl Drop for CatalogSlot {
    fn drop(&mut self) {
        self.0.catalog_inflight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What a pool lease is used for. Each kind has its own in-flight counter;
/// the per-account limit applies to their sum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseKind {
    /// Tracks, video, manifests and licenses.
    Playback,
    /// Metadata served by a playback account.
    Metadata,
}

impl LeaseKind {
    fn counter(self, account: &AccountState) -> &AtomicUsize {
        match self {
            LeaseKind::Playback => &account.playback_inflight,
            LeaseKind::Metadata => &account.metadata_inflight,
        }
    }
}

impl AccountState {
    /// Share of failed requests, weighted toward the last 15 minutes.
    pub fn recent_error_rate(&self) -> f64 {
        let now_ms = Utc::now().timestamp_millis();
        self.recent
            .lock()
            .map(|mut recent| {
                recent.decay(now_ms);
                recent.error_rate()
            })
            .unwrap_or(0.0)
    }

    fn note_recent(&self, error: bool) {
        if let Ok(mut recent) = self.recent.lock() {
            recent.decay(Utc::now().timestamp_millis());
            if error {
                recent.errors += 1.0;
            } else {
                recent.requests += 1.0;
            }
        }
    }

    /// Requests started within the last hour.
    pub fn requests_last_hour(&self, now_ms: i64) -> usize {
        self.hour_picks
            .lock()
            .map(|mut picks| {
                while picks.front().is_some_and(|at| *at <= now_ms - 3_600_000) {
                    picks.pop_front();
                }
                picks.len()
            })
            .unwrap_or(0)
    }

    /// FULL/PREVIEW confidence for playback: 0 confirmed FULL, 1 not checked
    /// lately, 2 recently returned PREVIEW.
    async fn full_tier(&self, now: i64) -> u8 {
        let checked = self.premium_checked_at.load(Ordering::Relaxed);
        if checked <= 0 || checked < now - FULL_CHECK_MAX_AGE_SECS {
            return 1;
        }
        match self.premium_status.read().await.as_str() {
            "premium" => 0,
            "preview-only" => 2,
            _ => 1,
        }
    }

    /// Seconds until this account can try a token refresh again, when it has
    /// no usable token and its last refresh failed. Such an account cannot
    /// serve anything, so selection skips it instead of failing over from it.
    async fn refresh_cooldown(&self, now: i64) -> Option<u64> {
        let retry_at = self.heal_next_retry.load(Ordering::Relaxed);
        if retry_at <= now || self.has_usable_token(now).await {
            return None;
        }
        Some((retry_at - now) as u64)
    }

    /// Requests of any kind this playback account is sending to Tidal now.
    fn pool_inflight(&self) -> usize {
        self.playback_inflight.load(Ordering::SeqCst) + self.metadata_inflight.load(Ordering::SeqCst)
    }
}

/// How long a request waits for a free per-account slot before giving up.
const LEASE_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Woken whenever a pool lease is released, so waiting requests retry at once.
fn lease_freed() -> &'static tokio::sync::Notify {
    static FREED: OnceLock<tokio::sync::Notify> = OnceLock::new();
    FREED.get_or_init(tokio::sync::Notify::new)
}

/// One in-flight request on a pool account; released on drop, after which
/// the account rests for the given number of milliseconds.
pub struct PlaybackLease(Arc<AccountState>, LeaseKind, u32);

impl PlaybackLease {
    pub fn account(&self) -> &Arc<AccountState> {
        &self.0
    }
}

impl Drop for PlaybackLease {
    fn drop(&mut self) {
        if self.2 > 0 {
            let until = Utc::now().timestamp_millis() + i64::from(self.2);
            self.0.rest_until_ms.fetch_max(until, Ordering::SeqCst);
        }
        self.1.counter(&self.0).fetch_sub(1, Ordering::SeqCst);
        lease_freed().notify_waiters();
    }
}

pub enum CatalogClaim {
    Slot(CatalogSlot),
    /// Usable catalog accounts exist, but all are at the concurrency limit.
    Busy,
    /// No active catalog account outside a 429 pause.
    Unavailable,
}

/// Legacy helpers stored URL-escaped base64 secrets. Decode once; never turn
/// a literal '+' into a space. Request builders perform their own encoding.
pub(crate) fn normalize_client_secret(secret: &str) -> String {
    percent_encoding::percent_decode_str(secret)
        .decode_utf8().map(|s| s.into_owned()).unwrap_or_else(|_| secret.to_string())
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
    pub disabled_at: Option<i64>,
    pub is_catalog: Option<i32>,
    pub notes: String,
    pub access_token: Option<String>,
    pub expires_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub created_at: i64,
    pub heal_failures: i64,
    pub heal_next_retry: i64,
    pub rejected_access_token: Option<String>,
    pub last_refresh_error: Option<String>,
    pub preview_streak: i64,
    pub rate_limited_until: i64,
    pub auth_failing_since: i64,
    pub country_code: Option<String>,
    pub subscription_json: Option<String>,
    pub subscription_checked_at: i64,
    pub subscription_error: Option<String>,
    pub subscription_error_at: i64,
}

/// Extract a numeric user id from an auto-generated "Tidal Account (<id>)" label.
fn user_id_from_label(label: &str) -> Option<&str> {
    let inner = label.strip_prefix("Tidal Account (")?.strip_suffix(')')?;
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
    /// Serializes select+account so concurrent requests can't snapshot the
    /// same counters and all pile onto one account. The critical section is
    /// in-memory only (no DB/Redis I/O under the lock).
    select_lock: Mutex<()>,
    /// Monotonic pick counter. Breaks exact score ties in round-robin order
    /// so identical accounts (e.g. fresh ones) spread evenly instead of all
    /// landing on index 0.
    rr_seq: AtomicU64,
    /// Round-robin cursor across catalog accounts (metadata load spreading).
    catalog_rr: AtomicU64,
    /// Shared cross-instance state (None = single-host mode, skip sync).
    upstash: OnceLock<Arc<UpstashStore>>,
    proxy_manager: OnceLock<Arc<ProxyManager>>,
}

impl AccountManager {
    /// First pause when Tidal sends no Retry-After, which it usually does not.
    const DEFAULT_RATE_LIMIT_SECS: u64 = 30;
    /// Longest pause that repeated strikes build up to.
    const RATE_LIMIT_MAX_SECS: u64 = 15 * 60;
    /// A 429 this long after the previous pause ended starts again at strike 1.
    const RATE_LIMIT_FORGIVE_SECS: i64 = 30 * 60;

    /// Retry-After may be seconds or an HTTP date. An absent/malformed value
    /// still gets a conservative pause rather than another immediate request.
    pub(crate) fn retry_after_secs(value: Option<&str>) -> u64 {
        let now = Utc::now().timestamp();
        let parsed = value.and_then(|raw| {
            raw.trim().parse::<u64>().ok().or_else(|| {
                chrono::DateTime::parse_from_rfc2822(raw.trim())
                    .ok()
                    .map(|date| date.timestamp().saturating_sub(now).max(0) as u64)
            })
        });
        parsed.unwrap_or(Self::DEFAULT_RATE_LIMIT_SECS).max(1)
    }

    pub(crate) fn rate_limit_remaining(account: &AccountState) -> Option<u64> {
        let remaining = account
            .rate_limited_until
            .load(Ordering::Relaxed)
            .saturating_sub(Utc::now().timestamp());
        (remaining > 0).then_some(remaining as u64)
    }

    /// Pause for the given strike: Retry-After (or the default) doubled for
    /// every earlier strike, up to `RATE_LIMIT_MAX_SECS` unless Tidal asked
    /// for longer. Pure function, unit tested.
    pub(crate) fn rate_limit_pause_secs(retry_after: Option<&str>, strike: u32) -> u64 {
        let base = Self::retry_after_secs(retry_after);
        let doubled = base.saturating_mul(1u64 << strike.saturating_sub(1).min(16));
        doubled.min(Self::RATE_LIMIT_MAX_SECS.max(base))
    }

    /// Strikes against an account that is resting after recent 429s; 0 once
    /// it has gone `RATE_LIMIT_FORGIVE_SECS` without one. Selection puts
    /// such accounts after the others, so their pause is not followed by
    /// another burst straight away.
    pub(crate) fn rate_limit_heat(account: &AccountState, now: i64) -> u32 {
        let until = account.rate_limited_until.load(Ordering::Relaxed);
        if until <= 0 || now - until > Self::RATE_LIMIT_FORGIVE_SECS {
            return 0;
        }
        account.rate_limit_strikes.load(Ordering::Relaxed)
    }

    pub(crate) async fn pause_account(account: &AccountState, retry_after: Option<&str>) -> u64 {
        let now = Utc::now().timestamp();
        let previous_until = account.rate_limited_until.load(Ordering::Relaxed);
        let strike = if now < previous_until {
            // Requests already in flight when the pause began are the same
            // burst, not a new strike.
            account.rate_limit_strikes.load(Ordering::Relaxed).max(1)
        } else if previous_until > 0 && now - previous_until <= Self::RATE_LIMIT_FORGIVE_SECS {
            account.rate_limit_strikes.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            account.rate_limit_strikes.store(1, Ordering::Relaxed);
            1
        };
        let seconds = Self::rate_limit_pause_secs(retry_after, strike);
        let until = now.saturating_add(i64::try_from(seconds).unwrap_or(i64::MAX));
        account.rate_limited_until.fetch_max(until, Ordering::Relaxed);
        // The deadline is absolute, so a restart keeps the remaining pause
        // instead of sending the account straight back to Tidal.
        if let Some(db) = account.db.get() {
            if let Err(error) = sqlx::query(
                "UPDATE accounts SET rate_limited_until = MAX(rate_limited_until, ?) WHERE id = ?",
            )
            .bind(until)
            .bind(&account.id)
            .execute(db)
            .await
            {
                tracing::warn!(account = %account.id, "Could not persist rate limit pause: {error}");
            }
        }
        Self::rate_limit_remaining(account).unwrap_or(seconds)
    }

    pub fn new(db: Option<SqlitePool>, weights: SwitchingWeights) -> Self {
        Self {
            accounts: RwLock::new(Vec::new()),
            weights,
            db,
            select_lock: Mutex::new(()),
            rr_seq: AtomicU64::new(0),
            catalog_rr: AtomicU64::new(0),
            upstash: OnceLock::new(),
            proxy_manager: OnceLock::new(),
        }
    }

    /// Attach shared state once at startup (before serving).
    pub fn set_upstash(&self, store: Option<Arc<UpstashStore>>) {
        if let Some(s) = store {
            let _ = self.upstash.set(s);
        }
    }

    pub fn set_proxy_manager(&self, manager: Arc<ProxyManager>) {
        let _ = self.proxy_manager.set(manager);
    }

    fn upstash(&self) -> Option<Arc<UpstashStore>> {
        self.upstash.get().cloned()
    }

    async fn observe_uptime(&self, account: &AccountState) {
        if let Some(db) = &self.db {
            account.attach_db(db);
        }
        account.observe_uptime(Utc::now().timestamp()).await;
    }

    async fn load_uptime(&self, account: &AccountState) -> Result<(), sqlx::Error> {
        if let Some(db) = &self.db {
            *account.uptime_history().lock().await = crate::account_uptime::AccountUptime::load(
                db, &account.id, Utc::now().timestamp(),
            ).await?;
        }
        self.observe_uptime(account).await;
        Ok(())
    }

    /// Restart availability tracking for every account; returns how many were reset.
    pub async fn reset_uptime(&self) -> Result<usize, AppError> {
        let accounts = self.list_accounts().await;
        let now = Utc::now().timestamp();
        for account in &accounts {
            if let Some(db) = &self.db {
                account.attach_db(db);
            }
            account.reset_uptime(now).await?;
        }
        Ok(accounts.len())
    }

    pub async fn load_from_db(&self) -> Result<(), AppError> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok(()),
        };

        let rows: Vec<DbAccountRow> = sqlx::query_as::<_, DbAccountRow>(
            "SELECT a.id, a.label, a.client_id, a.client_secret, a.refresh_token,
             a.user_id, a.is_active, a.auto_disabled, a.disabled_at, a.is_catalog, a.notes,
             t.access_token, t.expires_at, a.updated_at, a.created_at,
             a.heal_failures, a.heal_next_retry, a.rejected_access_token, a.last_refresh_error,
             a.preview_streak, a.rate_limited_until, a.auth_failing_since, a.country_code,
             a.subscription_json, a.subscription_checked_at, a.subscription_error,
             a.subscription_error_at
             FROM accounts a
             LEFT JOIN tokens t ON t.account_id = a.id
             ORDER BY a.created_at ASC",
        )
        .fetch_all(db)
        .await?;

        let mut accounts = self.accounts.write().await;
        for row in rows {
            let normalized_secret = normalize_client_secret(&row.client_secret);
            if normalized_secret != row.client_secret {
                sqlx::query("UPDATE accounts SET client_secret = ? WHERE id = ?")
                    .bind(&normalized_secret).bind(&row.id).execute(db).await?;
            }
            // Backfill user_id for accounts created before it was persisted
            // (label was "Tidal Account (<id>)").
            let user_id = match row.user_id {
                Some(uid) => Some(uid),
                None => user_id_from_label(&row.label).map(|s| s.to_string()),
            };
            if user_id.is_some() && self.db.is_some() {
                if let Some(db) = &self.db {
                    let _ = sqlx::query(
                        "UPDATE accounts SET user_id = ? WHERE id = ? AND user_id IS NULL",
                    )
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
            if row.is_active == 0 {
                state.disabled_at.store(row.disabled_at.unwrap_or(0), Ordering::Relaxed);
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
            state
                .created_at
                .store(row.created_at, Ordering::Relaxed);
            state
                .updated_at
                .store(row.updated_at.unwrap_or(0), Ordering::Relaxed);
            state.heal_failures.store(row.heal_failures.max(0) as u64, Ordering::Relaxed);
            state.heal_next_retry.store(row.heal_next_retry.max(0), Ordering::Relaxed);
            *state.rejected_access_token.write().await = row.rejected_access_token;
            state.set_last_refresh_error(row.last_refresh_error);
            state.preview_streak.store(row.preview_streak.max(0), Ordering::Relaxed);
            state.rate_limited_until.store(row.rate_limited_until.max(0), Ordering::Relaxed);
            state.auth_failing_since.store(row.auth_failing_since.max(0), Ordering::Relaxed);
            state.set_country(row.country_code);
            *state.subscription.write().unwrap_or_else(|e| e.into_inner()) = row
                .subscription_json
                .as_deref()
                .and_then(|json| serde_json::from_str(json).ok());
            state
                .subscription_checked_at
                .store(row.subscription_checked_at.max(0), Ordering::Relaxed);
            *state.subscription_error.write().unwrap_or_else(|e| e.into_inner()) = row.subscription_error;
            state
                .subscription_error_at
                .store(row.subscription_error_at.max(0), Ordering::Relaxed);
            self.load_uptime(&state).await?;
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
            .bind(&state.client_secret)
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

        state.created_at.store(now, Ordering::Relaxed);
        state.updated_at.store(now, Ordering::Relaxed);
        self.observe_uptime(&state).await;
        self.accounts.write().await.push(state.clone());
        self.push_account_to_redis(&state).await;
        if let Some(proxy_manager) = self.proxy_manager.get() {
            if proxy_manager.proxies_enabled() {
                if let Err(e) = proxy_manager.working_client_for(&id).await {
                    tracing::warn!("Account {id} added without a working proxy: {e}");
                }
            }
        }
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
            store
                .del_many(&[UpstashStore::k_account(id), UpstashStore::k_token(id)])
                .await;
            store.srem(&UpstashStore::k_accounts_set(), id).await;
        }
        if let Some(proxy_manager) = self.proxy_manager.get() {
            proxy_manager.forget_account(id).await;
        }
        Ok(())
    }

    pub async fn get_account_by_id(&self, id: &str) -> Option<Arc<AccountState>> {
        let accounts = self.accounts.read().await;
        accounts.iter().find(|a| a.id == id).cloned()
    }

    /// Record one use of an account (selection accounting). Shared by the
    /// pool selector, the catalog selector, and the preferred-account path
    /// so every served request is visible to the balancer.
    pub fn note_selection(account: &AccountState) {
        let now_ms = Utc::now().timestamp_millis();
        account.last_used.store(now_ms / 1000, Ordering::Relaxed);
        account.last_pick_ms.store(now_ms, Ordering::Relaxed);
        account.request_count.fetch_add(1, Ordering::Relaxed);
        account.note_recent(false);
        if let Ok(mut picks) = account.hour_picks.lock() {
            picks.push_back(now_ms);
        }
        crate::request_log::note_account(
            &account.id,
            &account.label,
            if account.is_catalog.load(Ordering::Relaxed) {
                "catalog"
            } else {
                "playback"
            },
        );
    }

    /// When an account counts as last used for selection: its real last
    /// pick, moved later by its recent error rate. The smallest value wins,
    /// so the pool rotates least-recently-used first and an account that
    /// keeps failing waits its turn longer. Pure function, unit tested.
    pub(crate) fn effective_last_use(
        weights: &SwitchingWeights,
        last_pick_ms: i64,
        recent_error_rate: f64,
    ) -> i64 {
        last_pick_ms + (weights.error_penalty_ms as f64 * recent_error_rate.clamp(0.0, 1.0)) as i64
    }

    /// Claim one in-flight playback request on an already chosen account.
    pub(crate) fn lease(account: &Arc<AccountState>) -> PlaybackLease {
        Self::lease_kind(account, LeaseKind::Playback, 0)
    }

    fn lease_kind(account: &Arc<AccountState>, kind: LeaseKind, rest_ms: u32) -> PlaybackLease {
        kind.counter(account).fetch_add(1, Ordering::SeqCst);
        Self::note_selection(account);
        PlaybackLease(account.clone(), kind, rest_ms)
    }

    /// Pick a playback account and hold one of its in-flight slots until the
    /// returned lease is dropped. No per-account limit.
    pub async fn lease_account_excluding(
        &self,
        exclude_ids: &[String],
    ) -> Result<PlaybackLease, AppError> {
        self.lease_account_for(exclude_ids, LeaseKind::Playback, LeaseRules::default()).await
    }

    /// Like `lease_account_excluding`, but an account that is busy with
    /// `rules.concurrency` requests (of any kind), resting after its last
    /// one, or at its hourly limit is skipped for a free one. Released
    /// leases make the account rest `rules.rest_ms`. When every usable
    /// account is busy the request waits for a slot, like a semaphore, and
    /// gives up after `LEASE_WAIT`.
    pub async fn lease_account_for(
        &self,
        exclude_ids: &[String],
        kind: LeaseKind,
        rules: LeaseRules,
    ) -> Result<PlaybackLease, AppError> {
        let LeaseRules { concurrency: limit, rest_ms, per_hour } = rules;
        let deadline = tokio::time::Instant::now() + LEASE_WAIT;
        loop {
            // Register before checking, so a release in between is not missed.
            let freed = lease_freed().notified();
            tokio::pin!(freed);
            freed.as_mut().enable();
            if let Some(lease) = self.try_lease(exclude_ids, kind, rules).await? {
                return Ok(lease);
            }
            if tokio::time::Instant::now() >= deadline {
                let mut limits = Vec::new();
                if limit > 0 {
                    limits.push(format!("at most {limit} request(s) to Tidal at a time"));
                }
                if rest_ms > 0 {
                    limits.push(format!("a {rest_ms} ms rest after each"));
                }
                if per_hour > 0 {
                    limits.push(format!("at most {per_hour} requests an hour"));
                }
                return Err(AppError::AccountsBusy(
                    format!(
                        "{}: each allows {}, and none freed up within {} s. Retry shortly.",
                        crate::error::ACCOUNTS_BUSY,
                        limits.join(", "),
                        LEASE_WAIT.as_secs()
                    ),
                    if per_hour > 0 { 5 } else { 1 },
                ));
            }
            // The timer also catches accounts that finish resting or become usable without a release.
            tokio::select! {
                _ = &mut freed => {}
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
            }
        }
    }

    /// One selection pass. `Ok(None)` means usable accounts exist but all
    /// of them are at the limit.
    async fn try_lease(
        &self,
        exclude_ids: &[String],
        kind: LeaseKind,
        rules: LeaseRules,
    ) -> Result<Option<PlaybackLease>, AppError> {
        let LeaseRules { concurrency: limit, rest_ms, per_hour } = rules;
        // Serialize select+account: without this, N concurrent requests read
        // the same counters and all pick the same (currently-best) account,
        // leaving the rest idle under load.
        let _guard = self.select_lock.lock().await;
        let accounts = self.accounts.read().await;
        if accounts.is_empty() {
            return Err(AppError::Internal(
                "No Tidal credentials available; add an account via the admin panel".into(),
            ));
        }

        let now = Utc::now().timestamp();
        let mut pool: Vec<usize> = Vec::new();
        let mut shortest_pause: Option<u64> = None;
        let mut shortest_cooldown: Option<u64> = None;
        for (i, account) in accounts.iter().enumerate() {
            // Catalog-only credentials never serve playback.
            if !account.is_active.load(Ordering::Relaxed)
                || account.is_catalog.load(Ordering::Relaxed)
            {
                continue;
            }
            if let Some(seconds) = Self::rate_limit_remaining(account) {
                if !exclude_ids.contains(&account.id) {
                    shortest_pause = Some(shortest_pause.map_or(seconds, |old| old.min(seconds)));
                }
                continue;
            }
            if let Some(seconds) = account.refresh_cooldown(now).await {
                shortest_cooldown = Some(shortest_cooldown.map_or(seconds, |old| old.min(seconds)));
                continue;
            }
            pool.push(i);
        }

        let now_ms = Utc::now().timestamp_millis();
        let usable: Vec<usize> = pool
            .into_iter()
            .filter(|&i| !exclude_ids.contains(&accounts[i].id))
            .collect();
        let eligible: Vec<usize> = usable
            .iter()
            .copied()
            .filter(|&i| {
                (limit == 0 || accounts[i].pool_inflight() < limit as usize)
                    && accounts[i].rest_until_ms.load(Ordering::SeqCst) <= now_ms
                    && (per_hour == 0 || accounts[i].requests_last_hour(now_ms) < per_hour as usize)
            })
            .collect();
        if eligible.is_empty() && !usable.is_empty() {
            return Ok(None);
        }

        if eligible.is_empty() {
            if let Some(seconds) = shortest_pause {
                return Err(AppError::RateLimited(seconds));
            }
            if let Some(seconds) = shortest_cooldown {
                return Err(AppError::ServiceUnavailableRetry(
                    "All accounts are waiting for a token refresh".into(),
                    seconds,
                ));
            }
            return Err(AppError::ServiceUnavailable(
                "All accounts are inactive or have expired tokens".into(),
            ));
        }

        // Order: confirmed FULL first for playback (unchecked next, recent
        // PREVIEW last), then fewest requests in flight, then accounts
        // without recent 429s, then the least recently used once recent
        // errors are counted in.
        let mut scored: Vec<(u8, usize, u32, i64, usize)> = Vec::with_capacity(eligible.len());
        for i in eligible {
            let account = &accounts[i];
            let tier = match kind {
                LeaseKind::Playback => account.full_tier(now).await,
                LeaseKind::Metadata => 0,
            };
            let last_use = Self::effective_last_use(
                &self.weights,
                account.last_pick_ms.load(Ordering::Relaxed),
                account.recent_error_rate(),
            );
            scored.push((tier, account.pool_inflight(), Self::rate_limit_heat(account, now), last_use, i));
        }
        // Rotate before the stable sort so exact ties take turns instead of
        // always starting at creation index 0.
        let seq = self.rr_seq.fetch_add(1, Ordering::Relaxed);
        let rot = (seq as usize) % scored.len();
        scored.rotate_left(rot);
        scored.sort_by_key(|&(tier, busy, heat, last_use, _)| (tier, busy, heat, last_use));
        Ok(Some(Self::lease_kind(&accounts[scored[0].4], kind, rest_ms)))
    }

    pub async fn select_account_excluding(
        &self,
        exclude_ids: &[String],
    ) -> Result<Arc<AccountState>, AppError> {
        let lease = self.lease_account_excluding(exclude_ids).await?;
        Ok(lease.account().clone())
    }

    pub async fn select_account(&self) -> Result<Arc<AccountState>, AppError> {
        self.select_account_excluding(&[]).await
    }

    /// Playback pool size (upstream: PlaybackCredentialPool.size).
    /// Only accounts that can serve a request right now count: active,
    /// non-catalog, holding a usable token and not on a 429 pause. Enabled
    /// accounts without a token must not widen the gate, or the working
    /// ones would each take several requests at once. Minimum 1 so a
    /// request still runs, triggers a token refresh and surfaces the real
    /// error when nothing is ready.
    pub async fn playback_slots(&self) -> usize {
        let now = Utc::now().timestamp();
        let accounts = self.accounts.read().await;
        let mut slots = 0;
        for account in accounts.iter() {
            if !account.is_catalog.load(Ordering::Relaxed)
                && account.is_active.load(Ordering::Relaxed)
                && Self::rate_limit_remaining(account).is_none()
                && account.has_usable_token(now).await
            {
                slots += 1;
            }
        }
        slots.max(1)
    }

    /// Number of selectable playback (active, non-catalog) accounts.
    pub async fn playback_count(&self) -> usize {
        self.accounts
            .read()
            .await
            .iter()
            .filter(|a| {
                !a.is_catalog.load(Ordering::Relaxed) && a.is_active.load(Ordering::Relaxed)
            })
            .count()
    }

    /// Dedicated metadata credential (upstream _catalog_cred). Prefers an
    /// active catalog account; returns None when none is flagged.
    pub async fn find_catalog_account(&self) -> Option<Arc<AccountState>> {
        let accounts = self.accounts.read().await;
        accounts
            .iter()
            .find(|a| a.is_catalog.load(Ordering::Relaxed) && a.is_active.load(Ordering::Relaxed))
            .cloned()
            .or_else(|| {
                accounts
                    .iter()
                    .find(|a| a.is_catalog.load(Ordering::Relaxed))
                    .cloned()
            })
    }

    /// Next active catalog account in round-robin order (`None` when no
    /// active catalog credential exists). Records the selection so catalog
    /// load shows up in stats like any other use.
    pub async fn next_active_catalog(&self) -> Option<Arc<AccountState>> {
        let accounts = self.accounts.read().await;
        let catalog: Vec<Arc<AccountState>> = accounts
            .iter()
            .filter(|a| {
                a.is_catalog.load(Ordering::Relaxed)
                    && a.is_active.load(Ordering::Relaxed)
                    && Self::rate_limit_remaining(a).is_none()
            })
            .cloned()
            .collect();
        if catalog.is_empty() {
            return None;
        }
        let seq = self.catalog_rr.fetch_add(1, Ordering::Relaxed);
        let acc = catalog[(seq as usize) % catalog.len()].clone();
        Self::note_selection(&acc);
        Some(acc)
    }

    /// Round-robin like `next_active_catalog`, but each pick also claims one
    /// of the account's concurrent metadata slots (`limit` 0 = unlimited).
    /// Accounts already at the limit are skipped, so the caller can send
    /// the overflow to the playback pool.
    pub async fn claim_catalog_slot(&self, limit: u32) -> CatalogClaim {
        let accounts = self.accounts.read().await;
        let now = Utc::now().timestamp();
        let mut catalog: Vec<&Arc<AccountState>> = Vec::new();
        for a in accounts.iter() {
            if a.is_catalog.load(Ordering::Relaxed)
                && a.is_active.load(Ordering::Relaxed)
                && Self::rate_limit_remaining(a).is_none()
                && a.refresh_cooldown(now).await.is_none()
            {
                catalog.push(a);
            }
        }
        if catalog.is_empty() {
            return CatalogClaim::Unavailable;
        }
        let start = (self.catalog_rr.fetch_add(1, Ordering::Relaxed) % catalog.len() as u64) as usize;
        catalog.rotate_left(start);
        // Round-robin among the accounts without recent 429s first.
        catalog.sort_by_key(|account| Self::rate_limit_heat(account, now));
        for account in catalog {
            if account.try_claim_catalog_slot(limit) {
                Self::note_selection(account);
                return CatalogClaim::Slot(CatalogSlot(account.clone()));
            }
        }
        CatalogClaim::Busy
    }

    /// Metadata account selection (upstream catalog=True): round-robin
    /// across active catalog credentials so metadata load spreads instead
    /// of pinning to the first catalog account, otherwise the normal pool.
    pub async fn select_catalog_account(&self) -> Result<Arc<AccountState>, AppError> {
        if let Some(acc) = self.next_active_catalog().await {
            return Ok(acc);
        }
        if let Some(seconds) = self.catalog_rate_limit_remaining().await {
            return Err(AppError::RateLimited(seconds));
        }
        self.select_account_excluding(&[]).await
    }

    pub(crate) async fn catalog_rate_limit_remaining(&self) -> Option<u64> {
        self.accounts
            .read()
            .await
            .iter()
            .filter(|a| {
                a.is_catalog.load(Ordering::Relaxed) && a.is_active.load(Ordering::Relaxed)
            })
            .filter_map(|a| Self::rate_limit_remaining(a))
            .min()
    }

    /// Flag or unflag an account as catalog-only (kept out of playback).
    pub async fn set_account_catalog(&self, id: &str, catalog: bool) -> Result<(), AppError> {
        if let Some(account) = self.get_account_by_id(id).await {
            account.is_catalog.store(catalog, Ordering::Relaxed);
            let now = Utc::now().timestamp();
            account.updated_at.store(now, Ordering::Relaxed);
            if let Some(db) = &self.db {
                sqlx::query("UPDATE accounts SET is_catalog = ?, updated_at = ? WHERE id = ?")
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
            account.note_recent(true);
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

    /// A real track request just came back FULL: the subscription works, so
    /// the account needs no separate FULL check for a while. Cheap enough to
    /// call on every such answer; it writes at most once a minute.
    pub async fn confirm_full(&self, account: &AccountState) {
        let now = Utc::now().timestamp();
        account.traffic_preview_streak.store(0, Ordering::Relaxed);
        if account.premium_checked_at.load(Ordering::Relaxed) > now - 60
            && account.premium_status.read().await.as_str() == "premium"
        {
            return;
        }
        *account.premium_status.write().await = "premium".to_string();
        account.premium_checked_at.store(now, Ordering::Relaxed);
        if account.preview_streak.swap(0, Ordering::Relaxed) != 0 {
            self.save_preview_streak(account).await;
        }
    }

    /// This account answered PREVIEW to a track another account then served
    /// FULL, so the track is fine and the account is not. After
    /// `TRAFFIC_PREVIEWS_BEFORE_PREVIEW_ONLY` in a row it is marked
    /// PREVIEW-only, which puts it last for playback until a FULL result.
    /// Returns true when it was marked.
    pub async fn note_traffic_preview(&self, account: &AccountState) -> bool {
        let streak = account.traffic_preview_streak.fetch_add(1, Ordering::Relaxed) + 1;
        if streak < TRAFFIC_PREVIEWS_BEFORE_PREVIEW_ONLY {
            return false;
        }
        account.traffic_preview_streak.store(0, Ordering::Relaxed);
        *account.premium_status.write().await = "preview-only".to_string();
        account.premium_checked_at.store(Utc::now().timestamp(), Ordering::Relaxed);
        tracing::warn!(
            account = %account.label,
            "Marked PREVIEW-only: {} tracks in a row came back PREVIEW but FULL on another account",
            TRAFFIC_PREVIEWS_BEFORE_PREVIEW_ONLY
        );
        true
    }

    pub async fn set_premium(&self, id: &str, status: &str) {
        if let Some(account) = self.get_account_by_id(id).await {
            *account.premium_status.write().await = status.to_string();
            account
                .premium_checked_at
                .store(Utc::now().timestamp(), Ordering::Relaxed);
            // Only a confirmed FULL result ends a PREVIEW streak; an
            // inconclusive check says nothing about the subscription.
            if status == "premium" {
                account.traffic_preview_streak.store(0, Ordering::Relaxed);
            }
            if status == "premium" && account.preview_streak.swap(0, Ordering::Relaxed) != 0 {
                self.save_preview_streak(&account).await;
            }
        }
    }

    /// Count one automatic check that returned PREVIEW. The third in a row
    /// makes the account catalog-only: it can still serve metadata, while
    /// playback would only return previews. Returns true when it was moved.
    pub async fn note_preview_check(&self, account: &AccountState) -> Result<bool, AppError> {
        let streak = account.preview_streak.fetch_add(1, Ordering::Relaxed) + 1;
        if streak < PREVIEW_CHECKS_BEFORE_CATALOG {
            self.save_preview_streak(account).await;
            return Ok(false);
        }
        // Keep the streak until the flag is stored, so a failed write is
        // retried by the next check.
        self.set_account_catalog(&account.id, true).await?;
        account.preview_streak.store(0, Ordering::Relaxed);
        self.save_preview_streak(account).await;
        Ok(true)
    }

    async fn save_preview_streak(&self, account: &AccountState) {
        let Some(db) = &self.db else {
            return;
        };
        if let Err(error) = sqlx::query("UPDATE accounts SET preview_streak = ? WHERE id = ?")
            .bind(account.preview_streak.load(Ordering::Relaxed))
            .bind(&account.id)
            .execute(db)
            .await
        {
            tracing::warn!(account = %account.id, "Could not persist PREVIEW streak: {error}");
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
        let idx = accounts
            .iter()
            .position(|a| a.id == id)
            .ok_or_else(|| AppError::NotFound(format!("Account {} not found", id)))?;

        let old = &accounts[idx];
        let new_label = label.unwrap_or_else(|| old.label.clone());
        let new_client_id = client_id.unwrap_or_else(|| old.client_id.clone());
        let new_client_secret = client_secret.unwrap_or_else(|| old.client_secret.clone());
        let new_refresh_token = refresh_token.unwrap_or_else(|| old.refresh_token());
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
            let mut tx = db.begin().await?;
            sqlx::query(
                "UPDATE accounts SET label = ?, client_id = ?, client_secret = ?, refresh_token = ?, user_id = ?, updated_at = ?,
                 heal_failures = ?, heal_next_retry = ?, rejected_access_token = ?, last_refresh_error = ?, auth_failing_since = ? WHERE id = ?",
            )
            .bind(&updated.label)
            .bind(&updated.client_id)
            .bind(&updated.client_secret)
            .bind(updated.refresh_token())
            .bind(&new_user_id)
            .bind(now)
            .bind(i64::try_from(updated.heal_failures.load(Ordering::Relaxed)).unwrap_or(i64::MAX))
            .bind(updated.heal_next_retry.load(Ordering::Relaxed))
            .bind(updated.rejected_access_token.read().await.clone())
            .bind(updated.last_refresh_error())
            .bind(updated.auth_failing_since.load(Ordering::Relaxed))
            .bind(&old.id)
            .execute(&mut *tx)
            .await?;
            if updated.client_id != old.client_id || updated.client_secret != old.client_secret
                || updated.refresh_token() != old.refresh_token() {
                sqlx::query("DELETE FROM tokens WHERE account_id = ?")
                    .bind(id).execute(&mut *tx).await?;
            }
            tx.commit().await?;
        }

        accounts[idx] = updated.clone();
        self.observe_uptime(&updated).await;
        drop(accounts);
        self.push_account_to_redis(&updated).await;
        Ok(())
    }

    /// Mark an account as system-disabled (eligible for auto-heal) or clear it
    /// (owner intent / successful recovery — never auto-healed while clear).
    pub async fn set_auto_disabled(&self, id: &str, disabled: bool) -> Result<(), AppError> {
        if let Some(account) = self.get_account_by_id(id).await {
            let was_disabled = account.auto_disabled.swap(disabled, Ordering::Relaxed);
            if !disabled || !was_disabled {
                account.heal_failures.store(0, Ordering::Relaxed);
                account.heal_next_retry.store(0, Ordering::Relaxed);
                account.auth_forbidden_streak.store(0, Ordering::Relaxed);
                account.persist_refresh_state().await?;
            }
            let now = Utc::now().timestamp();
            account.updated_at.store(now, Ordering::Relaxed);
            if let Some(db) = &self.db {
                sqlx::query("UPDATE accounts SET auto_disabled = ?, updated_at = ? WHERE id = ?")
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
        let accounts = self.accounts.write().await;
        if let Some(account) = accounts.iter().find(|account| account.id == id) {
            let now = Utc::now().timestamp();
            let disabled_at = if active {
                0
            } else if account.is_active.load(Ordering::Relaxed) {
                now
            } else {
                account.disabled_at.load(Ordering::Relaxed)
            };
            // A rejected credential must stop serving requests immediately,
            // even if persisting the transition later fails.
            account.is_active.store(active, Ordering::Relaxed);
            account.auto_disabled.store(false, Ordering::Relaxed);
            account.heal_failures.store(0, Ordering::Relaxed);
            account.heal_next_retry.store(0, Ordering::Relaxed);
            account.auth_forbidden_streak.store(0, Ordering::Relaxed);
            // The owner decided: an auth-failure switch-off becomes a plain manual state.
            account.auth_failing_since.store(0, Ordering::Relaxed);
            account.disabled_at.store(disabled_at, Ordering::Relaxed);
            account.updated_at.store(now, Ordering::Relaxed);
            self.observe_uptime(account).await;
            if let Some(db) = &self.db {
                sqlx::query("UPDATE accounts SET is_active = ?, auto_disabled = 0, disabled_at = ?, updated_at = ?, heal_failures = 0, heal_next_retry = 0, auth_failing_since = 0 WHERE id = ?")
                    .bind(active as i32)
                    .bind(disabled_at)
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

    /// Switch off an account whose token refresh has failed with 403 for
    /// `max_secs`. Unlike `set_system_disabled` it is not auto-healed: the
    /// account stays off, and stops retrying, until the owner turns it on.
    pub(crate) async fn disable_after_auth_failures(&self, expected: &AccountState, max_secs: i64, now: i64) -> Result<bool, AppError> {
        let accounts = self.accounts.write().await;
        let Some(account) = accounts.iter().find(|a| std::ptr::eq(a.as_ref(), expected)) else {
            return Ok(false);
        };
        let since = account.auth_failing_since.load(Ordering::Relaxed);
        let enabled = account.is_active.load(Ordering::Relaxed) || account.auto_disabled.load(Ordering::Relaxed);
        if max_secs <= 0 || since <= 0 || now - since < max_secs || !enabled {
            return Ok(false);
        }
        account.is_active.store(false, Ordering::Relaxed);
        account.auto_disabled.store(false, Ordering::Relaxed);
        let disabled_at = match account.disabled_at.load(Ordering::Relaxed) {
            0 => now,
            at => at,
        };
        account.disabled_at.store(disabled_at, Ordering::Relaxed);
        account.updated_at.store(now, Ordering::Relaxed);
        self.observe_uptime(account).await;
        if let Some(db) = &self.db {
            sqlx::query("UPDATE accounts SET is_active = 0, auto_disabled = 0, disabled_at = ?, updated_at = ? WHERE id = ?")
                .bind(disabled_at).bind(now).bind(&account.id)
                .execute(db).await?;
        }
        self.push_account_to_redis(account).await;
        Ok(true)
    }

    /// Conditional background transition; an explicit owner OFF always wins.
    pub(crate) async fn set_system_disabled(&self, expected: &AccountState, disabled: bool) -> Result<bool, AppError> {
        let accounts = self.accounts.write().await;
        let Some(account) = accounts.iter().find(|a| std::ptr::eq(a.as_ref(), expected)) else {
            return Ok(false);
        };
        if disabled {
            if !account.is_active.load(Ordering::Relaxed) && !account.auto_disabled.load(Ordering::Relaxed) {
                return Ok(false);
            }
        } else if !account.auto_disabled.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let now = Utc::now().timestamp();
        account.auto_disabled.store(disabled, Ordering::Relaxed);
        account.is_active.store(!disabled, Ordering::Relaxed);
        let disabled_at = if disabled { account.disabled_at.load(Ordering::Relaxed).max(1) } else { 0 };
        let disabled_at = if disabled_at == 1 { now } else { disabled_at };
        account.disabled_at.store(disabled_at, Ordering::Relaxed);
        account.updated_at.store(now, Ordering::Relaxed);
        self.observe_uptime(account).await;
        if let Some(db) = &self.db {
            sqlx::query("UPDATE accounts SET is_active = ?, auto_disabled = ?, disabled_at = ?, updated_at = ? WHERE id = ?")
                .bind(!disabled as i32).bind(disabled as i32).bind(disabled_at).bind(now).bind(&account.id)
                .execute(db).await?;
        }
        self.push_account_to_redis(account).await;
        Ok(true)
    }

    pub(crate) async fn store_refreshed_credentials(
        &self, expected: &AccountState, old_refresh: &str, rotated: Option<&str>, token: &str, expires_at: i64,
    ) -> Result<(), AppError> {
        let accounts = self.accounts.write().await;
        let account = accounts.iter().find(|a| std::ptr::eq(a.as_ref(), expected))
            .ok_or_else(|| AppError::ServiceUnavailable("Account changed during refresh".into()))?;
        if account.refresh_token() != old_refresh {
            return Err(AppError::ServiceUnavailable("Credentials changed during refresh".into()));
        }
        if let Some(db) = &self.db {
            crate::token_manager::persist_token(db, account, old_refresh, rotated, token, expires_at).await?;
        }
        if let Some(rotated) = rotated { account.replace_refresh_token(rotated.into()); }
        *account.access_token.write().await = Some(token.into());
        account.token_expires_at.store(expires_at, Ordering::Relaxed);
        self.observe_uptime(account).await;
        account.updated_at.store(Utc::now().timestamp(), Ordering::Relaxed);
        self.push_account_to_redis(account).await;
        Ok(())
    }

    /// Serialize one account for Redis. Contains Tidal secrets — the
    /// payload is never logged, only key names at debug level.
    async fn account_to_json(acc: &AccountState) -> String {
        serde_json::json!({
            "id": acc.id,
            "label": acc.label,
            "client_id": acc.client_id,
            "client_secret": acc.client_secret,
            "refresh_token": acc.refresh_token(),
            "user_id": acc.user_id.read().await.clone(),
            "is_active": acc.is_active.load(Ordering::Relaxed),
            "disabled_at": acc.disabled_at.load(Ordering::Relaxed),
            "auto_disabled": acc.auto_disabled.load(Ordering::Relaxed),
            "is_catalog": acc.is_catalog.load(Ordering::Relaxed),
            "notes": acc.notes.read().await.clone(),
            "created_at": acc.created_at.load(Ordering::Relaxed),
            "updated_at": acc.updated_at.load(Ordering::Relaxed),
        })
        .to_string()
    }

    /// Publish one account record + index entry (no expiry: backup
    /// semantics). Best-effort; called after every local mutation.
    pub(crate) async fn push_account_to_redis(&self, acc: &AccountState) {
        let store = match self.upstash() {
            Some(s) => s,
            None => return,
        };
        let payload = Self::account_to_json(acc).await;
        store
            .set(&UpstashStore::k_account(&acc.id), &payload, None)
            .await;
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
            let keys: Vec<String> = remote_ids
                .iter()
                .map(|id| UpstashStore::k_account(id))
                .collect();
            let values = store.mget(&keys).await;
            let mut accounts = self.accounts.write().await;
            let mut restored_ids = Vec::new();
            for (id, payload) in remote_ids.iter().zip(values.iter()) {
                let raw = match payload {
                    Some(r) => r,
                    None => continue,
                };
                let v: serde_json::Value = match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let str_field =
                    |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                let remote_updated = v.get("updated_at").and_then(|x| x.as_i64()).unwrap_or(0);
                let remote_created = v
                    .get("created_at")
                    .and_then(|x| x.as_i64())
                    .filter(|ts| *ts > 0)
                    .or_else(|| (remote_updated > 0).then_some(remote_updated))
                    .unwrap_or_else(|| Utc::now().timestamp());
                let user_id = v
                    .get("user_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let is_active = v.get("is_active").and_then(|x| x.as_bool()).unwrap_or(true);
                let disabled_at = if is_active {
                    0
                } else {
                    v.get("disabled_at").and_then(|x| x.as_i64()).unwrap_or(0)
                };
                let auto_disabled = v
                    .get("auto_disabled")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                let is_catalog = v
                    .get("is_catalog")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
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
                    rebuilt.disabled_at.store(disabled_at, Ordering::Relaxed);
                    rebuilt.auto_disabled.store(auto_disabled, Ordering::Relaxed);
                    rebuilt.is_catalog.store(is_catalog, Ordering::Relaxed);
                    rebuilt.updated_at.store(remote_updated, Ordering::Relaxed);
                    if let Some(db) = &self.db {
                        let _ = sqlx::query(
                            "UPDATE accounts SET label = ?, client_id = ?, client_secret = ?,
                             refresh_token = ?, user_id = ?, is_active = ?, auto_disabled = ?, disabled_at = ?,
                             is_catalog = ?, notes = ?, updated_at = ?,
                             heal_failures = ?, heal_next_retry = ?, rejected_access_token = ?, last_refresh_error = ?, auth_failing_since = ? WHERE id = ?",
                        )
                        .bind(&rebuilt.label)
                        .bind(&rebuilt.client_id)
                        .bind(&rebuilt.client_secret)
                        .bind(rebuilt.refresh_token())
                        .bind(&user_id)
                        .bind(is_active as i32)
                        .bind(auto_disabled as i32)
                        .bind(disabled_at)
                        .bind(is_catalog as i32)
                        .bind(&notes)
                        .bind(remote_updated)
                        .bind(i64::try_from(rebuilt.heal_failures.load(Ordering::Relaxed)).unwrap_or(i64::MAX))
                        .bind(rebuilt.heal_next_retry.load(Ordering::Relaxed))
                        .bind(rebuilt.rejected_access_token.read().await.clone())
                        .bind(rebuilt.last_refresh_error())
                        .bind(rebuilt.auth_failing_since.load(Ordering::Relaxed))
                        .bind(&rebuilt.id)
                        .execute(db)
                        .await;
                    }
                    self.observe_uptime(&rebuilt).await;
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
                    state.disabled_at.store(disabled_at, Ordering::Relaxed);
                    if is_catalog {
                        state.is_catalog.store(true, Ordering::Relaxed);
                    }
                    state.created_at.store(remote_created, Ordering::Relaxed);
                    state.updated_at.store(remote_updated, Ordering::Relaxed);
                    if let Some(db) = &self.db {
                        let _ = sqlx::query(
                            "INSERT INTO accounts (id, label, client_id, client_secret, refresh_token,
                             user_id, is_active, auto_disabled, disabled_at, is_catalog, notes, created_at, updated_at)
                             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                             ON CONFLICT(id) DO NOTHING",
                        )
                        .bind(id)
                        .bind(&state.label)
                        .bind(&state.client_id)
                        .bind(&state.client_secret)
                        .bind(state.refresh_token())
                        .bind(&user_id)
                        .bind(is_active as i32)
                        .bind(auto_disabled as i32)
                        .bind(disabled_at)
                        .bind(is_catalog as i32)
                        .bind(&notes)
                        .bind(remote_created)
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
                    if let Err(error) = self.load_uptime(&state).await {
                        tracing::warn!(account = %state.id, "Could not load account uptime: {error}");
                    }
                    accounts.push(state);
                    restored_ids.push(id.clone());
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
            if !restored_ids.is_empty() {
                if let Some(proxy_manager) = self.proxy_manager.get().filter(|pm| pm.proxies_enabled()) {
                    let proxy_manager = proxy_manager.clone();
                    tokio::spawn(async move {
                        for id in restored_ids {
                            if let Err(e) = proxy_manager.working_client_for(&id).await {
                                tracing::warn!("Restored account {id} has no working proxy: {e}");
                            }
                        }
                    });
                }
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
    use super::{AccountManager, AccountState, SwitchingWeights, utc_day};
    use crate::error::AppError;
    use std::sync::atomic::Ordering;

    fn rules(concurrency: u32, rest_ms: u32) -> super::LeaseRules {
        super::LeaseRules { concurrency, rest_ms, per_hour: 0 }
    }

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

    #[test]
    fn retry_after_parses_seconds_and_http_date() {
        assert_eq!(AccountManager::retry_after_secs(Some("45")), 45);
        assert_eq!(AccountManager::retry_after_secs(Some("0")), 1);
        assert_eq!(AccountManager::retry_after_secs(None), 30);
        assert_eq!(AccountManager::retry_after_secs(Some("invalid")), 30);
        let date = (chrono::Utc::now() + chrono::Duration::seconds(90))
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        let parsed = AccountManager::retry_after_secs(Some(&date));
        assert!((89..=90).contains(&parsed), "HTTP date gave {parsed}s");
    }

    #[tokio::test]
    async fn rate_limited_accounts_are_not_selected_until_pause_expires() {
        let am = AccountManager::new(None, SwitchingWeights::default());
        let a = am
            .add_account("a".into(), "c".into(), "s".into(), "rt-a".into(), None)
            .await
            .unwrap();
        let b = am
            .add_account("b".into(), "c".into(), "s".into(), "rt-b".into(), None)
            .await
            .unwrap();
        AccountManager::pause_account(&a, Some("120")).await;
        AccountManager::pause_account(&a, Some("5")).await;
        assert!(AccountManager::rate_limit_remaining(&a).unwrap() >= 119);
        assert_eq!(am.select_account().await.unwrap().id, b.id);
        AccountManager::pause_account(&b, Some("60")).await;
        assert!(matches!(am.select_account().await, Err(AppError::RateLimited(1..=60))));
        a.rate_limited_until.store(0, Ordering::Relaxed);
        assert_eq!(am.select_account().await.unwrap().id, a.id);
    }

    #[tokio::test]
    async fn paused_catalog_account_does_not_fall_back_to_playback() {
        let am = AccountManager::new(None, SwitchingWeights::default());
        am.add_account("play".into(), "c".into(), "s".into(), "rt-play".into(), None)
            .await
            .unwrap();
        let catalog = am
            .add_account("catalog".into(), "c".into(), "s".into(), "rt-cat".into(), None)
            .await
            .unwrap();
        am.set_account_catalog(&catalog.id, true).await.unwrap();
        AccountManager::pause_account(&catalog, Some("90")).await;
        assert!(matches!(am.select_catalog_account().await, Err(AppError::RateLimited(_))));
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
        acc.created_at.store(12300, Ordering::Relaxed);
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
        assert_eq!(v["created_at"], 12300);
        assert_eq!(v["updated_at"], 12345);
    }

    #[test]
    fn carry_over_preserves_metrics_but_invalidates_changed_credentials() {
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
        old.rate_limited_until.store(8_888_999, Ordering::Relaxed);
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
        assert_eq!(new.token_expires_at.load(Ordering::Relaxed), 0);
        assert!(new.is_catalog.load(Ordering::Relaxed));
        assert_eq!(new.rate_limited_until.load(Ordering::Relaxed), 8_888_999);
    }

    #[tokio::test]
    async fn label_edit_preserves_tokens_but_credential_edit_invalidates_db_cache() {
        let db = crate::db::init_pool("sqlite::memory:").await.unwrap();
        let am = AccountManager::new(Some(db.clone()), SwitchingWeights::default());
        let original = am.add_account("old".into(), "c".into(), "s%3D".into(), "r".into(), None).await.unwrap();
        let expiry = chrono::Utc::now().timestamp() + 3600;
        am.store_refreshed_credentials(&original, "r", None, "cached", expiry).await.unwrap();
        am.update_account(&original.id, Some("new".into()), None, None, None, None).await.unwrap();
        let renamed = am.get_account_by_id(&original.id).await.unwrap();
        assert_eq!(renamed.access_token.read().await.as_deref(), Some("cached"));
        assert_eq!(renamed.token_expires_at.load(Ordering::Relaxed), expiry);
        am.update_account(&original.id, None, None, None, Some("replacement".into()), None).await.unwrap();
        let reloaded = AccountManager::new(Some(db), SwitchingWeights::default());
        reloaded.load_from_db().await.unwrap();
        let changed = reloaded.get_account_by_id(&original.id).await.unwrap();
        assert!(changed.access_token.read().await.is_none());
        assert_eq!(changed.token_expires_at.load(Ordering::Relaxed), 0);
        assert!(am.store_refreshed_credentials(&original, "r", Some("stale"), "stale", expiry).await.is_err());
    }

    #[test]
    fn legacy_secret_is_decoded_without_treating_plus_as_space() {
        assert_eq!(super::normalize_client_secret("abc+def%3D"), "abc+def=");
        assert_eq!(super::normalize_client_secret("abc+def="), "abc+def=");
    }

    #[tokio::test]
    async fn catalog_accounts_stay_out_of_playback() {
        use super::{AccountManager, SwitchingWeights};
        use std::sync::Arc;
        let am = Arc::new(AccountManager::new(None, SwitchingWeights::default()));
        let playback = am
            .add_account(
                "play".into(),
                "c".into(),
                "s".into(),
                "rt-play".into(),
                None,
            )
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
            .add_account(
                "play".into(),
                "c".into(),
                "s".into(),
                "rt-play".into(),
                None,
            )
            .await
            .unwrap();
        assert!(am.find_catalog_account().await.is_none());
        let picked = am.select_catalog_account().await.unwrap();
        assert_eq!(picked.id, playback.id);
    }

    #[test]
    fn selection_prefers_least_recently_used_and_penalizes_recent_errors() {
        use super::{AccountManager, SwitchingWeights};
        let w = SwitchingWeights::default();
        let clean = AccountManager::effective_last_use(&w, 1_000_000, 0.0);
        assert_eq!(clean, 1_000_000);
        // Failing every recent request counts as being used 10 minutes later.
        assert_eq!(AccountManager::effective_last_use(&w, 1_000_000, 1.0), 1_600_000);
        assert_eq!(AccountManager::effective_last_use(&w, 1_000_000, 0.5), 1_300_000);
        // A slightly older pick with many errors loses to a clean, newer one.
        assert!(AccountManager::effective_last_use(&w, 900_000, 0.8) > clean);
    }

    #[test]
    fn recent_error_rate_fades_instead_of_lasting_forever() {
        let mut recent = super::RecentOutcomes::default();
        recent.decay(1);
        recent.requests = 100.0;
        recent.errors = 100.0;
        assert_eq!(recent.error_rate(), 1.0);
        // After a quiet hour four half-lives have passed; new clean traffic dominates.
        recent.decay(1 + 60 * 60 * 1000);
        recent.requests += 20.0;
        assert!(recent.error_rate() < 0.25, "{}", recent.error_rate());
    }

    #[tokio::test]
    async fn requests_spread_evenly_sequential() {
        use super::{AccountManager, SwitchingWeights};
        use std::collections::HashMap;
        use std::sync::Arc;
        let am = Arc::new(AccountManager::new(None, SwitchingWeights::default()));
        for i in 0..3 {
            am.add_account(
                format!("a{}", i),
                "c".into(),
                "s".into(),
                format!("rt-{}", i),
                None,
            )
            .await
            .unwrap();
        }
        let mut hits: HashMap<String, usize> = HashMap::new();
        for _ in 0..90 {
            let picked = am.select_account_excluding(&[]).await.unwrap();
            *hits.entry(picked.id.clone()).or_default() += 1;
        }
        assert_eq!(hits.len(), 3, "every account must serve traffic");
        let (mut lo, mut hi) = (usize::MAX, 0usize);
        for &n in hits.values() {
            lo = lo.min(n);
            hi = hi.max(n);
        }
        assert!(
            hi - lo <= 1,
            "sequential picks must spread evenly, got {:?}",
            hits
        );
    }

    #[tokio::test]
    async fn requests_spread_evenly_concurrent() {
        use super::{AccountManager, SwitchingWeights};
        use std::collections::HashMap;
        use std::sync::Arc;
        use std::sync::Mutex as StdMutex;
        let am = Arc::new(AccountManager::new(None, SwitchingWeights::default()));
        for i in 0..4 {
            am.add_account(
                format!("a{}", i),
                "c".into(),
                "s".into(),
                format!("rt-c{}", i),
                None,
            )
            .await
            .unwrap();
        }
        let hits = Arc::new(StdMutex::new(HashMap::<String, usize>::new()));
        let mut tasks = Vec::new();
        for _ in 0..40 {
            let amc = am.clone();
            let hitsc = hits.clone();
            tasks.push(tokio::spawn(async move {
                let picked = amc.select_account_excluding(&[]).await.unwrap();
                *hitsc.lock().unwrap().entry(picked.id.clone()).or_default() += 1;
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        let hits = hits.lock().unwrap();
        assert_eq!(
            hits.len(),
            4,
            "concurrent load must reach every account: {:?}",
            hits
        );
        for (id, n) in hits.iter() {
            assert!(
                *n >= 5,
                "account {} starved under concurrent load: {:?}",
                id,
                hits
            );
        }
    }

    #[tokio::test]
    async fn pool_size_ignores_inactive_and_catalog() {
        use super::{AccountManager, SwitchingWeights};
        use std::sync::Arc;
        let am = Arc::new(AccountManager::new(None, SwitchingWeights::default()));
        let a = am
            .add_account("a".into(), "c".into(), "s".into(), "rt-a".into(), None)
            .await
            .unwrap();
        let b = am
            .add_account("b".into(), "c".into(), "s".into(), "rt-b".into(), None)
            .await
            .unwrap();
        let c = am
            .add_account("c".into(), "c".into(), "s".into(), "rt-c".into(), None)
            .await
            .unwrap();
        am.set_account_catalog(&c.id, true).await.unwrap();
        let now = chrono::Utc::now().timestamp();
        for account in [&a, &b, &c] {
            *account.access_token.write().await = Some("token".into());
            account.token_expires_at.store(now + 3600, Ordering::Relaxed);
        }
        assert_eq!(am.playback_count().await, 2);
        assert_eq!(am.playback_slots().await, 2);
        am.set_account_active(&b.id, false).await.unwrap();
        assert_eq!(am.playback_count().await, 1);
        assert_eq!(am.playback_slots().await, 1);
        // Only the remaining active playback account is selectable.
        for _ in 0..3 {
            let picked = am.select_account_excluding(&[]).await.unwrap();
            assert_eq!(picked.id, a.id);
        }
    }

    #[tokio::test]
    async fn catalog_slots_cap_concurrent_requests_per_account() {
        use super::{AccountManager, CatalogClaim, SwitchingWeights};
        let am = AccountManager::new(None, SwitchingWeights::default());
        assert!(matches!(am.claim_catalog_slot(1).await, CatalogClaim::Unavailable));
        let mut ids = Vec::new();
        for label in ["a", "b"] {
            let account = am
                .add_account(label.into(), "c".into(), "s".into(), format!("rt-{label}"), None)
                .await
                .unwrap();
            am.set_account_catalog(&account.id, true).await.unwrap();
            ids.push(account.id.clone());
        }

        // With a limit of one, two requests land on different accounts and a
        // third finds every account busy.
        let CatalogClaim::Slot(first) = am.claim_catalog_slot(1).await else {
            panic!("first catalog slot");
        };
        let CatalogClaim::Slot(second) = am.claim_catalog_slot(1).await else {
            panic!("second catalog slot");
        };
        assert_ne!(first.account().id, second.account().id);
        assert!(matches!(am.claim_catalog_slot(1).await, CatalogClaim::Busy));

        // Finishing a request frees its account again.
        let freed = first.account().id.clone();
        drop(first);
        let CatalogClaim::Slot(third) = am.claim_catalog_slot(1).await else {
            panic!("slot freed by the finished request");
        };
        assert_eq!(third.account().id, freed);

        // Zero keeps today's behaviour: no cap.
        let mut unlimited = Vec::new();
        for _ in 0..10 {
            let CatalogClaim::Slot(slot) = am.claim_catalog_slot(0).await else {
                panic!("unlimited catalog slot");
            };
            unlimited.push(slot);
        }
        drop((second, third, unlimited));
        for id in &ids {
            let account = am.get_account_by_id(id).await.unwrap();
            assert_eq!(account.catalog_inflight.load(Ordering::Relaxed), 0);
        }
    }

    #[tokio::test]
    async fn three_preview_checks_in_a_row_move_an_account_to_catalog() {
        use super::{AccountManager, SwitchingWeights};
        let db = crate::db::init_pool("sqlite::memory:").await.unwrap();
        let am = AccountManager::new(Some(db.clone()), SwitchingWeights::default());
        let account = am
            .add_account("a".into(), "c".into(), "s".into(), "r".into(), None)
            .await
            .unwrap();

        assert!(!am.note_preview_check(&account).await.unwrap());
        assert!(!am.note_preview_check(&account).await.unwrap());
        // An inconclusive check neither counts nor clears the streak.
        am.set_premium(&account.id, "unknown").await;
        assert_eq!(account.preview_streak.load(Ordering::Relaxed), 2);
        // A FULL result starts the count again.
        am.set_premium(&account.id, "premium").await;
        assert_eq!(account.preview_streak.load(Ordering::Relaxed), 0);

        assert!(!am.note_preview_check(&account).await.unwrap());
        assert!(!am.note_preview_check(&account).await.unwrap());
        assert!(!account.is_catalog.load(Ordering::Relaxed));
        let stored: (i64,) = sqlx::query_as("SELECT preview_streak FROM accounts WHERE id = ?")
            .bind(&account.id)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(stored.0, 2);

        assert!(am.note_preview_check(&account).await.unwrap());
        assert!(account.is_catalog.load(Ordering::Relaxed));
        assert_eq!(account.preview_streak.load(Ordering::Relaxed), 0);
        assert_eq!(am.playback_count().await, 0);
        let stored: (i64, i64) =
            sqlx::query_as("SELECT is_catalog, preview_streak FROM accounts WHERE id = ?")
                .bind(&account.id)
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(stored, (1, 0));
    }

    #[tokio::test]
    async fn playback_slots_count_only_accounts_that_can_serve_now() {
        use super::{AccountManager, SwitchingWeights};
        let am = AccountManager::new(None, SwitchingWeights::default());
        let mut accounts = Vec::new();
        for label in ["a", "b", "c", "d"] {
            accounts.push(
                am.add_account(label.into(), "c".into(), "s".into(), format!("rt-{label}"), None)
                    .await
                    .unwrap(),
            );
        }
        // Enabled accounts without a token keep the floor of one slot.
        assert_eq!(am.playback_count().await, 4);
        assert_eq!(am.playback_slots().await, 1);

        let now = chrono::Utc::now().timestamp();
        for account in &accounts {
            *account.access_token.write().await = Some("token".into());
            account.token_expires_at.store(now + 3600, Ordering::Relaxed);
        }
        assert_eq!(am.playback_slots().await, 4);

        accounts[0].token_expires_at.store(now - 1, Ordering::Relaxed);
        assert_eq!(am.playback_slots().await, 3);
        *accounts[1].rejected_access_token.write().await = Some("token".into());
        assert_eq!(am.playback_slots().await, 2);
        accounts[2].rate_limited_until.store(now + 60, Ordering::Relaxed);
        assert_eq!(am.playback_slots().await, 1);
        // Failover still walks every enabled account.
        assert_eq!(am.playback_count().await, 4);
    }

    #[tokio::test]
    async fn uptime_tracks_missing_rejected_and_replaced_tokens() {
        use crate::account_uptime::UptimeStatus;

        let db = crate::db::init_pool("sqlite::memory:").await.unwrap();
        let am = AccountManager::new(Some(db.clone()), SwitchingWeights::default());
        let account = am.add_account("a".into(), "c".into(), "s".into(), "r".into(), None)
            .await.unwrap();
        let now = chrono::Utc::now().timestamp();
        assert_eq!(account.uptime_summary(now).await.current_status, Some(UptimeStatus::Waiting));

        am.store_refreshed_credentials(&account, "r", None, "usable", now + 3600)
            .await.unwrap();
        assert_eq!(account.uptime_summary(now).await.current_status, Some(UptimeStatus::Up));
        // A stale 401 must not turn a newer, usable token yellow.
        crate::token_manager::TokenManager::reject_access_token(&account, "older-token").await;
        assert_eq!(account.uptime_summary(now).await.current_status, Some(UptimeStatus::Up));
        crate::token_manager::TokenManager::reject_access_token(&account, "usable").await;
        assert_eq!(account.uptime_summary(now).await.current_status, Some(UptimeStatus::Waiting));

        am.store_refreshed_credentials(&account, "r", None, "replacement", now + 3600)
            .await.unwrap();
        assert_eq!(account.uptime_summary(now).await.current_status, Some(UptimeStatus::Up));
        am.update_account(&account.id, None, None, None, Some("new-refresh".into()), None)
            .await.unwrap();
        let edited = am.get_account_by_id(&account.id).await.unwrap();
        assert_eq!(edited.uptime_summary(now).await.current_status, Some(UptimeStatus::Waiting));
        am.set_account_active(&account.id, false).await.unwrap();
        assert_eq!(edited.uptime_summary(now).await.current_status, Some(UptimeStatus::Down));

        let states: Vec<(bool, bool)> = sqlx::query_as(
            "SELECT is_active, token_ready FROM account_uptime_events WHERE account_id = ? ORDER BY id",
        ).bind(&account.id).fetch_all(&db).await.unwrap();
        assert_eq!(states, vec![(true, false), (true, true), (true, false), (true, true), (true, false), (false, false)]);
        db.close().await;
    }

    #[tokio::test]
    async fn uptime_reset_forgets_history_and_restarts_from_the_current_state() {
        use crate::account_uptime::UptimeStatus;

        let db = crate::db::init_pool("sqlite::memory:").await.unwrap();
        let am = AccountManager::new(Some(db.clone()), SwitchingWeights::default());
        let account = am.add_account("a".into(), "c".into(), "s".into(), "r".into(), None)
            .await.unwrap();
        let other = am.add_account("b".into(), "c".into(), "s".into(), "r2".into(), None)
            .await.unwrap();
        am.set_account_active(&account.id, false).await.unwrap();
        am.set_account_active(&account.id, true).await.unwrap();
        am.set_account_active(&other.id, false).await.unwrap();

        assert_eq!(am.reset_uptime().await.unwrap(), 2);
        let states: Vec<(String, bool, bool)> = sqlx::query_as(
            "SELECT account_id, is_active, token_ready FROM account_uptime_events ORDER BY account_id",
        ).fetch_all(&db).await.unwrap();
        let mut expected = vec![(account.id.clone(), true, false), (other.id.clone(), false, false)];
        expected.sort();
        assert_eq!(states, expected);

        // Earlier downtime is gone both in memory and after a restart.
        let later = chrono::Utc::now().timestamp() + 60;
        let summary = account.uptime_summary(later).await;
        assert_eq!(summary.current_status, Some(UptimeStatus::Waiting));
        assert_eq!(summary.down_seconds, 0);
        assert!(summary.unknown_seconds >= crate::account_uptime::WINDOW_SECS - 61);
        let reloaded = AccountManager::new(Some(db.clone()), SwitchingWeights::default());
        reloaded.load_from_db().await.unwrap();
        let restored = reloaded.get_account_by_id(&account.id).await.unwrap();
        assert_eq!(restored.uptime_summary(later).await.down_seconds, 0);
        db.close().await;

        // Without a database the in-memory history is reset the same way.
        let am = AccountManager::new(None, SwitchingWeights::default());
        let account = am.add_account("a".into(), "c".into(), "s".into(), "r".into(), None)
            .await.unwrap();
        am.set_account_active(&account.id, false).await.unwrap();
        assert_eq!(am.reset_uptime().await.unwrap(), 1);
        let summary = account.uptime_summary(chrono::Utc::now().timestamp() + 60).await;
        assert_eq!(summary.current_status, Some(UptimeStatus::Down));
        assert_eq!(summary.up_seconds + summary.waiting_seconds, 0);
    }

    #[tokio::test]
    async fn disabled_at_tracks_transition_and_survives_reload() {
        let path = std::env::temp_dir().join(format!(
            "hifi-disabled-at-{}.db",
            uuid::Uuid::new_v4()
        ));
        let db = crate::db::init_pool(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        let am = AccountManager::new(Some(db.clone()), SwitchingWeights::default());
        let account = am
            .add_account("a".into(), "c".into(), "s".into(), "rt".into(), None)
            .await
            .unwrap();
        assert_eq!(account.disabled_at.load(Ordering::Relaxed), 0);

        am.set_account_active(&account.id, false).await.unwrap();
        let disabled_at = account.disabled_at.load(Ordering::Relaxed);
        assert!(disabled_at > 0);
        am.set_account_active(&account.id, false).await.unwrap();
        assert_eq!(account.disabled_at.load(Ordering::Relaxed), disabled_at);

        let reloaded = AccountManager::new(Some(db.clone()), SwitchingWeights::default());
        reloaded.load_from_db().await.unwrap();
        let restored = reloaded.get_account_by_id(&account.id).await.unwrap();
        assert_eq!(restored.disabled_at.load(Ordering::Relaxed), disabled_at);
        reloaded.set_account_active(&account.id, true).await.unwrap();
        assert_eq!(restored.disabled_at.load(Ordering::Relaxed), 0);

        let states: Vec<bool> = sqlx::query_scalar(
            "SELECT is_active FROM account_uptime_events WHERE account_id = ? ORDER BY id",
        )
        .bind(&account.id)
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(states, vec![true, false, true]);

        // System failures and recovery use the same history as manual toggles.
        assert!(reloaded.set_system_disabled(&restored, true).await.unwrap());
        reloaded.update_account(&account.id, Some("renamed".into()), None, None, None, None)
            .await.unwrap();
        let edited = reloaded.get_account_by_id(&account.id).await.unwrap();
        assert!(std::sync::Arc::ptr_eq(restored.uptime_history(), edited.uptime_history()));
        assert!(reloaded.set_system_disabled(&edited, false).await.unwrap());
        let states: Vec<bool> = sqlx::query_scalar(
            "SELECT is_active FROM account_uptime_events WHERE account_id = ? ORDER BY id",
        )
        .bind(&account.id)
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(states, vec![true, false, true, false, true]);
        reloaded.remove_account(&account.id).await.unwrap();
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM account_uptime_events")
            .fetch_one(&db).await.unwrap();
        assert_eq!(remaining, 0);

        db.close().await;
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn adding_and_removing_account_updates_proxy_assignments() {
        use crate::config::Config;
        use crate::proxy_manager::ProxyManager;
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let proxy_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut request = [0; 4096];
                    let _ = stream.read(&mut request).await;
                    let _ = stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                        .await;
                });
            }
        });

        let path = std::env::temp_dir().join(format!(
            "hifi-proxy-lifecycle-{}.db",
            uuid::Uuid::new_v4()
        ));
        let db = crate::db::init_pool(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        let mut config = Config::from_env();
        config.use_proxies = true;
        config.fallback_to_direct = false;
        config.proxies_file = std::path::PathBuf::from("/tmp/hifi-test-no-proxies-file");
        let proxy_manager = Arc::new(ProxyManager::new(Arc::new(config), Some(db.clone())));
        proxy_manager.configure(true, vec![proxy_url]).await.unwrap();
        let account_manager = AccountManager::new(Some(db.clone()), SwitchingWeights::default());
        account_manager.set_proxy_manager(proxy_manager.clone());

        let account = account_manager
            .add_account("a".into(), "c".into(), "s".into(), "rt".into(), None)
            .await
            .unwrap();
        let status = proxy_manager.status().await;
        assert_eq!(status["assignments"].as_array().unwrap().len(), 1);
        assert_eq!(status["assignments"][0]["account_id"], account.id);
        account_manager.remove_account(&account.id).await.unwrap();
        let status = proxy_manager.status().await;
        assert!(status["assignments"].as_array().unwrap().is_empty());
        let saved: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM proxy_assignments")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(saved, 0);

        server.abort();
        db.close().await;
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn catalog_picks_round_robin() {
        use super::{AccountManager, SwitchingWeights};
        use std::sync::Arc;
        let am = Arc::new(AccountManager::new(None, SwitchingWeights::default()));
        let c1 = am
            .add_account("c1".into(), "c".into(), "s".into(), "rt-c1".into(), None)
            .await
            .unwrap();
        let c2 = am
            .add_account("c2".into(), "c".into(), "s".into(), "rt-c2".into(), None)
            .await
            .unwrap();
        am.set_account_catalog(&c1.id, true).await.unwrap();
        am.set_account_catalog(&c2.id, true).await.unwrap();
        let mut order = Vec::new();
        for _ in 0..4 {
            order.push(am.select_catalog_account().await.unwrap().id.clone());
        }
        assert_eq!(
            order,
            vec![c1.id.clone(), c2.id.clone(), c1.id.clone(), c2.id.clone()]
        );
        assert!(am.next_active_catalog().await.is_some());
    }

    async fn picks(am: &AccountManager, count: usize, id: &str) -> usize {
        let mut hits = 0;
        for _ in 0..count {
            if am.select_account().await.unwrap().id == id {
                hits += 1;
            }
        }
        hits
    }

    #[tokio::test]
    async fn new_account_joins_level_instead_of_taking_every_request() {
        let am = AccountManager::new(None, SwitchingWeights::default());
        for label in ["a", "b"] {
            am.add_account(label.into(), "c".into(), "s".into(), format!("rt-{label}"), None)
                .await
                .unwrap();
        }
        for _ in 0..60 {
            am.select_account().await.unwrap();
        }
        let late = am
            .add_account("late".into(), "c".into(), "s".into(), "rt-late".into(), None)
            .await
            .unwrap();
        let hits = picks(&am, 30, &late.id).await;
        assert!((8..=12).contains(&hits), "newcomer took {hits} of 30 requests");
    }

    #[tokio::test]
    async fn account_back_from_a_pause_does_not_catch_up() {
        let am = AccountManager::new(None, SwitchingWeights::default());
        let mut accounts = Vec::new();
        for label in ["a", "b", "c"] {
            accounts.push(
                am.add_account(label.into(), "c".into(), "s".into(), format!("rt-{label}"), None)
                    .await
                    .unwrap(),
            );
        }
        let paused = &accounts[0];
        AccountManager::pause_account(paused, Some("120")).await;
        assert_eq!(picks(&am, 60, &paused.id).await, 0);
        paused.rate_limited_until.store(0, Ordering::Relaxed);
        let hits = picks(&am, 30, &paused.id).await;
        assert!((8..=12).contains(&hits), "returning account took {hits} of 30 requests");
        // The counter shown in the admin panel still reports real use.
        assert_eq!(paused.request_count.load(Ordering::Relaxed), hits as u64);
    }

    #[tokio::test]
    async fn parallel_requests_take_idle_accounts_before_busy_ones() {
        let am = AccountManager::new(None, SwitchingWeights::default());
        let mut accounts = Vec::new();
        for label in ["a", "b", "c"] {
            accounts.push(
                am.add_account(label.into(), "c".into(), "s".into(), format!("rt-{label}"), None)
                    .await
                    .unwrap(),
            );
        }
        // By recency alone every request would go to the first account.
        let now_ms = chrono::Utc::now().timestamp_millis();
        accounts[0].last_pick_ms.store(1, Ordering::Relaxed);
        accounts[1].last_pick_ms.store(now_ms, Ordering::Relaxed);
        accounts[2].last_pick_ms.store(now_ms, Ordering::Relaxed);
        let mut leases = Vec::new();
        for _ in 0..3 {
            leases.push(am.lease_account_excluding(&[]).await.unwrap());
        }
        let mut ids: Vec<&str> = leases.iter().map(|lease| lease.account().id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 3, "each parallel request must get its own account");
        assert!(accounts.iter().all(|a| a.playback_inflight.load(Ordering::SeqCst) == 1));
        drop(leases);
        assert!(accounts.iter().all(|a| a.playback_inflight.load(Ordering::SeqCst) == 0));
    }

    #[tokio::test]
    async fn busy_accounts_are_skipped_and_full_pool_waits_for_a_slot() {
        use super::LeaseKind;
        use std::sync::Arc;
        let am = Arc::new(AccountManager::new(None, SwitchingWeights::default()));
        for label in ["a", "b"] {
            am.add_account(label.into(), "c".into(), "s".into(), format!("rt-{label}"), None)
                .await
                .unwrap();
        }
        let first = am.lease_account_for(&[], LeaseKind::Playback, rules(1, 0)).await.unwrap();
        // Metadata counts too: the second account now sends one request to Tidal.
        let second = am.lease_account_for(&[], LeaseKind::Metadata, rules(1, 0)).await.unwrap();
        assert_ne!(first.account().id, second.account().id);

        let waiting = {
            let am = am.clone();
            tokio::spawn(async move { am.lease_account_for(&[], LeaseKind::Playback, rules(1, 0)).await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(!waiting.is_finished(), "a full pool must wait instead of doubling up");
        let freed = first.account().id.clone();
        drop(first);
        let third = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(third.account().id, freed);
        assert_eq!(third.account().playback_inflight.load(Ordering::SeqCst), 1);
        drop(second);
        drop(third);
        // 0 lifts the limit.
        let leases: Vec<_> = futures::future::join_all(
            (0..4).map(|_| am.lease_account_for(&[], LeaseKind::Playback, rules(0, 0))),
        )
        .await;
        assert!(leases.iter().all(|lease| lease.is_ok()));
    }

    #[tokio::test]
    async fn accounts_waiting_for_a_token_refresh_are_not_selected() {
        use super::LeaseKind;
        let am = AccountManager::new(None, SwitchingWeights::default());
        let mut accounts = Vec::new();
        for label in ["dead", "alive"] {
            accounts.push(
                am.add_account(label.into(), "c".into(), "s".into(), format!("rt-{label}"), None)
                    .await
                    .unwrap(),
            );
        }
        let now = chrono::Utc::now().timestamp();
        *accounts[1].access_token.write().await = Some("token".into());
        accounts[1].token_expires_at.store(now + 3600, Ordering::Relaxed);
        accounts[0].heal_next_retry.store(now + 600, Ordering::Relaxed);
        for _ in 0..5 {
            let lease = am.lease_account_for(&[], LeaseKind::Playback, rules(0, 0)).await.unwrap();
            assert_eq!(lease.account().id, accounts[1].id);
        }
        assert_eq!(accounts[0].request_count.load(Ordering::Relaxed), 0);
        // Once its cooldown ends, a request may try the refresh again.
        accounts[0].heal_next_retry.store(now - 1, Ordering::Relaxed);
        let busy = am.lease_account_for(&[], LeaseKind::Playback, rules(1, 0)).await.unwrap();
        let other = am.lease_account_for(&[], LeaseKind::Playback, rules(1, 0)).await.unwrap();
        assert_ne!(busy.account().id, other.account().id);
        drop((busy, other));
        // With nothing else left the caller learns when to come back.
        accounts[0].heal_next_retry.store(now + 600, Ordering::Relaxed);
        accounts[1].heal_next_retry.store(now + 60, Ordering::Relaxed);
        accounts[1].token_expires_at.store(now - 1, Ordering::Relaxed);
        assert!(matches!(
            am.lease_account_for(&[], LeaseKind::Playback, rules(0, 0)).await,
            Err(AppError::ServiceUnavailableRetry(_, 55..=60))
        ));
    }

    #[tokio::test]
    async fn long_auth_403_streak_switches_the_account_off_until_the_owner_returns() {
        let db = crate::db::init_pool("sqlite::memory:").await.unwrap();
        let am = AccountManager::new(Some(db.clone()), SwitchingWeights::default());
        let account = am
            .add_account("a".into(), "c".into(), "s".into(), "rt-a".into(), None)
            .await
            .unwrap();
        let day = 24 * 3600;
        let now = 10 * day;
        // No streak, or a shorter one, or the setting off: nothing happens.
        assert!(!am.disable_after_auth_failures(&account, day, now).await.unwrap());
        account.auth_failing_since.store(now - day + 1, Ordering::Relaxed);
        account.persist_refresh_state().await.unwrap();
        assert!(!am.disable_after_auth_failures(&account, day, now).await.unwrap());
        assert!(!am.disable_after_auth_failures(&account, 0, now + day).await.unwrap());

        assert!(am.disable_after_auth_failures(&account, day, now + 1).await.unwrap());
        assert!(!account.is_active.load(Ordering::Relaxed));
        // Not auto-disabled, so auto-heal leaves it alone.
        assert!(!account.auto_disabled.load(Ordering::Relaxed));
        assert_eq!(account.disabled_at.load(Ordering::Relaxed), now + 1);
        assert!(!am.disable_after_auth_failures(&account, day, now + 2).await.unwrap());

        // Both the switch-off and the streak survive a restart.
        let reloaded = AccountManager::new(Some(db.clone()), SwitchingWeights::default());
        reloaded.load_from_db().await.unwrap();
        let restored = reloaded.get_account_by_id(&account.id).await.unwrap();
        assert!(!restored.is_active.load(Ordering::Relaxed));
        assert_eq!(restored.auth_failing_since.load(Ordering::Relaxed), now - day + 1);

        // Turning it on is the owner's call and starts from a clean slate.
        reloaded.set_account_active(&account.id, true).await.unwrap();
        assert_eq!(restored.auth_failing_since.load(Ordering::Relaxed), 0);
        db.close().await;
    }

    #[tokio::test]
    async fn playback_prefers_confirmed_full_then_unchecked_then_preview() {
        use super::LeaseKind;
        let am = AccountManager::new(None, SwitchingWeights::default());
        let mut accounts = Vec::new();
        for label in ["preview", "unchecked", "full"] {
            accounts.push(
                am.add_account(label.into(), "c".into(), "s".into(), format!("rt-{label}"), None)
                    .await
                    .unwrap(),
            );
        }
        let now = chrono::Utc::now().timestamp();
        *accounts[0].premium_status.write().await = "preview-only".into();
        accounts[0].premium_checked_at.store(now, Ordering::Relaxed);
        am.confirm_full(&accounts[2]).await;
        for _ in 0..5 {
            let lease = am.lease_account_for(&[], LeaseKind::Playback, rules(0, 0)).await.unwrap();
            assert_eq!(lease.account().id, accounts[2].id);
        }
        // While the FULL account is busy, an unchecked one beats a PREVIEW one.
        let busy = am.lease_account_for(&[], LeaseKind::Playback, rules(1, 0)).await.unwrap();
        let next = am.lease_account_for(&[], LeaseKind::Playback, rules(1, 0)).await.unwrap();
        assert_eq!((busy.account().id.as_str(), next.account().id.as_str()), (accounts[2].id.as_str(), accounts[1].id.as_str()));
        let last = am.lease_account_for(&[], LeaseKind::Playback, rules(1, 0)).await.unwrap();
        assert_eq!(last.account().id, accounts[0].id);
        drop((busy, next, last));
        // Metadata does not care about FULL.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..6 {
            seen.insert(am.lease_account_for(&[], LeaseKind::Metadata, rules(0, 0)).await.unwrap().account().id.clone());
        }
        assert_eq!(seen.len(), 3);
        // A FULL result older than 8 hours no longer counts.
        accounts[2].premium_checked_at.store(now - super::FULL_CHECK_MAX_AGE_SECS - 1, Ordering::Relaxed);
        assert_eq!(accounts[2].full_tier(now).await, 1);
    }

    #[tokio::test]
    async fn traffic_previews_mark_preview_only_after_a_streak_and_full_resets_it() {
        let am = AccountManager::new(None, SwitchingWeights::default());
        let account = am
            .add_account("a".into(), "c".into(), "s".into(), "rt-a".into(), None)
            .await
            .unwrap();
        let limit = super::TRAFFIC_PREVIEWS_BEFORE_PREVIEW_ONLY;
        for _ in 1..limit {
            assert!(!am.note_traffic_preview(&account).await);
        }
        // One FULL in between starts the count over.
        am.confirm_full(&account).await;
        for _ in 1..limit {
            assert!(!am.note_traffic_preview(&account).await);
        }
        assert!(am.note_traffic_preview(&account).await);
        assert_eq!(*account.premium_status.read().await, "preview-only");
        assert_eq!(account.full_tier(chrono::Utc::now().timestamp()).await, 2);
        // A later FULL (from traffic or the scheduled check) restores it.
        am.confirm_full(&account).await;
        assert_eq!(account.full_tier(chrono::Utc::now().timestamp()).await, 0);
    }

    #[tokio::test]
    async fn hourly_limit_skips_accounts_that_used_their_quota() {
        use super::{LeaseKind, LeaseRules};
        let am = AccountManager::new(None, SwitchingWeights::default());
        let mut accounts = Vec::new();
        for label in ["a", "b"] {
            accounts.push(
                am.add_account(label.into(), "c".into(), "s".into(), format!("rt-{label}"), None)
                    .await
                    .unwrap(),
            );
        }
        let per_hour = LeaseRules { concurrency: 0, rest_ms: 0, per_hour: 2 };
        for _ in 0..4 {
            am.lease_account_for(&[], LeaseKind::Playback, per_hour).await.unwrap();
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        assert!(accounts.iter().all(|a| a.requests_last_hour(now_ms) == 2));
        // Both used their quota: a fifth request waits rather than exceeding it.
        let waiting = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            am.lease_account_for(&[], LeaseKind::Playback, per_hour),
        )
        .await;
        assert!(waiting.is_err());
        // An hour later the quota is free again.
        assert_eq!(accounts[0].requests_last_hour(now_ms + 3_600_001), 0);
    }

    #[tokio::test]
    async fn account_rests_between_requests() {
        use super::LeaseKind;
        let am = AccountManager::new(None, SwitchingWeights::default());
        for label in ["a", "b"] {
            am.add_account(label.into(), "c".into(), "s".into(), format!("rt-{label}"), None)
                .await
                .unwrap();
        }
        let first = am.lease_account_for(&[], LeaseKind::Playback, rules(1, 300)).await.unwrap();
        let rested = first.account().id.clone();
        drop(first);
        // The resting account is skipped while another one is free.
        let second = am.lease_account_for(&[], LeaseKind::Playback, rules(1, 300)).await.unwrap();
        assert_ne!(second.account().id, rested);
        drop(second);
        // Both rest now: the next request waits for the first rest to end.
        let started = std::time::Instant::now();
        let third = am.lease_account_for(&[], LeaseKind::Playback, rules(1, 300)).await.unwrap();
        assert_eq!(third.account().id, rested);
        assert!(started.elapsed() >= std::time::Duration::from_millis(150));
    }

    #[test]
    fn repeated_rate_limits_double_the_pause_up_to_the_cap() {
        assert_eq!(AccountManager::rate_limit_pause_secs(None, 1), 30);
        assert_eq!(AccountManager::rate_limit_pause_secs(None, 2), 60);
        assert_eq!(AccountManager::rate_limit_pause_secs(None, 4), 240);
        assert_eq!(AccountManager::rate_limit_pause_secs(None, 9), 900);
        assert_eq!(AccountManager::rate_limit_pause_secs(Some("10"), 3), 40);
        // A longer Retry-After than the cap is still honoured.
        assert_eq!(AccountManager::rate_limit_pause_secs(Some("3600"), 2), 3600);
    }

    #[tokio::test]
    async fn strikes_grow_while_hot_and_reset_after_a_quiet_spell() {
        use chrono::Utc;
        let am = AccountManager::new(None, SwitchingWeights::default());
        let account = am
            .add_account("a".into(), "c".into(), "s".into(), "rt".into(), None)
            .await
            .unwrap();
        let now = Utc::now().timestamp();
        assert!(AccountManager::pause_account(&account, None).await >= 29);
        // Another 429 from the same burst, while paused, is not a new strike.
        AccountManager::pause_account(&account, None).await;
        assert_eq!(account.rate_limit_strikes.load(Ordering::Relaxed), 1);

        // The pause ended a minute ago: the next 429 doubles it.
        account.rate_limited_until.store(now - 60, Ordering::Relaxed);
        assert!(AccountManager::pause_account(&account, None).await >= 59);
        assert_eq!(account.rate_limit_strikes.load(Ordering::Relaxed), 2);

        // Quiet for longer than the forgive window: back to strike 1.
        account
            .rate_limited_until
            .store(now - AccountManager::RATE_LIMIT_FORGIVE_SECS - 1, Ordering::Relaxed);
        assert_eq!(AccountManager::rate_limit_heat(&account, now), 0);
        let seconds = AccountManager::pause_account(&account, None).await;
        assert!((29..=30).contains(&seconds), "got {seconds}s");
        assert_eq!(account.rate_limit_strikes.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn accounts_with_recent_429s_go_after_the_others() {
        use super::LeaseKind;
        use chrono::Utc;
        let am = AccountManager::new(None, SwitchingWeights::default());
        let hot = am
            .add_account("hot".into(), "c".into(), "s".into(), "rt-hot".into(), None)
            .await
            .unwrap();
        let cool = am
            .add_account("cool".into(), "c".into(), "s".into(), "rt-cool".into(), None)
            .await
            .unwrap();
        // The hot account's pause is over and it is the least recently used.
        AccountManager::pause_account(&hot, None).await;
        hot.rate_limited_until
            .store(Utc::now().timestamp() - 1, Ordering::Relaxed);
        cool.last_pick_ms
            .store(Utc::now().timestamp_millis(), Ordering::Relaxed);
        for _ in 0..3 {
            assert_eq!(am.select_account().await.unwrap().id, cool.id);
        }
        // With the cool account busy, the hot one still serves.
        let busy = am.lease_account_for(&[], LeaseKind::Playback, rules(1, 0)).await.unwrap();
        assert_eq!(busy.account().id, cool.id);
        let overflow = am.lease_account_for(&[], LeaseKind::Playback, rules(1, 0)).await.unwrap();
        assert_eq!(overflow.account().id, hot.id);
    }

    #[tokio::test]
    async fn rate_limit_pause_survives_reload() {
        let db = crate::db::init_pool("sqlite::memory:").await.unwrap();
        let am = AccountManager::new(Some(db.clone()), SwitchingWeights::default());
        let account = am
            .add_account("a".into(), "c".into(), "s".into(), "rt".into(), None)
            .await
            .unwrap();
        AccountManager::pause_account(&account, Some("120")).await;
        // A shorter pause must not shorten the stored one.
        AccountManager::pause_account(&account, Some("5")).await;
        let reloaded = AccountManager::new(Some(db), SwitchingWeights::default());
        reloaded.load_from_db().await.unwrap();
        let restored = reloaded.get_account_by_id(&account.id).await.unwrap();
        assert!(AccountManager::rate_limit_remaining(&restored).unwrap() >= 119);
        assert!(matches!(
            reloaded.select_account().await,
            Err(AppError::RateLimited(_))
        ));
    }
}
