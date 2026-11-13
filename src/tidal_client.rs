use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::time::Duration;

use chrono::Utc;
use rand::Rng;
use reqwest::Client;
use serde_json::{Value, json};

use crate::account_manager::{AccountManager, AccountState, CatalogClaim, LeaseKind, LeaseRules, PlaybackLease};
use crate::config::Config;
use crate::error::AppError;
use crate::notifier::Notifier;
use crate::proxy_manager::ProxyManager;
use crate::settings::AppSettings;
use crate::token_manager::TokenManager;

pub struct TidalClient {
    proxy_manager: Arc<ProxyManager>,
    token_manager: Arc<TokenManager>,
    account_manager: Arc<AccountManager>,
    notifier: Arc<Notifier>,
    config: Arc<Config>,
    settings: Arc<AppSettings>,
    catalog_rate_limited_until: AtomicI64,
    /// Round-robin cursor over account proxies for search without login.
    search_egress: AtomicUsize,
    /// Client id sent with search without login.
    search_client_id: &'static str,
}

/// Egresses one search tries without login before it falls back to the
/// accounts.
const SEARCH_WITHOUT_LOGIN_TRIES: usize = 2;

/// Other accounts one request may try after Tidal rate limits it. One more
/// 429 is returned to the caller instead of walking the whole pool.
pub(crate) const RATE_LIMIT_FALLBACKS: usize = 2;

/// The 429s one request has met while failing over.
#[derive(Default)]
pub(crate) struct RateLimitFallback {
    first_try: Option<usize>,
    hits: usize,
}

impl RateLimitFallback {
    /// Record a 429 from `account_id` and schedule another account, or
    /// return the error once the fallbacks are used up.
    pub(crate) fn schedule(
        &mut self,
        account_try: usize,
        account_id: &str,
        error: AppError,
        failed_ids: &mut Vec<String>,
        last_error: &mut Option<AppError>,
    ) -> Result<(), AppError> {
        self.hits += 1;
        if self.hits > RATE_LIMIT_FALLBACKS {
            return Err(error);
        }
        self.first_try.get_or_insert(account_try);
        failed_ids.push(account_id.to_string());
        *last_error = Some(error);
        Ok(())
    }

    /// After the first 429 only `RATE_LIMIT_FALLBACKS` more accounts are
    /// tried, whatever they answer.
    pub(crate) fn exhausted(&self, account_try: usize) -> bool {
        self.first_try.is_some_and(|first| account_try > first + RATE_LIMIT_FALLBACKS)
    }
}

impl TidalClient {
    pub fn new(
        proxy_manager: Arc<ProxyManager>,
        token_manager: Arc<TokenManager>,
        account_manager: Arc<AccountManager>,
        notifier: Arc<Notifier>,
        config: Arc<Config>,
        settings: Arc<AppSettings>,
    ) -> Self {
        Self {
            proxy_manager,
            token_manager,
            account_manager,
            notifier,
            config,
            settings,
            catalog_rate_limited_until: AtomicI64::new(0),
            search_egress: AtomicUsize::new(0),
            search_client_id: crate::setup::REQUEST_CLIENT_ID,
        }
    }

    /// Current HTTP client (whatever the proxy swap holds right now).
    pub fn http_client(&self) -> Client {
        self.proxy_manager.client()
    }

    /// Client gated for Tidal traffic: resolves a working proxy first,
    /// or errors (never silently leaks direct) unless fallback is enabled.
    pub async fn working_client(&self) -> Result<Client, AppError> {
        self.proxy_manager.working_client().await
    }

    pub async fn working_client_for(&self, account_id: &str) -> Result<Client, AppError> {
        self.proxy_manager.working_client_for(account_id).await
    }

