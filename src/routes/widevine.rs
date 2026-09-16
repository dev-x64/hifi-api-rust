use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method};
use axum::response::Response;

use crate::error::AppError;
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
/// Returns (status, content_type, body).
pub(crate) async fn fetch_widevine_license(
    state: &AppState,
    method: &str,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<(u16, String, Vec<u8>), AppError> {
    let account = state.account_manager.select_account().await?;
    let hc = state.tidal_client.working_client().await?;
    let token = state
        .token_manager
        .get_token(&account, &hc)
        .await?;

    let url = "https://api.tidal.com/v2/widevine";

    let mut req = hc
        .request(
            method.parse::<Method>().unwrap_or(Method::POST),
            url,
        )
        .header("authorization", format!("Bearer {}", token))
        .header("User-Agent", state.config.user_agent.as_str())
        .body(body.to_vec());

    req = req.header(
        "Content-Type",
        content_type.unwrap_or("application/octet-stream"),
    );

    let resp = req.send().await.map_err(|_| {
        AppError::ServiceUnavailable("Error communicating with widevine server".into())
    })?;

    let status = resp.status();
    if state.config.dev_mode {
        tracing::info!("[DEV] {} {} → {}", method, url, status.as_u16());
    }
    let resp_content_type = resp
        .headers()
        .get("Content-Type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let content = resp.bytes().await.unwrap_or_default();

    Ok((status.as_u16(), resp_content_type, content.to_vec()))
}
