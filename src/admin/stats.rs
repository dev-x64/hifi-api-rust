use axum::Json;
use axum::extract::State;
use chrono::Utc;
use serde_json::{Value, json};

use crate::AppState;
use crate::account_manager::AccountState;
use crate::error::AppError;

const LIVE_PROBE_MAX_AGE_SECS: i64 = 8 * 60 * 60;

async fn is_live_now(account: &AccountState, now: i64) -> bool {
    use std::sync::atomic::Ordering;

    if !account.is_active.load(Ordering::Relaxed)
        || account.rate_limited_until.load(Ordering::Relaxed) > now
        || account.token_expires_at.load(Ordering::Relaxed) <= now
    {
        return false;
    }

    let token = account.access_token.read().await;
    let Some(token) = token.as_deref().filter(|token| !token.is_empty()) else {
        return false;
    };
    if account.rejected_access_token.read().await.as_deref() == Some(token) {
        return false;
    }

    if account.is_catalog.load(Ordering::Relaxed) {
        return true;
    }

    account.premium_status.read().await.as_str() == "premium"
        && account.premium_checked_at.load(Ordering::Relaxed) >= now - LIVE_PROBE_MAX_AGE_SECS
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
    for account in &accounts {
        if is_live_now(account, now).await {
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
    } else if let Some(acc) = state.account_manager.find_catalog_account().await {
        json!({
            "mode": "account",
            "label": acc.label,
            "active": acc.is_active.load(std::sync::atomic::Ordering::Relaxed),
        })
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
        assert!(is_live_now(&account, now).await);

        account
            .rate_limited_until
            .store(now + 60, Ordering::Relaxed);
        assert!(!is_live_now(&account, now).await);
        account.rate_limited_until.store(0, Ordering::Relaxed);
        account
            .premium_checked_at
            .store(now - LIVE_PROBE_MAX_AGE_SECS - 1, Ordering::Relaxed);
        assert!(!is_live_now(&account, now).await);
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
        assert!(is_live_now(&account, now).await);

        *account.rejected_access_token.write().await = Some("token".into());
        assert!(!is_live_now(&account, now).await);
    }
}
