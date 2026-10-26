use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::Json;
use axum::body::{Body, HttpBody};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header::RETRY_AFTER};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::{Value, json};
use std::net::{IpAddr, SocketAddr};

use crate::AppState;

const MAX_ENTRIES: usize = 5000;
const RATE_WINDOW_SECONDS: i64 = 60;
/// Completed seconds averaged into the "now" rate. One second alone is too
/// jumpy to read at low traffic.
const RATE_NOW_SECONDS: i64 = 5;
const SLOW_MS: u64 = 3000;
const STUCK_MS: u64 = 10_000;
/// Error responses listed with their explanation in the admin overview.
const RECENT_ERRORS: usize = 50;
/// Largest error body read back for its `detail` message.
const MAX_ERROR_BODY: u64 = 16 * 1024;
const MAX_ERROR_DETAIL: usize = 300;

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
    /// Turned away before any handler ran, so Tidal was never involved.
    pub rejected: bool,
    /// The `detail` message of an error response. Empty when it carried none.
    pub error: String,
}

impl LogEntry {
    /// An error of the API itself. A refused client or a probe of an unknown
    /// path says nothing about the service.
    fn is_api_error(&self) -> bool {
        self.status >= 400
            && !self.rejected
            && (Traffic::of(self) != Traffic::Service || self.path.starts_with("/playback/"))
    }

    fn to_json(&self) -> Value {
        json!({
            "ts": self.ts,
            "method": self.method,
            "path": self.path,
            "detail": self.detail,
            "status": self.status,
            "latency_ms": self.latency_ms,
            "client_ip": self.client_ip,
            "cache": self.cache,
            "accounts": self.accounts.iter().map(|account| json!({
                "id": account.id,
                "label": account.label,
                "role": account.role,
            })).collect::<Vec<_>>(),
            "slow": self.latency_ms >= SLOW_MS,
            "error": self.error,
        })
    }
}

/// Put on a response refused before its handler ran (blocked or unauthorized client).
#[derive(Clone, Copy)]
pub(crate) struct Rejected;

/// What serving a request asked of Tidal. Only the first three reach it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Traffic {
    /// Routes served through the playback queue.
    Playback,
    Search,
    /// Every other metadata route.
    Catalog,
    /// A Tidal route answered from the response cache.
    Cached,
    /// Queue polling, the index, unknown paths and rejected clients.
    Service,
}

impl Traffic {
    fn of(entry: &LogEntry) -> Self {
        if entry.rejected {
            return Traffic::Service;
        }
        let class = match entry.path.split('/').find(|segment| !segment.is_empty()) {
            Some("track" | "trackManifests" | "dash" | "video" | "widevine") => Traffic::Playback,
            Some("search") => Traffic::Search,
            Some(
                "info" | "album" | "artist" | "mix" | "playlist" | "cover" | "lyrics"
                | "recommendations" | "topvideos",
            ) => Traffic::Catalog,
            _ => return Traffic::Service,
        };
        if matches!(entry.cache.as_str(), "HIT" | "STALE" | "NEGATIVE") {
            Traffic::Cached
        } else {
            class
        }
    }
}

/// Requests completed within one second, split by the traffic classes the
/// overview reports separately.
#[derive(Clone, Copy, Default)]
struct SecondCounts {
    playback: u64,
    search: u64,
    catalog: u64,
    cached: u64,
    /// 429 responses on Tidal routes: fresh, repeated from the cache, or returned
    /// while every account is paused.
    throttled: u64,
}

impl SecondCounts {
    fn of(entry: &LogEntry) -> Self {
        let traffic = Traffic::of(entry);
        // Every call to Tidal picks an account first. A request that picked none was
        // answered locally (bad parameters, a full queue, no usable account), except
        // a queued playback job, which picks its account once a slot frees up.
        let queued = traffic == Traffic::Playback && entry.status == StatusCode::ACCEPTED.as_u16();
        let sent = !entry.accounts.is_empty() || queued;
        Self {
            playback: u64::from(sent && traffic == Traffic::Playback),
            search: u64::from(sent && traffic == Traffic::Search),
            catalog: u64::from(sent && traffic == Traffic::Catalog),
            cached: u64::from(traffic == Traffic::Cached),
            // Our own "every account is busy" answer is not Tidal throttling.
            throttled: u64::from(
                traffic != Traffic::Service
                    && entry.status == 429
                    && !crate::error::is_accounts_busy(&entry.error),
            ),
        }
    }