    pub fn proxy_manager(&self) -> &Arc<ProxyManager> {
        &self.proxy_manager
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn token_manager(&self) -> &TokenManager {
        &self.token_manager
    }

    pub fn account_manager(&self) -> &AccountManager {
        &self.account_manager
    }

    pub fn v1_playback_url(&self, track_id: i64) -> String {
        self.v1_playback_url_at("https://api.tidal.com/v1", track_id)
    }

    fn v1_playback_url_at(&self, api_base: &str, track_id: i64) -> String {
        format!("{}/tracks/{}/{}", api_base, track_id, self.settings.v1_playback_endpoint())
    }

    pub async fn make_request(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
    ) -> Result<Value, AppError> {
        self.make_request_with_account(url, params, None).await
    }

    pub async fn make_request_with_account(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
        preferred_account: Option<Arc<AccountState>>,
    ) -> Result<Value, AppError> {
        self.make_request_with_limit(
            url,
            params,
            preferred_account,
            None,
            self.settings.track_requests_per_account(),
            LeaseKind::Playback,
        )
        .await
    }

    /// Lease a playback account for one request, skipping accounts already
    /// sending `track_concurrency_per_account` requests to Tidal, resting for
    /// `track_request_delay_ms` after their last one, or at their hourly limit.
    pub async fn lease_pool_account(
        &self,
        exclude_ids: &[String],
        kind: LeaseKind,
    ) -> Result<PlaybackLease, AppError> {
        self.lease_pool_account_for(exclude_ids, kind, None).await
    }

    /// Like `lease_pool_account`, also skipping accounts whose Tidal quota
    /// for this endpoint `scope` is used up.
    async fn lease_pool_account_for(
        &self,
        exclude_ids: &[String],
        kind: LeaseKind,
        scope: Option<&'static str>,
    ) -> Result<PlaybackLease, AppError> {
        let rules = LeaseRules {
            scope,
            concurrency: self.settings.track_concurrency_per_account(),
            rest_ms: self.settings.track_request_delay_ms(),
            per_hour: self.settings.requests_per_hour_per_account(),
        };
        self.account_manager
            .lease_account_for(exclude_ids, kind, rules)
            .await
    }

    async fn make_request_with_limit(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
        preferred_account: Option<Arc<AccountState>>,
        account_limit: Option<usize>,
        requests_per_account: u32,
        kind: LeaseKind,
    ) -> Result<Value, AppError> {
        let requests_per_account = requests_per_account.max(1);

        let mut failed_ids: Vec<String> = Vec::new();
        // Accounts that answered PREVIEW to this request; counted against them
        // only if another account then gets FULL for the same track.
        let mut previewed: Vec<Arc<AccountState>> = Vec::new();
        let account_count = self.account_manager.playback_count().await;
        let max_account_attempts = std::cmp::max(1, account_count)
            .min(account_limit.unwrap_or(usize::MAX));
        let mut last_account_error: Option<AppError> = None;
        let mut rate_limits = RateLimitFallback::default();
        let mut preferred_cooldown_consumed = false;
        let scope = AccountManager::quota_scope(url);

        for account_try in 0..max_account_attempts {
            if preferred_cooldown_consumed && account_try > 0
                || rate_limits.exhausted(account_try)
            {
                break;
            }
            // Held for every attempt on this account; released on failover.
            let lease = if account_try == 0 && failed_ids.is_empty() {
                match preferred_account.clone() {
                    Some(a) => {
                        let blocked = AccountManager::rate_limit_remaining(&a).or_else(|| {
                            scope.and_then(|s| a.quota_remaining(s, Utc::now().timestamp()))
                        });
                        if let Some(seconds) = blocked {
                            rate_limits.schedule(
                                account_try,
                                &a.id,
                                AppError::RateLimited(seconds),
                                &mut failed_ids,
                                &mut last_account_error,
                            )?;
                            preferred_cooldown_consumed = true;
                            match self.lease_pool_account_for(&failed_ids, kind, scope).await {
                                Ok(alternative) => alternative,
                                Err(_) => return Err(AppError::RateLimited(seconds)),
                            }
                        } else {
                            // Account the preferred pick like any selection so
                            // the balancer sees its true load.
                            AccountManager::lease(&a)
                        }
                    }
                    None => match self
                        .lease_pool_account_for(&failed_ids, kind, scope)
                        .await
                    {
                        Ok(a) => a,
                        Err(e) => {
                            self.alert_if_all_down(&e).await;
                            return Err(e.after_failover(last_account_error));
                        }
                    },
                }
            } else {
                match self
                    .lease_pool_account_for(&failed_ids, kind, scope)
                    .await
                {
                    Ok(a) => a,
                    Err(e) => {
                        self.alert_if_all_down(&e).await;
                        return Err(e.after_failover(last_account_error));
                    }
                }
            };
            let account = lease.account().clone();

            let mut http = self.working_client_for(&account.id).await?;

            let mut refreshed_after_401 = false;
            for attempt in 0..requests_per_account {
                let token = match self.token_manager.get_token(&account, &http).await {
                    Ok(t) => t,
                    Err(e @ AppError::RateLimited(_)) => {
                        rate_limits.schedule(
                            account_try,
                            &account.id,
                            e,
                            &mut failed_ids,
                            &mut last_account_error,
                        )?;
                        break;
                    }
                    Err(e) => {
                        self.account_manager
                            .mark_account_error(&account.id, &format!("token failure: {:?}", e))
                            .await;
                        last_account_error = Some(e);
                        failed_ids.push(account.id.clone());
                        break;
                    }
                };

                if attempt > 0 {
                    let jitter = rand::thread_rng().gen_range(100..500);
                    tokio::time::sleep(Duration::from_millis(jitter)).await;
                }

                // A refresh may have moved this account when the optional
                // rotate-on-refresh setting is enabled.
                if let Some(seconds) = AccountManager::rate_limit_remaining(&account) {
                    rate_limits.schedule(
                        account_try,
                        &account.id,
                        AppError::RateLimited(seconds),
                        &mut failed_ids,
                        &mut last_account_error,
                    )?;
                    break;
                }
                http = self.working_client_for(&account.id).await?;

                let mut req = http
                    .get(url)
                    .header("authorization", format!("Bearer {}", token))
                .header("X-Tidal-Token", account.client_id.as_str())
                    .header("User-Agent", self.config.user_agent.as_str())
                    .header("Accept", "*/*")
                    .header("Accept-Encoding", "gzip")
                    .header("Accept-Language", "en-US,en;q=0.9")
                    .header("X-Platform", "android")
                    .header("X-Tidal-Platform", "android");

                if let Some(ref p) = params {
                    req = req.query(&p);
                }

                let resp = match req.send().await {
                    Ok(r) => {
                        self.proxy_manager.note_success_for(&account.id).await;
                        r
                    }
                    Err(e) => {
                        if e.is_connect() || e.is_timeout() {
                            self.proxy_manager.note_failure_for(&account.id).await;
                        }
                        return Err(e.into());
                    }
                };
                let status = resp.status();

                match status.as_u16() {
                    401 => {
                        if !refreshed_after_401 {
                            if attempt + 1 < requests_per_account {
                                refreshed_after_401 = true;
                                match self.token_manager.refresh_after_unauthorized(&account, &http, &token).await {
                                    Ok(_) => continue,
                                    Err(e @ AppError::RateLimited(_)) => {
                                        rate_limits.schedule(
                                            account_try, &account.id, e,
                                            &mut failed_ids, &mut last_account_error,
                                        )?;
                                        break;
                                    }
                                    Err(e) => last_account_error = Some(e),
                                }
                            } else {
                                last_account_error = Some(AppError::Unauthorized(
                                    "Tidal rejected token; per-account request limit prevented refresh retry".into(),
                                ));
                            }
                        } else {
                            TokenManager::reject_refreshed_token(&account, &token).await;
                            last_account_error = Some(AppError::Unauthorized("Tidal rejected refreshed token".into()));
                        }
                        self.account_manager.mark_account_error(&account.id, "Tidal 401 unauthorized").await;
                        failed_ids.push(account.id.clone());
                        break;
                    }
                    404 => {
                        return Err(AppError::NotFound("Resource not found".into()));
                    }
                    429 => {
                        let (seconds, quota) = AccountManager::note_429(
                            &account,
                            url,
                            resp.headers()
                                .get(reqwest::header::RETRY_AFTER)
                                .and_then(|v| v.to_str().ok()),
                        ).await;
                        if quota {
                            // Only this endpoint is exhausted on this account;
                            // others may still have quota left, and this one
                            // is skipped for it from now on.
                            failed_ids.push(account.id.clone());
                            last_account_error = Some(AppError::RateLimited(seconds));
                            break;
                        }
                        self.account_manager
                            .mark_account_error(&account.id, "Tidal HTTP 429")
                            .await;
                        tracing::warn!(account = %account.label, seconds, "Tidal rate limit: pausing account");
                        rate_limits.schedule(
                            account_try,
                            &account.id,
                            AppError::RateLimited(seconds),
                            &mut failed_ids,
                            &mut last_account_error,
                        )?;
                        break;
                    }
                    403 => {
                        if attempt + 1 < requests_per_account {
                            continue;
                        }
                        self.account_manager
                            .mark_account_error(&account.id, "Tidal 403 forbidden")
                            .await;
                        let (healthy, total) = self.account_manager.healthy_count().await;
                        self.notifier
                            .alert_403(&account.label, healthy, total)
                            .await;
                        failed_ids.push(account.id.clone());
                        last_account_error =
                            Some(AppError::UpstreamError(status, "Upstream API error".into()));
                        break;
                    }
                    _ => {
                        if !status.is_success() {
                            if attempt + 1 < requests_per_account && status.as_u16() >= 500 {
                                continue;
                            }
                            self.account_manager
                                .mark_account_error(
                                    &account.id,
                                    &format!("Tidal HTTP {}", status.as_u16()),
                                )
                                .await;
                            failed_ids.push(account.id.clone());
                            last_account_error =
                                Some(AppError::UpstreamError(status, "Upstream API error".into()));
                            break;
                        }
                    }
                }

                let body = resp.text().await?;
                self.dev_log("GET", url, status.as_u16(), &body);
                let data: Value = serde_json::from_str(&body).map_err(|e| {
                    AppError::UpstreamError(
                        status,
                        format!(
                            "Failed to parse Tidal response: {} | body: {}",
                            e,
                            body.chars().take(200).collect::<String>()
                        ),
                    )
                })?;

                // Preview-only (FULL requires subscription) → try next account instead of returning 30s snippet
                let is_preview = data.get("assetPresentation").and_then(|v| v.as_str())
                    == Some("PREVIEW")
                    || data
                        .pointer("/data/attributes/trackPresentation")
                        .and_then(|v| v.as_str())
                        == Some("PREVIEW");
                // Ordinary traffic doubles as the FULL check: a FULL answer proves
                // the subscription, so the scheduled probe can skip busy accounts.
                // A PREVIEW may be about this one track, so it proves nothing.
                let is_full = data.get("assetPresentation").and_then(|v| v.as_str()) == Some("FULL")
                    || data
                        .pointer("/data/attributes/trackPresentation")
                        .and_then(|v| v.as_str())
                        == Some("FULL");
                if is_full && kind == LeaseKind::Playback {
                    self.account_manager.confirm_full(&account).await;
                    for other in &previewed {
                        self.account_manager.note_traffic_preview(other).await;
                    }
                }
                if is_preview {
                    previewed.push(account.clone());
                    failed_ids.push(account.id.clone());
                    last_account_error = Some(AppError::PreviewOnly(format!(
                        "Preview only for track: account {} cannot provide FULL (subscription required)",
                        account.id
                    )));
                    break;
                }

                if url.contains("playbackinfo") || url.contains("trackManifests") {
                    return Ok(json!({"version": self.config.api_version, "data": data}));
                }

                return Ok(json!({"version": self.config.api_version, "data": data}));
            }
        }

        Err(last_account_error.unwrap_or(AppError::ServiceUnavailable(
            "All accounts failed after fallback".into(),
        )))
    }

    async fn alert_if_all_down(&self, e: &AppError) {
        if let AppError::ServiceUnavailable(msg) = e {
            if msg.contains("All accounts") {
                let (_, total) = self.account_manager.healthy_count().await;
                self.notifier.alert_all_down(total).await;
            }
        }
    }

    pub async fn make_authed_request(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
        token: &str,
        account_id: &str,
    ) -> Result<Value, AppError> {
        let account = self.account_manager.get_account_by_id(account_id).await;
        if let Some(seconds) = account.as_deref().and_then(|a| {
            AccountManager::rate_limit_remaining(a).or_else(|| {
                AccountManager::quota_scope(url)
                    .and_then(|s| a.quota_remaining(s, Utc::now().timestamp()))
            })
        }) {
            return Err(AppError::RateLimited(seconds));
        }
        let http = self.working_client_for(account_id).await?;
        let mut req = http
            .get(url)
            .header("authorization", format!("Bearer {}", token))
            .header("User-Agent", self.config.user_agent.as_str())
            .header("Accept", "*/*")
            .header("Accept-Encoding", "gzip")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("X-Platform", "android")
            .header("X-Tidal-Platform", "android");

        if let Some(account) = &account {
            req = req.header("X-Tidal-Token", account.client_id.as_str());
        }
        if let Some(ref p) = params {
            req = req.query(&p);
        }

        let resp = match req.send().await {
            Ok(resp) => {
                self.proxy_manager.note_success_for(account_id).await;
                resp
            }
            Err(e) => {
                if e.is_connect() || e.is_timeout() {
                    self.proxy_manager.note_failure_for(account_id).await;
                }
                return Err(e.into());
            }
        };
        let status = resp.status();

        if status.as_u16() == 429 {
            let seconds = match account {
                Some(ref account) => AccountManager::note_429(
                    account,
                    url,
                    resp.headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok()),
                ).await.0,
                None => AccountManager::retry_after_secs(
                    resp.headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok()),
                ),
            };
            self.account_manager
                .mark_account_error(account_id, "Tidal HTTP 429")
                .await;
            return Err(AppError::RateLimited(seconds));
        }
        if !status.is_success() {
            return Err(AppError::UpstreamError(status, "Upstream API error".into()));
        }

