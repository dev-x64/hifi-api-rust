use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::account_manager::AccountManager;
use crate::notifier::Notifier;
use crate::proxy_manager::ProxyManager;
use crate::token_manager::TokenManager;
use chrono::Utc;
use futures::{StreamExt, stream};

/// Always-on renewal + recovery. Explicit owner OFF is never reactivated.
pub async fn start_autoheal_loop(
    account_manager: Arc<AccountManager>,
    token_manager: Arc<TokenManager>,
    proxy_manager: Arc<ProxyManager>,
    notifier: Arc<Notifier>,
) {
    tokio::spawn(async move {
        loop {
            let accounts = account_manager.list_accounts().await;
            stream::iter(accounts)
                .for_each_concurrent(4, |account| {
                    let am = &account_manager;
                    let tm = &token_manager;
                    let pm = &proxy_manager;
                    let notifier = &notifier;
                    async move {
                        let active = account.is_active.load(Ordering::Relaxed);
                        let auto_disabled = account.auto_disabled.load(Ordering::Relaxed);
                        let now = Utc::now().timestamp();
                        if (!active && !auto_disabled)
                            || account.heal_next_retry.load(Ordering::Relaxed) > now
                            || AccountManager::rate_limit_remaining(&account).is_some()
                            || (active && !TokenManager::needs_renewal(&account).await)
                        {
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(rand::random::<u64>() % 1000))
                            .await;
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
                                Err(e) => tracing::warn!("Could not persist recovery: {}", e),
                                _ => {}
                            },
                            Err(e) => tracing::debug!("Auto-heal for {}: {}", account.label, e),
                        }
                    }
                })
                .await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