    fn add(&mut self, other: SecondCounts) {
        self.playback += other.playback;
        self.search += other.search;
        self.catalog += other.catalog;
        self.cached += other.cached;
        self.throttled += other.throttled;
    }

    /// Requests that were sent on to Tidal.
    fn upstream(&self) -> u64 {
        self.playback + self.search + self.catalog
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct RequestRate {
    /// Requests per second over the last `RATE_NOW_SECONDS` completed seconds.
    pub now: f64,
    /// Requests per second over the last 60 seconds.
    pub avg_60s: f64,
    /// Requests in the busiest of the last 60 completed seconds.
    pub peak_60s: u64,
}

/// Only requests that reach Tidal count toward `all`; `cached` is reported beside it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct RequestRates {
    pub all: RequestRate,
    pub playback: RequestRate,
    pub search: RequestRate,
    pub catalog: RequestRate,
    pub cached: RequestRate,
    /// 429 responses on Tidal routes over the last 60 seconds.
    pub throttled_60s: u64,
    /// Requests sent to Tidal in each of the last 60 completed seconds, oldest
    /// first, as `[playback, search, catalog]`.
    pub history: Vec<[u64; 3]>,
}

pub struct RequestLog {
    entries: Mutex<VecDeque<LogEntry>>,
    requests_per_second: Mutex<VecDeque<(i64, SecondCounts)>>,
    total_requests: AtomicU64,
}

impl RequestLog {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(MAX_ENTRIES)),
            requests_per_second: Mutex::new(VecDeque::with_capacity(RATE_WINDOW_SECONDS as usize)),
            total_requests: AtomicU64::new(0),
        }
    }

    pub fn record(&self, entry: LogEntry) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        let timestamp = entry.ts;
        let counted = SecondCounts::of(&entry);
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= MAX_ENTRIES {
                entries.pop_front();
            }
            entries.push_back(entry);
        }
        if let Ok(mut buckets) = self.requests_per_second.lock() {
            buckets.retain(|(second, _)| *second > timestamp - RATE_WINDOW_SECONDS);
            if let Some((_, counts)) = buckets.iter_mut().find(|(second, _)| *second == timestamp) {
                counts.add(counted);
            } else {
                buckets.push_back((timestamp, counted));
            }
        }
    }

    /// Completed API requests recorded since this process started.
    pub fn total_requests(&self) -> u64 {
        self.total_requests.load(Ordering::Relaxed)
    }

    /// Requests per second sent to Tidal: right now and averaged over the
    /// last 60 seconds, in total and per traffic class.
    pub fn request_rates(&self) -> RequestRates {
        self.request_rates_at(chrono::Utc::now().timestamp())
    }

    /// The 60-second window includes the current second. "Now" covers only
    /// completed seconds, so a second still filling up does not understate it.
    fn request_rates_at(&self, now: i64) -> RequestRates {
        let mut window = SecondCounts::default();
        let mut recent = SecondCounts::default();
        let mut peak = SecondCounts::default();
        let mut peak_upstream = 0;
        let mut history = vec![[0; 3]; RATE_WINDOW_SECONDS as usize];
        if let Ok(buckets) = self.requests_per_second.lock() {
            for (second, counts) in buckets.iter() {
                if *second > now - RATE_WINDOW_SECONDS && *second <= now {
                    window.add(*counts);
                }
                if *second >= now - RATE_NOW_SECONDS && *second < now {
                    recent.add(*counts);
                }
                if *second >= now - RATE_WINDOW_SECONDS && *second < now {
                    history[(*second - (now - RATE_WINDOW_SECONDS)) as usize] =
                        [counts.playback, counts.search, counts.catalog];
                    // Each class peaks in its own second, so the total is not their sum.
                    peak.playback = peak.playback.max(counts.playback);
                    peak.search = peak.search.max(counts.search);
                    peak.catalog = peak.catalog.max(counts.catalog);
                    peak.cached = peak.cached.max(counts.cached);
                    peak_upstream = peak_upstream.max(counts.upstream());
                }
            }
        }
        let rate = |recent: u64, window: u64, peak_60s: u64| RequestRate {
            now: recent as f64 / RATE_NOW_SECONDS as f64,
            avg_60s: window as f64 / RATE_WINDOW_SECONDS as f64,
            peak_60s,
        };
        RequestRates {
            all: rate(recent.upstream(), window.upstream(), peak_upstream),
            playback: rate(recent.playback, window.playback, peak.playback),
            search: rate(recent.search, window.search, peak.search),
            catalog: rate(recent.catalog, window.catalog, peak.catalog),
            cached: rate(recent.cached, window.cached, peak.cached),
            throttled_60s: window.throttled,
            history,
        }
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
        latencies.sort_unstable();
        percentile(&latencies, 0.95)
    }

    /// Response time percentiles of successful Tidal requests in the bounded
    /// log, per traffic class. Cache hits and queued (202) answers say nothing
    /// about Tidal's speed and are left out.
    pub fn latency_by_class(&self) -> Value {
        let mut search = Vec::new();
        let mut catalog = Vec::new();
        if let Ok(entries) = self.entries.lock() {
            for entry in entries.iter() {
                if entry.status >= 400 || entry.status == StatusCode::ACCEPTED.as_u16() {
                    continue;
                }
                match Traffic::of(entry) {
                    Traffic::Search => search.push(entry.latency_ms),
                    Traffic::Catalog => catalog.push(entry.latency_ms),
                    _ => {}
                }
            }
        }
        let summary = |mut values: Vec<u64>| {
            values.sort_unstable();
            json!({
                "count": values.len(),
                "p50_ms": percentile(&values, 0.5),
                "p95_ms": percentile(&values, 0.95),
                "p99_ms": percentile(&values, 0.99),
            })
        };
        json!({ "search": summary(search), "catalog": summary(catalog) })
    }

    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.entries
            .lock()
            .map(|e| e.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Client-visible API outcomes in the bounded request log. Account
    /// failovers are deliberately counted once, as one incoming request.
    pub fn outcome_counts(&self) -> (u64, u64) {
        self.entries
            .lock()
            .map(|entries| {
                let total = entries.len() as u64;
                let errors = entries.iter().filter(|entry| entry.status >= 400).count() as u64;
                (total, errors)
            })
            .unwrap_or((0, 0))
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
            .map(LogEntry::to_json)
            .collect();
        let recent_errors: Vec<Value> = entries
            .iter()
            .rev()
            .filter(|e| e.is_api_error())
            .take(RECENT_ERRORS)
            .map(LogEntry::to_json)
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
            "recent_errors": recent_errors,
        })
    }
}