        let body = resp.text().await?;
        self.dev_log("GET", url, status.as_u16(), &body);
        let data: Value = serde_json::from_str(&body).map_err(|e| {
            AppError::UpstreamError(
                status,
                format!(
                    "Failed to parse Tidal response: {} | body: {}",
                    e,
                    body.chars().take(200).collect::<String>()
                ),
            )
        })?;
        Ok(data)
    }

    /// Verbose upstream logging (upstream DEV_MODE). No-op unless enabled.
    fn dev_log(&self, method: &str, url: &str, status: u16, body: &str) {
        if !self.config.dev_mode {
            return;
        }
        tracing::info!(
            "[DEV] {} {} → {}\n  body: {}",
            method,
            url,
            status,
            body.chars().take(1000).collect::<String>(),
        );
    }

    /// Metadata request (upstream catalog=True): static CATALOG_TOKEN first,
    /// then the dedicated catalog account, then the playback pool.
    /// Returns the wrapped {version, data} envelope like make_request.
    pub async fn make_catalog_request(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
    ) -> Result<Value, AppError> {
        let owned: Vec<(String, String)> = params
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let borrowed: Vec<(&str, &str)> = owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let data = self.catalog_get(url, borrowed).await?;
        Ok(json!({"version": self.config.api_version, "data": data}))
    }

    /// Raw metadata GET (unwrapped payload) with the same catalog preference.
    pub async fn make_catalog_authed_request(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
    ) -> Result<Value, AppError> {
        let owned: Vec<(String, String)> = params
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let borrowed: Vec<(&str, &str)> = owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        self.catalog_get(url, borrowed).await
    }

    /// Shared catalog resolution: static token → catalog account → pool.
    /// A 429 uses at most one other credential, including the playback pool.
    /// Catalog accounts at their concurrency limit are skipped; when all are
    /// busy the request overflows to the pool.
    async fn catalog_get(&self, url: &str, params: Vec<(&str, &str)>) -> Result<Value, AppError> {
        if AccountManager::quota_scope(url) == Some("search") {
            match self.search_without_login(url, &params).await {
                Ok(data) => return Ok(data),
                Err(e) => tracing::warn!("Search without login failed, using accounts: {}", e),
            }
        }
        let mut rate_limit_error: Option<AppError> = None;
        let mut fallback_tried = false;
        let mut catalog_busy = false;
        let concurrency = self.settings.catalog_concurrency_per_account();
        if !self.config.catalog_token.is_empty() {
            match self.catalog_static_get(url, params.clone()).await {
                Ok(data) => return Ok(data),
                Err(e @ AppError::RateLimited(_)) => rate_limit_error = Some(e),
                Err(e) => {
                    tracing::debug!("Catalog static token failed, trying catalog account: {}", e);
                }
            }
        }
        for _ in 0..2 {
            if rate_limit_error.is_some() && fallback_tried {
                return Err(rate_limit_error.unwrap());
            }
            // Held for the whole upstream call; released when it goes out of scope.
            let slot = match self.account_manager.claim_catalog_slot(concurrency).await {
                CatalogClaim::Slot(slot) => slot,
                CatalogClaim::Busy => {
                    catalog_busy = true;
                    break;
                }
                CatalogClaim::Unavailable => break,
            };
            if rate_limit_error.is_some() {
                fallback_tried = true;
            }
            match self.catalog_account_get(slot.account(), url, params.clone()).await {
                Ok(data) => return Ok(data),
                Err(e @ AppError::RateLimited(_)) => {
                    if fallback_tried {
                        return Err(e);
                    }
                    rate_limit_error = Some(e);
                }
                Err(e) => {
                    if fallback_tried {
                        return Err(e);
                    }
                    tracing::debug!("Catalog account failed, falling back to pool: {}", e);
                    break;
                }
            }
        }
        if fallback_tried {
            return Err(rate_limit_error.unwrap());
        }
        if rate_limit_error.is_none() {
            rate_limit_error = self
                .account_manager
                .catalog_rate_limit_remaining()
                .await
                .map(AppError::RateLimited);
        }
        if let Some(previous_429) = rate_limit_error {
            return match self
                .make_request_with_limit(
                    url,
                    Some(params),
                    None,
                    Some(1),
                    self.settings.catalog_requests_per_account(),
                    LeaseKind::Metadata,
                )
                .await
            {
                Ok(wrapped) => Ok(wrapped.get("data").cloned().unwrap_or(Value::Null)),
                Err(AppError::ServiceUnavailable(_)) => Err(previous_429),
                Err(e) => Err(e),
            };
        }
        // Without a playback pool there is nowhere for the overflow to go:
        // shed it instead of reporting every account as down.
        if catalog_busy && self.account_manager.playback_count().await == 0 {
            return Err(AppError::AccountsBusy(
                crate::error::CATALOG_BUSY.into(),
                1,
            ));
        }
        // No catalog configured (or it failed or is busy): normal pool request.
        self.make_request_with_limit(
            url,
            Some(params),
            None,
            None,
            self.settings.catalog_requests_per_account(),
            LeaseKind::Metadata,
        )
            .await
            .map(|wrapped| wrapped.get("data").cloned().unwrap_or(Value::Null))
    }

    /// Search sent with only `X-Tidal-Token`, no account token. Tidal counts
    /// the daily search quota per signed-in user and answers about half of
    /// the account searches with 429 once it is used up; without login the
    /// same searches go through (load-tested at 300/min over the proxies,
    /// 180/min on one proxy, no 429). Goes out through the account proxies
    /// in turn so the host IP is never used.
    async fn search_without_login(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<Value, AppError> {
        crate::request_log::note_account("search-client", "без входа", "search");
        let accounts: Vec<Arc<AccountState>> = self
            .account_manager
            .list_accounts()
            .await
            .into_iter()
            .filter(|a| a.is_active.load(Ordering::Relaxed))
            .collect();
        let tries = accounts.len().clamp(1, SEARCH_WITHOUT_LOGIN_TRIES);
        let mut last_error = AppError::ServiceUnavailable("No egress for search".into());
        for _ in 0..tries {
            let egress = (!accounts.is_empty()).then(|| {
                let i = self.search_egress.fetch_add(1, Ordering::Relaxed);
                accounts[i % accounts.len()].id.clone()
            });
            let http = match egress.as_deref() {
                Some(id) => self.working_client_for(id).await?,
                None => self.working_client().await?,
            };
            let mut req = http
                .get(url)
                .header("X-Tidal-Token", self.search_client_id)
                .header("User-Agent", self.config.user_agent.as_str())
                .header("Accept", "*/*")
                .header("Accept-Encoding", "gzip")
                .header("Accept-Language", "en-US,en;q=0.9")
                .header("X-Platform", "android")
                .header("X-Tidal-Platform", "android");
            if !params.is_empty() {
                req = req.query(params);
            }
            let resp = match req.send().await {
                Ok(resp) => {
                    if let Some(id) = egress.as_deref() {
                        self.proxy_manager.note_success_for(id).await;
                    }
                    resp
                }
                Err(e) => {
                    if let Some(id) = egress.as_deref()
                        && (e.is_connect() || e.is_timeout())
                    {
                        self.proxy_manager.note_failure_for(id).await;
                    }
                    last_error = e.into();
                    continue;
                }
            };
            let status = resp.status();
            if status.as_u16() == 429 {
                last_error = AppError::RateLimited(AccountManager::retry_after_secs(
                    resp.headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok()),
                ));
                continue;
            }
            if !status.is_success() {
                last_error = AppError::UpstreamError(
                    status,
                    "Search without login failed".into(),
                );
                // A 4xx other than 403 (a blocked proxy) is the same on any
                // egress.
                if status.is_client_error() && status.as_u16() != 403 {
                    break;
                }
                continue;
            }
            let body = resp.text().await?;
            self.dev_log("GET", url, status.as_u16(), &body);
            return serde_json::from_str(&body).map_err(|e| {
                AppError::UpstreamError(status, format!("Failed to parse Tidal response: {}", e))
            });
        }
        Err(last_error)
    }

    async fn catalog_static_get(
        &self,
        url: &str,
        params: Vec<(&str, &str)>,
    ) -> Result<Value, AppError> {
        crate::request_log::note_account("catalog-token", "CATALOG_TOKEN", "catalog");
        let remaining = self
            .catalog_rate_limited_until
            .load(Ordering::Relaxed)
            .saturating_sub(Utc::now().timestamp());
        if remaining > 0 {
            return Err(AppError::RateLimited(remaining as u64));
        }
        let http = self.working_client().await?;
        let mut req = http
            .get(url)
            .header(
                "authorization",
                format!("Bearer {}", self.config.catalog_token),
            )
            .header("User-Agent", self.config.user_agent.as_str())
            .header("Accept", "*/*")
            .header("Accept-Encoding", "gzip")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("X-Platform", "android")
            .header("X-Tidal-Platform", "android");
        if !params.is_empty() {
            req = req.query(&params);
        }
        let resp = match req.send().await {
            Ok(resp) => {
                self.proxy_manager.note_success();
                resp
            }
            Err(e) => {
                if e.is_connect() || e.is_timeout() {
                    self.proxy_manager.note_failure();
                }
                return Err(e.into());
            }
        };
        let status = resp.status();
        if status.as_u16() == 429 {
            let seconds = AccountManager::retry_after_secs(
                resp.headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok()),
            );
            self.catalog_rate_limited_until.fetch_max(
                Utc::now()
                    .timestamp()
                    .saturating_add(i64::try_from(seconds).unwrap_or(i64::MAX)),
                Ordering::Relaxed,
            );
            return Err(AppError::RateLimited(seconds));
        }
        if !status.is_success() {
            return Err(AppError::UpstreamError(
                status,
                "Catalog token request failed".into(),
            ));
        }
        let body = resp.text().await?;
        self.dev_log("GET", url, status.as_u16(), &body);
        serde_json::from_str(&body).map_err(|e| {
            AppError::UpstreamError(status, format!("Failed to parse Tidal response: {}", e))
        })
    }

    async fn catalog_account_get(
        &self,
        account: &Arc<AccountState>,
        url: &str,
        params: Vec<(&str, &str)>,
    ) -> Result<Value, AppError> {
        if let Some(seconds) = AccountManager::rate_limit_remaining(account) {
            return Err(AppError::RateLimited(seconds));
        }
        // An endpoint quota is not a rate limit: let catalog_get fall back to
        // the pool, where accounts with quota left can still answer.
        if let Some(seconds) = AccountManager::quota_scope(url)
            .and_then(|s| account.quota_remaining(s, Utc::now().timestamp()))
        {
            return Err(AppError::ServiceUnavailableRetry(
                "Catalog account quota for this endpoint is used up".into(),
                seconds,
            ));
        }
        let mut http = self.working_client_for(&account.id).await?;
        let mut token = self.token_manager.get_token(account, &http).await?;
        let requests_per_account = self.settings.catalog_requests_per_account().max(1);
        for attempt in 0..requests_per_account {
            if let Some(seconds) = AccountManager::rate_limit_remaining(account) {
                return Err(AppError::RateLimited(seconds));
            }
            http = self.working_client_for(&account.id).await?;
            let mut req = http
                .get(url)
                .header("authorization", format!("Bearer {}", token))
                .header("X-Tidal-Token", account.client_id.as_str())
                .header("User-Agent", self.config.user_agent.as_str())
                .header("Accept", "*/*")
                .header("Accept-Encoding", "gzip")
                .header("Accept-Language", "en-US,en;q=0.9")
                .header("X-Platform", "android")
                .header("X-Tidal-Platform", "android");
            if !params.is_empty() {
                req = req.query(&params);
            }
            let resp = match req.send().await {
                Ok(resp) => {
                    self.proxy_manager.note_success_for(&account.id).await;
                    resp
                }
                Err(e) => {
                    if e.is_connect() || e.is_timeout() {
                        self.proxy_manager.note_failure_for(&account.id).await;
                    }
                    return Err(e.into());
                }
            };
            let status = resp.status();
            if status.as_u16() == 429 {
                let (seconds, quota) = AccountManager::note_429(
                    account,
                    url,
                    resp.headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok()),
                ).await;
                if quota {
                    return Err(AppError::ServiceUnavailableRetry(
                        "Catalog account quota for this endpoint is used up".into(),
                        seconds,
                    ));
                }
                self.account_manager
                    .mark_account_error(&account.id, "Tidal HTTP 429")
                    .await;
                return Err(AppError::RateLimited(seconds));
            }
            if status.as_u16() == 401 && attempt + 1 < requests_per_account {
                token = self.token_manager.refresh_after_unauthorized(account, &http, &token).await?;
                continue;
            }
            if status.as_u16() == 401 && attempt > 0 {
                TokenManager::reject_refreshed_token(account, &token).await;
            }
            if !status.is_success() {
                if attempt + 1 < requests_per_account
                    && (status.as_u16() == 403 || status.as_u16() >= 500)
                {
                    continue;
                }
                return Err(AppError::UpstreamError(
                    status,
                    "Catalog account request failed".into(),
                ));
            }
            let body = resp.text().await?;
            self.dev_log("GET", url, status.as_u16(), &body);
            return serde_json::from_str(&body).map_err(|e| {
                AppError::UpstreamError(status, format!("Failed to parse Tidal response: {}", e))
            });
        }
        Err(AppError::Unauthorized(
            "Catalog account unauthorized".into(),
        ))
    }
}

