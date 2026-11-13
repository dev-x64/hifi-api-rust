use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use moka::future::Cache;
use tokio::sync::Mutex;

use crate::AppState;

const HOUR_SECS: u64 = 3600;
const DAY_SECS: u64 = 24 * HOUR_SECS;
/// Fresh period of metadata routes other than search, playlists and mixes,
/// and the cap on the stale window that follows any fresh period.
const MAX_FRESH_SECS: u64 = 12 * HOUR_SECS;
/// Text search: Tidal's answer to a query changes slowly.
const SEARCH_FRESH_SECS: u64 = DAY_SECS;
/// ISRC lookup (`/search/?i=`): the tracks of a recording almost never change.
const ISRC_FRESH_SECS: u64 = 7 * DAY_SECS;
/// A search that found nothing is asked again soon, with no stale window,
/// so a fresh release does not stay "not found".
const EMPTY_SEARCH_FRESH_SECS: u64 = 15 * 60;
/// Hard memory bound for cached bodies (moka weight = body bytes).
const MAX_CACHE_BYTES: u64 = 256 * 1024 * 1024;
/// Hard memory bound for negatively cached error bodies.
const MAX_NEG_BYTES: u64 = 32 * 1024 * 1024;
/// Hard TTL ceiling for negative entries (manual per-status expiry inside
/// is authoritative; this just sweeps).
const NEG_CACHE_TTL_SECS: u64 = 300;
/// Safety cap for a single cached body. Our metadata endpoints return small
/// JSON; anything bigger passes through uncached (see below).
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// How long a verdict on a substituted track id is reused.
const REPLACEMENT_TTL_SECS: u64 = 6 * 3600;

/// Metadata prefixes safe to cache. Playback routes (/track, /trackManifests,
/// /dash, /widevine, /video, /topvideos) and /admin are NEVER cached — their
/// URLs/tokens expire and mutations must stay fresh.
fn cacheable(path: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "/info/",
        "/search/",
        "/album/",
        "/artist/",
        "/mix/",
        "/playlist/",
        "/cover/",
        "/lyrics/",
        "/recommendations/",
    ];
    PREFIXES.iter().any(|p| path.starts_with(p))
}

/// Canonical cache key for a raw query string: percent-decode every pair
/// then sort, so `?s=a+b`, `?s=a%20b` and `?b=2&a=1`-style variants of the
/// same logical request share one entry instead of fragmenting the cache.
/// Decoding is safe here because the handlers decode identically before
/// responding. Pure function — unit tested.
fn normalize_query(query: Option<&str>) -> String {
    let q = query.unwrap_or("");
    if q.is_empty() {
        return String::new();
    }
    let mut pairs: Vec<(String, String)> = form_urlencoded::parse(q.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if pairs.len() < 2 {
        // Fast path still canonicalizes encoding (`a+b` == `a%20b`).
        let mut ser = form_urlencoded::Serializer::new(String::new());
        for (k, v) in &pairs {
            ser.append_pair(k, v);
        }
        return ser.finish();
    }
    pairs.sort();
    let mut ser = form_urlencoded::Serializer::new(String::new());
    for (k, v) in &pairs {
        ser.append_pair(k, v);
    }
    ser.finish()
}

/// ISRC lookup: `/search/` with a non-blank `i`, the way the handler reads it.
fn is_isrc_lookup(normalized_query: &str) -> bool {
    form_urlencoded::parse(normalized_query.as_bytes())
        .any(|(k, v)| k == "i" && !v.trim().is_empty())
}

/// True when a search answer lists no items: `data.items`, or every
/// `data.<type>.items` of a top-hits answer, is an empty array. An answer
/// of unknown shape counts as found.
fn search_found_nothing(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    let data = value.get("data").unwrap_or(&value);
    let mut lists: Vec<usize> = Vec::new();
    if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
        lists.push(items.len());
    }
    if let Some(object) = data.as_object() {
        for nested in object.values() {
            if let Some(items) = nested.get("items").and_then(|v| v.as_array()) {
                lists.push(items.len());
            }
        }
    }
    !lists.is_empty() && lists.iter().all(|n| *n == 0)
}

