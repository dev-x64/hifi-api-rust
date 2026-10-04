use axum::Json;
use axum::extract::State;
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::account_manager::AccountState;
use crate::error::AppError;

const LIVE_PROBE_MAX_AGE_SECS: i64 = 8 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenStatus {
    Missing,
    Expired,
    Rejected,
    Valid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Disabled,
    NoToken,
    RateLimited,
    NeedsCheck,
    PreviewOnly,
    Ready,
}

#[derive(Debug, Serialize)]
pub struct AccountAvailability {
    pub state: Readiness,
    pub token: TokenStatus,
    pub full_check_fresh: bool,
    pub metadata_ready: bool,
}

/// The overview and account cards must explain readiness using the same rules.
pub async fn account_availability(account: &AccountState, now: i64) -> AccountAvailability {
    use std::sync::atomic::Ordering;

    let token = account.access_token.read().await;
    let token_status = match token.as_deref().filter(|t| !t.is_empty()) {
        None => TokenStatus::Missing,
        Some(token) if account.rejected_access_token.read().await.as_deref() == Some(token) => {
            TokenStatus::Rejected
        }
        Some(_) if account.token_expires_at.load(Ordering::Relaxed) <= now => TokenStatus::Expired,
        Some(_) => TokenStatus::Valid,
    };
    drop(token);
    let checked = account.premium_checked_at.load(Ordering::Relaxed);
    let fresh = checked > 0 && checked >= now - LIVE_PROBE_MAX_AGE_SECS;
    let enabled = account.is_active.load(Ordering::Relaxed);
    let paused = account.rate_limited_until.load(Ordering::Relaxed) > now;
    let premium = account.premium_status.read().await;
    let state = if !enabled {
        Readiness::Disabled
    } else if token_status != TokenStatus::Valid {
        Readiness::NoToken
    } else if paused {
        Readiness::RateLimited
    } else if account.is_catalog.load(Ordering::Relaxed) {
        Readiness::Ready
    } else if premium.as_str() == "preview-only" && fresh {
        Readiness::PreviewOnly
    } else if premium.as_str() != "premium" || !fresh {
        Readiness::NeedsCheck
    } else {
        Readiness::Ready
    };
    AccountAvailability {
        state,
        token: token_status,
        full_check_fresh: fresh,
        metadata_ready: enabled && !paused && token_status == TokenStatus::Valid,
    }
}

#[derive(Default, Serialize)]
struct ReadinessCounts {
    total: usize,
    enabled: usize,
    ready: usize,
    disabled: usize,
    no_token: usize,
    rate_limited: usize,
    needs_check: usize,
    preview_only: usize,
    metadata_ready: usize,
}

impl ReadinessCounts {
    fn include(&mut self, availability: &AccountAvailability) {
        self.total += 1;
        self.enabled += usize::from(availability.state != Readiness::Disabled);
        self.metadata_ready += usize::from(availability.metadata_ready);
        match availability.state {
            Readiness::Ready => self.ready += 1,
            Readiness::Disabled => self.disabled += 1,
            Readiness::NoToken => self.no_token += 1,
            Readiness::RateLimited => self.rate_limited += 1,
            Readiness::NeedsCheck => self.needs_check += 1,
            Readiness::PreviewOnly => self.preview_only += 1,
        }
    }
}

