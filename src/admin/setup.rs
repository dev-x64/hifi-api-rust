use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::AppError;
use crate::setup::{
    AUTH_CLIENT_ID, AUTH_CLIENT_SECRET, REQUEST_CLIENT_ID, REQUEST_CLIENT_SECRET, issued_token_expires_at,
};
use crate::AppState;

#[derive(Clone)]
pub enum SetupStatus {
    Pending,
    Complete {
        account_id: String,
        label: String,
        user_id: String,
    },
    Error(String),
}

pub struct SetupSession {
    pub device_code: String,
    pub status: SetupStatus,
    pub label: Option<String>,
}

pub type Sessions = Arc<RwLock<HashMap<String, SetupSession>>>;

pub fn new_session_store() -> Sessions {
    Arc::new(RwLock::new(HashMap::new()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct DeviceAuthorization {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    interval: i64,
    expires_in: i64,
}

fn de_string_or_int<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    struct S;
    impl Visitor<'_> for S {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string or integer")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_owned())
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> {
            Ok(v)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }
    }
    d.deserialize_any(S)
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_in: i64,
    #[serde(default)]
    token_type: String,
    #[serde(default)]
    user: Option<UserInfo>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct UserInfo {
    #[serde(rename = "userId", deserialize_with = "de_string_or_int")]
    user_id: String,
    #[serde(default, rename = "countryCode")]
    country_code: String,
}

#[derive(Deserialize, Default)]
pub struct StartSetupRequest {
    pub label: Option<String>,
    /// When true, the new account serves metadata only.
    #[serde(default)]
    pub catalog: bool,
}

/// New accounts are stored under REQUEST_CLIENT_ID, with the secret the pool's
/// accounts on it already refresh with, so the stored pairing is a proven one.
async fn setup_client_secret(state: &AppState) -> String {
    state
        .account_manager
        .list_accounts()
        .await
        .into_iter()
        .find(|account| account.client_id == REQUEST_CLIENT_ID && !account.client_secret.is_empty())
        .map(|account| account.client_secret.clone())
        .unwrap_or_else(|| REQUEST_CLIENT_SECRET.to_string())
}

pub async fn start_setup(
    State(state): State<AppState>,
    body: Option<Json<StartSetupRequest>>,
) -> Result<Json<Value>, AppError> {
    let catalog = body.as_ref().is_some_and(|b| b.catalog);
    let client_secret = setup_client_secret(&state).await;
    let custom_label = body
        .and_then(|b| b.label.clone())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let mut http_client = state.proxy_manager.working_auth_client().await?;
    let request_code = |client: &reqwest::Client| {
        client
            .post("https://auth.tidal.com/v1/oauth2/device_authorization")
            .form(&[("client_id", AUTH_CLIENT_ID), ("scope", "r_usr+w_usr+w_sub")])
            .send()
    };
    // A proxy that cannot connect gets one replacement before giving up.
    let auth_resp = match request_code(&http_client).await {
        Err(e) if e.is_connect() || e.is_timeout() => {
            tracing::warn!("Device auth request failed: {}", error_chain(&e));
            http_client = state.proxy_manager.replace_auth_client().await?;
            request_code(&http_client).await
        }
        other => other,
    }
    .map_err(|e| AppError::ServiceUnavailable(format!("Device auth request failed: {}", error_chain(&e))))?;
    if !auth_resp.status().is_success() {
        let status = auth_resp.status();
        let body = auth_resp.text().await.unwrap_or_default();
        return Err(AppError::ServiceUnavailable(format!(
            "Tidal refused sign-in by code: HTTP {status} {}",
            body.chars().take(200).collect::<String>()
        )));
    }
    let auth_resp: DeviceAuthorization = auth_resp
        .json()
        .await
        .map_err(|e| AppError::ServiceUnavailable(format!("Device auth parse failed: {}", e)))?;

    let session_id = Uuid::new_v4().to_string();
    let device_code = auth_resp.device_code.clone();
    let raw_url = auth_resp.verification_uri_complete;
    let url = if raw_url.starts_with("http") {
        raw_url
    } else {
        format!("https://{}", raw_url)
    };

    {
        let mut sessions = state.setup_sessions.write().await;
        sessions.insert(
            session_id.clone(),
            SetupSession {
                device_code: device_code.clone(),
                status: SetupStatus::Pending,
                label: custom_label.clone(),
            },
        );
    }

    let sessions = state.setup_sessions.clone();
    let sid = session_id.clone();
    let am = state.account_manager.clone();
    let hc = http_client;

    tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
        loop {
            if tokio::time::Instant::now() > deadline {
                let mut sessions = sessions.write().await;
                if let Some(session) = sessions.get_mut(&sid) {
                    session.status = SetupStatus::Error("Timed out waiting for authorization".into());
                }
                break;
            }

            tokio::time::sleep(Duration::from_secs(5)).await;

            let res = match hc
                .post("https://auth.tidal.com/v1/oauth2/token")
                .form(&[
                    ("client_id", AUTH_CLIENT_ID),
                    ("scope", "r_usr+w_usr+w_sub"),
                    ("device_code", &device_code),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .basic_auth(
                    crate::token_manager::oauth_basic_component(AUTH_CLIENT_ID),
                    Some(crate::token_manager::oauth_basic_component(AUTH_CLIENT_SECRET)),
                )
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let mut sessions = sessions.write().await;
                    if let Some(session) = sessions.get_mut(&sid) {
                        session.status = SetupStatus::Error(format!("Poll failed: {}", error_chain(&e)));
                    }
                    break;
                }
            };

            // 400 is authorization_pending or slow_down; anything else
            // other than a 429 means the sign-in will not complete.
            if !res.status().is_success() {
                let status = res.status();
                if status.as_u16() == 400 || status.as_u16() == 429 {
                    continue;
                }
                let body = res.text().await.unwrap_or_default();
                let mut sessions = sessions.write().await;
                if let Some(session) = sessions.get_mut(&sid) {
                    session.status = SetupStatus::Error(format!(
                        "Tidal refused the token: HTTP {status} {}",
                        body.chars().take(200).collect::<String>()
                    ));
                }
                break;
            }

            let token_resp: TokenResponse = match res.text().await {
                Ok(text) => match serde_json::from_str(&text) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("OAuth token parse failed: {}", e);
                        let mut sessions = sessions.write().await;
                        if let Some(session) = sessions.get_mut(&sid) {
                            session.status = SetupStatus::Error(format!("Parse failed: {}", e));
                        }
                        break;
                    }
                },
                Err(e) => {
                    let mut sessions = sessions.write().await;
                    if let Some(session) = sessions.get_mut(&sid) {
                        session.status = SetupStatus::Error(format!("Parse failed: {}", e));
                    }
                    break;
                }
            };

            let user_id = token_resp
                .user
                .as_ref()
                .map(|u| u.user_id.clone())
                .unwrap_or_else(|| "unknown".into());
            // Read the live session label (may have been updated via PATCH while polling).
            let live_label = sessions
                .read()
                .await
                .get(&sid)
                .and_then(|s| s.label.clone());
            let label = live_label
                .or(custom_label.clone())
                .unwrap_or_else(|| format!("Tidal Account ({})", user_id));
            // Sign-in uses the TV client and the account is kept under
            // REQUEST_CLIENT_ID; refreshing with it now proves the pairing
            // works before the account is stored.
            let (access_token, refresh_token, expires_in) =
                match refresh_with_client(&hc, REQUEST_CLIENT_ID, &client_secret, &token_resp.refresh_token).await {
                    Ok((access, refresh, expires_in)) => (access, refresh.unwrap_or(token_resp.refresh_token), expires_in),
                    Err(e) => {
                        let mut sessions = sessions.write().await;
                        if let Some(session) = sessions.get_mut(&sid) {
                            session.status = SetupStatus::Error(format!(
                                "Signed in, but client {REQUEST_CLIENT_ID} could not refresh the token: {e}"
                            ));
                        }
                        break;
                    }
                };
            let expires_at = issued_token_expires_at(expires_in);

            match am
                .add_account(
                    label.clone(),
                    REQUEST_CLIENT_ID.to_string(),
                    client_secret.clone(),
                    refresh_token.clone(),
                    Some(user_id.clone()),
                )
                .await
            {
                Ok(acc) => {
                    acc.set_country(token_resp.user.as_ref().map(|u| u.country_code.clone()));
                    if catalog {
                        if let Err(e) = am.set_account_catalog(&acc.id, true).await {
                            tracing::warn!("Could not mark {} as catalog-only: {}", user_id, e);
                        }
                    }
                    if let Err(e) = am
                        .store_refreshed_credentials(
                            &acc,
                            &refresh_token,
                            None,
                            &access_token,
                            expires_at,
                        )
                        .await
                    {
                        // The refresh token is already persisted. Auto-heal can
                        // mint a new access token if this cache write failed.
                        tracing::warn!(
                            "Could not cache issued access token for {}: {}",
                            user_id,
                            e
                        );
                    }
                    let mut sessions = sessions.write().await;
                    if let Some(session) = sessions.get_mut(&sid) {
                        session.status = SetupStatus::Complete {
                            account_id: acc.id.clone(),
                            label,
                            user_id,
                        };
                    }
                }
                Err(e) => {
                    let mut sessions = sessions.write().await;
                    if let Some(session) = sessions.get_mut(&sid) {
                        session.status = SetupStatus::Error(format!("Add failed: {}", e));
                    }
                }
            }
            break;
        }
    });

    Ok(Json(json!({
        "session_id": session_id,
        "verification_uri": url
    })))
}