/// (fresh, stale) seconds of a cached answer, by how often it changes:
/// playlists and mixes often, text search daily, ISRC lookups and the
/// rest (albums, artists, track info, covers, lyrics, recommendations)
/// rarely. After the fresh period an entry is still served instantly
/// (X-Cache: STALE) while a background request refreshes it, so expiry
/// misses become hits and only cold keys miss. Pure function — unit tested.
fn cache_windows(path: &str, normalized_query: &str, body: &[u8]) -> (u64, u64) {
    let fresh = if path.starts_with("/search/") {
        if search_found_nothing(body) {
            return (EMPTY_SEARCH_FRESH_SECS, 0);
        }
        if is_isrc_lookup(normalized_query) {
            ISRC_FRESH_SECS
        } else {
            SEARCH_FRESH_SECS
        }
    } else if path.starts_with("/playlist/") || path.starts_with("/mix/") {
        15 * 60
    } else {
        MAX_FRESH_SECS
    };
    (fresh, fresh.min(MAX_FRESH_SECS))
}

/// Freshness of a cached entry of a given age (seconds). Entries younger
/// than its fresh period serve instantly; entries inside the stale window
/// still serve instantly while a background refresh runs; anything older
/// is refetched inline. Pure function — unit tested.
fn freshness(entry: &CachedResponse, age_secs: i64) -> Freshness {
    if age_secs < entry.fresh_secs as i64 {
        Freshness::Fresh
    } else if age_secs < (entry.fresh_secs + entry.stale_secs) as i64 {
        Freshness::Stale
    } else {
        Freshness::Expired
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Freshness {
    Fresh,
    Stale,
    Expired,
}

/// How long to negatively cache an error status (seconds). `None` means
/// "never cache": only repeatable upstream verdicts (not-found, throttled,
/// server errors) get short TTLs so error stampedes collapse without
/// masking recovery. Pure function — unit tested.
fn negative_ttl(status: u16) -> Option<u64> {
    match status {
        404 => Some(60),
        429 | 500 | 502 | 503 | 504 => Some(15),
        _ => None,
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub(crate) struct CachedResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Bytes,
    fetched_at: i64,
    fresh_secs: u64,
    stale_secs: u64,
}

impl CachedResponse {
    fn new(path: &str, normalized_query: &str, status: u16, headers: Vec<(String, String)>, body: Bytes) -> Self {
        let (fresh_secs, stale_secs) = cache_windows(path, normalized_query, &body);
        Self { status, headers, body, fetched_at: now_secs(), fresh_secs, stale_secs }
    }
}

/// Each entry is dropped once its own fresh and stale windows have passed.
struct EntryExpiry;

impl moka::Expiry<String, CachedResponse> for EntryExpiry {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &CachedResponse,
        _created_at: std::time::Instant,
    ) -> Option<Duration> {
        Some(Duration::from_secs(value.fresh_secs + value.stale_secs))
    }

    fn expire_after_update(
        &self,
        _key: &String,
        value: &CachedResponse,
        _updated_at: std::time::Instant,
        _duration_until_expiry: Option<Duration>,
    ) -> Option<Duration> {
        Some(Duration::from_secs(value.fresh_secs + value.stale_secs))
    }
}

/// A briefly cached error response with its own expiry.
#[derive(Clone)]
struct NegativeEntry {
    status: u16,
    headers: Vec<(String, String)>,
    body: Bytes,
    expires_at: i64,
}

pub struct ResponseCache {
    cache: Cache<String, CachedResponse>,
    /// Short-TTL error responses (404/429/5xx) so error stampedes
    /// collapse instead of hammering Tidal on every request.
    negative: Cache<String, NegativeEntry>,
    /// Per-key in-flight guards: concurrent identical requests collapse onto
    /// the leader instead of stampeding Tidal. Auto-evicts via TTL.
    inflight: Cache<String, Arc<Mutex<()>>>,
    /// Whether the id Tidal substituted for a requested one may be served,
    /// keyed `requested:substituted`: `None` when it is the same recording,
    /// otherwise the difference.
    pub(crate) replacements: Cache<String, Option<String>>,
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub coalesced: AtomicU64,
    /// Stale-window serves (instant, refreshed in background).
    pub stale: AtomicU64,
    /// Serves from the negative cache (no upstream call).
    pub negative_hits: AtomicU64,
}

impl ResponseCache {
    pub fn new() -> Self {
        Self {
            cache: Cache::builder()
                .expire_after(EntryExpiry)
                .weigher(|_k, v: &CachedResponse| v.body.len() as u32)
                .max_capacity(MAX_CACHE_BYTES)
                .build(),
            negative: Cache::builder()
                .time_to_live(Duration::from_secs(NEG_CACHE_TTL_SECS))
                .weigher(|_k, v: &NegativeEntry| v.body.len() as u32)
                .max_capacity(MAX_NEG_BYTES)
                .build(),
            inflight: Cache::builder()
                .time_to_live(Duration::from_secs(60))
                .max_capacity(10000)
                .build(),
            replacements: Cache::builder()
                .time_to_live(Duration::from_secs(REPLACEMENT_TTL_SECS))
                .max_capacity(10000)
                .build(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            coalesced: AtomicU64::new(0),
            stale: AtomicU64::new(0),
            negative_hits: AtomicU64::new(0),
        }
    }

    pub async fn invalidate_all(&self) {
        self.cache.invalidate_all();
        self.negative.invalidate_all();
        self.replacements.invalidate_all();
    }

    pub(crate) async fn get(&self, key: &str) -> Option<CachedResponse> {
        self.cache.get(key).await
    }

    pub(crate) async fn insert(&self, key: String, value: CachedResponse) {
        self.cache.insert(key, value).await;
    }
}

impl Default for ResponseCache {
    fn default() -> Self {
        Self::new()
    }
}

fn build_response(status: u16, headers: &[(String, String)], body: Bytes, label: &str) -> Response {
    let mut builder = Response::builder().status(status);
    for (k, v) in headers {
        // Drop stale framing headers; the body is rebuilt.
        if k.eq_ignore_ascii_case("content-length") || k.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        builder = builder.header(k.as_str(), v.as_str());
    }
    builder
        .header("X-Cache", label)
        .body(Body::from(body))
        .unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "cache rebuild failed").into_response()
        })
}