const PROBE_TRACK_IDS: &[i64] = &[427520487, 39249713, 58990511, 144371283];
const PROBE_REQ_SECS: u64 = 20;

impl TidalClient {
    fn playback_presentation(body: &Value) -> Option<&str> {
        body.get("assetPresentation").and_then(Value::as_str)
    }

    /// FULL/PREVIEW diagnostic used by both the admin action and maintenance loop.
    /// It never changes the account's availability.
    /// Manual, read-only lookup of the account's Tidal subscription.
    pub async fn account_subscription(&self, account: &Arc<AccountState>) -> Result<Value, AppError> {
        self.account_subscription_at(account, "https://api.tidal.com/v1").await
    }

    async fn account_subscription_at(
        &self,
        account: &Arc<AccountState>,
        api_base: &str,
    ) -> Result<Value, AppError> {
        let client = self.working_client_for(&account.id).await?;
        let mut token = self.token_manager.get_token(account, &client).await?;
        let known_user = account
            .user_id
            .read()
            .await
            .clone()
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()));
        let (user_id, country) = match known_user.zip(account.country()) {
            Some((id, country)) => (id, country),
            None => {
                let session = self
                    .subscription_get(account, &client, &mut token, &format!("{api_base}/sessions"), &[])
                    .await?;
                let id = match &session["userId"] {
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.clone(),
                    _ => return Err(AppError::UpstreamError(
                        reqwest::StatusCode::BAD_GATEWAY,
                        "Tidal session has no user id".into(),
                    )),
                };
                account.set_country(session["countryCode"].as_str().map(str::to_string));
                if let Err(error) = account.persist_refresh_state().await {
                    tracing::warn!(account = %account.id, "Could not save account country: {error}");
                }
                let country = account.country().unwrap_or_else(|| self.config.country_code.clone());
                (id, country)
            }
        };
        let url = format!("{api_base}/users/{user_id}/subscription");
        let body = self
            .subscription_get(account, &client, &mut token, &url, &[("countryCode", country.as_str())])
            .await?;
        Ok(json!({ "user_id": user_id, "subscription": body }))
    }

    /// One GET for the subscription lookup: a 401 refreshes the token once,
    /// a 429 pauses the account like any other request would.
    async fn subscription_get(
        &self,
        account: &Arc<AccountState>,
        client: &Client,
        token: &mut String,
        url: &str,
        query: &[(&str, &str)],
    ) -> Result<Value, AppError> {
        let mut retried_unauthorized = false;
        loop {
            if let Some(seconds) = AccountManager::rate_limit_remaining(account) {
                return Err(AppError::RateLimited(seconds));
            }
            let request = client
                .get(url)
                .query(query)
                .header("authorization", format!("Bearer {}", token))
                .header("X-Tidal-Token", account.client_id.as_str())
                .header("User-Agent", self.config.user_agent.as_str());
            let response = match tokio::time::timeout(Duration::from_secs(PROBE_REQ_SECS), request.send()).await {
                Ok(Ok(response)) => {
                    self.proxy_manager.note_success_for(&account.id).await;
                    response
                }
                Ok(Err(error)) => {
                    if error.is_connect() || error.is_timeout() {
                        self.proxy_manager.note_failure_for(&account.id).await;
                    }
                    return Err(AppError::BadGateway(format!("Network error reaching Tidal: {error}")));
                }
                Err(_) => {
                    self.proxy_manager.note_failure_for(&account.id).await;
                    return Err(AppError::Timeout);
                }
            };
            let status = response.status().as_u16();
            match status {
                200 => {
                    return response.json::<Value>().await.map_err(|_| {
                        AppError::UpstreamError(
                            reqwest::StatusCode::BAD_GATEWAY,
                            "Invalid subscription response from Tidal".into(),
                        )
                    });
                }
                401 if !retried_unauthorized => {
                    retried_unauthorized = true;
                    *token = self
                        .token_manager
                        .refresh_after_unauthorized(account, client, token)
                        .await?;
                }
                429 => {
                    let (seconds, _) = AccountManager::note_429(
                        account,
                        url,
                        response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|v| v.to_str().ok()),
                    )
                    .await;
                    self.account_manager
                        .mark_account_error(&account.id, "Tidal subscription check HTTP 429")
                        .await;
                    return Err(AppError::RateLimited(seconds));
                }
                _ => {
                    if status == 401 {
                        TokenManager::reject_refreshed_token(account, token).await;
                    }
                    let text = response.text().await.unwrap_or_default();
                    let detail: String = text.chars().take(200).collect();
                    // Never pass Tidal's 401/403 through: the panel reads those as its own session.
                    return Err(AppError::UpstreamError(
                        reqwest::StatusCode::BAD_GATEWAY,
                        format!("Tidal HTTP {status}: {detail}"),
                    ));
                }
            }
        }
    }

    pub async fn probe_account_premium(&self, account: &Arc<AccountState>) -> (String, String) {
        self.probe_account_premium_at(account, "https://api.tidal.com/v1")
            .await
    }

    async fn probe_account_premium_at(
        &self,
        account: &Arc<AccountState>,
        api_base: &str,
    ) -> (String, String) {
        if AccountManager::rate_limit_remaining(account).is_some() {
            return ("unknown".into(), "Account is rate limited".into());
        }
        let client = match self.working_client_for(&account.id).await {
            Ok(client) => client,
            Err(_) => {
                return (
                    "unknown".into(),
                    "No working egress for this account".into(),
                );
            }
        };
        let mut token = match self.token_manager.get_token(account, &client).await {
            Ok(token) => token,
            Err(_) => return ("error".into(), "Token unavailable".into()),
        };
        let mut previews = 0;
        for track_id in PROBE_TRACK_IDS {
            let mut retried_unauthorized = false;
            loop {
                if AccountManager::rate_limit_remaining(account).is_some() {
                    return ("unknown".into(), "Account is rate limited".into());
                }
                let client = match self.working_client_for(&account.id).await {
                    Ok(client) => client,
                    Err(_) => {
                        return (
                            "unknown".into(),
                            "No working egress for this account".into(),
                        );
                    }
                };
                let url = self.v1_playback_url_at(api_base, *track_id);
                let request = client
                    .get(&url)
                    .query(&[
                        ("audioquality", "HI_RES_LOSSLESS"),
                        ("playbackmode", "STREAM"),
                        ("assetpresentation", "FULL"),
                    ])
                    .header("authorization", format!("Bearer {}", token))
                    .header("X-Tidal-Token", account.client_id.as_str())
                    .header("User-Agent", self.config.user_agent.as_str());
                let response = match tokio::time::timeout(
                    Duration::from_secs(PROBE_REQ_SECS),
                    request.send(),
                )
                .await
                {
                    Ok(Ok(response)) => {
                        self.proxy_manager.note_success_for(&account.id).await;
                        response
                    }
                    Ok(Err(error)) => {
                        if error.is_connect() || error.is_timeout() {
                            self.proxy_manager.note_failure_for(&account.id).await;
                        }
                        return ("unknown".into(), "Network error reaching Tidal".into());
                    }
                    Err(_) => {
                        self.proxy_manager.note_failure_for(&account.id).await;
                        return ("unknown".into(), "Network error reaching Tidal".into());
                    }
                };
                match response.status().as_u16() {
                    200 => {
                        let body = match response.json::<Value>().await {
                            Ok(body) => body,
                            Err(_) => {
                                return ("unknown".into(), "Invalid playback response".into());
                            }
                        };
                        match Self::playback_presentation(&body) {
                            Some("FULL") => return ("premium".into(), String::new()),
                            Some("PREVIEW") => {
                                previews += 1;
                                break;
                            }
                            _ => {
                                return (
                                    "unknown".into(),
                                    "Playback response has no clear presentation".into(),
                                );
                            }
                        }
                    }
                    401 if !retried_unauthorized => {
                        retried_unauthorized = true;
                        match self
                            .token_manager
                            .refresh_after_unauthorized(account, &client, &token)
                            .await
                        {
                            Ok(fresh) => {
                                token = fresh;
                                continue;
                            }
                            Err(_) => {
                                return (
                                    "unknown".into(),
                                    "Tidal returned 401 and token refresh failed".into(),
                                );
                            }
                        }
                    }
                    401 => {
                        TokenManager::reject_refreshed_token(account, &token).await;
                        return (
                            "unknown".into(),
                            "Tidal returned 401 after token refresh".into(),
                        );
                    }
                    403 => {
                        return (
                            "unknown".into(),
                            "Tidal returned 403; account may be restricted".into(),
                        );
                    }
                    429 => {
                        AccountManager::note_429(
                            account,
                            &url,
                            response
                                .headers()
                                .get(reqwest::header::RETRY_AFTER)
                                .and_then(|v| v.to_str().ok()),
                        ).await;
                        self.account_manager
                            .mark_account_error(&account.id, "Tidal probe HTTP 429")
                            .await;
                        return ("unknown".into(), "Tidal throttled the probe".into());
                    }
                    status if status >= 500 => {
                        return ("unknown".into(), format!("Tidal HTTP {}", status));
                    }
                    status => {
                        return ("unknown".into(), format!("Tidal HTTP {}", status));
                    }
                }
            }
        }
        if previews == PROBE_TRACK_IDS.len() {
            (
                "preview-only".into(),
                "All probe tracks returned PREVIEW".into(),
            )
        } else {
            (
                "unknown".into(),
                "Probe did not get a conclusive response".into(),
            )
        }
    }
}

