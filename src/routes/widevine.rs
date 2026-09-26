use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method};
use axum::response::Response;

use crate::error::AppError;
use crate::account_manager::AccountManager;
use crate::AppState;

pub async fn widevine_proxy(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    // Upstream parity: forward the caller's Content-Type, defaulting to
    // application/octet-stream like binimum/hifi-api.
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let op = crate::playback::PlaybackOp::Widevine {
        method: method.to_string(),
        content_type: Some(content_type),
        body: body.to_vec(),
    };
    state.playback.dispatch(&state, op).await
}

/// Core /widevine/ fetch (shared by immediate and queued execution).
/// Returns (status, content_type, body). Fails over across playback
/// accounts on retryable statuses so widevine load spreads and one
/// banned account doesn't fail the request.
pub(crate) async fn fetch_widevine_license(
    state: &AppState,
    method: &str,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<(u16, String, Vec<u8>), AppError> {
    let pool = state.account_manager.playback_count().await.max(1);
    let mut failed_ids: Vec<String> = Vec::new();
    let mut last_err: Option<AppError> = None;
    let mut first_rate_limit_try: Option<usize> = None;
    // Last retryable HTTP response, returned when every account fails over.
    let mut last_http: Option<(u16, String, Vec<u8>)> = None;

    for account_try in 0..pool {
        if first_rate_limit_try.is_some_and(|first| account_try > first + 1) {
            break;
        }
        let account = match state
            .account_manager
            .select_account_excluding(&failed_ids)
            .await
        {
            Ok(a) => a,
            Err(e) => {
                if matches!(last_err, Some(AppError::RateLimited(_))) {
                    return Err(last_err.unwrap());
                }
                return match last_http {
                    Some(r) => Ok(r),
                    None => Err(last_err.unwrap_or(e)),
                };
            }
        };
        let mut hc = state.tidal_client.working_client_for(&account.id).await?;
        let token = match state.token_manager.get_token(&account, &hc).await {
            Ok(t) => t,
            Err(e @ AppError::RateLimited(_)) => {
                if first_rate_limit_try.is_some() {
                    return Err(e);
                }
                first_rate_limit_try = Some(account_try);
                failed_ids.push(account.id.clone());
                last_err = Some(e);
                continue;
            }
            Err(e) => {
                state
                    .account_manager
                    .mark_account_error(&account.id, &format!("token failure: {:?}", e))
                    .await;
                failed_ids.push(account.id.clone());
                last_err = Some(e);
                continue;
            }
        };
        if let Some(seconds) = AccountManager::rate_limit_remaining(&account) {
            let e = AppError::RateLimited(seconds);
            if first_rate_limit_try.is_some() {
                return Err(e);
            }
            first_rate_limit_try = Some(account_try);
            failed_ids.push(account.id.clone());
            last_err = Some(e);
            continue;
        }
        hc = state.tidal_client.working_client_for(&account.id).await?;

        let url = "https://api.tidal.com/v2/widevine";

        let send = |client: &reqwest::Client, token: &str| {
            client.request(
                method.parse::<Method>().unwrap_or(Method::POST),
                url,
            )
            .header("authorization", format!("Bearer {}", token))
            .header("X-Tidal-Token", account.client_id.as_str())
            .header("User-Agent", state.config.user_agent.as_str())
            .body(body.to_vec())
            .header(
                "Content-Type",
                content_type.unwrap_or("application/octet-stream"),
            )
            .send()
        };

        // Send once; on 401 refresh the token and retry once on the same
        // account before failing over. Transport errors are proxy-level
        // (same as make_request): surface immediately, no failover.
        let mut token_owned = token;
        let mut resp = match send(&hc, &token_owned).await {
            Ok(r) => {
                state.proxy_manager.note_success_for(&account.id).await;
                r
            },
            Err(e) => {
                if e.is_connect() || e.is_timeout() {
                    state.proxy_manager.note_failure_for(&account.id).await;
                }
                return Err(AppError::ServiceUnavailable(
                    "Error communicating with widevine server".into(),
                ));
            }
        };
        if resp.status().as_u16() == 401 {
            match state.token_manager.refresh_after_unauthorized(&account, &hc, &token_owned).await {
                Ok(fresh) => {
                    token_owned = fresh;
                    hc = state.tidal_client.working_client_for(&account.id).await?;
                    match send(&hc, &token_owned).await {
                        Ok(r) => {
                            state.proxy_manager.note_success_for(&account.id).await;
                            resp = r;
                        },
                        Err(e) => {
                            if e.is_connect() || e.is_timeout() {
                                state.proxy_manager.note_failure_for(&account.id).await;
                            }
                            return Err(AppError::ServiceUnavailable(
                                "Error communicating with widevine server".into(),
                            ));
                        }
                    }
                }
                Err(e) => {
                    if matches!(e, AppError::RateLimited(_)) {
                        if first_rate_limit_try.is_some() {
                            return Err(e);
                        }
                        first_rate_limit_try = Some(account_try);
                        failed_ids.push(account.id.clone());
                        last_err = Some(e);
                        continue;
                    }
                    state
                        .account_manager
                        .mark_account_error(
                            &account.id,
                            &format!("token refresh failure: {:?}", e),
                        )
                        .await;
                    failed_ids.push(account.id.clone());
                    last_err = Some(e);
                    continue;
                }
            }
        }

        let status = resp.status();
        if status.as_u16() == 401 {
            crate::token_manager::TokenManager::reject_refreshed_token(&account, &token_owned).await;
        }
        if status.as_u16() == 429 {
            let seconds = AccountManager::pause_account(
                &account,
                resp.headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok()),
            );
            state
                .account_manager
                .mark_account_error(&account.id, "Tidal HTTP 429")
                .await;
            let e = AppError::RateLimited(seconds);
            if first_rate_limit_try.is_some() {
                return Err(e);
            }
            first_rate_limit_try = Some(account_try);
            failed_ids.push(account.id.clone());
            last_err = Some(e);
            continue;
        }
        if state.config.dev_mode {
            tracing::info!("[DEV] {} {} → {}", method, url, status.as_u16());
        }
        let resp_content_type = resp
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/json")
            .to_string();
        let content = resp.bytes().await.unwrap_or_default().to_vec();

        let code = status.as_u16();
        if code == 403 || code >= 500 {
            state
                .account_manager
                .mark_account_error(&account.id, &format!("Tidal HTTP {}", code))
                .await;
            failed_ids.push(account.id.clone());
            last_http = Some((code, resp_content_type, content));
            last_err = Some(AppError::UpstreamError(status, "Upstream API error".into()));
            continue;
        }

        return Ok((code, resp_content_type, content));
    }

    if matches!(last_err, Some(AppError::RateLimited(_))) {
        return Err(last_err.unwrap());
    }
    match last_http {
        Some(r) => Ok(r),
        None => Err(last_err.unwrap_or(AppError::ServiceUnavailable(
            "All accounts failed".into(),
        ))),
    }
}