/// Nearest-rank percentile of an ascending list; None when it is empty.
pub fn percentile(sorted: &[u64], p: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let index = ((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1;
    Some(sorted[index])
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

/// Read the `detail` message of an error response that is already buffered
/// and small, handing the response back unchanged. Streamed and large bodies
/// are left alone.
async fn take_error_detail(resp: Response) -> (Response, String) {
    let size = resp.body().size_hint().exact();
    let Some(size) = size.filter(|size| resp.status().as_u16() >= 400 && *size <= MAX_ERROR_BODY)
    else {
        return (resp, String::new());
    };
    let (parts, body) = resp.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, size as usize).await else {
        return (
            (StatusCode::BAD_GATEWAY, "Upstream body read failed").into_response(),
            String::new(),
        );
    };
    let detail = match serde_json::from_slice::<Value>(&bytes) {
        Ok(body) => body.get("detail").and_then(Value::as_str).unwrap_or_default().to_string(),
        // Axum's own rejections (a malformed path or query) are plain text.
        Err(_) => String::from_utf8_lossy(&bytes).trim().to_string(),
    };
    let detail = detail.chars().take(MAX_ERROR_DETAIL).collect();
    (Response::from_parts(parts, Body::from(bytes)), detail)
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
        response.extensions_mut().insert(Rejected);
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
    let (resp, error) = take_error_detail(resp).await;
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
        rejected: resp.extensions().get::<Rejected>().is_some(),
        error,
    });

    resp
}

