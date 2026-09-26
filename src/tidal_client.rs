use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use chrono::Utc;
use rand::Rng;
use reqwest::Client;
use serde_json::{Value, json};

use crate::account_manager::{AccountManager, AccountState};
use crate::config::Config;
use crate::error::AppError;
use crate::notifier::Notifier;
use crate::proxy_manager::ProxyManager;
use crate::token_manager::TokenManager;

pub struct TidalClient {
    proxy_manager: Arc<ProxyManager>,
    token_manager: Arc<TokenManager>,
    account_manager: Arc<AccountManager>,
    notifier: Arc<Notifier>,
    config: Arc<Config>,
    catalog_rate_limited_until: AtomicI64,
}

/// A 429 may use one other account. A second 429 is returned to the caller
/// instead of walking the whole pool.
fn schedule_rate_limit_fallback(
    account_try: usize,
    account_id: &str,
    error: AppError,
    first_rate_limit_try: &mut Option<usize>,
    failed_ids: &mut Vec<String>,
    last_error: &mut Option<AppError>,
) -> Result<(), AppError> {
    if first_rate_limit_try.is_some() {
        return Err(error);
    }
    *first_rate_limit_try = Some(account_try);
    failed_ids.push(account_id.to_string());
    *last_error = Some(error);
    Ok(())
}

impl TidalClient {
    pub fn new(
        proxy_manager: Arc<ProxyManager>,
        token_manager: Arc<TokenManager>,
        account_manager: Arc<AccountManager>,
        notifier: Arc<Notifier>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            proxy_manager,
            token_manager,
            account_manager,
            notifier,
            config,
            catalog_rate_limited_until: AtomicI64::new(0),
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
        self.make_request_with_limit(url, params, preferred_account, None)
            .await
    }

