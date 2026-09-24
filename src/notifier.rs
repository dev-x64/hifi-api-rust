use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::settings::AppSettings;

/// Discord-only ban/outage alerts. Empty webhook URL = disabled.
/// Same-type alerts are throttled to at most one per 15 minutes.
pub struct Notifier {
    settings: Arc<AppSettings>,
    client: reqwest::Client,
    last_sent: Mutex<HashMap<(String, String), i64>>,
}

const MIN_INTERVAL_SECS: i64 = 900;

impl Notifier {
    pub fn new(settings: Arc<AppSettings>) -> Arc<Self> {
        if settings.discord_webhook_url().is_empty() {
            tracing::info!("Discord alerts disabled (webhook not configured)");
        } else {
            tracing::info!("Discord alerts enabled");
        }
        Arc::new(Self {
            settings,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to build notifier HTTP client"),
            last_sent: Mutex::new(HashMap::new()),
        })
    }

    pub fn configured(&self) -> bool {
        !self.settings.discord_webhook_url().is_empty()
    }

    /// Only accept Discord webhook endpoints, never arbitrary outbound URLs.
    pub fn validate_webhook_url(raw: &str) -> Result<String, String> {
        let raw = raw.trim();
        let url = reqwest::Url::parse(raw).map_err(|_| "Invalid Discord webhook URL".to_string())?;
        let host = url.host_str().unwrap_or_default();
        let host_allowed = matches!(
            host,
            "discord.com" | "discordapp.com" | "canary.discord.com" | "ptb.discord.com"
        );
        let path: Vec<_> = url
            .path_segments()
            .map(|segments| segments.collect())
            .unwrap_or_default();
        if url.scheme() != "https"
            || !host_allowed
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || path.len() != 4
            || path[0] != "api"
            || path[1] != "webhooks"
            || path[2].is_empty()
            || path[3].is_empty()
        {
            return Err(
                "Use a Discord HTTPS webhook URL (https://discord.com/api/webhooks/ID/TOKEN)"
                    .into(),
            );
        }
        Ok(url.to_string())
    }

