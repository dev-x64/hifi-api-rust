use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::account_manager::AccountManager;
use crate::notifier::Notifier;
use crate::proxy_manager::ProxyManager;
use crate::tidal_client::TidalClient;
use crate::token_manager::TokenManager;
use chrono::Utc;
use futures::{StreamExt, stream};

const LOOP_DELAY_SECS: u64 = 30;
const FULL_CHECK_INTERVAL_SECS: i64 = 6 * 60 * 60;
const FULL_CHECK_INITIAL_SPREAD_SECS: i64 = 5 * 60;
const FULL_CHECK_REPEAT_SPREAD_SECS: i64 = 15 * 60;

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

/// Always-on renewal + recovery. Explicit owner OFF is never reactivated.
pub async fn start_autoheal_loop(
    account_manager: Arc<AccountManager>,
    token_manager: Arc<TokenManager>,
    proxy_manager: Arc<ProxyManager>,
    tidal_client: Arc<TidalClient>,
    notifier: Arc<Notifier>,
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
                    async move {
                        let active = account.is_active.load(Ordering::Relaxed);
                        let auto_disabled = account.auto_disabled.load(Ordering::Relaxed);
                        let now = Utc::now().timestamp();
                        if !active && !auto_disabled {
                            return;
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
                        if !refresh_due && !check_full {
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
