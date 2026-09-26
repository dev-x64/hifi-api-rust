use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use chrono::Utc;
use futures::future::join_all;
use rand::Rng;
use reqwest::Client;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::{Mutex, RwLock};

use crate::config::Config;
use crate::error::AppError;

fn build_client(proxy_url: Option<&str>, user_agent: &str) -> Result<Client, String> {
    build_tidal_client(proxy_url, user_agent, false)
}

fn build_auth_client(proxy_url: Option<&str>, user_agent: &str) -> Result<Client, String> {
    build_tidal_client(proxy_url, user_agent, true)
}

fn build_tidal_client(proxy_url: Option<&str>, user_agent: &str, auth: bool) -> Result<Client, String> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Accept", if auth { "application/json" } else { "*/*" }.parse().unwrap());
    headers.insert("Accept-Encoding", "gzip".parse().unwrap());
    if !auth {
        headers.insert("Accept-Language", "en-US,en;q=0.9".parse().unwrap());
        headers.insert("X-Platform", "android".parse().unwrap());
        headers.insert("X-Tidal-Platform", "android".parse().unwrap());
    }
    let mut builder = Client::builder()
        .gzip(true)
        .default_headers(headers)
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .http2_adaptive_window(true)
        .pool_max_idle_per_host(if auth { 2 } else { 20 })
        .pool_idle_timeout(Duration::from_secs(30))
        .user_agent(user_agent);
    if auth {
        builder = builder.http1_only();
    }
    if let Some(url) = proxy_url {
        let proxy =
            reqwest::Proxy::all(url).map_err(|e| format!("Invalid proxy URL: {}", e))?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|e| format!("Failed to build HTTP client: {}", e))
}

pub struct ProxyManager {
    config: Arc<Config>,
    db: Option<SqlitePool>,
    enabled: AtomicBool,
    proxies: RwLock<Vec<String>>,
    direct_client: Client,
    direct_auth_client: Client,
    client: ArcSwap<Client>,
    /// Proxy URL currently in use (None = direct connection).
    current: RwLock<Option<String>>,
    /// True once a usable client is confirmed (always true when proxies disabled).
    ready: AtomicBool,
    /// Consecutive failures on the current client.
    fails: AtomicU64,
    /// Last resolve attempt (unix secs) — throttles resolve storms.
    last_try: AtomicI64,
    /// Set while a background rotation is in flight.
    rotating: AtomicBool,
    generation: AtomicU64,
    switch_lock: Mutex<()>,
    account_routes: Mutex<AccountRoutes>,
}

struct AccountProxy {
    url: String,
    client: Client,
    auth_client: Client,
    failures: u64,
    verified: bool,
}

#[derive(Default)]
struct AccountRoutes {
    assignments: HashMap<String, AccountProxy>,
    avoided: HashMap<String, String>,
    last_failed: HashMap<String, i64>,
}

impl ProxyManager {
    pub fn new(config: Arc<Config>, db: Option<SqlitePool>) -> Self {
        let proxies = if config.proxies_file.exists() {
            Self::load_proxies_from_file(&config.proxies_file)
        } else {
            Vec::new()
        };

        let direct = build_client(None, &config.user_agent).expect("Failed to build HTTP client");
        Self {
            enabled: AtomicBool::new(config.use_proxies),
            config: config.clone(),
            db,
            proxies: RwLock::new(proxies),
            direct_client: direct.clone(),
            direct_auth_client: build_auth_client(None, &config.user_agent).expect("Failed to build auth client"),
            client: ArcSwap::from_pointee(direct),
            current: RwLock::new(None),
            // Direct mode is always ready; proxy mode resolves in the background.
            ready: AtomicBool::new(false),
            fails: AtomicU64::new(0),
            last_try: AtomicI64::new(0),
            rotating: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            switch_lock: Mutex::new(()),
            account_routes: Mutex::new(AccountRoutes::default()),
        }
    }

    /// Restore local account-to-proxy bindings before requests start. A restored
    /// proxy is checked on first use, and invalid/removed entries are replaced.
    pub async fn load_assignments(&self) {
        let _switch = self.switch_lock.lock().await;
        let Some(db) = &self.db else { return; };
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT p.account_id, p.proxy_url FROM proxy_assignments p JOIN accounts a ON a.id = p.account_id",
        )
        .fetch_all(db).await;
        let rows = match rows {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("Could not load proxy assignments: {e}");
                return;
            }
        };
        let entries = self.proxies.read().await.clone();
        let mut routes = self.account_routes.lock().await;
        routes.assignments.clear();
        routes.avoided.clear();
        routes.last_failed.clear();
        for (account_id, url) in rows {
            if !entries.contains(&url) { continue; }
            if let Ok(client) = build_client(Some(&url), &self.config.user_agent) {
                routes.assignments.insert(account_id, AccountProxy {
                    auth_client: build_auth_client(Some(&url), &self.config.user_agent).expect("valid proxy"),
                    url,
                    client,
                    failures: 0,
                    verified: false,
                });
            }
        }
        drop(routes);
        if self.proxies_enabled() {
            self.rebalance_assignments_locked(&entries).await;
        }
    }

    pub fn proxies_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub async fn entries(&self) -> Vec<String> {
        self.proxies.read().await.clone()
    }

    pub fn normalize_proxies(proxies: Vec<String>, enabled: bool) -> Result<Vec<String>, AppError> {
        let mut normalized = Vec::new();
        for raw in proxies {
            let url = raw.trim();
            if url.is_empty() { continue; }
            reqwest::Proxy::all(url).map_err(|e| AppError::BadRequest(format!("Invalid proxy URL: {e}")))?;
            if !normalized.iter().any(|existing| existing == url) {
                normalized.push(url.to_string());
            }
        }
        if enabled && normalized.is_empty() {
            return Err(AppError::BadRequest("Добавьте хотя бы один адрес прокси перед включением".into()));
        }
        Ok(normalized)
    }

    pub async fn configure(&self, enabled: bool, proxies: Vec<String>) -> Result<(), AppError> {
        let proxies = Self::normalize_proxies(proxies, enabled)?;
        let active = proxies.clone();
        let _guard = self.switch_lock.lock().await;
        let previous = self.proxies.read().await.clone();
        let added = active.iter().any(|url| !previous.contains(url));
        let was_enabled = self.proxies_enabled();
        self.generation.fetch_add(1, Ordering::AcqRel);
        *self.proxies.write().await = proxies;
        *self.current.write().await = None;
        self.client.store(Arc::new(self.direct_client.clone()));
        self.enabled.store(enabled, Ordering::Release);
        self.ready.store(!enabled, Ordering::Release);
        self.fails.store(0, Ordering::Relaxed);
        self.last_try.store(0, Ordering::Relaxed);
        let removed = {
            let mut routes = self.account_routes.lock().await;
            let mut removed = Vec::new();
            routes.assignments.retain(|id, route| {
                if active.contains(&route.url) { true } else {
                    removed.push(id.clone());
                    false
                }
            });
            routes.avoided.retain(|_, url| active.contains(url));
            routes.last_failed.clear();
            removed
        };
        if let Some(db) = &self.db {
            for id in removed {
                if let Err(e) = sqlx::query("DELETE FROM proxy_assignments WHERE account_id = ?")
                    .bind(id).execute(db).await {
                    tracing::warn!("Could not clear removed proxy assignment: {e}");
                }
            }
        }
        if enabled && (added || !was_enabled) {
            self.rebalance_assignments_locked(&active).await;
        }
        tracing::info!("Proxy mode {}", if enabled { "enabled" } else { "disabled" });
        Ok(())
    }

    /// Called with switch_lock held. Move at most one account onto each tested
    /// spare proxy, leaving all other account bindings intact.
    async fn rebalance_assignments_locked(&self, entries: &[String]) {
        let mut routes = self.account_routes.lock().await;
        let mut loads: HashMap<String, usize> = HashMap::new();
        for route in routes.assignments.values() {
            *loads.entry(route.url.clone()).or_default() += 1;
        }
        if !loads.values().any(|count| *count > 1) {
            return;
        }
        let free: Vec<&String> = entries.iter().filter(|url| !loads.contains_key(*url)).collect();
        let checks = join_all(free.iter().map(|url| self.test_proxy(url))).await;
        for (target, works) in free.into_iter().zip(checks) {
            if !works { continue; }
            let Some(source) = loads.iter()
                .filter(|(_, count)| **count > 1)
                .max_by(|(url_a, count_a), (url_b, count_b)| count_a.cmp(count_b).then(url_a.cmp(url_b)))
                .map(|(url, _)| url.clone()) else { break; };
            let Some(account_id) = routes.assignments.iter()
                .filter(|(_, route)| route.url.as_str() == source.as_str())
                .map(|(id, _)| id.clone())
                .max() else { continue; };
            let client = match build_client(Some(target), &self.config.user_agent) {
                Ok(client) => client,
                Err(e) => {
                    tracing::warn!("Could not build proxy client: {e}");
                    continue;
                }
            };
            routes.assignments.insert(account_id.clone(), AccountProxy {
                auth_client: build_auth_client(Some(target), &self.config.user_agent).expect("valid proxy"),
                url: target.clone(), client, failures: 0, verified: true,
            });
            routes.avoided.remove(&account_id);
            routes.last_failed.remove(&account_id);
            *loads.get_mut(&source).unwrap() -= 1;
            loads.insert(target.clone(), 1);
            if let Some(db) = &self.db {
                if let Err(e) = sqlx::query(
                    "INSERT INTO proxy_assignments (account_id, proxy_url) VALUES (?, ?) \
                     ON CONFLICT(account_id) DO UPDATE SET proxy_url = excluded.proxy_url",
                )
                .bind(&account_id)
                .bind(target)
                .execute(db)
                .await {
                    tracing::warn!("Could not save proxy assignment: {e}");
                }
            }
            tracing::info!("Account {} rebalanced from {} to {}",
                account_id, mask_proxy(&source), mask_proxy(target));
        }
    }

    fn load_proxies_from_file(path: &std::path::Path) -> Vec<String> {
        if !path.exists() {
            tracing::warn!("Proxies file {:?} not found.", path);
            return Vec::new();
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to read proxies file: {}", e);
                return Vec::new();
            }
        };

        let proxies: Vec<String> = content
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        tracing::info!("Loaded {} proxies from file", proxies.len());
        proxies
    }

    /// Current client, no questions asked. Prefer `working_client` for Tidal traffic.
    pub fn client(&self) -> Client {
        if self.proxies_enabled() { (**self.client.load()).clone() } else { self.direct_client.clone() }
    }

    /// Resolve a usable client for Tidal traffic.
    /// - Proxies disabled → direct, always.
    /// - Proxies enabled + ready → proxied.
    /// - Proxies enabled + not ready → quick resolve (throttled); direct only if
    ///   FALLBACK_TO_DIRECT_CONNECTION=true, else 503 so the home IP never leaks.
    pub async fn working_client(&self) -> Result<Client, AppError> {
        if !self.proxies_enabled() {
            return Ok(self.direct_client.clone());
        }
        if self.ready.load(Ordering::Relaxed) {
            return Ok(self.client());
        }
        if self.try_resolve().await {
            return Ok(self.client());
        }
        if self.config.fallback_to_direct {
            tracing::warn!("No working proxy — falling back to direct connection (HOST IP MAY BE EXPOSED)");
            return Ok(self.direct_client.clone());
        }
        Err(AppError::ServiceUnavailable(
            "No working proxy available and direct fallback is disabled".into(),
        ))
    }

    /// A stable egress for one Tidal account. Free proxies are preferred, so
    /// accounts spread across the pool; extra proxies remain available for failover.
    pub async fn working_client_for(&self, account_id: &str) -> Result<Client, AppError> {
        // Healthy accounts must not wait behind another account's proxy probes.
        if !self.proxies_enabled() {
            return Ok(self.direct_client.clone());
        }
        {
            let routes = self.account_routes.lock().await;
            if let Some(route) = routes.assignments.get(account_id).filter(|r| r.verified) {
                return Ok(route.client.clone());
            }
        }
        let _switch = self.switch_lock.lock().await;
        if !self.proxies_enabled() {
            return Ok(self.direct_client.clone());
        }
        let entries = self.proxies.read().await.clone();
        let mut routes = self.account_routes.lock().await;
        if let Some(existing) = routes.assignments.get(account_id) {
            if entries.contains(&existing.url) {
                if existing.verified {
                    return Ok(existing.client.clone());
                }
                let url = existing.url.clone();
                drop(routes);
                let works = self.test_proxy(&url).await;
                routes = self.account_routes.lock().await;
                if works {
                    let existing = routes.assignments.get_mut(account_id).unwrap();
                    existing.verified = true;
                    return Ok(existing.client.clone());
                }
                routes.avoided.insert(account_id.to_string(), url);
            }
            routes.assignments.remove(account_id);
        }

        let now = Utc::now().timestamp();
        if routes.last_failed.get(account_id).is_some_and(|last| now - last < 30) {
            return self.unavailable_for(account_id);
        }

        let avoid = routes.avoided.get(account_id).map(String::as_str);
        let candidates = ordered_candidates(account_id, &entries, &routes.assignments, avoid);
        drop(routes);
        for group in candidates.chunks(5) {
            let checks = join_all(group.iter().map(|url| self.test_proxy(url))).await;
            for (url, works) in group.iter().zip(checks) {
                if !works { continue; }
                let client = match build_client(Some(url), &self.config.user_agent) {
                    Ok(client) => client,
                    Err(e) => {
                        tracing::warn!("Could not build proxy client: {e}");
                        continue;
                    }
                };
                let mut routes = self.account_routes.lock().await;
                routes.assignments.insert(account_id.to_string(), AccountProxy {
                    auth_client: build_auth_client(Some(url), &self.config.user_agent).expect("valid proxy"),
                    url: url.clone(), client: client.clone(), failures: 0, verified: true,
                });
                routes.avoided.remove(account_id);
                routes.last_failed.remove(account_id);
                if let Some(db) = &self.db {
                    if let Err(e) = sqlx::query(
                        "INSERT INTO proxy_assignments (account_id, proxy_url) VALUES (?, ?) \
                         ON CONFLICT(account_id) DO UPDATE SET proxy_url = excluded.proxy_url",
                    )
                    .bind(account_id)
                    .bind(url)
                    .execute(db)
                    .await {
                        tracing::warn!("Could not save proxy assignment: {e}");
                    }
                }
                tracing::info!("Account {} assigned proxy {}", account_id, mask_proxy(url));
                return Ok(client);
            }
        }
        self.account_routes.lock().await.last_failed.insert(account_id.to_string(), now);
        self.unavailable_for(account_id)
    }

    /// Device authorization has no account ID yet. Keep its auth-only client
    /// pinned to the selected generic egress for the whole setup session.
    pub async fn working_auth_client(&self) -> Result<Client, AppError> {
        self.working_client().await?;
        let _switch = self.switch_lock.lock().await;
        if !self.proxies_enabled() {
            return Ok(self.direct_auth_client.clone());
        }
        let current = self.current.read().await;
        if let Some(url) = current.as_deref() {
            return build_auth_client(Some(url), &self.config.user_agent)
                .map_err(AppError::ServiceUnavailable);
        }
        if self.config.fallback_to_direct {
            return Ok(self.direct_auth_client.clone());
        }
        Err(AppError::ServiceUnavailable("No auth proxy available".into()))
    }

    /// Auth uses HTTP/1.1 but exactly the same account-to-proxy binding as API traffic.
    pub async fn working_auth_client_for(&self, account_id: &str) -> Result<Client, AppError> {
        self.working_client_for(account_id).await?;
        if !self.proxies_enabled() {
            return Ok(self.direct_auth_client.clone());
        }
        if let Some(route) = self.account_routes.lock().await.assignments.get(account_id) {
            return Ok(route.auth_client.clone());
        }
        if self.config.fallback_to_direct {
            return Ok(self.direct_auth_client.clone());
        }
        Err(AppError::ServiceUnavailable("Account proxy changed during auth resolution".into()))
    }

    fn unavailable_for(&self, account_id: &str) -> Result<Client, AppError> {
        if self.config.fallback_to_direct {
            tracing::warn!("No working proxy for account {} — using direct connection", account_id);
            return Ok(self.direct_client.clone());
        }
        Err(AppError::ServiceUnavailable(
            "No working proxy available and direct fallback is disabled".into(),
        ))
    }

    pub async fn note_success_for(&self, account_id: &str) {
        if let Some(route) = self.account_routes.lock().await.assignments.get_mut(account_id) {
            route.failures = 0;
        }
    }

    pub async fn note_failure_for(&self, account_id: &str) {
        if !self.proxies_enabled() { return; }
        let should_rotate = {
            let mut routes = self.account_routes.lock().await;
            if let Some(route) = routes.assignments.get_mut(account_id) {
                route.failures += 1;
                route.failures >= 3
            } else { false }
        };
        if should_rotate {
            self.rotate_account(account_id).await;
        }
    }

    /// Drop only this account's binding; its next request picks a tested spare.
    pub async fn rotate_account(&self, account_id: &str) {
        let _switch = self.switch_lock.lock().await;
        let mut routes = self.account_routes.lock().await;
        let old = routes.assignments.remove(account_id).map(|r| r.url);
        if let Some(url) = old {
            routes.avoided.insert(account_id.to_string(), url.clone());
            routes.last_failed.remove(account_id);
            tracing::warn!("Account {} leaving proxy {}", account_id, mask_proxy(&url));
            if let Some(db) = &self.db {
                if let Err(e) = sqlx::query("DELETE FROM proxy_assignments WHERE account_id = ?")
                    .bind(account_id).execute(db).await {
                    tracing::warn!("Could not clear proxy assignment: {e}");
                }
            }
        }
    }

    pub async fn forget_account(&self, account_id: &str) {
        let _switch = self.switch_lock.lock().await;
        let mut routes = self.account_routes.lock().await;
        routes.assignments.remove(account_id);
        routes.avoided.remove(account_id);
        routes.last_failed.remove(account_id);
        if let Some(db) = &self.db {
            if let Err(e) = sqlx::query("DELETE FROM proxy_assignments WHERE account_id = ?")
                .bind(account_id).execute(db).await {
                tracing::warn!("Could not delete proxy assignment: {e}");
            }
        }
    }

    /// Attempt one resolve, throttled to at most once per 30s. Returns ready state.
    async fn try_resolve(&self) -> bool {
        if self.ready.load(Ordering::Relaxed) {
            return true;
        }
        let generation = self.generation.load(Ordering::Acquire);
        let now = Utc::now().timestamp();
        if now - self.last_try.load(Ordering::Relaxed) < 30 {
            return false;
        }
        self.last_try.store(now, Ordering::Relaxed);
        match self.get_working_proxy(None).await {
            Some(proxy) => {
                self.swap_to(Some(proxy), generation).await
            }
            None => false,
        }
    }

    async fn swap_to(&self, proxy: Option<String>, generation: u64) -> bool {
        let _guard = self.switch_lock.lock().await;
        if !self.proxies_enabled() || self.generation.load(Ordering::Acquire) != generation {
            return false;
        }
        match build_client(proxy.as_deref(), &self.config.user_agent) {
            Ok(client) => {
                self.client.store(Arc::new(client));
                *self.current.write().await = proxy.clone();
                self.ready.store(true, Ordering::Relaxed);
                self.fails.store(0, Ordering::Relaxed);
                match proxy {
                    Some(p) => tracing::info!("Proxy active: {}", mask_proxy(&p)),
                    None => tracing::info!("Proxy active: direct connection"),
                }
                true
            }
            Err(e) => {
                tracing::error!("{}", e);
                self.ready.store(false, Ordering::Relaxed);
                false
            }
        }
    }

    /// Kick off the initial resolve in the background (never blocks startup).
    pub fn spawn_initial_resolve(self: &Arc<Self>) {
        if !self.proxies_enabled() {
            self.ready.store(true, Ordering::Relaxed);
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            if this.try_resolve().await {
                tracing::info!("Initial proxy resolve succeeded");
            } else {
                tracing::warn!(
                    "No working proxy at startup{}. Retrying in the background.",
                    if this.config.fallback_to_direct {
                        " — direct fallback enabled (HOST IP MAY BE EXPOSED)"
                    } else {
                        " — Tidal traffic will 503 until one works"
                    }
                );
            }
        });
    }

    /// Record a successful Tidal round-trip.
    pub fn note_success(&self) {
        self.fails.store(0, Ordering::Relaxed);
    }

    /// Record a failed Tidal round-trip; rotate after 3 consecutive failures.
    pub fn note_failure(self: &Arc<Self>) {
        if !self.proxies_enabled() {
            return;
        }
        let fails = self.fails.fetch_add(1, Ordering::Relaxed) + 1;
        if fails >= 3 {
            self.fails.store(0, Ordering::Relaxed);
            self.rotate();
        }
    }

    /// True when token refreshes should rotate the proxy first.
    pub fn should_rotate_on_refresh(&self) -> bool {
        self.proxies_enabled() && self.config.rotate_proxies_on_refresh
    }

    fn rotate(self: &Arc<Self>) {
        if !self
            .rotating
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            let generation = this.generation.load(Ordering::Acquire);
            let next = {
                let proxies = this.proxies.read().await;
                if proxies.is_empty() {
                    None
                } else {
                    let current = this.current.read().await;
                    let start = current
                        .as_ref()
                        .and_then(|c| proxies.iter().position(|p| p == c))
                        .map(|i| (i + 1) % proxies.len())
                        .unwrap_or(0);
                    Some(proxies[start].clone())
                }
            };
            match next {
                Some(proxy) => {
                    tracing::warn!("Rotating proxy after failures → {}", mask_proxy(&proxy));
                    if !this.swap_to(Some(proxy), generation).await {
                        this.rotating.store(false, Ordering::Relaxed);
                        return;
                    }
                    // Verify in the background; if bad, mark not-ready so the
                    // next request resolves a tested one.
                    let check = this.current.read().await.clone();
                    if let Some(url) = check {
                        if !this.test_proxy(&url).await && this.generation.load(Ordering::Acquire) == generation {
                            tracing::warn!("Rotated proxy failed health check: {}", mask_proxy(&url));
                            this.ready.store(false, Ordering::Relaxed);
                        }
                    }
                }
                None => {
                    tracing::warn!("Proxy rotation requested but pool is empty");
                    if this.generation.load(Ordering::Acquire) == generation {
                        this.ready.store(false, Ordering::Relaxed);
                    }
                }
            }
            this.rotating.store(false, Ordering::Relaxed);
        });
    }

    pub async fn test_proxy(&self, proxy_url: &str) -> bool {
        let Ok(proxy) = reqwest::Proxy::all(proxy_url) else { return false; };
        let client = match Client::builder()
            .proxy(proxy)
            .timeout(Duration::from_secs(5))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        match client.get("http://example.com").send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    pub async fn get_working_proxy(&self, avoid_proxy: Option<&str>) -> Option<String> {
        let proxies = self.proxies.read().await;
        if proxies.is_empty() {
            return None;
        }

        let mut shuffled = proxies.clone();
        {
            let mut rng = rand::thread_rng();
            for i in (1..shuffled.len()).rev() {
                let j = rng.gen_range(0..=i);
                shuffled.swap(i, j);
            }
        }

        if let Some(avoid) = avoid_proxy {
            shuffled.retain(|p| p != avoid);
        }

        if shuffled.is_empty() {
            return None;
        }

        let candidates: Vec<&str> = shuffled.iter().take(3).map(|s| s.as_str()).collect();

        for proxy in candidates {
            if self.test_proxy(proxy).await {
                return Some(proxy.to_string());
            }
        }

        None
    }

    pub async fn status(&self) -> Value {
        let proxies = self.proxies.read().await;
        let current = self.current.read().await;
        let routes = self.account_routes.lock().await;
        let mut assignments: Vec<Value> = routes.assignments.iter().map(|(id, route)| json!({
            "account_id": id, "proxy": mask_proxy(&route.url), "verified": route.verified,
            "consecutive_fails": route.failures,
        })).collect();
        assignments.sort_by(|a, b| a["account_id"].as_str().cmp(&b["account_id"].as_str()));
        json!({
            "enabled": self.proxies_enabled(),
            "ready": self.ready.load(Ordering::Relaxed) || routes.assignments.values().any(|route| route.verified),
            "current": current.as_ref().map(|p| mask_proxy(p)),
            "pool_size": proxies.len(),
            "consecutive_fails": self.fails.load(Ordering::Relaxed),
            "last_try": self.last_try.load(Ordering::Relaxed),
            "fallback_to_direct": self.config.fallback_to_direct,
            "entries": proxies.clone(),
            "assignments": assignments,
        })
    }
}

fn ordered_candidates(
    account_id: &str,
    entries: &[String],
    assignments: &HashMap<String, AccountProxy>,
    avoid: Option<&str>,
) -> Vec<String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    account_id.hash(&mut hasher);
    let offset = hasher.finish() as usize;
    let mut candidates: Vec<(usize, &String)> = entries.iter().enumerate().collect();
    candidates.sort_by_key(|(index, url)| {
        let used = assignments.values().filter(|route| route.url.as_str() == url.as_str()).count();
        let avoided = usize::from(Some(url.as_str()) == avoid && entries.len() > 1);
        (avoided, used, (index + entries.len() - offset % entries.len()) % entries.len())
    });
    candidates.into_iter().map(|(_, url)| url.clone()).collect()
}

fn mask_proxy(url: &str) -> String {
    // Show host:port but hide userinfo (http://user:pass@host:port → http://***@host:port).
    match url.split_once("://") {
        Some((scheme, rest)) => match rest.rsplit_once('@') {
            Some((_, host)) => format!("{}://***@{}", scheme, host),
            None => url.to_string(),
        },
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn fake_proxy() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = [0; 4096];
                    let _ = stream.read(&mut buf).await;
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await;
                });
            }
        });
        (url, task)
    }

    #[test]
    fn assignments_use_distinct_proxies_before_sharing() {
        let entries: Vec<String> = (0..12).map(|n| format!("http://proxy-{n}:8080")).collect();
        let mut assignments = HashMap::new();
        for n in 0..10 {
            let id = format!("account-{n}");
            let url = ordered_candidates(&id, &entries, &assignments, None).remove(0);
            assignments.insert(id, AccountProxy {
                url,
                client: Client::new(),
                auth_client: Client::new(),
                failures: 0,
                verified: true,
            });
        }
        let distinct: std::collections::HashSet<_> = assignments.values().map(|r| r.url.clone()).collect();
        assert_eq!(distinct.len(), 10);
        let old = assignments.remove("account-0").unwrap().url;
        let replacement = ordered_candidates("account-0", &entries, &assignments, Some(&old));
        assert_ne!(replacement[0], old);
        assert!(!distinct.contains(&replacement[0]));
    }

    #[tokio::test]
    async fn healthy_account_api_and_auth_do_not_wait_for_other_proxy_probes() {
        let mut config = Config::from_env();
        config.use_proxies = true;
        config.fallback_to_direct = false;
        let manager = ProxyManager::new(Arc::new(config), None);
        manager.account_routes.lock().await.assignments.insert("healthy".into(), AccountProxy {
            url: "http://test-proxy:8080".into(),
            client: Client::new(),
            auth_client: Client::new(),
            failures: 0,
            verified: true,
        });
        let _busy_resolver = manager.switch_lock.lock().await;
        tokio::time::timeout(Duration::from_millis(100), manager.working_client_for("healthy")).await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_millis(100), manager.working_auth_client_for("healthy")).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn auth_uses_the_account_proxy_without_direct_fallback() {
        let (proxy, task) = fake_proxy().await;
        let mut config = Config::from_env();
        config.use_proxies = true;
        config.fallback_to_direct = false;
        let manager = ProxyManager::new(Arc::new(config), None);
        manager.configure(true, vec![proxy.clone()]).await.unwrap();
        manager.working_client_for("account").await.unwrap();
        let auth = manager.working_auth_client_for("account").await.unwrap();
        // The fake proxy answers this unresolvable host; a direct client fails.
        let response = auth.post("http://auth.invalid/token").body("dummy").send().await.unwrap();
        assert!(response.status().is_success());
        assert_eq!(manager.account_routes.lock().await.assignments["account"].url, proxy);
        task.abort();
    }

    #[tokio::test]
    async fn auth_headers_are_separate_from_api_and_redirects_are_not_followed() {
        use axum::{Router, Json, routing::get, http::StatusCode, extract::Request};
        let app = Router::new()
            .route("/headers", get(|request: Request| async move {
                assert_eq!(request.version(), reqwest::Version::HTTP_11);
                let h = request.headers();
                Json(json!({
                    "accept": h.get("accept").unwrap().to_str().unwrap(),
                    "ua": h.get("user-agent").unwrap().to_str().unwrap(),
                    "platform": h.get("x-tidal-platform").and_then(|v| v.to_str().ok()),
                }))
            }))
            .route("/redirect", get(|| async { (StatusCode::FOUND, [("Location", "/headers")]) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let auth = build_auth_client(None, "test-agent").unwrap();
        let api = build_client(None, "test-agent").unwrap();
        let headers: Value = auth.get(format!("{base}/headers")).send().await.unwrap().json().await.unwrap();
        assert_eq!(headers["accept"], "application/json");
        assert_eq!(headers["ua"], "test-agent");
        assert!(headers["platform"].is_null());
        let headers: Value = api.get(format!("{base}/headers")).send().await.unwrap().json().await.unwrap();
        assert_eq!(headers["platform"], "android");
        assert_eq!(headers["accept"], "*/*");
        let redirected = auth.get(format!("{base}/redirect")).send().await.unwrap();
        assert_eq!(redirected.status(), StatusCode::FOUND);
        server.abort();
    }

    #[tokio::test]
    async fn account_failover_keeps_other_binding_and_persists() {
        let (first, first_server) = fake_proxy().await;
        let (second, second_server) = fake_proxy().await;
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE accounts (id TEXT PRIMARY KEY)").execute(&db).await.unwrap();
        sqlx::query("CREATE TABLE proxy_assignments (account_id TEXT PRIMARY KEY, proxy_url TEXT NOT NULL)")
            .execute(&db).await.unwrap();
        for id in ["account-a", "account-b"] {
            sqlx::query("INSERT INTO accounts (id) VALUES (?)").bind(id).execute(&db).await.unwrap();
        }
        let mut config = Config::from_env();
        config.use_proxies = true;
        config.fallback_to_direct = false;
        config.proxies_file = std::path::PathBuf::from("/tmp/hifi-test-no-proxies-file");
        let config = Arc::new(config);
        let manager = ProxyManager::new(config.clone(), Some(db.clone()));
        manager.configure(true, vec![first.clone(), second.clone()]).await.unwrap();
        manager.working_client_for("account-a").await.unwrap();
        manager.working_client_for("account-b").await.unwrap();
        let before = manager.status().await;
        let bindings = before["assignments"].as_array().unwrap();
        let a_before = bindings.iter().find(|v| v["account_id"] == "account-a").unwrap()["proxy"].as_str().unwrap().to_string();
        let b_before = bindings.iter().find(|v| v["account_id"] == "account-b").unwrap()["proxy"].as_str().unwrap().to_string();
        assert_ne!(a_before, b_before);

        for _ in 0..3 { manager.note_failure_for("account-a").await; }
        manager.working_client_for("account-a").await.unwrap();
        let after = manager.status().await;
        let bindings = after["assignments"].as_array().unwrap();
        let a_after = bindings.iter().find(|v| v["account_id"] == "account-a").unwrap()["proxy"].as_str().unwrap();
        let b_after = bindings.iter().find(|v| v["account_id"] == "account-b").unwrap()["proxy"].as_str().unwrap();
        assert_ne!(a_after, a_before);
        assert_eq!(b_after, b_before);

        let restarted = ProxyManager::new(config, Some(db));
        restarted.configure(true, vec![first, second]).await.unwrap();
        restarted.load_assignments().await;
        let restored = restarted.status().await;
        let restored_a = restored["assignments"].as_array().unwrap().iter()
            .find(|v| v["account_id"] == "account-a").unwrap()["proxy"].as_str().unwrap();
        assert_eq!(restored_a, a_after);
        first_server.abort();
        second_server.abort();
    }

    #[tokio::test]
    async fn adding_proxies_moves_only_overloaded_accounts() {
        let mut proxies = Vec::new();
        let mut servers = Vec::new();
        for _ in 0..5 {
            let (url, server) = fake_proxy().await;
            proxies.push(url);
            servers.push(server);
        }
        let mut config = Config::from_env();
        config.use_proxies = true;
        config.fallback_to_direct = false;
        config.proxies_file = std::path::PathBuf::from("/tmp/hifi-test-no-proxies-file");
        let manager = ProxyManager::new(Arc::new(config), None);
        manager.configure(true, proxies[..3].to_vec()).await.unwrap();
        for n in 0..5 {
            manager.working_client_for(&format!("account-{n}")).await.unwrap();
        }
        let before = manager.status().await;
        let before: HashMap<String, String> = before["assignments"].as_array().unwrap().iter()
            .map(|row| (row["account_id"].as_str().unwrap().to_string(), row["proxy"].as_str().unwrap().to_string()))
            .collect();
        assert_eq!(before.values().collect::<std::collections::HashSet<_>>().len(), 3);

        manager.configure(true, proxies).await.unwrap();
        let after = manager.status().await;
        let after: HashMap<String, String> = after["assignments"].as_array().unwrap().iter()
            .map(|row| (row["account_id"].as_str().unwrap().to_string(), row["proxy"].as_str().unwrap().to_string()))
            .collect();
        assert_eq!(after.values().collect::<std::collections::HashSet<_>>().len(), 5);
        assert_eq!(before.iter().filter(|(id, url)|
            after.get(id.as_str()).map(String::as_str) == Some(url.as_str())
        ).count(), 3);
        for server in servers { server.abort(); }
    }
}
