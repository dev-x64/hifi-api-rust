use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub admin_key: String,
    pub country_code: String,
    pub host: String,
    pub port: u16,
    pub use_proxies: bool,
    pub trust_proxy: bool,
    pub proxies_file: PathBuf,
    pub fallback_to_direct: bool,
    pub max_retries: u32,
    pub discord_webhook_url: String,
    pub api_version: String,
    /// Upstream User-Agent (upstream: USER_AGENT, default okhttp/5.3.2).
    pub user_agent: String,
    /// Verbose upstream logging (upstream: DEV_MODE).
    pub dev_mode: bool,
    /// Rotate proxy on every token refresh (upstream: ROTATE_PROXIES_ON_REFRESH).
    pub rotate_proxies_on_refresh: bool,
    /// Dedicated metadata credential (upstream: CATALOG_CLIENT_ID/...).
    /// Kept out of the playback pool.
    pub catalog_client_id: String,
    pub catalog_client_secret: String,
    pub catalog_refresh_token: String,
    pub catalog_user_id: Option<String>,
    /// Static bearer token for metadata (upstream: CATALOG_TOKEN). No refresh.
    pub catalog_token: String,
    /// Legacy credential file (upstream: TOKEN_FILE, default token.json).
    /// Imported into the DB on startup when present.
    pub token_file: String,
    /// Upstash Redis REST base URL (empty = multi-host sync disabled).
    pub upstash_url: String,
    /// Upstash Redis REST token. Kept in memory only; redacted from Debug.
    pub upstash_token: String,
}

// Manual Debug: the REST token must never appear in logs.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &self.database_url)
            .field("admin_key", &"<redacted>")
            .field("country_code", &self.country_code)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("use_proxies", &self.use_proxies)
            .field("trust_proxy", &self.trust_proxy)
            .field("proxies_file", &self.proxies_file)
            .field("fallback_to_direct", &self.fallback_to_direct)
            .field("max_retries", &self.max_retries)
            .field("discord_webhook_url", &self.discord_webhook_url)
            .field("api_version", &self.api_version)
            .field("user_agent", &self.user_agent)
            .field("dev_mode", &self.dev_mode)
            .field("rotate_proxies_on_refresh", &self.rotate_proxies_on_refresh)
            .field("catalog_client_id", &self.catalog_client_id)
            .field("catalog_refresh_token", &"<redacted>")
            .field("catalog_token", &"<redacted>")
            .field("token_file", &self.token_file)
            .field("upstash_url", &self.upstash_url)
            .field("upstash_token", &"<redacted>")
            .finish()
    }
}

impl Config {
    pub fn from_env() -> Self {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "hifi.db".into());
        let admin_key = std::env::var("ADMIN_KEY").unwrap_or_else(|_| String::new());
        let country_code = std::env::var("COUNTRY_CODE").unwrap_or_else(|_| "US".into());
        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let port = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8000u16);
        let use_proxies = std::env::var("USE_PROXIES")
            .unwrap_or_default()
            .to_lowercase()
            == "true";
        let trust_proxy = std::env::var("TRUST_PROXY_HEADERS")
            .unwrap_or_else(|_| "true".into())
            .to_lowercase()
            == "true";
        let proxies_file = std::env::var("PROXIES_FILE")
            .unwrap_or_else(|_| "proxies.txt".into())
            .into();
        let fallback_to_direct = std::env::var("FALLBACK_TO_DIRECT_CONNECTION")
            .unwrap_or_default()
            .to_lowercase()
            == "true";
        let max_retries = std::env::var("MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2)
            .max(1);
        let discord_webhook_url = std::env::var("DISCORD_WEBHOOK_URL").unwrap_or_default();
        let user_agent = std::env::var("USER_AGENT").unwrap_or_else(|_| "okhttp/5.3.2".into());
        let dev_mode = env_flag("DEV_MODE", false);
        let rotate_proxies_on_refresh = env_flag("ROTATE_PROXIES_ON_REFRESH", false);
        // Dedicated metadata credential (upstream CATALOG_*). Falls back to
        // the main CLIENT_ID/SECRET when only a refresh token is given.
        let main_client_id = std::env::var("CLIENT_ID").unwrap_or_default();
        let main_client_secret = std::env::var("CLIENT_SECRET").unwrap_or_default();
        let catalog_client_id =
            std::env::var("CATALOG_CLIENT_ID").unwrap_or_else(|_| main_client_id.clone());
        let catalog_client_secret =
            std::env::var("CATALOG_CLIENT_SECRET").unwrap_or_else(|_| main_client_secret.clone());
        let catalog_refresh_token = std::env::var("CATALOG_REFRESH_TOKEN").unwrap_or_default();
        let catalog_user_id = std::env::var("CATALOG_USER_ID")
            .ok()
            .filter(|s| !s.is_empty());
        let catalog_token = std::env::var("CATALOG_TOKEN")
            .or_else(|_| std::env::var("CATALOG_ACCESS_TOKEN"))
            .unwrap_or_default();
        let token_file = std::env::var("TOKEN_FILE").unwrap_or_else(|_| "token.json".into());
        let upstash_url = std::env::var("UPSTASH_REDIS_REST_URL")
            .unwrap_or_default()
            .trim()
            .trim_end_matches('/')
            .to_string();
        let upstash_token = std::env::var("UPSTASH_REDIS_REST_TOKEN")
            .unwrap_or_default()
            .trim()
            .to_string();

        Self {
            database_url,
            admin_key,
            country_code,
            host,
            port,
            use_proxies,
            trust_proxy,
            proxies_file,
            fallback_to_direct,
            max_retries,
            discord_webhook_url,
            api_version: env!("CARGO_PKG_VERSION").into(),
            user_agent,
            dev_mode,
            rotate_proxies_on_refresh,
            catalog_client_id,
            catalog_client_secret,
            catalog_refresh_token,
            catalog_user_id,
            catalog_token,
            token_file,
            upstash_url,
            upstash_token,
        }
    }
}