pub async fn get_stats(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let accounts = state.account_manager.list_accounts().await;
    let total_account_attempts: u64 = accounts
        .iter()
        .map(|a| a.request_count.load(std::sync::atomic::Ordering::Relaxed))
        .sum();
    let total_account_errors: u64 = accounts
        .iter()
        .map(|a| a.error_count.load(std::sync::atomic::Ordering::Relaxed))
        .sum();
    let total_requests = state.request_log.total_requests();
    let (recent_requests, total_errors) = state.request_log.outcome_counts();
    let requests_per_second_60s = state.request_log.requests_last_60s() as f64 / 60.0;
    let recent_p95_ms = state.request_log.recent_p95_ms();
    let now = Utc::now().timestamp();
    let active_count = accounts
        .iter()
        .filter(|a| a.is_active.load(std::sync::atomic::Ordering::Relaxed))
        .count();
    let mut live_count = 0;
    let mut premium_count = 0;
    let mut account_readiness = ReadinessCounts::default();
    let mut playback_readiness = ReadinessCounts::default();
    let mut catalog_readiness = ReadinessCounts::default();
    for account in &accounts {
        let availability = account_availability(account, now).await;
        account_readiness.include(&availability);
        if account
            .is_catalog
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            catalog_readiness.include(&availability);
        } else {
            playback_readiness.include(&availability);
        }
        if availability.state == Readiness::Ready {
            live_count += 1;
        }
        if account.is_active.load(std::sync::atomic::Ordering::Relaxed)
            && account.premium_status.read().await.as_str() == "premium"
        {
            premium_count += 1;
        }
    }
    let playback_count = state.account_manager.playback_count().await;
    let pool = state.account_manager.playback_slots().await;
    let playback = state.playback.stats(pool).await;
    let catalog = if !state.config.catalog_token.is_empty() {
        json!({"mode": "static_token"})
    } else if let Some(account) = accounts.iter().find(|a| {
        a.is_catalog.load(std::sync::atomic::Ordering::Relaxed)
            && a.is_active.load(std::sync::atomic::Ordering::Relaxed)
            && a.rate_limited_until
                .load(std::sync::atomic::Ordering::Relaxed)
                <= now
    }) {
        json!({"mode": "account", "label": account.label, "active": true})
    } else {
        json!({"mode": "pool"})
    };

    let redis = match &state.upstash {
        None => json!({"configured": false, "status": "disabled"}),
        Some(store) => {
            // Which instance we're synced to (backend + host only — the
            // native URL embeds its password, so it is never exposed).
            let endpoint = store.describe();
            if store.is_alive(15).await {
                json!({"configured": true, "status": "ok", "endpoint": endpoint})
            } else {
                json!({"configured": true, "status": "unreachable", "endpoint": endpoint})
            }
        }
    };

    Ok(Json(json!({
        "total_requests": total_requests,
        "total_errors": total_errors,
        "total_account_attempts": total_account_attempts,
        "total_account_errors": total_account_errors,
        "error_rate": if recent_requests > 0 {
            format!("{:.2}%", (total_errors as f64 / recent_requests as f64) * 100.0)
        } else { "0.00%".into() },
        "requests_per_second_60s": requests_per_second_60s,
        "recent_p95_ms": recent_p95_ms,
        "recent_requests": recent_requests,
        "recent_error_rate_percent": (recent_requests > 0).then(|| total_errors as f64 * 100.0 / recent_requests as f64),
        "account_readiness": account_readiness,
        "playback_readiness": playback_readiness,
        "catalog_readiness": catalog_readiness,
        "total_accounts": accounts.len(),
        "active_accounts": active_count,
        "live_accounts": live_count,
        "premium_accounts": premium_count,
        "healthy_accounts": active_count,
        "playback_accounts": playback_count,
        "playback": playback,
        "catalog": catalog,
        "redis": redis,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_manager::AccountState;
    use std::sync::atomic::Ordering;

    fn account() -> AccountState {
        AccountState::new(
            "id".into(),
            "label".into(),
            "client".into(),
            "secret".into(),
            "refresh".into(),
            None,
            true,
            String::new(),
        )
    }

    #[tokio::test]
    async fn live_playback_requires_current_token_and_recent_full_probe() {
        let account = account();
        let now = Utc::now().timestamp();
        *account.access_token.write().await = Some("token".into());
        account
            .token_expires_at
            .store(now + 3600, Ordering::Relaxed);
        *account.premium_status.write().await = "premium".into();
        account.premium_checked_at.store(now, Ordering::Relaxed);
        assert!((account_availability(&account, now).await.state == Readiness::Ready));

        account
            .rate_limited_until
            .store(now + 60, Ordering::Relaxed);
        assert!(!(account_availability(&account, now).await.state == Readiness::Ready));
        account.rate_limited_until.store(0, Ordering::Relaxed);
        account
            .premium_checked_at
            .store(now - LIVE_PROBE_MAX_AGE_SECS - 1, Ordering::Relaxed);
        assert!(!(account_availability(&account, now).await.state == Readiness::Ready));
    }

    #[tokio::test]
    async fn live_catalog_requires_current_non_rejected_token() {
        let account = account();
        let now = Utc::now().timestamp();
        account.is_catalog.store(true, Ordering::Relaxed);
        *account.access_token.write().await = Some("token".into());
        account
            .token_expires_at
            .store(now + 3600, Ordering::Relaxed);
        assert!((account_availability(&account, now).await.state == Readiness::Ready));

        *account.rejected_access_token.write().await = Some("token".into());
        assert!(!(account_availability(&account, now).await.state == Readiness::Ready));
    }

    #[tokio::test]
    async fn readiness_reasons_are_exclusive_and_metadata_does_not_require_full() {
        let account = account();
        let now = Utc::now().timestamp();
        let mut counts = ReadinessCounts::default();
        let missing = account_availability(&account, now).await;
        assert_eq!(missing.state, Readiness::NoToken);
        assert_eq!(missing.token, TokenStatus::Missing);
        counts.include(&missing);

        *account.access_token.write().await = Some("token".into());
        account
            .token_expires_at
            .store(now + 3600, Ordering::Relaxed);
        let unchecked = account_availability(&account, now).await;
        assert_eq!(unchecked.state, Readiness::NeedsCheck);
        assert!(unchecked.metadata_ready);
        counts.include(&unchecked);

        *account.premium_status.write().await = "preview-only".into();
        account.premium_checked_at.store(now, Ordering::Relaxed);
        let preview = account_availability(&account, now).await;
        assert_eq!(preview.state, Readiness::PreviewOnly);
        assert!(preview.metadata_ready);
        counts.include(&preview);

        *account.premium_status.write().await = "premium".into();
        let full = account_availability(&account, now).await;
        assert_eq!(full.state, Readiness::Ready);
        counts.include(&full);

        account
            .rate_limited_until
            .store(now + 60, Ordering::Relaxed);
        let paused = account_availability(&account, now).await;
        assert_eq!(paused.state, Readiness::RateLimited);
        assert!(!paused.metadata_ready);
        counts.include(&paused);

        account.is_active.store(false, Ordering::Relaxed);
        let disabled = account_availability(&account, now).await;
        assert_eq!(disabled.state, Readiness::Disabled);
        counts.include(&disabled);
        assert_eq!(counts.total, 6);
        assert_eq!(counts.enabled, 5);
        assert_eq!(
            counts.ready
                + counts.disabled
                + counts.no_token
                + counts.needs_check
                + counts.preview_only
                + counts.rate_limited,
            counts.total
        );
        assert_eq!(counts.metadata_ready, 3);
    }

    #[tokio::test]
    async fn expired_and_rejected_tokens_override_a_previous_full_check() {
        let account = account();
        let now = Utc::now().timestamp();
        *account.access_token.write().await = Some("token".into());
        *account.premium_status.write().await = "premium".into();
        account.premium_checked_at.store(now, Ordering::Relaxed);
        account.token_expires_at.store(now, Ordering::Relaxed);
        let expired = account_availability(&account, now).await;
        assert_eq!(expired.state, Readiness::NoToken);
        assert_eq!(expired.token, TokenStatus::Expired);
        account
            .token_expires_at
            .store(now + 3600, Ordering::Relaxed);
        *account.rejected_access_token.write().await = Some("token".into());
        let rejected = account_availability(&account, now).await;
        assert_eq!(rejected.state, Readiness::NoToken);
        assert_eq!(rejected.token, TokenStatus::Rejected);
    }
}