#[cfg(test)]
mod tests {
    use super::{
        LogEntry, LoggedAccount, RequestLog, RequestRate, Traffic, should_log_path,
        take_error_detail,
    };

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
            rejected: false,
            error: String::new(),
        }
    }

    /// A request that picked an account, as every call to Tidal does.
    fn served(ts: i64, path: &str) -> LogEntry {
        let mut item = entry(ts, 1);
        item.path = path.into();
        item.accounts.push(LoggedAccount {
            id: "a".into(),
            label: "A".into(),
            role: "playback".into(),
        });
        item
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
        log.record(served(now - 60, "/track/:id"));
        for _ in 0..5001 {
            log.record(served(now, "/track/:id"));
        }

        assert_eq!(log.snapshot().len(), 5000);
        assert_eq!(log.total_requests(), 5002);
        assert_eq!(log.request_rates_at(now).all.avg_60s, 5001.0 / 60.0);
    }

    #[test]
    fn rates_split_playback_and_search_into_now_and_average() {
        let log = RequestLog::new();
        let now = 1_000_000;
        let record = |ts: i64, path: &str, times: usize| {
            for _ in 0..times {
                log.record(served(ts, path));
            }
        };
        // Older than the "now" window, still inside the 60-second average.
        record(now - 30, "/track/1", 6);
        record(now - 30, "/search", 12);
        record(now - 30, "/album", 3);
        // Completed seconds that make up "now".
        record(now - 5, "/trackManifests/7", 2);
        record(now - 2, "/widevine", 1);
        record(now - 1, "/video", 2);
        record(now - 1, "/search", 10);
        // Polling a queued job reports its account, but neither it nor unknown paths call Tidal.
        record(now - 1, "/playback/requests", 4);
        record(now - 1, "/", 2);
        record(now - 1, "/wp-login.php", 5);
        // The second still in progress counts toward the average only.
        record(now, "/dash/9", 3);
        record(now, "/search", 3);

        let rates = log.request_rates_at(now);
        // Peaks cover completed seconds only, and the total peaks in its own busiest second.
        assert_eq!(rates.playback, RequestRate { now: 1.0, avg_60s: 14.0 / 60.0, peak_60s: 6 });
        assert_eq!(rates.search, RequestRate { now: 2.0, avg_60s: 25.0 / 60.0, peak_60s: 12 });
        assert_eq!(rates.catalog, RequestRate { now: 0.0, avg_60s: 3.0 / 60.0, peak_60s: 3 });
        assert_eq!(rates.all, RequestRate { now: 15.0 / 5.0, avg_60s: 42.0 / 60.0, peak_60s: 21 });
        // One slot per completed second, oldest first; the current second is left out.
        assert_eq!(rates.history.len(), 60);
        assert_eq!(rates.history[30], [6, 12, 3]);
        assert_eq!(rates.history[55], [2, 0, 0]);
        assert_eq!(rates.history[59], [2, 10, 0]);
        assert_eq!(rates.history.iter().flatten().sum::<u64>(), 36);
        // Service requests stay in the request log and the lifetime total.
        assert_eq!(log.total_requests(), 53);

        // Six seconds later everything recorded so far has left "now".
        let later = log.request_rates_at(now + 6);
        assert_eq!(later.all.now, 0.0);
        assert_eq!(later.all.avg_60s, 42.0 / 60.0);
        assert_eq!(later.history[54], [3, 3, 0]);
        // A second that scrolled out of the chart no longer holds the peak.
        assert_eq!(log.request_rates_at(now + 31).all.peak_60s, 12);
    }

    #[test]
    fn cached_and_rejected_requests_do_not_count_as_tidal_traffic() {
        let classify = |path: &str, cache: &str, rejected: bool| {
            let mut item = entry(0, 1);
            item.path = path.into();
            item.cache = cache.into();
            item.rejected = rejected;
            Traffic::of(&item)
        };
        assert_eq!(classify("/search/", "MISS", false), Traffic::Search);
        assert_eq!(classify("/search/", "SKIP", false), Traffic::Search);
        for verdict in ["HIT", "STALE", "NEGATIVE"] {
            assert_eq!(classify("/search/", verdict, false), Traffic::Cached, "{verdict}");
        }
        assert_eq!(classify("/track/:id", "", false), Traffic::Playback);
        assert_eq!(classify("/track/:id", "", true), Traffic::Service);
        for path in ["/album/similar", "/artist", "/cover", "/lyrics", "/info", "/topvideos"] {
            assert_eq!(classify(path, "", false), Traffic::Catalog, "{path}");
        }
        for path in ["/", "/playback/requests", "/robots.txt", "/searching"] {
            assert_eq!(classify(path, "", false), Traffic::Service, "{path}");
        }

        let log = RequestLog::new();
        let now = 1_000_000;
        let record = |path: &str, cache: &str, status: u16, picked_account: bool, rejected: bool| {
            let mut item = if picked_account { served(now - 1, path) } else { entry(now - 1, 1) };
            item.path = path.into();
            item.cache = cache.into();
            item.status = status;
            item.rejected = rejected;
            log.record(item);
        };
        record("/search", "MISS", 200, true, false);
        record("/search", "MISS", 429, true, false);
        record("/search", "HIT", 200, false, false);
        record("/search", "NEGATIVE", 429, false, false);
        // A client refused before its handler is neither load nor a Tidal 429.
        record("/search", "", 429, false, true);
        // Answered without picking an account: bad parameters, a full queue.
        record("/search", "SKIP", 400, false, false);
        // Our own "no free slot" 429s are not Tidal throttling either.
        for detail in [
            "Playback queue full (4/4); retry shortly",
            "Catalog accounts are at their concurrent request limit",
            "All playback accounts are busy: retry shortly",
        ] {
            let mut item = entry(now - 1, 1);
            item.path = "/track/:id".into();
            item.status = 429;
            item.error = detail.into();
            log.record(item);
        }
        // Every account paused: nothing is sent, yet the client still sees Tidal's limit.
        record("/album", "SKIP", 429, false, false);
        // A queued playback job reaches Tidal once a slot frees up; only playback queues.
        record("/track/:id", "", 202, false, false);
        record("/search", "", 202, false, false);
        let rates = log.request_rates_at(now);
        assert_eq!(rates.search.avg_60s, 2.0 / 60.0);
        assert_eq!(rates.playback.avg_60s, 1.0 / 60.0);
        assert_eq!(rates.catalog.avg_60s, 0.0);
        assert_eq!(rates.all.avg_60s, 3.0 / 60.0);
        assert_eq!(rates.cached.avg_60s, 2.0 / 60.0);
        assert_eq!(rates.cached.peak_60s, 2);
        assert_eq!(rates.all.peak_60s, 3);
        assert_eq!(rates.throttled_60s, 3);
        assert_eq!(rates.history[59], [1, 2, 0]);
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
    fn latency_by_class_counts_only_successful_tidal_answers() {
        let log = RequestLog::new();
        let now = chrono::Utc::now().timestamp();
        for latency in 1..=100 {
            let mut item = entry(now, latency);
            item.path = "/search/".into();
            item.cache = "MISS".into();
            log.record(item);
        }
        for (path, cache, status, latency) in [
            ("/search/", "HIT", 200, 1),        // from cache
            ("/search/", "MISS", 500, 99_999), // failed
            ("/album/", "MISS", 200, 40),
            ("/album/", "MISS", 202, 1),       // queued, not finished
        ] {
            let mut item = entry(now, latency);
            item.path = path.into();
            item.cache = cache.into();
            item.status = status;
            log.record(item);
        }
        let latency = log.latency_by_class();
        assert_eq!(latency["search"]["count"], 100);
        assert_eq!(latency["search"]["p50_ms"], 50);
        assert_eq!(latency["search"]["p95_ms"], 95);
        assert_eq!(latency["search"]["p99_ms"], 99);
        assert_eq!(latency["catalog"]["count"], 1);
        assert_eq!(latency["catalog"]["p99_ms"], 40);
        assert_eq!(super::percentile(&[], 0.5), None);
    }

    #[test]
    fn outcome_counts_each_incoming_request_once() {
        let log = RequestLog::new();
        let now = chrono::Utc::now().timestamp();
        log.record(entry(now, 10));
        let mut failed = entry(now, 20);
        failed.status = 409;
        log.record(failed);
        assert_eq!(log.outcome_counts(), (2, 1));
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

    #[test]
    fn recent_errors_keep_the_newest_api_failures_with_reason_and_accounts() {
        let log = RequestLog::new();
        let record = |ts: i64, path: &str, status: u16, rejected: bool| {
            let mut item = entry(ts, 10);
            item.path = path.into();
            item.status = status;
            item.rejected = rejected;
            item.detail = ts.to_string();
            log.record(item);
        };
        for ts in 0..60 {
            record(ts, "/track/:id", 503, false);
            record(ts, "/track/:id", 200, false);
        }
        // Not failures of the service: a scanner probe and a refused client.
        record(60, "/wp-login.php", 404, false);
        record(61, "/track/:id", 401, true);
        // A queued playback job reports its failure on the polling route.
        record(62, "/playback/requests", 409, false);
        let mut failed = entry(63, 1514);
        failed.status = 503;
        failed.error = "Preview only for track".into();
        failed.accounts = vec![LoggedAccount {
            id: "abc".into(),
            label: "Main".into(),
            role: "playback".into(),
        }];
        log.record(failed);

        let summary = log.summary(1);
        let errors = summary["recent_errors"].as_array().unwrap();
        assert_eq!(errors.len(), 50);
        assert_eq!(errors[0]["error"], "Preview only for track");
        assert_eq!(errors[0]["latency_ms"], 1514);
        assert_eq!(errors[0]["accounts"][0]["label"], "Main");
        assert_eq!(errors[1]["path"], "/playback/requests");
        assert_eq!(errors[2]["detail"], "59");
        assert_eq!(errors[49]["detail"], "12");
        assert!(errors.iter().all(|e| e["status"] != 200 && e["status"] != 401 && e["status"] != 404));
        // The list is independent of how much of the log was asked for.
        assert_eq!(summary["recent"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn error_detail_is_read_without_changing_the_response() {
        use axum::body::{Body, to_bytes};
        use axum::response::IntoResponse;

        let failed = crate::error::AppError::ServiceUnavailableRetry("Playback queue full".into(), 5);
        let (resp, detail) = take_error_detail(failed.into_response()).await;
        assert_eq!(detail, "Playback queue full");
        assert_eq!(resp.status(), 503);
        assert_eq!(resp.headers().get("retry-after").unwrap(), "5");
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], br#"{"detail":"Playback queue full"}"#);

        // Long messages are cut for the log only; a plain-text body is the message itself.
        let long = crate::error::AppError::Internal("é".repeat(400)).into_response();
        assert_eq!(take_error_detail(long).await.1, "é".repeat(300));
        let plain = (axum::http::StatusCode::BAD_REQUEST, "Invalid URL\n").into_response();
        assert_eq!(take_error_detail(plain).await.1, "Invalid URL");
        let other = (axum::http::StatusCode::GONE, axum::Json(serde_json::json!({"status": "x"})));
        assert_eq!(take_error_detail(other.into_response()).await.1, "");

        // Successful and streamed responses are passed through unread.
        let ok = axum::Json(serde_json::json!({"detail": "fine"})).into_response();
        assert_eq!(take_error_detail(ok).await.1, "");
        let chunks = futures::stream::iter([Ok::<_, std::io::Error>(r#"{"detail":"streamed"}"#)]);
        let streamed = (axum::http::StatusCode::BAD_GATEWAY, Body::from_stream(chunks)).into_response();
        let (resp, detail) = take_error_detail(streamed).await;
        assert_eq!(detail, "");
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], br#"{"detail":"streamed"}"#);
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