fn env_flag(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| {
            let v = v.to_lowercase();
            v == "true" || v == "1" || v == "yes"
        })
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::Config;

    struct EnvGuard {
        saved: Vec<(String, Option<String>)>,
    }

    impl EnvGuard {
        fn take(keys: &[&str]) -> Self {
            let saved = keys
                .iter()
                .map(|k| (k.to_string(), std::env::var(k).ok()))
                .collect();
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, v) in self.saved.drain(..) {
                unsafe {
                    match v {
                        Some(val) => std::env::set_var(&k, val),
                        None => std::env::remove_var(&k),
                    }
                }
            }
        }
    }

    #[test]
    fn upstream_env_parity() {
        let _guard = EnvGuard::take(&[
            "USER_AGENT",
            "DEV_MODE",
            "ROTATE_PROXIES_ON_REFRESH",
            "CATALOG_TOKEN",
            "CATALOG_ACCESS_TOKEN",
            "TOKEN_FILE",
        ]);
        unsafe {
            std::env::remove_var("USER_AGENT");
            std::env::remove_var("DEV_MODE");
            std::env::remove_var("ROTATE_PROXIES_ON_REFRESH");
            std::env::remove_var("CATALOG_TOKEN");
            std::env::remove_var("CATALOG_ACCESS_TOKEN");
            std::env::remove_var("TOKEN_FILE");
        }
        let cfg = Config::from_env();
        assert_eq!(cfg.user_agent, "okhttp/5.3.2");
        assert!(!cfg.dev_mode);
        assert!(!cfg.rotate_proxies_on_refresh);
        assert!(cfg.catalog_token.is_empty());
        assert_eq!(cfg.token_file, "token.json");

        unsafe {
            std::env::set_var("USER_AGENT", "test-agent/1.0");
            std::env::set_var("DEV_MODE", "true");
            std::env::set_var("ROTATE_PROXIES_ON_REFRESH", "1");
            std::env::set_var("CATALOG_ACCESS_TOKEN", "legacy-fallback");
        }
        let cfg = Config::from_env();
        assert_eq!(cfg.user_agent, "test-agent/1.0");
        assert!(cfg.dev_mode);
        assert!(cfg.rotate_proxies_on_refresh);
        // Legacy CATALOG_ACCESS_TOKEN is honored when CATALOG_TOKEN is unset.
        assert_eq!(cfg.catalog_token, "legacy-fallback");

        unsafe {
            std::env::set_var("CATALOG_TOKEN", "primary");
        }
        let cfg = Config::from_env();
        assert_eq!(cfg.catalog_token, "primary");
    }
}
