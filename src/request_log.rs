use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use axum::Json;
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header::RETRY_AFTER};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::net::{IpAddr, SocketAddr};

use crate::AppState;

const MAX_ENTRIES: usize = 5000;
const RATE_WINDOW_SECONDS: i64 = 60;
const SLOW_MS: u64 = 3000;
const STUCK_MS: u64 = 10_000;

#[derive(Clone)]
pub(crate) struct LoggedAccount {
    pub id: String,
    pub label: String,
    pub role: String,
}

type AccountTrace = Arc<Mutex<Vec<LoggedAccount>>>;

tokio::task_local! {
    static REQUEST_ACCOUNTS: AccountTrace;
}

/// Attach an account selection to the request currently being served. The
/// task-local scope keeps concurrent requests isolated without exposing any
/// credentials in response headers.
pub(crate) fn note_account(id: &str, label: &str, role: &str) {
    let _ = REQUEST_ACCOUNTS.try_with(|trace| {
        if let Ok(mut accounts) = trace.lock() {
            let duplicate = accounts
                .last()
                .is_some_and(|account| account.id == id && account.role == role);
            if !duplicate {
                accounts.push(LoggedAccount {
                    id: id.to_string(),
                    label: label.to_string(),
                    role: role.to_string(),
                });
            }
        }
    });
}

/// Run work with an isolated account trace and return every account selected
/// during it. Used by HTTP requests and by queued playback workers.
pub(crate) async fn capture_accounts<F, T>(future: F) -> (T, Vec<LoggedAccount>)
where
    F: Future<Output = T>,
{
    let trace = Arc::new(Mutex::new(Vec::new()));
    let output = REQUEST_ACCOUNTS.scope(trace.clone(), future).await;
    let accounts = trace.lock().map(|items| items.clone()).unwrap_or_default();
    (output, accounts)
}

fn should_log_path(path: &str) -> bool {
    path != "/admin" && !path.starts_with("/admin/") && path != "/health" && path != "/favicon.ico"
}

#[derive(Clone)]
pub struct LogEntry {
    pub ts: i64,
    pub method: String,
    pub path: String,
    /// Track/resource identifier when the request names one
    /// (path id for /track/{id}, /trackManifests/{id}, and /dash/{id},
    /// `id=` or `s=` query value otherwise). Empty when none.
    pub detail: String,
    pub status: u16,
    pub latency_ms: u64,
    pub client_ip: String,
    /// Cache verdict from the response, if this was a cacheable route.
    pub cache: String,
    /// Accounts selected while serving this request, in attempt order.
    pub(crate) accounts: Vec<LoggedAccount>,
}

pub struct RequestLog {
    entries: Mutex<VecDeque<LogEntry>>,
    requests_per_second: Mutex<VecDeque<(i64, u64)>>,
}

