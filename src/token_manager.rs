use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chrono::Utc;
use moka::future::Cache;
use reqwest::Client;
use serde_json::Value;
use sqlx::SqlitePool;
use tokio::sync::{Mutex, Semaphore};

use crate::account_manager::{AccountManager, AccountState};
use crate::error::AppError;
use crate::proxy_manager::ProxyManager;
use crate::upstash::UpstashStore;

const TOKEN_URL: &str = "https://auth.tidal.com/v1/oauth2/token";
const AUTH_INITIAL_ATTEMPTS: usize = 4;
const AUTH_RETRY_BASE_MS: u64 = 1_500;
const AUTH_RETRY_MAX_MS: u64 = 20_000;
const AUTH_FORBIDDEN_ROTATE_THRESHOLD: u64 = 4;
const AUTH_CONCURRENCY: usize = 2;

struct TokenAttemptFailure {
    error: AppError,
    retryable: bool,
    retry_after: u64,
    forbidden: bool,
}

pub(crate) fn oauth_basic_component(value: &str) -> String {
    form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

pub struct TokenManager {
    db: Option<SqlitePool>,
    refresh_locks: Cache<String, Arc<Mutex<()>>>,
    account_manager: OnceLock<Arc<AccountManager>>,
    proxy_manager: OnceLock<Arc<ProxyManager>>,
    upstash: OnceLock<Arc<UpstashStore>>,
    auth_slots: Semaphore,
    #[cfg(test)]
    pub(crate) token_url: String,
}

impl TokenManager {
    pub fn new(db: Option<SqlitePool>) -> Self {
        Self {
            db,
            refresh_locks: Cache::builder()
                .time_to_live(Duration::from_secs(3600))
                .max_capacity(500)
                .build(),
            account_manager: OnceLock::new(),
            proxy_manager: OnceLock::new(),
            upstash: OnceLock::new(),
            auth_slots: Semaphore::new(AUTH_CONCURRENCY),
            #[cfg(test)]
            token_url: TOKEN_URL.into(),
        }
    }

    pub fn set_account_manager(&self, am: Arc<AccountManager>) {
        let _ = self.account_manager.set(am);
    }

    pub fn set_proxy_manager(&self, pm: Arc<ProxyManager>) {
        let _ = self.proxy_manager.set(pm);
    }

    pub fn set_upstash(&self, store: Option<Arc<UpstashStore>>) {
        if let Some(store) = store {
            let _ = self.upstash.set(store);
        }
    }

    async fn cached_token(account: &AccountState) -> Option<String> {
        let token = account.access_token.read().await.clone()?;
        if token.is_empty()
            || Utc::now().timestamp() >= account.token_expires_at.load(Ordering::Relaxed)
            || account.rejected_access_token.read().await.as_ref() == Some(&token)
        {
            return None;
        }
        Some(token)
    }

    pub(crate) async fn needs_renewal(account: &AccountState) -> bool {
        account.token_expires_at.load(Ordering::Relaxed) <= Utc::now().timestamp() + 300
            || Self::cached_token(account).await.is_none()
    }

    pub(crate) async fn reject_access_token(account: &AccountState, rejected: &str) {
        let current = account.access_token.read().await;
        if current.as_deref() == Some(rejected) {
            *account.rejected_access_token.write().await = Some(rejected.into());
        }
        drop(current);
        account.observe_uptime(Utc::now().timestamp()).await;
    }

    pub(crate) async fn reject_refreshed_token(account: &AccountState, rejected: &str) {
        Self::reject_access_token(account, rejected).await;
        account.set_last_refresh_error(Some("Tidal HTTP 401: refreshed access token rejected".into()));
        Self::defer_retry(account, 30);
    }

    /// Do not reuse a token rejected by Tidal, even if Redis still advertises it.
    async fn shared_token(&self, account: &AccountState, previous: Option<&str>) -> Option<String> {
        let raw = self
            .upstash
            .get()?
            .get(&UpstashStore::k_token(&account.id))
            .await?;
        let data: Value = serde_json::from_str(&raw).ok()?;
        let token = data.get("t")?.as_str()?;
        let expires = data.get("e")?.as_i64()?;
        if token.is_empty()
            || Utc::now().timestamp() >= expires - 60
            || previous == Some(token)
            || account.rejected_access_token.read().await.as_deref() == Some(token)
        {
            return None;
        }
        *account.access_token.write().await = Some(token.into());
        account.token_expires_at.store(expires, Ordering::Relaxed);
        account.set_last_refresh_error(None);
        account.observe_uptime(Utc::now().timestamp()).await;
        Some(token.into())
    }

    pub async fn get_token(
        &self,
        account: &AccountState,
        client: &Client,
    ) -> Result<String, AppError> {
        if let Some(seconds) = AccountManager::rate_limit_remaining(account) {
            return Err(AppError::RateLimited(seconds));
        }
        if let Some(token) = Self::cached_token(account).await {
            return Ok(token);
        }
        self.refresh(account, client, None, false).await
    }

    /// Force a real refresh (manual action or proactive renewal), coalescing
    /// concurrent calls that observed the same access-token generation.
    pub async fn refresh_token(
        &self,
        account: &AccountState,
        client: &Client,
    ) -> Result<String, AppError> {
        let previous = account.access_token.read().await.clone();
        self.refresh(account, client, previous.as_deref(), true)
            .await
    }

    /// A 401 invalidates only the token used for that request, never a newer
    /// token another request has already installed.
    pub async fn refresh_after_unauthorized(
        &self,
        account: &AccountState,
        client: &Client,
        rejected: &str,
    ) -> Result<String, AppError> {
        Self::reject_access_token(account, rejected).await;
        self.refresh(account, client, Some(rejected), true).await
    }

    pub(crate) fn defer_retry(account: &AccountState, minimum: u64) -> u64 {
        let failures = account
            .heal_failures
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let delay = (30_u64.saturating_mul(1 << failures.saturating_sub(1).min(7)))
            .min(3600)
            .saturating_add(rand::random::<u64>() % 16)
            .max(minimum);
        account.heal_next_retry.store(
            Utc::now()
                .timestamp()
                .saturating_add(i64::try_from(delay).unwrap_or(i64::MAX)),
            Ordering::Relaxed,
        );
        delay
    }

    async fn refresh(
        &self,
        account: &AccountState,
        client: &Client,
        previous: Option<&str>,
        force: bool,
    ) -> Result<String, AppError> {
        let lock = self
            .refresh_locks
            .get_with(account.id.clone(), async { Arc::new(Mutex::new(())) })
            .await;
        let _guard = lock.lock().await;
        if let Some(token) = Self::cached_token(account).await {
            if !force || Some(token.as_str()) != previous {
                return Ok(token);
            }
        }
        if let Some(seconds) = AccountManager::rate_limit_remaining(account) {
            return Err(AppError::RateLimited(seconds));
        }
        let remaining = account.heal_next_retry.load(Ordering::Relaxed) - Utc::now().timestamp();
        if remaining > 0 {
            return Err(AppError::ServiceUnavailableRetry(
                "Token refresh is cooling down".into(),
                remaining as u64,
            ));
        }
        if let Some(token) = self
            .shared_token(account, if force { previous } else { None })
            .await
        {
            return Ok(token);
        }

        // Keep simultaneous account recoveries from turning a shared auth/IP
        // problem into a burst. Per-account refreshes are already coalesced by
        // the lock above.
        let _auth_slot = self
            .auth_slots
            .acquire()
            .await
            .expect("auth semaphore open");
        // Bound the complete fast-retry cycle, proxy resolution and persistence.
        let result = tokio::time::timeout(
            Duration::from_secs(45),
            self.request_token_cycle(account, client),
        )
        .await;
        match result {
            Ok(Ok(token)) => {
                account.set_last_refresh_error(None);
                account.heal_failures.store(0, Ordering::Relaxed);
                account.heal_next_retry.store(0, Ordering::Relaxed);
                account.auth_forbidden_streak.store(0, Ordering::Relaxed);
                Ok(token)
            }
            result => {
                let error = match result {
                    Ok(Err(error)) => error,
                    _ => AppError::Timeout,
                };
                // Only explicit OAuth credential errors disable the account.
                // HTML/WAF 403s, transport failures and 5xx are temporary.
                if matches!(error, AppError::Unauthorized(_)) {
                    if let Some(am) = self.account_manager.get() {
                        if let Err(e) = am.set_system_disabled(account, true).await {
                            tracing::warn!(
                                "Could not persist auth state for {}: {}",
                                account.id,
                                e
                            );
                        }
                    } else if account.is_active.load(Ordering::Relaxed) {
                        account.auto_disabled.store(true, Ordering::Relaxed);
                        account.is_active.store(false, Ordering::Relaxed);
                        account
                            .disabled_at
                            .store(Utc::now().timestamp(), Ordering::Relaxed);
                    }
                }
                let minimum = match &error {
                    AppError::RateLimited(seconds)
                    | AppError::ServiceUnavailableRetry(_, seconds) => *seconds,
                    AppError::Unauthorized(_) => 300,
                    _ => 0,
                };
                account.set_last_refresh_error(Some(error.to_string()));
                let delay = Self::defer_retry(account, minimum);
                tracing::warn!(account = %account.label, retry_in = delay, "Token refresh failed: {}", error);
                Err(error)
            }
        }
    }

    async fn request_token_cycle(
        &self,
        account: &AccountState,
        fallback: &Client,
    ) -> Result<String, AppError> {
        let client;
        let http = if let Some(pm) = self.proxy_manager.get() {
            if pm.should_rotate_on_refresh() {
                pm.rotate_account(&account.id).await;
                account.auth_forbidden_streak.store(0, Ordering::Relaxed);
            } else if pm.proxies_enabled()
                && account.auth_forbidden_streak.load(Ordering::Relaxed)
                    >= AUTH_FORBIDDEN_ROTATE_THRESHOLD
            {
                tracing::warn!(
                    account = %account.label,
                    failures = account.auth_forbidden_streak.load(Ordering::Relaxed),
                    "Rotating account proxy after repeated Tidal auth 403 responses"
                );
                pm.rotate_account(&account.id).await;
                account.auth_forbidden_streak.store(0, Ordering::Relaxed);
            }
            client = pm.working_auth_client_for(&account.id).await?;
            &client
        } else {
            fallback
        };

        let attempts = if account.heal_failures.load(Ordering::Relaxed) == 0 {
            AUTH_INITIAL_ATTEMPTS
        } else {
            1
        };
        let mut use_basic = account.auth_use_basic.load(Ordering::Relaxed);
        let mut last_error = None;
        for attempt in 0..attempts {
            match self.request_token_once(account, http, use_basic).await {
                Ok(token) => {
                    account.auth_use_basic.store(use_basic, Ordering::Relaxed);
                    account.auth_forbidden_streak.store(0, Ordering::Relaxed);
                    return Ok(token);
                }
                Err(failure) => {
                    if failure.forbidden {
                        account
                            .auth_forbidden_streak
                            .fetch_add(1, Ordering::Relaxed);
                        use_basic = !use_basic;
                        account.auth_use_basic.store(use_basic, Ordering::Relaxed);
                    } else {
                        account.auth_forbidden_streak.store(0, Ordering::Relaxed);
                    }
                    let is_last = attempt + 1 == attempts;
                    let long_retry_after =
                        failure.retry_after.saturating_mul(1_000) > AUTH_RETRY_MAX_MS;
                    if !failure.retryable || is_last || long_retry_after {
                        return Err(failure.error);
                    }
                    let delay = auth_retry_delay(attempt, failure.retry_after);
                    tracing::warn!(
                        account = %account.label,
                        attempt = attempt + 1,
                        max_attempts = attempts,
                        retry_in_ms = delay.as_millis(),
                        next_form = if use_basic { "basic" } else { "body" },
                        "Tidal token refresh attempt failed: {}",
                        failure.error
                    );
                    last_error = Some(failure.error);
                    sleep_auth_retry(delay).await;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            AppError::ServiceUnavailable("No token refresh attempt completed".into())
        }))
    }

    async fn request_token_once(
        &self,
        account: &AccountState,
        http: &Client,
        use_basic: bool,
    ) -> Result<String, TokenAttemptFailure> {
        let old_refresh = account.refresh_token();
        // RFC 6749 section 2.3.1: form-encode each Basic component exactly once.
        // Stored secrets are raw (legacy %3D is normalized at account ingestion).
        #[cfg(not(test))]
        let url = TOKEN_URL;
        #[cfg(test)]
        let url = self.token_url.as_str();
        let mut fields = vec![
            ("client_id", account.client_id.as_str()),
            ("refresh_token", old_refresh.as_str()),
            ("grant_type", "refresh_token"),
        ];
        let mut request = http
            .post(url)
            .header(reqwest::header::ACCEPT, "application/json");
        if use_basic {
            request = request.basic_auth(
                oauth_basic_component(&account.client_id),
                Some(oauth_basic_component(&account.client_secret)),
            );
            fields.push(("scope", "r_usr+w_usr+w_sub"));
        } else {
            fields.push(("client_secret", account.client_secret.as_str()));
        }
        let response = request.form(&fields).send().await;
        let response = match response {
            Ok(response) => {
                if let Some(pm) = self.proxy_manager.get() {
                    pm.note_success_for(&account.id).await;
                }
                response
            }
            Err(e) => {
                if e.is_connect() || e.is_timeout() {
                    if let Some(pm) = self.proxy_manager.get() {
                        pm.note_failure_for(&account.id).await;
                    }
                }
                let retryable = e.is_connect() || e.is_timeout();
                return Err(TokenAttemptFailure {
                    error: e.into(),
                    retryable,
                    retry_after: 0,
                    forbidden: false,
                });
            }
        };
        let status = response.status();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if status.as_u16() == 429 {
            let seconds = AccountManager::pause_account(account, retry_after.as_deref());
            return Err(TokenAttemptFailure {
                error: AppError::RateLimited(seconds),
                retryable: true,
                retry_after: seconds,
                forbidden: false,
            });
        }
        let data: Value = response.json().await.unwrap_or_default();
        if !status.is_success() {
            let oauth_error = data.get("error").and_then(Value::as_str).unwrap_or("");
            if matches!(status.as_u16(), 400 | 401)
                && matches!(oauth_error, "invalid_grant" | "invalid_client")
            {
                return Err(TokenAttemptFailure {
                    error: AppError::Unauthorized(format!("Tidal auth HTTP {status}: OAuth {oauth_error}")),
                    retryable: false,
                    retry_after: 0,
                    forbidden: false,
                });
            }
            let retry_after_secs = retry_after
                .as_deref()
                .map(|value| AccountManager::retry_after_secs(Some(value)))
                .unwrap_or(0);
            let error = if retry_after.is_some() {
                AppError::ServiceUnavailableRetry(
                    format!("Tidal auth HTTP {status}"),
                    retry_after_secs,
                )
            } else {
                AppError::UpstreamError(status, format!("Tidal auth HTTP {status}"))
            };
            return Err(TokenAttemptFailure {
                error,
                retryable: matches!(
                    status.as_u16(),
                    403 | 408 | 425 | 429 | 500 | 502 | 503 | 504
                ),
                retry_after: retry_after_secs,
                forbidden: status.as_u16() == 403,
            });
        }
        let token = data
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| TokenAttemptFailure {
                error: AppError::Internal("Empty access_token in auth response".into()),
                retryable: false,
                retry_after: 0,
                forbidden: false,
            })?
            .to_string();
        let expires_in = data
            .get("expires_in")
            .and_then(Value::as_i64)
            .filter(|n| *n > 0)
            .ok_or_else(|| TokenAttemptFailure {
                error: AppError::Internal("Invalid expires_in in auth response".into()),
                retryable: false,
                retry_after: 0,
                forbidden: false,
            })?;
        let expires_at = Utc::now()
            .timestamp()
            .saturating_add(expires_in)
            .saturating_sub(60.min(expires_in / 10));
        let rotated = data
            .get("refresh_token")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty());

        // Serialize credential persistence with account replacement/deletion.
        if let Some(am) = self.account_manager.get() {
            am.store_refreshed_credentials(account, &old_refresh, rotated, &token, expires_at)
                .await
                .map_err(non_retryable_attempt)?;
        } else {
            if let Some(db) = &self.db {
                persist_token(db, account, &old_refresh, rotated, &token, expires_at)
                    .await
                    .map_err(non_retryable_attempt)?;
            }
            if let Some(rotated) = rotated {
                account.replace_refresh_token(rotated.into());
            }
            *account.access_token.write().await = Some(token.clone());
            account
                .token_expires_at
                .store(expires_at, Ordering::Relaxed);
            account.observe_uptime(Utc::now().timestamp()).await;
        }
        if let Some(store) = self.upstash.get() {
            let payload = serde_json::json!({"t": token, "e": expires_at}).to_string();
            store
                .set(
                    &UpstashStore::k_token(&account.id),
                    &payload,
                    Some(expires_in as u64),
                )
                .await;
        }
        Ok(token)
    }
}