/// True when the response declares more than the cacheable body cap.
/// Such bodies pass through uncached (never buffer unboundedly).
fn body_too_big(resp: &Response) -> bool {
    resp.headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .is_some_and(|n| n > MAX_BODY_BYTES)
}

/// Buffer a small response for caching. If a streamed body crosses the
/// cache limit, return the original response with its already-read chunks
/// replayed ahead of the remaining stream.
async fn collect_parts(resp: Response) -> Result<(u16, Vec<(String, String)>, Bytes), Response> {
    let (parts, body) = resp.into_parts();
    let mut stream = body.into_data_stream();
    let mut chunks = Vec::new();
    let mut total = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(_) => {
                return Err((StatusCode::BAD_GATEWAY, "Upstream body read failed").into_response());
            }
        };
        total = total.saturating_add(chunk.len());
        chunks.push(chunk);
        if total > MAX_BODY_BYTES {
            let prefix = futures::stream::iter(chunks.into_iter().map(Ok::<Bytes, axum::Error>));
            return Err(Response::from_parts(
                parts,
                Body::from_stream(prefix.chain(stream)),
            ));
        }
    }
    let mut body = Vec::with_capacity(total);
    for chunk in chunks {
        body.extend_from_slice(&chunk);
    }
    let bytes = Bytes::from(body);
    let mut headers = Vec::new();
    for (k, v) in parts.headers.iter() {
        if let Ok(vs) = v.to_str() {
            headers.push((k.to_string(), vs.to_string()));
        }
    }
    Ok((parts.status.as_u16(), headers, bytes))
}

