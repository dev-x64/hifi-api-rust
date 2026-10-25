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
const FULL_CHECK_INTERVAL_SECS: i64 = 60 * 60;
const FULL_CHECK_INITIAL_SPREAD_SECS: i64 = 5 * 60;
const FULL_CHECK_REPEAT_SPREAD_SECS: i64 = 15 * 60;
const SUBSCRIPTION_CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;
const SUBSCRIPTION_INITIAL_SPREAD_SECS: i64 = 10 * 60;
const SUBSCRIPTION_REPEAT_SPREAD_SECS: i64 = 60 * 60;
/// A failed check is retried after this long instead of a full day.
const SUBSCRIPTION_RETRY_SECS: i64 = 60 * 60;
/// A fresh token also checks the subscription, unless it was checked this recently.
const SUBSCRIPTION_AFTER_REFRESH_SECS: i64 = 60 * 60;
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
    let failed_at = account.subscription_error_at.load(Ordering::Relaxed);
    let due_at = if failed_at > 0 {
        failed_at + SUBSCRIPTION_RETRY_SECS
    } else if checked_at > 0 {
        checked_at
            + SUBSCRIPTION_CHECK_INTERVAL_SECS
            + account_spread_secs(&account.id, SUBSCRIPTION_REPEAT_SPREAD_SECS)
    } else {
        loop_started_at + account_spread_secs(&account.id, SUBSCRIPTION_INITIAL_SPREAD_SECS)
    };
    now >= due_at
}

/// What, if anything, the owner must hear about a subscription answer from
/// Tidal: overdue payment, no premium access on a paid plan, a plan that is
/// no longer active, or one ending within three days. Pure function, unit tested.
pub(crate) fn subscription_problem(subscription: &serde_json::Value, now: i64) -> Option<String> {
    if subscription["paymentOverdue"].as_bool() == Some(true) {
        return Some("Payment is overdue.".into());
    }
    if subscription["premiumAccess"].as_bool() == Some(false) {
        return Some("Tidal reports no premium access.".into());
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

/// True when Tidal reports the FREE plan, which cannot play full tracks.
/// `premiumAccess: true` wins over the plan name: Tidal grants it to FREE
/// accounts on a trial or promotion.
pub(crate) fn subscription_is_free(subscription: &serde_json::Value) -> bool {
    subscription["premiumAccess"].as_bool() != Some(true)
        && subscription["subscription"]["type"]
            .as_str()
            .or_else(|| subscription["type"].as_str())
            .is_some_and(|plan| plan.eq_ignore_ascii_case("FREE"))
}

/// Store a subscription answer, move a FREE account to catalog-only and
/// alert the owner about a plan that needs attention. Shared by the
/// automatic and the manual check.
pub(crate) async fn apply_subscription(
    account_manager: &AccountManager,
    notifier: &Notifier,
    account: &crate::account_manager::AccountState,
    subscription: serde_json::Value,
    now: i64,
) {
    account.store_subscription(subscription.clone(), now).await;
    if subscription_is_free(&subscription) {
        if account.is_catalog.load(Ordering::Relaxed) {
            return;
        }
        match account_manager.set_account_catalog(&account.id, true).await {
            Ok(()) => {
                tracing::warn!(account = %account.label, "Account moved to catalog-only: Tidal plan is FREE");
                notifier.alert_free_plan(&account.label).await;
            }
            Err(e) => tracing::warn!("Could not move {} to catalog-only: {}", account.label, e),
        }
        return;
    }
    if let Some(problem) = subscription_problem(&subscription, now) {
        tracing::warn!(account = %account.label, problem, "Subscription needs attention");
        notifier.alert_subscription(&account.label, &problem).await;
    }
}

/// Automatic subscription check: daily, after a token refresh, or an hour
/// after a failed attempt. A failure is kept next to the last good answer.
pub(crate) async fn check_subscription_now(
    tidal: &TidalClient,
    account_manager: &AccountManager,
    notifier: &Notifier,
    account: &Arc<crate::account_manager::AccountState>,
) {
    let now = Utc::now().timestamp();
    match tidal.account_subscription(account).await {
        Ok(result) => {
            apply_subscription(account_manager, notifier, account, result["subscription"].clone(), now)
                .await;
        }
        Err(e) => {
            tracing::warn!(account = %account.label, error = %e, "Automatic subscription check failed");
            account.store_subscription_error(e.to_string(), now).await;
        }
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
                        let mut check_subscription = !cooling_down
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
                                Ok(_) => {
                                    let last_attempt = account
                                        .subscription_checked_at
                                        .load(Ordering::Relaxed)
                                        .max(account.subscription_error_at.load(Ordering::Relaxed));
                                    check_subscription |= now - last_attempt
                                        >= SUBSCRIPTION_AFTER_REFRESH_SECS
                                        && !account.is_catalog.load(Ordering::Relaxed);
                                    match am.set_system_disabled(&account, false).await {
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
                                    }
                                }
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
                            check_subscription_now(tidal, am, notifier, &account).await;
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
        let no_access = json!({"status": "ACTIVE", "premiumAccess": false, "subscription": {"type": "HIFI"}});
        assert_eq!(subscription_problem(&no_access, 0).as_deref(), Some("Tidal reports no premium access."));
        let access = json!({"status": "ACTIVE", "premiumAccess": true, "validUntil": "2099-01-01T00:00:00.000+0000"});
        assert_eq!(subscription_problem(&access, 0), None);
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

    #[tokio::test]
    async fn failed_subscription_check_keeps_the_answer_and_retries_in_an_hour() {
        let account = account("account-c");
        let answer = serde_json::json!({"status": "ACTIVE"});
        account.store_subscription(answer.clone(), 10_000).await;
        account.store_subscription_error("Tidal HTTP 500".into(), 20_000).await;
        assert_eq!(account.subscription(), Some(answer.clone()));
        assert_eq!(account.subscription_checked_at.load(Ordering::Relaxed), 10_000);
        assert!(!subscription_check_due(&account, 20_000 + SUBSCRIPTION_RETRY_SECS - 1, 0));
        assert!(subscription_check_due(&account, 20_000 + SUBSCRIPTION_RETRY_SECS, 0));
        account.store_subscription(answer, 30_000).await;
        assert_eq!(account.subscription_error(), None);
        assert!(!subscription_check_due(&account, 30_000 + SUBSCRIPTION_RETRY_SECS, 0));
    }

    #[test]
    fn free_plan_is_recognised_in_either_shape() {
        use serde_json::json;
        assert!(subscription_is_free(&json!({"subscription": {"type": "FREE"}, "status": "ACTIVE"})));
        assert!(subscription_is_free(&json!({"type": "free"})));
        assert!(!subscription_is_free(&json!({"subscription": {"type": "HIFI"}, "status": "ACTIVE"})));
        assert!(!subscription_is_free(&json!({"status": "ACTIVE"})));
        // premiumAccess overrides the plan name only towards premium.
        assert!(!subscription_is_free(&json!({"subscription": {"type": "FREE"}, "premiumAccess": true})));
        assert!(subscription_is_free(&json!({"subscription": {"type": "FREE"}, "premiumAccess": false})));
        assert!(!subscription_is_free(&json!({"subscription": {"type": "HIFI"}, "premiumAccess": false})));
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