    async fn send_to_discord(&self, url: &str, payload: Value) -> Result<(), String> {
        let response = self
            .client
            .post(url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "Discord request timed out"
                } else {
                    "Could not reach Discord"
                }
                .to_string()
            })?;
        if !response.status().is_success() {
            return Err(format!("Discord returned HTTP {}", response.status().as_u16()));
        }
        Ok(())
    }

    fn embed(title: &str, description: &str, color: u32, fields: Vec<Value>) -> Value {
        json!({
            "embeds": [{
                "title": title,
                "description": description,
                "color": color,
                "fields": fields,
                "footer": { "text": "HiFi API" },
                "timestamp": Utc::now().to_rfc3339(),
            }]
        })
    }

    async fn send_throttled(&self, kind: &str, payload: Value) {
        let url = self.settings.discord_webhook_url();
        if url.is_empty() {
            return;
        }
        {
            let mut last = self.last_sent.lock().await;
            let now = Utc::now().timestamp();
            let alert_key = (kind.to_string(), url.clone());
            if let Some(&prev) = last.get(&alert_key) {
                if now - prev < MIN_INTERVAL_SECS {
                    tracing::debug!("Discord alert '{}' throttled", kind);
                    return;
                }
            }
            last.insert(alert_key, now);
        }
        if let Err(e) = self.send_to_discord(&url, payload).await {
            tracing::warn!("Failed to send Discord alert: {}", e);
        }
    }

    /// Fired when Tidal 403s an account (suspension risk).
    pub async fn alert_403(&self, label: &str, healthy: usize, total: usize) {
        let payload = Self::embed(
            "🚨 Account 403 — suspension risk",
            "Tidal forbade this account. Check whether it needs fresh credentials.",
            0xF85149,
            vec![
                json!({"name": "Account", "value": label, "inline": true}),
                json!({"name": "Healthy", "value": format!("{}/{}", healthy, total), "inline": true}),
            ],
        );
        self.send_throttled("403", payload).await;
    }

    /// Fired when no usable account remains.
    pub async fn alert_all_down(&self, total: usize) {
        let payload = Self::embed(
            "🛑 All accounts down",
            "No active account available. Playback is returning 503.",
            0xF85149,
            vec![json!({"name": "Total accounts", "value": total.to_string(), "inline": true})],
        );
        self.send_throttled("down", payload).await;
    }

    /// Fired when auto-heal recovers an account.
    pub async fn alert_healed(&self, label: &str, healthy: usize, total: usize) {
        let payload = Self::embed(
            "✅ Account recovered",
            "Auto-heal refreshed credentials and returned the account to rotation.",
            0x3FB950,
            vec![
                json!({"name": "Account", "value": label, "inline": true}),
                json!({"name": "Healthy", "value": format!("{}/{}", healthy, total), "inline": true}),
            ],
        );
        self.send_throttled("healed", payload).await;
    }

    /// Manual on-demand report from the admin panel (bypasses throttle).
    pub async fn send_report(&self, payload: Value) -> Result<(), String> {
        let url = self.settings.discord_webhook_url();
        if url.is_empty() {
            return Err("Discord webhook is not configured".into());
        }
        self.send_to_discord(&url, payload).await
    }

    /// Overall health snapshot. All values pre-aggregated by the caller —
    /// no account identities in here.
    pub fn status_report(
        healthy: usize,
        total: usize,
        total_requests: u64,
        total_errors: u64,
        cache_hits: u64,
        cache_misses: u64,
        proxy_summary: String,
    ) -> Value {
        let ok = healthy > 0;
        Self::embed(
            if ok {
                "📊 Status — operational"
            } else {
                "📊 Status — DOWN"
            },
            "On-demand snapshot from the admin panel.",
            if ok { 0x1F6FEB } else { 0xF85149 },
            vec![
                json!({"name": "Accounts", "value": format!("{}/{} healthy", healthy, total), "inline": true}),
                json!({"name": "Traffic", "value": format!("{} requests · {} errors", total_requests, total_errors), "inline": true}),
                json!({"name": "Cache", "value": format!("{} hits · {} misses", cache_hits, cache_misses), "inline": true}),
                json!({"name": "Proxies", "value": proxy_summary, "inline": true}),
            ],
        )
    }

    /// Per-account roster. Accounts are codenamed TIDAL-1, TIDAL-2, … in
    /// stable id order — real labels, user IDs and credentials never leave
    /// the server. Caller passes one line per account.
    pub fn accounts_report(total: usize, lines: Vec<(String, String)>) -> Value {
        let mut fields = Vec::new();
        for (code, status) in lines.into_iter().take(25) {
            fields.push(json!({"name": code, "value": status, "inline": true}));
        }
        if fields.is_empty() {
            fields.push(json!({"name": "No accounts", "value": "Add one via the admin panel.", "inline": false}));
        }
        Self::embed(
            "👥 Accounts roster",
            &format!("{} account(s). Codenames are stable per id order within a report.", total),
            0x1F6FEB,
            fields,
        )
    }

    /// Manual test from the admin panel (bypasses throttle).
    pub async fn send_test(&self) -> Result<(), String> {
        let payload = Self::embed(
            "✅ Discord alerts working",
            "Test alert from the HiFi API admin panel. Ban and outage alerts will arrive as embeds like this one.",
            0x3FB950,
            vec![],
        );
        self.send_report(payload).await
    }
}

#[cfg(test)]
mod tests {
    use super::Notifier;

    #[test]
    fn accepts_only_discord_webhook_urls() {
        assert!(
            Notifier::validate_webhook_url("https://discord.com/api/webhooks/123/token").is_ok()
        );
        for url in [
            "http://discord.com/api/webhooks/123/token",
            "https://discord.com.evil.test/api/webhooks/123/token",
            "https://user:pass@discord.com/api/webhooks/123/token",
            "https://discord.com:444/api/webhooks/123/token",
            "https://discord.com/api/webhooks/123",
            "https://discord.com/api/webhooks/123/token?wait=true",
            "https://127.0.0.1/api/webhooks/123/token",
        ] {
            assert!(Notifier::validate_webhook_url(url).is_err(), "{url}");
        }
    }
}