pub async fn cache_responses(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if req.method() != Method::GET || !cacheable(req.uri().path()) {
        return next.run(req).await;
    }
    let path = req.uri().path().to_string();
    let query = normalize_query(req.uri().query());
    let key = format!("{}?{}", path, query);

    // Fresh entries serve instantly.
    if let Some(hit) = state.cache.get(&key).await {
        if freshness(&hit, now_secs() - hit.fetched_at) == Freshness::Fresh {
            state.cache.hits.fetch_add(1, Ordering::Relaxed);
            return build_response(hit.status, &hit.headers, hit.body.clone(), "HIT");
        }
        // Stale window: serve instantly and refresh in the background.
        // `next` + `req` move into the task untouched. A failed refresh
        // keeps the stale entry until hard expiry, so upstream blips never
        // surface as user-facing misses.
        if freshness(&hit, now_secs() - hit.fetched_at) == Freshness::Stale {
            state.cache.hits.fetch_add(1, Ordering::Relaxed);
            state.cache.stale.fetch_add(1, Ordering::Relaxed);
            let stale_resp = build_response(hit.status, &hit.headers, hit.body.clone(), "STALE");
            let bg_state = state.clone();
            let bg_key = key.clone();
            let bg_path = path.clone();
            let bg_query = query.clone();
            tokio::spawn(async move {
                let guard = bg_state
                    .cache
                    .inflight
                    .get_with(bg_key.clone(), async { Arc::new(Mutex::new(())) })
                    .await;
                // Another refresh already running: skip, don't pile up.
                let Ok(_lock) = guard.try_lock() else { return };
                let resp = next.run(req).await;
                if resp.status() == StatusCode::OK {
                    if let Ok((status, headers, bytes)) = collect_parts(resp).await {
                        bg_state
                            .cache
                            .insert(
                                bg_key,
                                CachedResponse::new(&bg_path, &bg_query, status, headers, bytes),
                            )
                            .await;
                    }
                }
            });
            return stale_resp;
        }
    }

    // Negative cache: repeat errors serve instantly without upstream calls.
    if let Some(neg) = state.cache.negative.get(&key).await {
        if now_secs() < neg.expires_at {
            state.cache.negative_hits.fetch_add(1, Ordering::Relaxed);
            return build_response(neg.status, &neg.headers, neg.body.clone(), "NEGATIVE");
        }
    }

    // Singleflight: serialize identical concurrent misses on a per-key lock,
    // then re-check — followers get the leader's cached response.
    let guard = state
        .cache
        .inflight
        .get_with(key.clone(), async { Arc::new(Mutex::new(())) })
        .await;
    let _lock = guard.lock().await;
    if let Some(hit) = state.cache.get(&key).await {
        if freshness(&hit, now_secs() - hit.fetched_at) != Freshness::Expired {
            state.cache.hits.fetch_add(1, Ordering::Relaxed);
            state.cache.coalesced.fetch_add(1, Ordering::Relaxed);
            let label = if freshness(&hit, now_secs() - hit.fetched_at) == Freshness::Fresh {
                "HIT"
            } else {
                "STALE"
            };
            return build_response(hit.status, &hit.headers, hit.body.clone(), label);
        }
    }
    if let Some(neg) = state.cache.negative.get(&key).await {
        if now_secs() < neg.expires_at {
            state.cache.negative_hits.fetch_add(1, Ordering::Relaxed);
            return build_response(neg.status, &neg.headers, neg.body.clone(), "NEGATIVE");
        }
    }
    state.cache.misses.fetch_add(1, Ordering::Relaxed);

    let mut resp = next.run(req).await;
    let status = resp.status();
    if status == StatusCode::OK {
        // Oversized bodies pass through uncached (never buffer unboundedly).
        if body_too_big(&resp) {
            resp.headers_mut()
                .insert("X-Cache", "SKIP".parse().unwrap());
            return resp;
        }
        match collect_parts(resp).await {
            Ok((status, headers, bytes)) => {
                let cached = CachedResponse::new(&path, &query, status, headers, bytes.clone());
                state.cache.insert(key, cached.clone()).await;
                return build_response(cached.status, &cached.headers, bytes, "MISS");
            }
            Err(mut passthrough) => {
                passthrough
                    .headers_mut()
                    .insert("X-Cache", "SKIP".parse().unwrap());
                return passthrough;
            }
        }
    }

    // Cacheable error verdicts (404/429/5xx) collapse repeat-error
    // stampedes; everything else passes through uncached.
    if negative_ttl(status.as_u16()).is_some() && !body_too_big(&resp) {
        let ttl = negative_ttl(status.as_u16()).unwrap_or(15);
        match collect_parts(resp).await {
            Ok((_, headers, bytes)) => {
                let entry = NegativeEntry {
                    status: status.as_u16(),
                    headers: headers.clone(),
                    body: bytes.clone(),
                    expires_at: now_secs() + ttl as i64,
                };
                state.cache.negative.insert(key, entry).await;
                return build_response(status.as_u16(), &headers, bytes, "MISS");
            }
            Err(mut passthrough) => {
                passthrough
                    .headers_mut()
                    .insert("X-Cache", "SKIP".parse().unwrap());
                return passthrough;
            }
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::{
        CachedResponse, Freshness, MAX_BODY_BYTES, cache_windows, collect_parts, freshness,
        negative_ttl, normalize_query,
    };
    use axum::body::{Body, Bytes, to_bytes};
    use axum::http::StatusCode;
    use axum::response::Response;

    #[test]
    fn query_canonicalizes_encoding_and_order() {
        // Order variants share one key.
        assert_eq!(normalize_query(Some("b=2&a=1")), "a=1&b=2");
        assert_eq!(normalize_query(Some("a=1&b=2")), "a=1&b=2");
        // Encoding variants decode to the same key (`+` == `%20`).
        assert_eq!(
            normalize_query(Some("s=daft+punk")),
            normalize_query(Some("s=daft%20punk"))
        );
        // Repeated params keep their multiplicity, sorted.
        assert_eq!(
            normalize_query(Some("formats=FLAC&formats=AACLC&adaptive=true")),
            "adaptive=true&formats=AACLC&formats=FLAC"
        );
        // Single pair and empty queries still work.
        assert_eq!(normalize_query(Some("id=42")), "id=42");
        assert_eq!(normalize_query(None), "");
        assert_eq!(normalize_query(Some("")), "");
    }

    fn entry(path: &str, query: &str, body: &str) -> CachedResponse {
        CachedResponse::new(path, &normalize_query(Some(query)), 200, Vec::new(), Bytes::from(body.to_string()))
    }

    const FOUND: &str = r#"{"version":"2.0","data":{"limit":50,"items":[{"id":1}]}}"#;
    const NOTHING: &str = r#"{"version":"2.0","data":{"limit":50,"totalNumberOfItems":0,"items":[]}}"#;

    #[test]
    fn freshness_windows() {
        let hour = 3600;
        let day = 24 * hour;
        // Playlists and mixes: 15 minutes fresh, then 15 minutes served stale while refreshed.
        let playlist = entry("/playlist/", "id=1", "{}");
        assert_eq!(freshness(&entry("/mix/", "id=1", "{}"), 15 * 60 - 1), Freshness::Fresh);
        assert_eq!(freshness(&playlist, 0), Freshness::Fresh);
        assert_eq!(freshness(&playlist, 15 * 60), Freshness::Stale);
        assert_eq!(freshness(&playlist, 30 * 60 - 1), Freshness::Stale);
        assert_eq!(freshness(&playlist, 30 * 60), Freshness::Expired);
        // Text search: a day fresh, then 12 hours stale.
        let search = entry("/search/", "s=daft+punk&limit=50", FOUND);
        assert_eq!(freshness(&search, day - 1), Freshness::Fresh);
        assert_eq!(freshness(&search, day), Freshness::Stale);
        assert_eq!(freshness(&search, day + 12 * hour), Freshness::Expired);
        // ISRC lookup: a week fresh, then 12 hours stale.
        let isrc = entry("/search/", "i=USUM71703861&limit=50", FOUND);
        assert_eq!(freshness(&isrc, 7 * day - 1), Freshness::Fresh);
        assert_eq!(freshness(&isrc, 7 * day + 12 * hour - 1), Freshness::Stale);
        assert_eq!(freshness(&isrc, 7 * day + 12 * hour), Freshness::Expired);
        // A blank `i` is a text search, as in the handler.
        assert_eq!(freshness(&entry("/search/", "i=+&s=x", FOUND), day), Freshness::Stale);
        // Nothing found: 15 minutes, never served stale.
        for empty in [entry("/search/", "s=new+release", NOTHING), entry("/search/", "i=X", NOTHING)] {
            assert_eq!(freshness(&empty, 15 * 60 - 1), Freshness::Fresh);
            assert_eq!(freshness(&empty, 15 * 60), Freshness::Expired);
        }
        // Everything else: 12 + 12 hours at most.
        let album = entry("/album/", "id=1", "{}");
        assert_eq!(freshness(&album, 12 * hour - 1), Freshness::Fresh);
        assert_eq!(freshness(&entry("/artist/similar/", "id=1", "{}"), 12 * hour), Freshness::Stale);
        assert_eq!(freshness(&entry("/lyrics/", "id=1", "{}"), 24 * hour - 1), Freshness::Stale);
        assert_eq!(freshness(&entry("/info/", "id=1", "{}"), 24 * hour), Freshness::Expired);
    }

    #[test]
    fn empty_search_detection() {
        let windows = |body: &str| cache_windows("/search/", "s=x", body.as_bytes());
        assert_eq!(windows(NOTHING), (15 * 60, 0));
        // Top hits: empty only when every list is empty.
        assert_eq!(windows(r#"{"data":{"artists":{"items":[]},"tracks":{"items":[]}}}"#), (15 * 60, 0));
        assert_eq!(windows(r#"{"data":{"artists":{"items":[]},"tracks":{"items":[{"id":1}]}}}"#).1, 12 * 3600);
        // Unknown shape or broken JSON is not treated as "nothing found".
        assert_eq!(windows(r#"{"data":{"foo":1}}"#).0, 24 * 3600);
        assert_eq!(windows("not json").0, 24 * 3600);
        // Other routes never use the empty-search window.
        assert_eq!(cache_windows("/album/", "id=1", NOTHING.as_bytes()).0, 12 * 3600);
    }

    #[test]
    fn negative_ttl_only_for_repeatable_verdicts() {
        assert_eq!(negative_ttl(404), Some(60));
        assert_eq!(negative_ttl(429), Some(15));
        assert_eq!(negative_ttl(500), Some(15));
        assert_eq!(negative_ttl(503), Some(15));
        assert_eq!(negative_ttl(200), None);
        assert_eq!(negative_ttl(400), None);
        assert_eq!(negative_ttl(401), None);
        assert_eq!(negative_ttl(403), None);
    }

    #[tokio::test]
    async fn oversized_stream_passes_through_without_losing_consumed_chunks() {
        let first = Bytes::from(vec![b'a'; MAX_BODY_BYTES]);
        let last = Bytes::from_static(b"tail");
        let stream = futures::stream::iter(vec![Ok::<_, std::io::Error>(first), Ok(last)]);
        let response = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from_stream(stream))
            .unwrap();
        let passthrough = collect_parts(response)
            .await
            .expect_err("body exceeds cache limit");
        assert_eq!(passthrough.status(), StatusCode::OK);
        let body = to_bytes(passthrough.into_body(), MAX_BODY_BYTES + 4)
            .await
            .unwrap();
        assert_eq!(body.len(), MAX_BODY_BYTES + 4);
        assert!(body[..MAX_BODY_BYTES].iter().all(|b| *b == b'a'));
        assert_eq!(&body[MAX_BODY_BYTES..], b"tail");
    }
}
