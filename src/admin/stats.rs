use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::AppError;

pub async fn get_stats(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let accounts = state.account_manager.list_accounts().await;
    let total_requests: u64 = accounts
        .iter()
        .map(|a| a.request_count.load(std::sync::atomic::Ordering::Relaxed))
        .sum();
    let total_errors: u64 = accounts
        .iter()
        .map(|a| a.error_count.load(std::sync::atomic::Ordering::Relaxed))
        .sum();
    let requests_per_second_60s = state.request_log.requests_last_60s() as f64 / 60.0;
    let recent_p95_ms = state.request_log.recent_p95_ms();
    let active_count = accounts
        .iter()
        .filter(|a| a.is_active.load(std::sync::atomic::Ordering::Relaxed))
        .count();
    let mut premium_count = 0;
    for account in &accounts {
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
        "error_rate": if total_requests > 0 {
            format!("{:.2}%", (total_errors as f64 / total_requests as f64) * 100.0)
        } else { "0.00%".into() },
        "requests_per_second_60s": requests_per_second_60s,
        "recent_p95_ms": recent_p95_ms,
        "total_accounts": accounts.len(),
        "active_accounts": active_count,
        "premium_accounts": premium_count,
        "healthy_accounts": active_count,
        "playback_accounts": playback_count,
        "playback": playback,
        "catalog": catalog,
        "redis": redis,
    })))
}