impl RequestLog {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(MAX_ENTRIES)),
            requests_per_second: Mutex::new(VecDeque::with_capacity(RATE_WINDOW_SECONDS as usize)),
        }
    }

    pub fn record(&self, entry: LogEntry) {
        let timestamp = entry.ts;
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= MAX_ENTRIES {
                entries.pop_front();
            }
            entries.push_back(entry);
        }
        if let Ok(mut buckets) = self.requests_per_second.lock() {
            buckets.retain(|(second, _)| *second > timestamp - RATE_WINDOW_SECONDS);
            if let Some((_, count)) = buckets.iter_mut().find(|(second, _)| *second == timestamp) {
                *count += 1;
            } else {
                buckets.push_back((timestamp, 1));
            }
        }
    }

    /// Completed API requests in the last 60 seconds, including the current second.
    pub fn requests_last_60s(&self) -> u64 {
        let now = chrono::Utc::now().timestamp();
        self.requests_per_second
            .lock()
            .map(|buckets| {
                buckets
                    .iter()
                    .filter(|(second, _)| *second > now - RATE_WINDOW_SECONDS && *second <= now)
                    .map(|(_, count)| count)
                    .sum()
            })
            .unwrap_or(0)
    }

    /// p95 of the bounded recent request log, not a lifetime latency metric.
    pub fn recent_p95_ms(&self) -> Option<u64> {
        let mut latencies: Vec<u64> = self
            .entries
            .lock()
            .ok()?
            .iter()
            .map(|e| e.latency_ms)
            .collect();
        if latencies.is_empty() {
            return None;
        }
        latencies.sort_unstable();
        let index = ((latencies.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
        Some(latencies[index])
    }

    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.entries
            .lock()
            .map(|e| e.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn summary(&self, limit: usize) -> Value {
        let entries = self.snapshot();
        let total = entries.len();

        let mut by_endpoint: HashMap<String, usize> = HashMap::new();
        let mut by_status: HashMap<String, usize> = HashMap::new();
        let mut by_ip: HashMap<String, usize> = HashMap::new();
        let mut by_track: HashMap<String, usize> = HashMap::new();
        let mut latencies: Vec<u64> = Vec::with_capacity(total);
        let mut errors: u64 = 0;
        let mut user_errors: u64 = 0;
        let mut upstream_errors: u64 = 0;

        for e in &entries {
            *by_endpoint.entry(e.path.clone()).or_default() += 1;
            *by_status.entry(e.status.to_string()).or_default() += 1;
            *by_ip.entry(e.client_ip.clone()).or_default() += 1;
            if !e.detail.is_empty() {
                *by_track.entry(e.detail.clone()).or_default() += 1;
            }
            latencies.push(e.latency_ms);
            if e.status >= 400 {
                errors += 1;
                if e.status == 429 || e.status >= 500 {
                    upstream_errors += 1;
                } else {
                    user_errors += 1;
                }
            }
        }

        latencies.sort_unstable();
        let pct = |p: f64| -> u64 {
            if latencies.is_empty() {
                return 0;
            }
            let idx = ((p * latencies.len() as f64) as usize).min(latencies.len() - 1);
            latencies[idx]
        };

        let mut top_endpoints: Vec<(String, usize)> = by_endpoint.into_iter().collect();
        top_endpoints.sort_by(|a, b| b.1.cmp(&a.1));
        let mut top_ips: Vec<(String, usize)> = by_ip.into_iter().collect();
        top_ips.sort_by(|a, b| b.1.cmp(&a.1));
        let mut top_tracks: Vec<(String, usize)> = by_track.into_iter().collect();
        top_tracks.sort_by(|a, b| b.1.cmp(&a.1));
        let mut seen = std::collections::HashSet::new();
        let mut by_latency: Vec<&LogEntry> = entries.iter().collect();
        by_latency.sort_by(|a, b| b.latency_ms.cmp(&a.latency_ms));
        let slowest: Vec<Value> = by_latency
            .into_iter()
            .filter(|e| seen.insert((e.path.clone(), e.detail.clone())))
            .take(5)
            .map(|e| json!({"endpoint": e.path, "detail": e.detail, "latency_ms": e.latency_ms}))
            .collect();

        let recent: Vec<Value> = entries
            .iter()
            .rev()
            .take(limit.min(MAX_ENTRIES))
            .map(|e| {
                json!({
                    "ts": e.ts,
                    "method": e.method,
                    "path": e.path,
                    "detail": e.detail,
                    "status": e.status,
                    "latency_ms": e.latency_ms,
                    "client_ip": e.client_ip,
                    "cache": e.cache,
                    "accounts": e.accounts.iter().map(|account| json!({
                        "id": account.id,
                        "label": account.label,
                        "role": account.role,
                    })).collect::<Vec<_>>(),
                    "slow": e.latency_ms >= SLOW_MS,
                })
            })
            .collect();

        json!({
            "total": total,
            "errors": errors,
            "user_errors": user_errors,
            "upstream_errors": upstream_errors,
            "slowest": slowest,
            "p50_ms": pct(0.5),
            "p95_ms": pct(0.95),
            "by_endpoint": top_endpoints.into_iter().take(20).map(|(k, v)| json!({"endpoint": k, "hits": v})).collect::<Vec<_>>(),
            "by_status": by_status,
            "top_ips": top_ips.into_iter().take(10).map(|(k, v)| json!({"ip": k, "hits": v})).collect::<Vec<_>>(),
            "top_tracks": top_tracks.into_iter().take(10).map(|(k, v)| json!({"id": k, "hits": v})).collect::<Vec<_>>(),
            "recent": recent,
        })
    }
}

impl Default for RequestLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Collapse `/trackManifests/192157851` → `/trackManifests/:id` to bound cardinality.
pub fn normalize_path(path: &str) -> String {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return "/".to_string();
    }
    if segs.len() <= 2 {
        return format!("/{}", segs.join("/"));
    }
    format!("/{}/{}", segs[0], segs[1])
}

/// Pull the song/resource identifier out of a request so the log shows
/// *what* was requested, not just the endpoint shape:
/// path id for /track/{id}, /trackManifests/{id}, and /dash/{id}; otherwise
/// the `id=` query value or search text (`s=`). Truncated to 64 chars.
fn extract_detail(path: &str, query: Option<&str>) -> String {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() >= 2 && (segs[0] == "track" || segs[0] == "trackManifests" || segs[0] == "dash") {
        return segs[1].chars().take(64).collect();
    }
    if let Some(q) = query {
        let mut search = None;
        for (k, v) in form_urlencoded::parse(q.as_bytes()) {
            if k == "id" && !v.is_empty() {
                return v.chars().take(64).collect();
            }
            if k == "s" && search.is_none() {
                search = Some(v.to_string());
            }
        }
        if let Some(s) = search {
            if !s.is_empty() {
                return s.chars().take(64).collect();
            }
        }
    }
    String::new()
}

pub async fn log_requests(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let raw_path = req.uri().path();
    let log_path = should_log_path(raw_path);
    let method = req.method().to_string();
    let raw_path = raw_path.to_string();
    let path = normalize_path(&raw_path);
    let detail = extract_detail(&raw_path, req.uri().query());
    let ip = client_ip(&state, &req, addr);
    let start = Instant::now();

    let (resp, accounts) = if let Some(retry_after) = state.scanner_guard.check(ip, &raw_path, start) {
        let mut response = (
            StatusCode::FORBIDDEN,
            Json(json!({ "detail": "IP temporarily blocked after repeated scanner requests" })),
        )
            .into_response();
        response.headers_mut().insert(
            RETRY_AFTER,
            HeaderValue::from_str(&retry_after.to_string()).expect("valid retry interval"),
        );
        (response, Vec::new())
    } else {
        capture_accounts(next.run(req)).await
    };
    if !log_path {
        return resp;
    }
    let status = resp.status().as_u16();
    let latency_ms = start.elapsed().as_millis() as u64;
    let cache = resp
        .headers()
        .get("X-Cache")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if latency_ms >= STUCK_MS {
        tracing::warn!(
            "Slow response: {} {} → {} in {}ms",
            method,
            path,
            status,
            latency_ms
        );
    }

    state.request_log.record(LogEntry {
        ts: chrono::Utc::now().timestamp(),
        method,
        path,
        detail,
        status,
        latency_ms,
        client_ip: ip.to_string(),
        cache,
        accounts,
    });

    resp
}

#[cfg(test)]
mod tests {
    use super::{LogEntry, RequestLog, should_log_path};

    fn entry(ts: i64, latency_ms: u64) -> LogEntry {
        LogEntry {
            ts,
            method: "GET".into(),
            path: "/track/:id".into(),
            detail: String::new(),
            status: 200,
            latency_ms,
            client_ip: "127.0.0.1".into(),
            cache: String::new(),
            accounts: Vec::new(),
        }
    }

    #[test]
    fn excludes_panel_and_service_requests() {
        for path in [
            "/admin",
            "/admin/",
            "/admin/stats",
            "/admin/requests",
            "/admin/accounts",
            "/health",
            "/favicon.ico",
        ] {
            assert!(!should_log_path(path), "{path}");
        }

        for path in [
            "/trackManifests/123",
            "/search/",
            "/administrator",
            "/healthcheck",
        ] {
            assert!(should_log_path(path), "{path}");
        }
    }

    #[test]
    fn rate_uses_sixty_seconds_even_after_log_eviction() {
        let log = RequestLog::new();
        let now = chrono::Utc::now().timestamp();
        log.record(entry(now - 60, 1));
        for _ in 0..5001 {
            log.record(entry(now, 10));
        }

        assert_eq!(log.snapshot().len(), 5000);
        assert_eq!(log.requests_last_60s(), 5001);
    }

    #[test]
    fn recent_p95_uses_nearest_rank_and_is_empty_without_requests() {
        let log = RequestLog::new();
        assert_eq!(log.recent_p95_ms(), None);
        let now = chrono::Utc::now().timestamp();
        for latency in 1..=100 {
            log.record(entry(now, latency));
        }
        assert_eq!(log.recent_p95_ms(), Some(95));
    }

    #[test]
    fn summary_separates_error_classes_and_marks_slow_cache_hits() {
        let log = RequestLog::new();
        let now = chrono::Utc::now().timestamp();
        let mut ok = entry(now, 3500);
        ok.path = "/search/".into();
        ok.cache = "STALE".into();
        log.record(ok);
        let mut user = entry(now, 10);
        user.status = 404;
        log.record(user);
        let mut throttle = entry(now, 20);
        throttle.status = 429;
        log.record(throttle);
        let mut server = entry(now, 30);
        server.status = 503;
        log.record(server);

        let summary = log.summary(4);
        assert_eq!(summary["errors"], 3);
        assert_eq!(summary["user_errors"], 1);
        assert_eq!(summary["upstream_errors"], 2);
        assert_eq!(summary["slowest"][0]["endpoint"], "/search/");
        assert_eq!(summary["recent"][3]["cache"], "STALE");
        assert_eq!(summary["recent"][3]["slow"], true);
    }
}

pub(crate) fn client_ip(state: &AppState, req: &Request<Body>, fallback: SocketAddr) -> IpAddr {
    client_ip_from_headers(state, req.headers(), fallback)
}

pub(crate) fn client_ip_from_headers(
    state: &AppState,
    headers: &HeaderMap,
    fallback: SocketAddr,
) -> IpAddr {
    if state.config.trust_proxy {
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            if let Some(first) = xff.split(',').next().map(|s| s.trim()) {
                if let Ok(ip) = first.parse::<IpAddr>() {
                    return ip.to_canonical();
                }
            }
        }
        if let Some(real) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
            if let Ok(ip) = real.trim().parse::<IpAddr>() {
                return ip.to_canonical();
            }
        }
    }
    fallback.ip().to_canonical()
}
