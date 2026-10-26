use std::fmt;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, Clone)]
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    UpstreamError(StatusCode, String),
    RateLimited(u64),
    /// Tidal did not answer in time. Sent as 504.
    Timeout,
    /// Tidal or the account's proxy could not be reached. Sent as 502.
    BadGateway(String),
    /// Every account that tried got only a 30-second preview; retrying does
    /// not help. Sent as 403 with `code: "preview_only"`.
    PreviewOnly(String),
    /// No account can serve at all (none active, all waiting for a token).
    /// Sent as 503.
    ServiceUnavailable(String),
    ServiceUnavailableRetry(String, u64),
    /// No free slot: every account is at its concurrency limit or resting,
    /// or the playback queue is full. The client should retry after the
    /// given seconds. Sent as 429 with `code: "accounts_busy"`.
    AccountsBusy(String, u64),
    Internal(String),
}

impl AppError {
    /// The HTTP status this error is sent with. Tidal's 4xx pass through;
    /// its 5xx and unreadable 2xx are 502, since the failure is upstream.
    pub fn status(&self) -> StatusCode {
        match self {
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::UpstreamError(s, _) if !s.is_client_error() => StatusCode::BAD_GATEWAY,
            AppError::UpstreamError(s, _) => *s,
            AppError::RateLimited(_) | AppError::AccountsBusy(..) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Timeout => StatusCode::GATEWAY_TIMEOUT,
            AppError::BadGateway(_) => StatusCode::BAD_GATEWAY,
            AppError::PreviewOnly(_) => StatusCode::FORBIDDEN,
            AppError::ServiceUnavailable(_) | AppError::ServiceUnavailableRetry(..) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The error to report when no further account could be leased: a busy
    /// pool is the actual reason and wins over the previous account's failure.
    pub fn after_failover(self, previous: Option<AppError>) -> AppError {
        match self {
            AppError::AccountsBusy(..) => self,
            other => previous.unwrap_or(other),
        }
    }
}

/// Starts of the `AccountsBusy` messages, so logs and stats can tell our own
/// "no free slot" 429 apart from one sent by Tidal.
pub const ACCOUNTS_BUSY: &str = "All playback accounts are busy";
pub const QUEUE_FULL: &str = "Playback queue full";
pub const CATALOG_BUSY: &str = "Catalog accounts are at their concurrent request limit";

pub fn is_accounts_busy(detail: &str) -> bool {
    [ACCOUNTS_BUSY, QUEUE_FULL, CATALOG_BUSY].iter().any(|start| detail.starts_with(start))
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let (detail, retry_after) = match self {
            AppError::NotFound(d)
            | AppError::BadRequest(d)
            | AppError::Unauthorized(d)
            | AppError::UpstreamError(_, d)
            | AppError::BadGateway(d)
            | AppError::ServiceUnavailable(d)
            | AppError::Internal(d) => (d, None),
            AppError::RateLimited(secs) => ("Tidal rate limited the account".into(), Some(secs)),
            AppError::Timeout => ("Upstream timeout".into(), None),
            AppError::ServiceUnavailableRetry(d, secs) => (d, (secs > 0).then_some(secs)),
            AppError::PreviewOnly(d) => {
                return (status, Json(json!({"detail": d, "code": "preview_only"}))).into_response();
            }
            AppError::AccountsBusy(d, secs) => {
                let mut resp = (
                    status,
                    Json(json!({"detail": d, "code": "accounts_busy", "retry_after": secs})),
                )
                    .into_response();
                resp.headers_mut().insert(
                    axum::http::header::RETRY_AFTER,
                    secs.max(1).to_string().parse().unwrap(),
                );
                return resp;
            }
        };

        let mut resp = (status, Json(json!({"detail": detail}))).into_response();
        if let Some(secs) = retry_after {
            resp.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                secs.to_string().parse().unwrap(),
            );
        }
        resp
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!("Database error: {:?}", e);
        AppError::Internal("Database error".into())
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NotFound(d) => write!(f, "Not found: {}", d),
            AppError::BadRequest(d) => write!(f, "Bad request: {}", d),
            AppError::Unauthorized(d) => write!(f, "Unauthorized: {}", d),
            AppError::UpstreamError(s, d) => write!(f, "Upstream {}: {}", s, d),
            AppError::RateLimited(s) => write!(f, "Tidal rate limited the account; retry in {}s", s),
            AppError::Timeout => write!(f, "Timeout"),
            AppError::BadGateway(d) => write!(f, "Bad gateway: {}", d),
            AppError::PreviewOnly(d) => write!(f, "{}", d),
            AppError::ServiceUnavailable(d) => write!(f, "Service unavailable: {}", d),
            AppError::ServiceUnavailableRetry(d, s) => {
                write!(f, "Service unavailable: {} (retry in {}s)", d, s)
            }
            AppError::AccountsBusy(d, _) => write!(f, "{}", d),
            AppError::Internal(d) => write!(f, "Internal error: {}", d),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retry_after_header_present_when_secs_positive() {
        let resp = AppError::ServiceUnavailableRetry("busy".into(), 137).into_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            resp.headers().get("retry-after").unwrap(),
            "137"
        );
    }

    #[tokio::test]
    async fn retry_after_header_omitted_when_zero() {
        let resp = AppError::ServiceUnavailableRetry("busy".into(), 0).into_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(resp.headers().get("retry-after").is_none());
    }

    #[tokio::test]
    async fn busy_accounts_are_429_with_a_code_and_retry_after() {
        let resp = AppError::AccountsBusy(format!("{ACCOUNTS_BUSY}: retry"), 2).into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(resp.headers().get("retry-after").unwrap(), "2");
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "accounts_busy");
        assert!(body["detail"].as_str().unwrap().starts_with(ACCOUNTS_BUSY));
    }

    #[tokio::test]
    async fn network_failures_are_gateway_errors_not_503() {
        assert_eq!(AppError::Timeout.into_response().status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(AppError::BadGateway("x".into()).into_response().status(), StatusCode::BAD_GATEWAY);
        let tidal_503 = AppError::UpstreamError(StatusCode::SERVICE_UNAVAILABLE, "Upstream API error".into());
        assert_eq!(tidal_503.into_response().status(), StatusCode::BAD_GATEWAY);
        let tidal_403 = AppError::UpstreamError(StatusCode::FORBIDDEN, "Upstream API error".into());
        assert_eq!(tidal_403.into_response().status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn preview_only_is_403_with_a_code() {
        let resp = AppError::PreviewOnly("Preview only: track 5".into()).into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(resp.headers().get("retry-after").is_none());
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "preview_only");
    }

    #[tokio::test]
    async fn upstream_rate_limit_is_429_with_retry_after() {
        let resp = AppError::RateLimited(42).into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(resp.headers().get("retry-after").unwrap(), "42");
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            AppError::Timeout
        } else if e.is_connect() {
            AppError::BadGateway("Connection error to Tidal".into())
        } else if let Some(status) = e.status() {
            AppError::UpstreamError(status, format!("Upstream API error: {}", status))
        } else {
            AppError::Internal(format!("Request error: {}", e))
        }
    }
}