fn non_retryable_attempt(error: AppError) -> TokenAttemptFailure {
    TokenAttemptFailure {
        error,
        retryable: false,
        retry_after: 0,
        forbidden: false,
    }
}

fn auth_retry_delay(attempt: usize, retry_after: u64) -> Duration {
    let exponent = u32::try_from(attempt).unwrap_or(u32::MAX).min(8);
    let base_ms = AUTH_RETRY_BASE_MS
        .saturating_mul(1_u64 << exponent)
        .min(AUTH_RETRY_MAX_MS);
    let jitter_max = (base_ms / 4).min(1_000);
    let jitter_ms = if jitter_max == 0 {
        0
    } else {
        rand::random::<u64>() % (jitter_max + 1)
    };
    Duration::from_millis(
        base_ms
            .saturating_add(jitter_ms)
            .max(retry_after.saturating_mul(1_000)),
    )
}

async fn sleep_auth_retry(delay: Duration) {
    #[cfg(not(test))]
    tokio::time::sleep(delay).await;
    #[cfg(test)]
    {
        let _ = delay;
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

pub(crate) async fn persist_token(
    db: &SqlitePool,
    account: &AccountState,
    old_refresh: &str,
    rotated: Option<&str>,
    token: &str,
    expires_at: i64,
) -> Result<(), AppError> {
    let mut tx = db.begin().await?;
    let changed = sqlx::query(
        "UPDATE accounts SET refresh_token = ?, updated_at = ? WHERE id = ? AND refresh_token = ? AND client_id = ? AND client_secret = ?",
    ).bind(rotated.unwrap_or(old_refresh)).bind(Utc::now().timestamp()).bind(&account.id)
        .bind(old_refresh).bind(&account.client_id).bind(&account.client_secret).execute(&mut *tx).await?;
    if changed.rows_affected() == 0 {
        return Err(AppError::ServiceUnavailable(
            "Account credentials changed during refresh".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO tokens (account_id, access_token, expires_at, refreshed_at) VALUES (?, ?, ?, ?)
         ON CONFLICT(account_id) DO UPDATE SET access_token = excluded.access_token, expires_at = excluded.expires_at, refreshed_at = excluded.refreshed_at",
    ).bind(&account.id).bind(token).bind(expires_at).bind(Utc::now().timestamp())
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_manager::SwitchingWeights;
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::post,
    };
    use std::sync::atomic::AtomicUsize;

    struct MockAuth {
        url: String,
        calls: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<(HeaderMap, String)>>>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for MockAuth {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl MockAuth {
        async fn new(status: StatusCode, body: Value) -> Self {
            Self::with_retry_after(status, body, None).await
        }

        async fn with_retry_after(
            status: StatusCode,
            body: Value,
            retry_after: Option<&str>,
        ) -> Self {
            let calls = Arc::new(AtomicUsize::new(0));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let handler_calls = calls.clone();
            let handler_requests = requests.clone();
            let retry_after = retry_after.map(str::to_string);
            let app = Router::new().route(
                "/token",
                post(move |headers: HeaderMap, request: String| {
                    let calls = handler_calls.clone();
                    let requests = handler_requests.clone();
                    let body = body.clone();
                    let retry_after = retry_after.clone();
                    async move {
                        calls.fetch_add(1, Ordering::Relaxed);
                        requests.lock().await.push((headers, request));
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        let mut response = (status, Json(body)).into_response();
                        if let Some(value) = retry_after {
                            response.headers_mut().insert(
                                reqwest::header::RETRY_AFTER,
                                value.parse().unwrap(),
                            );
                        }
                        response
                    }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/token", listener.local_addr().unwrap());
            let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            Self {
                url,
                calls,
                requests,
                task,
            }
        }
    }

    async fn account() -> (Arc<AccountManager>, Arc<AccountState>) {
        let am = Arc::new(AccountManager::new(None, SwitchingWeights::default()));
        let account = am
            .add_account(
                "test".into(),
                "client".into(),
                "secret%3D".into(),
                "refresh+old=".into(),
                None,
            )
            .await
            .unwrap();
        *account.access_token.write().await = Some("old".into());
        account
            .token_expires_at
            .store(Utc::now().timestamp() + 3600, Ordering::Relaxed);
        (am, account)
    }

    #[tokio::test]
    async fn forced_401_refresh_is_coalesced_and_late_401_keeps_new_token() {
        let mock = MockAuth::new(
            StatusCode::OK,
            serde_json::json!({"access_token":"new", "expires_in":3600}),
        )
        .await;
        let (am, account) = account().await;
        let mut tm = TokenManager::new(None);
        tm.token_url = mock.url.clone();
        tm.set_account_manager(am);
        let client = Client::new();
        assert_eq!(tm.get_token(&account, &client).await.unwrap(), "old");
        let results = futures::future::join_all(
            (0..8).map(|_| tm.refresh_after_unauthorized(&account, &client, "old")),
        )
        .await;
        assert!(
            results
                .iter()
                .all(|result| result.as_ref().unwrap() == "new")
        );
        assert_eq!(mock.calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            tm.refresh_after_unauthorized(&account, &client, "old")
                .await
                .unwrap(),
            "new"
        );
        assert_eq!(mock.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn rotated_refresh_and_normalized_secret_survive_reload() {
        let mock = MockAuth::new(StatusCode::OK, serde_json::json!({"access_token":"new", "refresh_token":"rotated+=", "expires_in":3600})).await;
        let db = crate::db::init_pool("sqlite::memory:").await.unwrap();
        let am = Arc::new(AccountManager::new(
            Some(db.clone()),
            SwitchingWeights::default(),
        ));
        let account = am
            .add_account(
                "test".into(),
                "client".into(),
                "secret%3D".into(),
                "refresh+old=".into(),
                None,
            )
            .await
            .unwrap();
        let mut tm = TokenManager::new(Some(db.clone()));
        tm.token_url = mock.url.clone();
        tm.set_account_manager(am.clone());
        account.set_last_refresh_error(Some("Previous refresh failure".into()));
        tm.refresh_token(&account, &Client::new()).await.unwrap();
        assert_eq!(account.last_refresh_error(), None);
        assert_eq!(account.refresh_token(), "rotated+=");
        let reloaded = AccountManager::new(Some(db), SwitchingWeights::default());
        reloaded.load_from_db().await.unwrap();
        let restored = reloaded.get_account_by_id(&account.id).await.unwrap();
        assert_eq!(restored.refresh_token(), "rotated+=");
        assert_eq!(restored.client_secret, "secret=");
        assert_eq!(restored.access_token.read().await.as_deref(), Some("new"));
        let requests = mock.requests.lock().await;
        let (headers, body) = &requests[0];
        assert!(!headers.contains_key("authorization"));
        let fields: std::collections::HashMap<_, _> = form_urlencoded::parse(body.as_bytes())
            .into_owned()
            .collect();
        assert_eq!(fields["refresh_token"], "refresh+old=");
        assert_eq!(fields["client_secret"], "secret=");
    }

    #[tokio::test]
    async fn transient_403_uses_four_fast_alternating_attempts_then_backs_off() {
        let mock = MockAuth::new(StatusCode::FORBIDDEN, Value::Null).await;
        let (am, account) = account().await;
        let mut tm = TokenManager::new(None);
        tm.token_url = mock.url.clone();
        tm.set_account_manager(am);
        let client = Client::new();
        assert!(tm.refresh_token(&account, &client).await.is_err());
        let cause = account.last_refresh_error().unwrap();
        assert!(cause.contains("403"));
        assert!(account.is_active.load(Ordering::Relaxed));
        assert!(!account.auto_disabled.load(Ordering::Relaxed));
        assert_eq!(tm.get_token(&account, &client).await.unwrap(), "old");
        assert!(tm.refresh_token(&account, &client).await.is_err());
        assert_eq!(account.last_refresh_error().as_deref(), Some(cause.as_str()));
        assert_eq!(mock.calls.load(Ordering::Relaxed), AUTH_INITIAL_ATTEMPTS);
        assert_eq!(
            account.auth_forbidden_streak.load(Ordering::Relaxed),
            AUTH_FORBIDDEN_ROTATE_THRESHOLD
        );
        assert!(account.heal_next_retry.load(Ordering::Relaxed) >= Utc::now().timestamp() + 29);
        {
            let requests = mock.requests.lock().await;
            assert_eq!(requests.len(), AUTH_INITIAL_ATTEMPTS);
            for (index, (headers, body)) in requests.iter().enumerate() {
                if index % 2 == 0 {
                    assert!(!headers.contains_key("authorization"));
                    assert!(body.contains("client_secret=secret%3D"));
                } else {
                    assert!(headers.contains_key("authorization"));
                    assert!(!body.contains("client_secret"));
                }
            }
        }
        account.heal_next_retry.store(0, Ordering::Relaxed);
        assert!(tm.refresh_token(&account, &client).await.is_err());
        assert_eq!(account.heal_failures.load(Ordering::Relaxed), 2);
        assert_eq!(
            mock.calls.load(Ordering::Relaxed),
            AUTH_INITIAL_ATTEMPTS + 1
        );
        let requests = mock.requests.lock().await;
        let (headers, body) = &requests[AUTH_INITIAL_ATTEMPTS];
        assert!(!headers.contains_key("authorization"));
        assert!(body.contains("client_secret=secret%3D"));
        assert!(!body.contains("%253D"));
    }

    #[tokio::test]
    async fn oauth_rejection_keeps_increasing_backoff_and_manual_off_wins() {
        let mock = MockAuth::new(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error":"invalid_grant"}),
        )
        .await;
        let (am, account) = account().await;
        let mut tm = TokenManager::new(None);
        tm.token_url = mock.url.clone();
        tm.set_account_manager(am.clone());
        for failures in 1..=6 {
            account.heal_next_retry.store(0, Ordering::Relaxed);
            assert!(matches!(
                tm.refresh_token(&account, &Client::new()).await,
                Err(AppError::Unauthorized(_))
            ));
            assert_eq!(account.heal_failures.load(Ordering::Relaxed), failures);
            assert!(account.auto_disabled.load(Ordering::Relaxed));
        }
        assert!(account.heal_next_retry.load(Ordering::Relaxed) >= Utc::now().timestamp() + 959);
        am.set_account_active(&account.id, false).await.unwrap();
        assert!(!am.set_system_disabled(&account, false).await.unwrap());
        assert!(!am.set_system_disabled(&account, true).await.unwrap());
        assert!(!account.is_active.load(Ordering::Relaxed));
        assert!(!account.auto_disabled.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn auth_429_does_not_disable_account_or_retry_immediately() {
        let mock = MockAuth::with_retry_after(
            StatusCode::TOO_MANY_REQUESTS,
            Value::Null,
            Some("120"),
        )
        .await;
        let (am, account) = account().await;
        let mut tm = TokenManager::new(None);
        tm.token_url = mock.url.clone();
        tm.set_account_manager(am);
        let client = Client::new();
        assert!(matches!(
            tm.refresh_token(&account, &client).await,
            Err(AppError::RateLimited(120))
        ));
        assert!(matches!(
            tm.get_token(&account, &client).await,
            Err(AppError::RateLimited(_))
        ));
        assert_eq!(mock.calls.load(Ordering::Relaxed), 1);
        assert!(account.is_active.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn background_loop_recovers_auto_disabled_and_renews_active_but_leaves_manual_off() {
        use crate::{config::Config, notifier::Notifier, settings::AppSettings};
        let mock = MockAuth::new(
            StatusCode::OK,
            serde_json::json!({"access_token":"new", "expires_in":3600}),
        )
        .await;
        let (am, auto) = account().await;
        am.set_system_disabled(&auto, true).await.unwrap();
        let manual = am
            .add_account("manual".into(), "c".into(), "s".into(), "r".into(), None)
            .await
            .unwrap();
        am.set_account_active(&manual.id, false).await.unwrap();
        let active = am
            .add_account("active".into(), "c".into(), "s".into(), "r2".into(), None)
            .await
            .unwrap();
        *active.access_token.write().await = Some("nearly-expired".into());
        active
            .token_expires_at
            .store(Utc::now().timestamp() + 100, Ordering::Relaxed);
        for account in [&auto, &manual, &active] {
            account
                .premium_checked_at
                .store(Utc::now().timestamp(), Ordering::Relaxed);
        }
        let mut config = Config::from_env();
        config.use_proxies = false;
        let pm = Arc::new(ProxyManager::new(Arc::new(config), None));
        let mut tm = TokenManager::new(None);
        tm.token_url = mock.url.clone();
        tm.set_account_manager(am.clone());
        tm.set_proxy_manager(pm.clone());
        let settings = Arc::new(AppSettings::from_env());
        settings
            .set_discord_webhook_url(String::new(), None)
            .await
            .unwrap();
        let tm = Arc::new(tm);
        let tidal = Arc::new(crate::tidal_client::TidalClient::new(
            pm.clone(),
            tm.clone(),
            am.clone(),
            Notifier::new(settings.clone()),
            Arc::new(Config::from_env()),
            settings.clone(),
        ));
        crate::autoheal::start_autoheal_loop(
            am,
            tm,
            pm,
            tidal,
            Notifier::new(settings),
        )
        .await;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if auto.is_active.load(Ordering::Relaxed)
                    && active.access_token.read().await.as_deref() == Some("new")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(!manual.is_active.load(Ordering::Relaxed));
        assert!(!auto.auto_disabled.load(Ordering::Relaxed));
        assert_eq!(mock.calls.load(Ordering::Relaxed), 2);
    }
}
