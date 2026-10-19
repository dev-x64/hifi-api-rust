use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::account_manager::AccountManager;
use crate::notifier::Notifier;
use crate::proxy_manager::ProxyManager;
use crate::settings::AppSettings;
use crate::tidal_client::TidalClient;
use crate::token_manager::TokenManager;
use chrono::Utc;
use futures::{StreamExt, stream};

const LOOP_DELAY_SECS: u64 = 30;
const FULL_CHECK_INTERVAL_SECS: i64 = 6 * 60 * 60;
const FULL_CHECK_INITIAL_SPREAD_SECS: i64 = 5 * 60;
const FULL_CHECK_REPEAT_SPREAD_SECS: i64 = 15 * 60;
const SUBSCRIPTION_CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;
const SUBSCRIPTION_INITIAL_SPREAD_SECS: i64 = 10 * 60;
const SUBSCRIPTION_REPEAT_SPREAD_SECS: i64 = 60 * 60;
/// Warn this long before a subscription ends.
const SUBSCRIPTION_WARN_SECS: i64 = 3 * 24 * 60 * 60;

fn account_spread_secs(account_id: &str, window_secs: i64) -> i64 {
    let hash = account_id.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        hash.wrapping_mul(0x100000001b3) ^ u64::from(byte)
    });
    (hash % window_secs.max(1) as u64) as i64
}

fn full_check_due(
    account: &crate::account_manager::AccountState,
    now: i64,
    loop_started_at: i64,
) -> bool {
    if !account.is_active.load(Ordering::Relaxed)
        || account.is_catalog.load(Ordering::Relaxed)
        || AccountManager::rate_limit_remaining(account).is_some()
    {
        return false;
    }
    let checked_at = account.premium_checked_at.load(Ordering::Relaxed);
    let due_at = if checked_at > 0 {
        checked_at
            + FULL_CHECK_INTERVAL_SECS
            + account_spread_secs(&account.id, FULL_CHECK_REPEAT_SPREAD_SECS)
    } else {
        loop_started_at
            + account_spread_secs(&account.id, FULL_CHECK_INITIAL_SPREAD_SECS)
    };
    now >= due_at
}

fn subscription_check_due(
    account: &crate::account_manager::AccountState,
    now: i64,
    loop_started_at: i64,
) -> bool {
    if !account.is_active.load(Ordering::Relaxed)
        || account.is_catalog.load(Ordering::Relaxed)
        || AccountManager::rate_limit_remaining(account).is_some()
    {
        return false;
    }
    let checked_at = account.subscription_checked_at.load(Ordering::Relaxed);
    let due_at = if checked_at > 0 {
        checked_at
            + SUBSCRIPTION_CHECK_INTERVAL_SECS
            + account_spread_secs(&account.id, SUBSCRIPTION_REPEAT_SPREAD_SECS)
    } else {
        loop_started_at + account_spread_secs(&account.id, SUBSCRIPTION_INITIAL_SPREAD_SECS)
    };
    now >= due_at
}

/// What, if anything, the owner must hear about a subscription answer from
/// Tidal: overdue payment, a plan that is no longer active, or one ending
/// within three days. Pure function, unit tested.
pub(crate) fn subscription_problem(subscription: &serde_json::Value, now: i64) -> Option<String> {
    if subscription["paymentOverdue"].as_bool() == Some(true) {
        return Some("Payment is overdue.".into());
    }
    if let Some(status) = subscription["status"].as_str().filter(|s| *s != "ACTIVE") {
        return Some(format!("Subscription status is {status}."));
    }
    let until = subscription["validUntil"].as_str().and_then(|text| {
        chrono::DateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f%z").ok()
    })?;
    let left = until.timestamp() - now;
    if left <= 0 {
        Some(format!("Subscription ended on {}.", until.format("%Y-%m-%d")))
    } else if left <= SUBSCRIPTION_WARN_SECS {
        Some(format!(
            "Subscription ends on {} (in {} h).",
            until.format("%Y-%m-%d %H:%M UTC"),
            left / 3600
        ))
    } else {
        None
    }
}

