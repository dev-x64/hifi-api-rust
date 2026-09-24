use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PROBE_WINDOW: Duration = Duration::from_secs(60);
const BAN_DURATION: Duration = Duration::from_secs(15 * 60);
const PROBE_LIMIT: usize = 3;
const MAX_IPS: usize = 50_000;

struct IpActivity {
    probes: VecDeque<Instant>,
    banned_until: Option<Instant>,
    last_seen: Instant,
}

struct GuardState {
    ips: HashMap<IpAddr, IpActivity>,
    last_cleanup: Instant,
}

/// A local, temporary IP ban for repeated requests to known scanner paths.
pub struct ScannerGuard {
    state: Mutex<GuardState>,
}

impl ScannerGuard {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(GuardState {
                ips: HashMap::new(),
                last_cleanup: Instant::now(),
            }),
        }
    }

    /// Returns the seconds left on a ban, including when this request starts it.
    pub fn check(&self, ip: IpAddr, path: &str, now: Instant) -> Option<u64> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if now.duration_since(state.last_cleanup) >= PROBE_WINDOW {
            state.ips.retain(|_, activity| {
                activity.banned_until.is_some_and(|until| until > now)
                    || now.duration_since(activity.last_seen) < PROBE_WINDOW
            });
            state.last_cleanup = now;
        }

        if let Some(activity) = state.ips.get_mut(&ip) {
            if let Some(until) = activity.banned_until {
                if until > now {
                    return Some(until.duration_since(now).as_secs().max(1));
                }
                // An expired ban starts with a clean probe count.
                state.ips.remove(&ip);
            }
        }

        if !is_scanner_path(path) {
            return None;
        }

        if !state.ips.contains_key(&ip) && state.ips.len() >= MAX_IPS {
            // Keep memory bounded under IP rotation. Prefer evicting an IP
            // that is not currently banned.
            let victim = state
                .ips
                .iter()
                .filter(|(_, activity)| activity.banned_until.is_none())
                .min_by_key(|(_, activity)| activity.last_seen)
                .or_else(|| {
                    state
                        .ips
                        .iter()
                        .min_by_key(|(_, activity)| activity.last_seen)
                })
                .map(|(ip, _)| *ip);
            if let Some(victim) = victim {
                state.ips.remove(&victim);
            }
        }

        let activity = state.ips.entry(ip).or_insert_with(|| IpActivity {
            probes: VecDeque::new(),
            banned_until: None,
            last_seen: now,
        });
        activity.last_seen = now;
        while activity
            .probes
            .front()
            .is_some_and(|probe| now.duration_since(*probe) >= PROBE_WINDOW)
        {
            activity.probes.pop_front();
        }
        activity.probes.push_back(now);
        if activity.probes.len() >= PROBE_LIMIT {
            activity.probes.clear();
            activity.banned_until = Some(now + BAN_DURATION);
            tracing::warn!(%ip, "Scanner IP banned for 15 minutes");
            return Some(BAN_DURATION.as_secs());
        }
        None
    }
}

fn is_scanner_path(path: &str) -> bool {
    let decoded = percent_encoding::percent_decode_str(path).decode_utf8_lossy();
    let path = decoded.to_ascii_lowercase();
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    segments.iter().any(|segment| {
        *segment == ".env"
            || segment.starts_with(".env.")
            || *segment == ".git"
            || segment.starts_with("wp-")
            || *segment == "wordpress"
            || *segment == "xmlrpc.php"
            || *segment == "phpinfo.php"
    }) || segments.windows(2).any(|pair| pair == ["var", "www"])
}

#[cfg(test)]
mod tests {
    use super::{is_scanner_path, ScannerGuard, BAN_DURATION, PROBE_WINDOW};
    use std::net::IpAddr;
    use std::time::{Duration, Instant};

    #[test]
    fn recognizes_scanner_paths_without_counting_normal_misses() {
        for path in [
            "/2019/wp-includes",
            "/wordpress/wp-includes",
            "/xmlrpc.php",
            "/phpinfo.php",
            "/.env.production",
            "/api/.env",
            "/.git/config",
            "/var/www",
            "/%2eenv",
            "/WP-LOGIN.PHP",
        ] {
            assert!(is_scanner_path(path), "{path}");
        }
        for path in ["/", "/health", "/track/123", "/missing", "/admin/settings"] {
            assert!(!is_scanner_path(path), "{path}");
        }
    }

    #[test]
    fn bans_on_third_probe_and_expires_after_fifteen_minutes() {
        let guard = ScannerGuard::new();
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        let other: IpAddr = "192.0.2.2".parse().unwrap();
        let start = Instant::now();
        assert_eq!(guard.check(ip, "/.env", start), None);
        assert_eq!(guard.check(ip, "/missing", start), None);
        assert_eq!(guard.check(other, "/.env", start), None);
        assert_eq!(
            guard.check(ip, "/wp-includes", start + Duration::from_secs(1)),
            None
        );
        assert_eq!(
            guard.check(ip, "/xmlrpc.php", start + Duration::from_secs(2)),
            Some(900)
        );
        assert_eq!(
            guard.check(ip, "/track/123", start + Duration::from_secs(3)),
            Some(899)
        );
        assert_eq!(
            guard.check(ip, "/health", start + BAN_DURATION + Duration::from_secs(2)),
            None
        );
        assert_eq!(
            guard.check(ip, "/.env", start + BAN_DURATION + Duration::from_secs(3)),
            None
        );
    }

    #[test]
    fn probes_outside_the_window_do_not_accumulate() {
        let guard = ScannerGuard::new();
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        let start = Instant::now();
        assert_eq!(guard.check(ip, "/.env", start), None);
        assert_eq!(guard.check(ip, "/.git/config", start + PROBE_WINDOW), None);
        assert_eq!(
            guard.check(
                ip,
                "/xmlrpc.php",
                start + PROBE_WINDOW + Duration::from_secs(1)
            ),
            None
        );
    }
}