/// reqwest's own message stops at "error sending request"; the cause
/// (DNS, refused connection, proxy handshake, TLS) is in its sources.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

/// One refresh with another client's credentials, form body first as the
/// token manager does. Returns the access token, a rotated refresh token if
/// Tidal sent one, and the lifetime.
async fn refresh_with_client(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<(String, Option<String>, i64), String> {
    #[derive(Deserialize)]
    struct Refreshed {
        access_token: String,
        #[serde(default)]
        refresh_token: Option<String>,
        #[serde(default)]
        expires_in: i64,
    }
    let res = http
        .post("https://auth.tidal.com/v1/oauth2/token")
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {status} {}", body.chars().take(200).collect::<String>()));
    }
    let refreshed: Refreshed = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok((refreshed.access_token, refreshed.refresh_token, refreshed.expires_in))
}

pub async fn check_setup(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let sessions = state.setup_sessions.read().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| AppError::NotFound("Session not found".into()))?;

    match &session.status {
        SetupStatus::Pending => Ok(Json(json!({"status": "pending"}))),
        SetupStatus::Complete {
            account_id,
            label,
            user_id,
        } => Ok(Json(json!({
            "status": "complete",
            "account_id": account_id,
            "label": label,
            "user_id": user_id,
        }))),
        SetupStatus::Error(msg) => Ok(Json(json!({
            "status": "error",
            "error": msg,
        }))),
    }
}

pub async fn update_setup(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<StartSetupRequest>,
) -> Result<Json<Value>, AppError> {
    let mut sessions = state.setup_sessions.write().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| AppError::NotFound("Session not found".into()))?;

    if !matches!(session.status, SetupStatus::Pending) {
        return Err(AppError::BadRequest("Session is no longer pending".into()));
    }

    session.label = body
        .label
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(Json(json!({"status": "pending"})))
}
