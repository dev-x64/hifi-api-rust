pub mod accounts;
pub mod auth_guard;
pub mod alerts;
pub mod backup;
pub mod api_keys;
pub mod cache;
pub mod proxies;
pub mod requests;
pub mod setup;
pub mod settings;
pub mod stats;
pub mod ui;

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::header::{CACHE_CONTROL, COOKIE, RETRY_AFTER, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::time::Instant;

use crate::request_log::{client_ip, client_ip_from_headers};
use crate::AppState;

const SESSION_COOKIE: &str = "hifi_admin_session";
const SESSION_SECONDS: i64 = 30 * 24 * 60 * 60;

pub(crate) fn locked_response(retry_after: u64) -> Response {
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({"detail": "Too many failed admin authentication attempts. Try again later."})),
    )
        .into_response();
    response.headers_mut().insert(
        RETRY_AFTER,
        HeaderValue::from_str(&retry_after.to_string()).expect("valid retry interval"),
    );
    response.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Deserialize)]
pub struct LoginRequest {
    key: String,
}

fn session_signature(key: &str, expiry: i64) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > block.len() {
        block[..32].copy_from_slice(&Sha256::digest(key.as_bytes()));
    } else {
        block[..key.len()].copy_from_slice(key.as_bytes());
    }
    let mut inner_pad = [0x36u8; 64];
    let mut outer_pad = [0x5cu8; 64];
    for (index, byte) in block.iter().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }
    let inner = Sha256::new()
        .chain_update(inner_pad)
        .chain_update(format!("hifi-admin-session-v1:{expiry}"))
        .finalize();
    Sha256::new().chain_update(outer_pad).chain_update(inner).finalize().into()
}

fn valid_session(headers: &HeaderMap, key: &str) -> bool {
    let Some(cookie) = headers.get(COOKIE).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(value) = cookie.split(';').map(str::trim).find_map(|part| part.strip_prefix(&format!("{SESSION_COOKIE}="))) else {
        return false;
    };
    let mut parts = value.split('.');
    let (Some("v1"), Some(expiry), Some(signature), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let Ok(expiry) = expiry.parse::<i64>() else { return false; };
    let now = chrono::Utc::now().timestamp();
    if expiry < now || expiry > now + SESSION_SECONDS { return false; }
    let Ok(signature) = URL_SAFE_NO_PAD.decode(signature) else { return false; };
    let expected = session_signature(key, expiry);
    if signature.len() != expected.len() { return false; }
    let difference = signature.iter().zip(expected.iter()).fold(0u8, |acc, (a, b)| acc | (*a ^ *b));
    difference == 0
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    let ip = client_ip_from_headers(&state, &headers, addr);
    let now = Instant::now();
    if let Some(retry_after) = state.admin_auth_guard.check(ip, now) {
        return locked_response(retry_after);
    }
    if !state.config.admin_key.is_empty() && body.key != state.config.admin_key {
        if let Some(retry_after) = state.admin_auth_guard.failed(ip, now) {
            return locked_response(retry_after);
        }
        return (StatusCode::UNAUTHORIZED, Json(json!({"detail": "Invalid admin key"}))).into_response();
    }
    state.admin_auth_guard.clear(ip);

    let mut response = Json(json!({"ok": true})).into_response();
    response.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if !state.config.admin_key.is_empty() {
        let expiry = chrono::Utc::now().timestamp() + SESSION_SECONDS;
        let value = format!("v1.{expiry}.{}", URL_SAFE_NO_PAD.encode(session_signature(&state.config.admin_key, expiry)));
        let secure = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https");
        let cookie = format!(
            "{SESSION_COOKIE}={value}; Path=/admin; Max-Age={SESSION_SECONDS}; HttpOnly; SameSite=Strict{}",
            if secure { "; Secure" } else { "" }
        );
        response.headers_mut().insert(SET_COOKIE, HeaderValue::from_str(&cookie).expect("valid cookie"));
    }
    response
}

pub async fn logout() -> Response {
    let mut response = Json(json!({"ok": true})).into_response();
    response.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_static("hifi_admin_session=; Path=/admin; Max-Age=0; HttpOnly; SameSite=Strict"),
    );
    response
}

pub async fn admin_auth(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let ip = client_ip(&state, &req, addr);
    let now = Instant::now();
    if let Some(retry_after) = state.admin_auth_guard.check(ip, now) {
        return Err(locked_response(retry_after));
    }
    let admin_key = req
        .headers()
        .get("X-Admin-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if state.config.admin_key.is_empty()
        || admin_key == state.config.admin_key
        || valid_session(req.headers(), &state.config.admin_key)
    {
        state.admin_auth_guard.clear(ip);
        let mut response = next.run(req).await;
        response.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return Ok(response);
    }

    if !admin_key.is_empty() {
        if let Some(retry_after) = state.admin_auth_guard.failed(ip, now) {
            return Err(locked_response(retry_after));
        }
    }

    Err((
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"detail": "Invalid or missing admin session"})),
    )
        .into_response())
}