/// Always-on renewal + recovery. Explicit owner OFF is never reactivated.
pub async fn start_autoheal_loop(
    account_manager: Arc<AccountManager>,
    token_manager: Arc<TokenManager>,
    proxy_manager: Arc<ProxyManager>,
    tidal_client: Arc<TidalClient>,
    notifier: Arc<Notifier>,
    settings: Arc<AppSettings>,
) {
    tokio::spawn(async move {
        let loop_started_at = Utc::now().timestamp();
        loop {
            let accounts = account_manager.list_accounts().await;
            stream::iter(accounts)
                .for_each_concurrent(4, |account| {
                    let am = &account_manager;
                    let tm = &token_manager;
                    let pm = &proxy_manager;
                    let tidal = &tidal_client;
                    let notifier = &notifier;
                    let settings = &settings;
                    async move {
                        let active = account.is_active.load(Ordering::Relaxed);
                        let auto_disabled = account.auto_disabled.load(Ordering::Relaxed);
                        let now = Utc::now().timestamp();
                        if !active && !auto_disabled {
                            return;
                        }
                        // Tidal has refused this token for too long: stop retrying for
                        // good. "abuse_detected" waits like any 403, since Tidal also
                        // returns it for one request while the account still works.
                        let hours = settings.auth_failure_disable_hours();
                        let suspended = account.last_refresh_error().is_some_and(|error| {
                            error.contains(crate::token_manager::ACCOUNT_SUSPENDED)
                        });
                        match am
                            .disable_after_auth_failures(&account, i64::from(hours) * 3600, now)
                            .await
                        {
                            Ok(true) => {
                                tracing::warn!(
                                    account = %account.label,
                                    hours,
                                    suspended,
                                    "Account switched off: token refresh keeps failing with 403"
                                );
                                notifier.alert_auth_disabled(&account.label, hours, suspended).await;
                                return;
                            }
                            Ok(false) => {}
                            Err(e) => tracing::warn!("Could not switch off {}: {}", account.label, e),
                        }
                        let cooling_down =
                            account.heal_next_retry.load(Ordering::Relaxed) > now;
                        let rate_limited =
                            AccountManager::rate_limit_remaining(&account).is_some();
                        let refresh_due = !cooling_down
                            && !rate_limited
                            && (auto_disabled
                                || (active && TokenManager::needs_renewal(&account).await));
                        let check_full = !cooling_down
                            && !rate_limited
                            && full_check_due(&account, now, loop_started_at);
                        let check_subscription = !cooling_down
                            && !rate_limited
                            && subscription_check_due(&account, now, loop_started_at);
                        if !refresh_due && !check_full && !check_subscription {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(rand::random::<u64>() % 1000))
                            .await;
                        if refresh_due {
                            // TokenManager resolves the account's own auth client/proxy
                            // inside its deadline; this fallback cannot bypass proxies.
                            let result = tm.refresh_token(&account, &pm.client()).await;
                            match result {
                                Ok(_) => match am.set_system_disabled(&account, false).await {
                                    Ok(true) => {
                                        tracing::info!(
                                            "Auto-heal: account {} recovered",
                                            account.label
                                        );
                                        let (healthy, total) = am.healthy_count().await;
                                        notifier.alert_healed(&account.label, healthy, total).await;
                                    }
                                    Err(e) => {
                                        tracing::warn!("Could not persist recovery: {}", e)
                                    }
                                    _ => {}
                                },
                                Err(e) => {
                                    tracing::debug!("Auto-heal for {}: {}", account.label, e);
                                    return;
                                }
                            }
                        }
                        if check_full
                            && account.is_active.load(Ordering::Relaxed)
                            && AccountManager::rate_limit_remaining(&account).is_none()
                        {
                            let previous = account.premium_status.read().await.clone();
                            let (status, reason) = tidal.probe_account_premium(&account).await;
                            am.set_premium(&account.id, &status).await;
                            if status == "preview-only" {
                                match am.note_preview_check(&account).await {
                                    Ok(true) => {
                                        tracing::warn!(
                                            account = %account.label,
                                            "Account moved to catalog-only after {} PREVIEW checks in a row",
                                            crate::account_manager::PREVIEW_CHECKS_BEFORE_CATALOG
                                        );
                                        notifier.alert_catalog_only(&account.label).await;
                                    }
                                    Ok(false) => {}
                                    Err(e) => tracing::warn!(
                                        "Could not move {} to catalog-only: {}",
                                        account.label,
                                        e
                                    ),
                                }
                            }
                            if status != previous {
                                tracing::info!(
                                    account = %account.label,
                                    previous,
                                    status,
                                    reason,
                                    "Automatic FULL/PREVIEW check changed status"
                                );
                            } else {
                                tracing::debug!(
                                    account = %account.label,
                                    status,
                                    reason,
                                    "Automatic FULL/PREVIEW check completed"
                                );
                            }
                        }
                        if check_subscription && account.is_active.load(Ordering::Relaxed) {
                            match tidal.account_subscription(&account).await {
                                Ok(result) => {
                                    let subscription = result["subscription"].clone();
                                    account.store_subscription(subscription.clone(), now).await;
                                    if let Some(problem) = subscription_problem(&subscription, now) {
                                        tracing::warn!(account = %account.label, problem, "Subscription needs attention");
                                        notifier.alert_subscription(&account.label, &problem).await;
                                    }
                                }
                                // Try again on the next daily round rather than every loop.
                                Err(e) => {
                                    account.subscription_checked_at.store(now, Ordering::Relaxed);
                                    tracing::debug!("Subscription check for {}: {}", account.label, e);
                                }
                            }
                        }
                    }
                })
                .await;
            tokio::time::sleep(Duration::from_secs(LOOP_DELAY_SECS)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_manager::AccountState;

    fn account(id: &str) -> AccountState {
        AccountState::new(
            id.into(),
            id.into(),
            "client".into(),
            "secret".into(),
            "refresh".into(),
            None,
            true,
            String::new(),
        )
    }

    #[test]
    fn subscription_problems_are_overdue_inactive_or_ending_soon() {
        use serde_json::json;
        // 2026-11-17T16:55:22Z, the real answer's validUntil.
        let until = 1_794_934_522;
        let active = json!({"status": "ACTIVE", "validUntil": "2026-11-17T16:55:22.503+0000", "paymentOverdue": false});
        assert_eq!(subscription_problem(&active, until - 10 * 24 * 3600), None);
        let soon = subscription_problem(&active, until - 2 * 24 * 3600).unwrap();
        assert!(soon.contains("2026-11-17") && soon.contains("in 48 h"), "{soon}");
        assert!(subscription_problem(&active, until + 1).unwrap().contains("ended"));
        let overdue = json!({"status": "ACTIVE", "paymentOverdue": true});
        assert_eq!(subscription_problem(&overdue, 0).as_deref(), Some("Payment is overdue."));
        let cancelled = json!({"status": "CANCELLED"});
        assert_eq!(subscription_problem(&cancelled, 0).as_deref(), Some("Subscription status is CANCELLED."));
    }

    #[test]
    fn subscription_check_runs_daily_and_skips_catalog_accounts() {
        let account = account("account-a");
        let started = 10_000;
        assert!(subscription_check_due(&account, started + SUBSCRIPTION_INITIAL_SPREAD_SECS, started));
        account.subscription_checked_at.store(20_000, Ordering::Relaxed);
        assert!(!subscription_check_due(&account, 20_000 + SUBSCRIPTION_CHECK_INTERVAL_SECS - 1, started));
        assert!(subscription_check_due(
            &account,
            20_000 + SUBSCRIPTION_CHECK_INTERVAL_SECS + SUBSCRIPTION_REPEAT_SPREAD_SECS,
            started
        ));
        account.is_catalog.store(true, Ordering::Relaxed);
        assert!(!subscription_check_due(&account, i64::MAX / 2, started));
    }

    #[test]
    fn full_check_is_staggered_and_repeats_after_interval() {
        let account = account("account-a");
        let started = 10_000;
        assert!(!full_check_due(&account, started - 1, started));
        assert!(full_check_due(
            &account,
            started + FULL_CHECK_INITIAL_SPREAD_SECS,
            started
        ));
        account.premium_checked_at.store(20_000, Ordering::Relaxed);
        assert!(!full_check_due(
            &account,
            20_000 + FULL_CHECK_INTERVAL_SECS - 1,
            started
        ));
        assert!(full_check_due(
            &account,
            20_000 + FULL_CHECK_INTERVAL_SECS + FULL_CHECK_REPEAT_SPREAD_SECS,
            started
        ));
    }

    #[test]
    fn full_check_skips_manual_off_catalog_and_rate_limited_accounts() {
        let account = account("account-b");
        account.is_active.store(false, Ordering::Relaxed);
        assert!(!full_check_due(&account, i64::MAX / 2, 0));
        account.is_active.store(true, Ordering::Relaxed);
        account.is_catalog.store(true, Ordering::Relaxed);
        assert!(!full_check_due(&account, i64::MAX / 2, 0));
        account.is_catalog.store(false, Ordering::Relaxed);
        account
            .rate_limited_until
            .store(Utc::now().timestamp() + 60, Ordering::Relaxed);
        assert!(!full_check_due(&account, Utc::now().timestamp(), 0));
    }
}