#[cfg(test)]
mod premium_tests {
    use super::TidalClient;
    use serde_json::json;

    #[test]
    fn presentation_requires_explicit_value() {
        assert_eq!(
            TidalClient::playback_presentation(&json!({"assetPresentation":"FULL"})),
            Some("FULL")
        );
        assert_eq!(
            TidalClient::playback_presentation(&json!({"assetPresentation":"PREVIEW"})),
            Some("PREVIEW")
        );
        assert_eq!(TidalClient::playback_presentation(&json!({})), None);
    }
}

#[cfg(test)]
mod rate_limit_tests {
    use super::TidalClient;
    use crate::account_manager::{AccountManager, SwitchingWeights};
    use crate::config::Config;
    use crate::error::AppError;
    use crate::notifier::Notifier;
    use crate::proxy_manager::ProxyManager;
    use crate::settings::AppSettings;
    use crate::token_manager::TokenManager;
    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::get;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn client_with_accounts(count: usize) -> (TidalClient, Arc<AccountManager>) {
        let mut config = Config::from_env();
        config.use_proxies = false;
        config.catalog_token.clear();
        let config = Arc::new(config);
        let manager = Arc::new(AccountManager::new(None, SwitchingWeights::default()));
        for i in 0..count {
            let name = format!("account-{i}");
            let account = manager
                .add_account(name.clone(), "client".into(), "secret".into(), name, None)
                .await
                .unwrap();
            *account.access_token.write().await = Some("test-token".into());
            account.token_expires_at.store(
                chrono::Utc::now().timestamp() + 3600,
                Ordering::Relaxed,
            );
        }
        let settings = Arc::new(AppSettings::from_env());
        // Keep the existing auth-refresh coverage explicit: it needs a
        // second upstream request on the same account.
        settings
            .apply(&serde_json::json!({
                "track_requests_per_account": 2,
                "catalog_requests_per_account": 2
            }))
            .unwrap();
        let client = TidalClient::new(
            Arc::new(ProxyManager::new(config.clone(), None)),
            Arc::new(TokenManager::new(None)),
            manager.clone(),
            Notifier::new(settings.clone()),
            config,
            settings,
        );
        (client, manager)
    }