    async fn make_request_with_limit(
        &self,
        url: &str,
        params: Option<Vec<(&str, &str)>>,
        preferred_account: Option<Arc<AccountState>>,
        account_limit: Option<usize>,
    ) -> Result<Value, AppError> {
        let max_retries = if self.proxy_manager.proxies_enabled() {
            self.config.max_retries
        } else {
            1
        };

        let mut failed_ids: Vec<String> = Vec::new();
        let account_count = self.account_manager.playback_count().await;
        let max_account_attempts = std::cmp::max(1, account_count)
            .min(account_limit.unwrap_or(usize::MAX));
        let mut last_account_error: Option<AppError> = None;
        let mut first_rate_limit_try: Option<usize> = None;
        let mut preferred_cooldown_consumed = false;

        for account_try in 0..max_account_attempts {
            if preferred_cooldown_consumed && account_try > 0
                || first_rate_limit_try.is_some_and(|first| account_try > first + 1)
            {
                break;
            }
            let account = if account_try == 0 && failed_ids.is_empty() {
                match preferred_account.clone() {
                    Some(a) => {
                        if let Some(seconds) = AccountManager::rate_limit_remaining(&a) {
                            failed_ids.push(a.id.clone());
                            first_rate_limit_try = Some(account_try);
                            preferred_cooldown_consumed = true;
                            match self.account_manager.select_account_excluding(&failed_ids).await {
                                Ok(alternative) => alternative,
                                Err(_) => return Err(AppError::RateLimited(seconds)),
                            }
                        } else {
                            // Account the preferred pick like any selection so
                            // the balancer sees its true load.
                            AccountManager::note_selection(&a);
                            a
                        }
                    }
                    None => match self
                        .account_manager
                        .select_account_excluding(&failed_ids)
                        .await
                    {
                        Ok(a) => a,
                        Err(e) => {
                            self.alert_if_all_down(&e).await;
                            return Err(last_account_error.unwrap_or(e));
                        }
                    },
                }
            } else {
                match self
                    .account_manager
                    .select_account_excluding(&failed_ids)
                    .await
                {
                    Ok(a) => a,
                    Err(e) => {
                        self.alert_if_all_down(&e).await;
                        return Err(last_account_error.unwrap_or(e));
                    }
                }
            };

            let mut http = self.working_client_for(&account.id).await?;

            for attempt in 0..max_retries {
                let token = match self.token_manager.get_token(&account, &http).await {
                    Ok(t) => t,
                    Err(e @ AppError::RateLimited(_)) => {
                        schedule_rate_limit_fallback(
                            account_try,
                            &account.id,
                            e,
                            &mut first_rate_limit_try,
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
                    schedule_rate_limit_fallback(
                        account_try,
                        &account.id,
                        AppError::RateLimited(seconds),
                        &mut first_rate_limit_try,
                        &mut failed_ids,
                        &mut last_account_error,
                    )?;
                    break;
                }
                http = self.working_client_for(&account.id).await?;

                let mut req = http
                    .get(url)
                    .header("authorization", format!("Bearer {}", token))
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
                        if let Err(e @ AppError::RateLimited(_)) = self
                            .token_manager
                            .refresh_token(&account, &http)
                            .await
                        {
                            schedule_rate_limit_fallback(
                                account_try,
                                &account.id,
                                e,
                                &mut first_rate_limit_try,
                                &mut failed_ids,
                                &mut last_account_error,
                            )?;
                            break;
                        }
                        if attempt >= max_retries - 1 {
                            self.account_manager
                                .mark_account_error(&account.id, "Tidal 401 unauthorized")
                                .await;
                        }
                        continue;
                    }
                    404 => {
                        let fresh_token = match self.token_manager.refresh_token(&account, &http).await {
                            Ok(token) => token,
                            Err(e @ AppError::RateLimited(_)) => {
                                schedule_rate_limit_fallback(
                                    account_try,
                                    &account.id,
                                    e,
                                    &mut first_rate_limit_try,
                                    &mut failed_ids,
                                    &mut last_account_error,
                                )?;
                                break;
                            }
                            Err(e) => return Err(e),
                        };

                        let stored = account.access_token.read().await;
                        if let Some(ref stored_token) = *stored {
                            if stored_token != &fresh_token {
                                drop(stored);
                                http = self.working_client_for(&account.id).await?;
                                let mut req2 = http
                                    .get(url)
                                    .header("authorization", format!("Bearer {}", fresh_token))
                                    .header("User-Agent", self.config.user_agent.as_str())
                                    .header("Accept", "*/*")
                                    .header("Accept-Encoding", "gzip")
                                    .header("Accept-Language", "en-US,en;q=0.9")
                                    .header("X-Platform", "android")
                                    .header("X-Tidal-Platform", "android");
                                if let Some(ref p) = params {
                                    req2 = req2.query(&p);
                                }
                                let resp2 = match req2.send().await {
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
                                let status2 = resp2.status();
                                if status2.as_u16() == 429 {
                                    let seconds = AccountManager::pause_account(
                                        &account,
                                        resp2.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok()),
                                    );
                                    self.account_manager.mark_account_error(&account.id, "Tidal HTTP 429").await;
                                    schedule_rate_limit_fallback(
                                        account_try,
                                        &account.id,
                                        AppError::RateLimited(seconds),
                                        &mut first_rate_limit_try,
                                        &mut failed_ids,
                                        &mut last_account_error,
                                    )?;
                                    break;
                                }
                                if status2.is_success() {
                                    let body2 = resp2.text().await?;
                                    let data: Value =
                                        serde_json::from_str(&body2).map_err(|e| {
                                            AppError::UpstreamError(
                                                status2,
                                                format!(
                                                    "Failed to parse Tidal response: {} | body: {}",
                                                    e,
                                                    body2.chars().take(200).collect::<String>()
                                                ),
                                            )
                                        })?;
                                    return Ok(
                                        json!({"version": self.config.api_version, "data": data}),
                                    );
                                }
                            }
                        }

                        return Err(AppError::NotFound("Resource not found".into()));
                    }
                    429 => {
                        let seconds = AccountManager::pause_account(
                            &account,
                            resp.headers()
                                .get(reqwest::header::RETRY_AFTER)
                                .and_then(|v| v.to_str().ok()),
                        );
                        self.account_manager
                            .mark_account_error(&account.id, "Tidal HTTP 429")
                            .await;
                        tracing::warn!(account = %account.label, seconds, "Tidal rate limit: pausing account");
                        schedule_rate_limit_fallback(
                            account_try,
                            &account.id,
                            AppError::RateLimited(seconds),
                            &mut first_rate_limit_try,
                            &mut failed_ids,
                            &mut last_account_error,
                        )?;
                        break;
                    }
                    403 => {
                        if attempt < max_retries - 1 {
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
                            if attempt < max_retries - 1 && status.as_u16() >= 500 {
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
                if is_preview {
                    failed_ids.push(account.id.clone());
                    last_account_error = Some(AppError::ServiceUnavailable(format!(
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
        if let Some(seconds) = account
            .as_deref()
            .and_then(AccountManager::rate_limit_remaining)
        {
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
                Some(ref account) => AccountManager::pause_account(
                    account,
                    resp.headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok()),
                ),
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
    async fn catalog_get(&self, url: &str, params: Vec<(&str, &str)>) -> Result<Value, AppError> {
        let mut rate_limit_error: Option<AppError> = None;
        let mut fallback_tried = false;
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
            let Some(acc) = self.account_manager.next_active_catalog().await else {
                break;
            };
            if rate_limit_error.is_some() {
                fallback_tried = true;
            }
            match self.catalog_account_get(&acc, url, params.clone()).await {
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
                .make_request_with_limit(url, Some(params), None, Some(1))
                .await
            {
                Ok(wrapped) => Ok(wrapped.get("data").cloned().unwrap_or(Value::Null)),
                Err(AppError::ServiceUnavailable(_)) => Err(previous_429),
                Err(e) => Err(e),
            };
        }
        // No catalog configured (or it failed): normal pool request.
        self.make_request(url, Some(params))
            .await
            .map(|wrapped| wrapped.get("data").cloned().unwrap_or(Value::Null))
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
        let mut http = self.working_client_for(&account.id).await?;
        let mut token = self.token_manager.get_token(account, &http).await?;
        for attempt in 0..2 {
            if let Some(seconds) = AccountManager::rate_limit_remaining(account) {
                return Err(AppError::RateLimited(seconds));
            }
            http = self.working_client_for(&account.id).await?;
            let mut req = http
                .get(url)
                .header("authorization", format!("Bearer {}", token))
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
                let seconds = AccountManager::pause_account(
                    account,
                    resp.headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok()),
                );
                self.account_manager
                    .mark_account_error(&account.id, "Tidal HTTP 429")
                    .await;
                return Err(AppError::RateLimited(seconds));
            }
            if status.as_u16() == 401 && attempt == 0 {
                token = self.token_manager.refresh_token(account, &http).await?;
                continue;
            }
            if !status.is_success() {
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

    /// Manual diagnostic only. Never changes the account's availability.
    pub async fn probe_account_premium(&self, account: &Arc<AccountState>) -> (String, String) {
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
        let token = match self.token_manager.get_token(account, &client).await {
            Ok(token) => token,
            Err(_) => return ("error".into(), "Token unavailable".into()),
        };
        let mut previews = 0;
        for track_id in PROBE_TRACK_IDS {
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
            let url = format!("https://api.tidal.com/v1/tracks/{}/playbackinfo", track_id);
            let request = client
                .get(&url)
                .query(&[
                    ("audioquality", "HI_RES_LOSSLESS"),
                    ("playbackmode", "STREAM"),
                    ("assetpresentation", "FULL"),
                ])
                .header("authorization", format!("Bearer {}", token))
                .header("User-Agent", self.config.user_agent.as_str());
            let response =
                match tokio::time::timeout(Duration::from_secs(PROBE_REQ_SECS), request.send())
                    .await
                {
                    Ok(Ok(response)) => response,
                    _ => return ("unknown".into(), "Network error reaching Tidal".into()),
                };
            match response.status().as_u16() {
                200 => {
                    let body = match response.json::<Value>().await {
                        Ok(body) => body,
                        Err(_) => return ("unknown".into(), "Invalid playback response".into()),
                    };
                    match Self::playback_presentation(&body) {
                        Some("FULL") => return ("premium".into(), String::new()),
                        Some("PREVIEW") => previews += 1,
                        _ => {
                            return (
                                "unknown".into(),
                                "Playback response has no clear presentation".into(),
                            );
                        }
                    }
                }
                401 => {
                    return (
                        "unknown".into(),
                        "Tidal returned 401; token may need refresh".into(),
                    );
                }
                403 => {
                    return (
                        "unknown".into(),
                        "Tidal returned 403; account may be restricted".into(),
                    );
                }
                429 => {
                    AccountManager::pause_account(
                        account,
                        response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|v| v.to_str().ok()),
                    );
                    self.account_manager
                        .mark_account_error(&account.id, "Tidal probe HTTP 429")
                        .await;
                    return ("unknown".into(), "Tidal throttled the probe".into());
                }
                status if status >= 500 => {
                    return ("unknown".into(), format!("Tidal HTTP {}", status));
                }
                status => return ("unknown".into(), format!("Tidal HTTP {}", status)),
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
        let client = TidalClient::new(
            Arc::new(ProxyManager::new(config.clone(), None)),
            Arc::new(TokenManager::new(None)),
            manager.clone(),
            Notifier::new(Arc::new(AppSettings::from_env())),
            config,
        );
        (client, manager)
    }

    #[tokio::test]
    async fn second_429_stops_before_third_account() {
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

        let (client, manager) = client_with_accounts(3).await;

        let result = client.make_request(&url, None).await;
        server.abort();
        assert!(matches!(result, Err(AppError::RateLimited(1..=120))));
        assert_eq!(hits.load(Ordering::Relaxed), 2);
        assert_eq!(
            manager
                .list_accounts()
                .await
                .iter()
                .filter(|a| AccountManager::rate_limit_remaining(a).is_some())
                .count(),
            2,
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