    #[tokio::test]
    async fn api_401_refreshes_when_two_requests_are_allowed() {
        use serde_json::json;
        use axum::{Json, http::HeaderMap, routing::post};
        let api_hits = Arc::new(AtomicUsize::new(0));
        let auth_hits = Arc::new(AtomicUsize::new(0));
        let calls = api_hits.clone();
        let refreshes = auth_hits.clone();
        let app = Router::new()
            .route("/token", post(move || {
                let refreshes = refreshes.clone();
                async move {
                    refreshes.fetch_add(1, Ordering::Relaxed);
                    Json(json!({"access_token":"fresh", "expires_in":3600}))
                }
            }))
            .route("/track", get(move |headers: HeaderMap| {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::Relaxed);
                    assert_eq!(headers["x-tidal-token"], "client");
                    if headers["authorization"] == "Bearer fresh" {
                        (StatusCode::OK, Json(json!({"title":"ok"})))
                    } else {
                        (StatusCode::UNAUTHORIZED, Json(json!({})))
                    }
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (mut client, manager) = client_with_accounts(1).await;
        let mut tm = TokenManager::new(None);
        tm.token_url = format!("{base}/token");
        tm.set_account_manager(manager);
        tm.set_proxy_manager(client.proxy_manager.clone());
        client.token_manager = Arc::new(tm);
        let result = client.make_request(&format!("{base}/track"), None).await;
        server.abort();
        assert_eq!(result.unwrap()["data"]["title"], "ok");
        assert_eq!(api_hits.load(Ordering::Relaxed), 2);
        assert_eq!(auth_hits.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn full_probe_refreshes_once_after_401() {
        use axum::{Json, http::HeaderMap, routing::post};
        use serde_json::json;

        let api_hits = Arc::new(AtomicUsize::new(0));
        let auth_hits = Arc::new(AtomicUsize::new(0));
        let calls = api_hits.clone();
        let refreshes = auth_hits.clone();
        let app = Router::new()
            .route(
                "/token",
                post(move || {
                    let refreshes = refreshes.clone();
                    async move {
                        refreshes.fetch_add(1, Ordering::Relaxed);
                        Json(json!({"access_token":"fresh", "expires_in":3600}))
                    }
                }),
            )
            .route(
                "/tracks/{id}/playbackinfopostpaywall",
                get(move |headers: HeaderMap| {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::Relaxed);
                        assert_eq!(headers["x-tidal-token"], "client");
                        if headers["authorization"] == "Bearer fresh" {
                            (StatusCode::OK, Json(json!({"assetPresentation":"FULL"})))
                        } else {
                            (StatusCode::UNAUTHORIZED, Json(json!({})))
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (mut client, manager) = client_with_accounts(1).await;
        let account = manager.list_accounts().await.remove(0);
        let mut tm = TokenManager::new(None);
        tm.token_url = format!("{base}/token");
        tm.set_account_manager(manager);
        tm.set_proxy_manager(client.proxy_manager.clone());
        client.token_manager = Arc::new(tm);

        let (status, reason) = client.probe_account_premium_at(&account, &base).await;
        server.abort();
        assert_eq!(status, "premium");
        assert!(reason.is_empty());
        assert_eq!(api_hits.load(Ordering::Relaxed), 2);
        assert_eq!(auth_hits.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn full_probe_uses_the_selected_v1_endpoint() {
        use axum::{Json, extract::{Path, Query}, http::HeaderMap};
        use serde_json::json;
        use std::collections::HashMap;
        let endpoints = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = endpoints.clone();
        let app = Router::new().route("/tracks/{id}/{endpoint}", get(
            move |Path((id, endpoint)): Path<(i64, String)>, Query(params): Query<HashMap<String, String>>, headers: HeaderMap| {
                let observed = observed.clone();
                async move {
                    assert!(super::PROBE_TRACK_IDS.contains(&id));
                    assert_eq!(params["assetpresentation"], "FULL");
                    assert_eq!(params["audioquality"], "HI_RES_LOSSLESS");
                    assert_eq!(headers["authorization"], "Bearer test-token");
                    assert_eq!(headers["x-tidal-token"], "client");
                    observed.lock().unwrap().push(endpoint);
                    Json(json!({"assetPresentation": "FULL"}))
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (client, manager) = client_with_accounts(1).await;
        let account = manager.list_accounts().await.remove(0);
        for endpoint in ["playbackinfopostpaywall", "playbackinfo"] {
            client.settings.apply(&json!({"v1_playback_endpoint": endpoint})).unwrap();
            assert_eq!(client.v1_playback_url(42), format!("https://api.tidal.com/v1/tracks/42/{endpoint}"));
            let (status, _) = client.probe_account_premium_at(&account, &base).await;
            assert_eq!(status, "premium");
        }
        server.abort();
        assert_eq!(*endpoints.lock().unwrap(), ["playbackinfopostpaywall", "playbackinfo"]);
    }

    #[tokio::test]
    async fn subscription_lookup_finds_the_user_and_refreshes_once() {
        use axum::extract::{Path, Query};
        use axum::{Json, http::HeaderMap, routing::post};
        use serde_json::json;
        use std::collections::HashMap;
        let auth_hits = Arc::new(AtomicUsize::new(0));
        let refreshes = auth_hits.clone();
        let app = Router::new()
            .route("/token", post(move || {
                let refreshes = refreshes.clone();
                async move {
                    refreshes.fetch_add(1, Ordering::Relaxed);
                    Json(json!({"access_token":"fresh", "expires_in":3600}))
                }
            }))
            .route("/sessions", get(|headers: HeaderMap| async move {
                if headers["authorization"] == "Bearer fresh" {
                    (StatusCode::OK, Json(json!({"userId": 208921067, "countryCode": "DE"})))
                } else {
                    (StatusCode::UNAUTHORIZED, Json(json!({})))
                }
            }))
            .route("/users/{id}/subscription", get(
                |Path(id): Path<String>, Query(query): Query<HashMap<String, String>>| async move {
                    assert_eq!(id, "208921067");
                    assert_eq!(query["countryCode"], "DE");
                    Json(json!({"subscription": {"type": "HIFI"}, "status": "ACTIVE", "validUntil": "2026-11-12T10:00:00.000+0000"}))
                },
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (mut client, manager) = client_with_accounts(1).await;
        let account = manager.list_accounts().await.remove(0);
        let mut tm = TokenManager::new(None);
        tm.token_url = format!("{base}/token");
        tm.set_account_manager(manager);
        tm.set_proxy_manager(client.proxy_manager.clone());
        client.token_manager = Arc::new(tm);

        let result = client.account_subscription_at(&account, &base).await.unwrap();
        server.abort();
        assert_eq!(result["user_id"], "208921067");
        assert_eq!(result["subscription"]["subscription"]["type"], "HIFI");
        assert_eq!(result["subscription"]["status"], "ACTIVE");
        assert_eq!(account.country().as_deref(), Some("DE"));
        assert_eq!(auth_hits.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn previews_count_against_an_account_only_when_another_gets_full() {
        use axum::{Json, http::HeaderMap};
        use serde_json::json;
        let app = Router::new().route(
            "/tracks/{id}/playbackinfo",
            get(|headers: HeaderMap| async move {
                let presentation = if headers["authorization"] == "Bearer tok-preview" { "PREVIEW" } else { "FULL" };
                Json(json!({"assetPresentation": presentation}))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/tracks/1/playbackinfo", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (client, manager) = client_with_accounts(2).await;
        let accounts = manager.list_accounts().await;
        let (preview, full) = (&accounts[0], &accounts[1]);
        *preview.access_token.write().await = Some("tok-preview".into());
        let limit = crate::account_manager::TRAFFIC_PREVIEWS_BEFORE_PREVIEW_ONLY;
        for round in 1..=limit {
            // Make the PREVIEW account the first pick each time.
            preview.last_pick_ms.store(1, Ordering::Relaxed);
            full.last_pick_ms.store(chrono::Utc::now().timestamp_millis(), Ordering::Relaxed);
            full.premium_checked_at.store(0, Ordering::Relaxed);
            let data = client.make_request(&url, None).await.unwrap();
            assert_eq!(data["data"]["assetPresentation"], "FULL");
            let marked = *preview.premium_status.read().await == "preview-only";
            assert_eq!(marked, round == limit, "round {round}");
        }
        // The FULL account was confirmed by the same traffic.
        assert_eq!(*full.premium_status.read().await, "premium");
        server.abort();
    }

    #[tokio::test]
    async fn previews_from_every_account_say_nothing_about_any_of_them() {
        use axum::Json;
        use serde_json::json;
        let app = Router::new().route(
            "/tracks/{id}/playbackinfo",
            get(|| async { Json(json!({"assetPresentation": "PREVIEW"})) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/tracks/1/playbackinfo", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (client, manager) = client_with_accounts(2).await;
        for _ in 0..crate::account_manager::TRAFFIC_PREVIEWS_BEFORE_PREVIEW_ONLY + 2 {
            assert!(client.make_request(&url, None).await.is_err());
        }
        for account in manager.list_accounts().await {
            assert_eq!(account.traffic_preview_streak.load(Ordering::Relaxed), 0);
            assert_ne!(*account.premium_status.read().await, "preview-only");
        }
        server.abort();
    }

    #[tokio::test]
    async fn resource_404_does_not_refresh_credentials() {
        let app = Router::new().route("/missing", get(|| async { StatusCode::NOT_FOUND }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/missing", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (client, _) = client_with_accounts(1).await;
        let result = client.make_request(&url, None).await;
        server.abort();
        assert!(matches!(result, Err(AppError::NotFound(_))));
    }

    #[tokio::test]
    async fn search_quota_skips_only_search_and_moves_to_the_next_account() {
        use axum::http::HeaderMap;
        let search_hits = Arc::new(AtomicUsize::new(0));
        let calls = search_hits.clone();
        let midnight = (chrono::Utc::now() + chrono::Duration::hours(5)).to_rfc2822();
        let app = Router::new()
            .route(
                "/v1/search/tracks",
                get(move || {
                    let calls = calls.clone();
                    let midnight = midnight.clone();
                    async move {
                        // The first account's daily search quota is gone.
                        if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                            let mut headers = HeaderMap::new();
                            headers.insert("retry-after", midnight.parse().unwrap());
                            (StatusCode::TOO_MANY_REQUESTS, headers, "{}")
                        } else {
                            (StatusCode::OK, HeaderMap::new(), "{\"items\":[]}")
                        }
                    }
                }),
            )
            .route("/v1/tracks/1", get(|| async { (StatusCode::OK, "{\"title\":\"ok\"}") }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (client, manager) = client_with_accounts(2).await;

        let search = format!("{base}/v1/search/tracks");
        assert!(client.make_request(&search, None).await.is_ok());
        assert_eq!(search_hits.load(Ordering::Relaxed), 2);
        let accounts = manager.list_accounts().await;
        let now = chrono::Utc::now().timestamp();
        let exhausted: Vec<_> = accounts
            .iter()
            .filter(|a| a.quota_remaining("search", now).is_some())
            .collect();
        assert_eq!(exhausted.len(), 1);
        // No account is paused: tracks keep using both.
        assert!(accounts.iter().all(|a| AccountManager::rate_limit_remaining(a).is_none()));
        // The exhausted account is skipped for search without asking Tidal.
        for _ in 0..3 {
            assert!(client.make_request(&search, None).await.is_ok());
        }
        assert_eq!(search_hits.load(Ordering::Relaxed), 5);
        assert!(client.make_request(&format!("{base}/v1/tracks/1"), None).await.is_ok());
        server.abort();
    }

    #[tokio::test]
    async fn search_goes_without_login_and_falls_back_to_accounts() {
        use axum::http::HeaderMap;
        // (authorization present, x-tidal-token) per call.
        let seen = Arc::new(std::sync::Mutex::new(Vec::<(bool, String)>::new()));
        let anon_429 = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (calls, limited) = (seen.clone(), anon_429.clone());
        let track_seen = Arc::new(std::sync::Mutex::new(Vec::<(bool, String)>::new()));
        let track_calls = track_seen.clone();
        let app = Router::new()
            .route(
                "/v1/search/tracks",
                get(move |headers: HeaderMap| {
                    let calls = calls.clone();
                    let limited = limited.clone();
                    async move {
                        let authed = headers.contains_key("authorization");
                        let token = headers["x-tidal-token"].to_str().unwrap().to_string();
                        calls.lock().unwrap().push((authed, token));
                        if !authed && limited.load(Ordering::Relaxed) {
                            return (StatusCode::TOO_MANY_REQUESTS, "{}");
                        }
                        (StatusCode::OK, "{\"items\":[]}")
                    }
                }),
            )
            .route(
                "/v1/tracks/1",
                get(move |headers: HeaderMap| {
                    let calls = track_calls.clone();
                    async move {
                        let token = headers["x-tidal-token"].to_str().unwrap().to_string();
                        calls.lock().unwrap().push((headers.contains_key("authorization"), token));
                        (StatusCode::OK, "{\"title\":\"ok\"}")
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (client, _manager) = client_with_accounts(3).await;
        let search = format!("{base}/v1/search/tracks");
        let anon = (false, crate::setup::REQUEST_CLIENT_ID.to_string());

        // Search: no account token at all.
        assert!(client.make_catalog_request(&search, None).await.is_ok());
        assert_eq!(seen.lock().unwrap().drain(..).collect::<Vec<_>>(), vec![anon.clone()]);

        // Other metadata keeps using the accounts.
        assert!(client.make_catalog_request(&format!("{base}/v1/tracks/1"), None).await.is_ok());
        assert_eq!(track_seen.lock().unwrap().clone(), vec![(true, "client".to_string())]);

        // A 429 without login tries one more egress, then the accounts answer.
        anon_429.store(true, Ordering::Relaxed);
        assert!(client.make_catalog_request(&search, None).await.is_ok());
        assert_eq!(
            seen.lock().unwrap().clone(),
            vec![anon.clone(), anon, (true, "client".to_string())]
        );
        server.abort();
    }

    #[tokio::test]
    async fn track_request_limit_controls_same_account_retries() {
        let hits = Arc::new(AtomicUsize::new(0));
        let calls = hits.clone();
        let app = Router::new().route(
            "/track",
            get(move || {
                let calls = calls.clone();
                async move {
                    if calls.fetch_add(1, Ordering::Relaxed) < 2 {
                        (StatusCode::BAD_GATEWAY, "{}")
                    } else {
                        (StatusCode::OK, "{\"title\":\"ok\"}")
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let url = format!("http://{}/track", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (client, _) = client_with_accounts(1).await;
        client
            .settings
            .apply(&serde_json::json!({"track_requests_per_account": 3}))
            .unwrap();

        let result = client.make_request(&url, None).await;
        server.abort();
        assert_eq!(result.unwrap()["data"]["title"], "ok");
        assert_eq!(hits.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn catalog_request_limit_controls_same_account_retries() {
        let hits = Arc::new(AtomicUsize::new(0));
        let calls = hits.clone();
        let app = Router::new().route(
            "/catalog",
            get(move || {
                let calls = calls.clone();
                async move {
                    if calls.fetch_add(1, Ordering::Relaxed) < 2 {
                        (StatusCode::BAD_GATEWAY, "{}")
                    } else {
                        (StatusCode::OK, "{\"title\":\"ok\"}")
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let url = format!("http://{}/catalog", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (client, manager) = client_with_accounts(1).await;
        let account = manager.list_accounts().await.remove(0);
        manager.set_account_catalog(&account.id, true).await.unwrap();
        client
            .settings
            .apply(&serde_json::json!({"catalog_requests_per_account": 3}))
            .unwrap();

        let result = client.catalog_get(&url, vec![]).await;
        server.abort();
        assert_eq!(result.unwrap()["title"], "ok");
        assert_eq!(hits.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn third_429_stops_before_fourth_account() {
        let hits = Arc::new(AtomicUsize::new(0));
        let calls = hits.clone();
        let app = Router::new().route(
            "/track",
            get(move || {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::Relaxed);
                    (StatusCode::TOO_MANY_REQUESTS, [("Retry-After", "120")])
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let url = format!("http://{}/track", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let (client, manager) = client_with_accounts(4).await;

        let result = client.make_request(&url, None).await;
        server.abort();
        assert!(matches!(result, Err(AppError::RateLimited(1..=120))));
        assert_eq!(hits.load(Ordering::Relaxed), 3);
        assert_eq!(
            manager
                .list_accounts()
                .await
                .iter()
                .filter(|a| AccountManager::rate_limit_remaining(a).is_some())
                .count(),
            3,
        );
    }

    #[tokio::test]
    async fn first_429_falls_back_and_returns_second_account_success() {
        let hits = Arc::new(AtomicUsize::new(0));
        let calls = hits.clone();
        let app = Router::new().route(
            "/track",
            get(move || {
                let calls = calls.clone();
                async move {
                    if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                        (StatusCode::TOO_MANY_REQUESTS, [("Retry-After", "120")], "")
                    } else {
                        (StatusCode::OK, [("Content-Type", "application/json")], "{\"assetPresentation\":\"FULL\"}")
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let url = format!("http://{}/track", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (client, manager) = client_with_accounts(2).await;

        let result = client.make_request(&url, None).await;
        server.abort();
        assert_eq!(result.unwrap()["data"]["assetPresentation"], "FULL");
        assert_eq!(hits.load(Ordering::Relaxed), 2);
        assert_eq!(
            manager
                .list_accounts()
                .await
                .iter()
                .filter(|a| AccountManager::rate_limit_remaining(a).is_some())
                .count(),
            1,
        );
    }

    #[tokio::test]
    async fn catalog_429_uses_second_catalog_account() {
        let hits = Arc::new(AtomicUsize::new(0));
        let calls = hits.clone();
        let app = Router::new().route(
            "/catalog",
            get(move || {
                let calls = calls.clone();
                async move {
                    if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                        (StatusCode::TOO_MANY_REQUESTS, [("Retry-After", "120")], "")
                    } else {
                        (StatusCode::OK, [("Content-Type", "application/json")], "{\"title\":\"album\"}")
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let url = format!("http://{}/catalog", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (client, manager) = client_with_accounts(2).await;
        for account in manager.list_accounts().await {
            manager.set_account_catalog(&account.id, true).await.unwrap();
        }

        let result = client.catalog_get(&url, vec![]).await;
        server.abort();
        assert_eq!(result.unwrap()["title"], "album");
        assert_eq!(hits.load(Ordering::Relaxed), 2);
    }
}
