use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const FAILURE_WINDOW: Duration = Duration::from_secs(5 * 60);
const LOCK_DURATION: Duration = Duration::from_secs(15 * 60);
const FAILURE_LIMIT: usize = 5;
const MAX_IPS: usize = 50_000;

struct IpActivity {
    failures: VecDeque<Instant>,
    locked_until: Option<Instant>,
    last_seen: Instant,
}

struct GuardState {
    ips: HashMap<IpAddr, IpActivity>,
    last_cleanup: Instant,
}

/// Temporary per-IP lockout for incorrect admin credentials.
pub struct AdminAuthGuard {
    state: Mutex<GuardState>,
}

impl AdminAuthGuard {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(GuardState {
                ips: HashMap::new(),
                last_cleanup: Instant::now(),
            }),
        }
    }

    /// Returns seconds until an existing lock expires.
    pub fn check(&self, ip: IpAddr, now: Instant) -> Option<u64> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let activity = state.ips.get(&ip)?;
        match activity.locked_until {
            Some(until) if until > now => Some(until.duration_since(now).as_secs().max(1)),
            Some(_) => {
                state.ips.remove(&ip);
                None
            }
            None => None,
        }
    }

    /// Records a failed credential. The fifth failure also starts the lock.
    pub fn failed(&self, ip: IpAddr, now: Instant) -> Option<u64> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if now.duration_since(state.last_cleanup) >= FAILURE_WINDOW {
            state.ips.retain(|_, activity| {
                activity.locked_until.is_some_and(|until| until > now)
                    || now.duration_since(activity.last_seen) < FAILURE_WINDOW
            });
            state.last_cleanup = now;
        }

        if let Some(activity) = state.ips.get(&ip) {
            if let Some(until) = activity.locked_until {
                if until > now {
                    return Some(until.duration_since(now).as_secs().max(1));
                }
                state.ips.remove(&ip);
            }
        }

        if !state.ips.contains_key(&ip) && state.ips.len() >= MAX_IPS {
            let victim = state
                .ips
                .iter()
                .filter(|(_, activity)| activity.locked_until.is_none())
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
            failures: VecDeque::new(),
            locked_until: None,
            last_seen: now,
        });
        activity.last_seen = now;
        while activity
            .failures
            .front()
            .is_some_and(|failure| now.duration_since(*failure) >= FAILURE_WINDOW)
        {
            activity.failures.pop_front();
        }
        activity.failures.push_back(now);
        if activity.failures.len() >= FAILURE_LIMIT {
            activity.failures.clear();
            activity.locked_until = Some(now + LOCK_DURATION);
            tracing::warn!(%ip, "Admin authentication locked for 15 minutes");
            return Some(LOCK_DURATION.as_secs());
        }
        None
    }

    /// A successful authentication clears previous mistakes from this IP.
    pub fn clear(&self, ip: IpAddr) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.ips.remove(&ip);
    }
}

#[cfg(test)]
mod tests {
    use super::{AdminAuthGuard, FAILURE_WINDOW, LOCK_DURATION};
    use std::net::IpAddr;
    use std::time::{Duration, Instant};

    #[test]
    fn fifth_failure_locks_only_that_ip_for_fifteen_minutes() {
        let guard = AdminAuthGuard::new();
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        let other: IpAddr = "192.0.2.2".parse().unwrap();
        let start = Instant::now();
        for second in 0..4 {
            assert_eq!(guard.failed(ip, start + Duration::from_secs(second)), None);
        }
        let locked_at = start + Duration::from_secs(4);
        assert_eq!(guard.failed(ip, locked_at), Some(900));
        assert_eq!(
            guard.check(ip, locked_at + Duration::from_secs(1)),
            Some(899)
        );
        assert_eq!(
            guard.failed(ip, locked_at + Duration::from_secs(2)),
            Some(898)
        );
        assert_eq!(guard.check(other, locked_at), None);
        assert_eq!(guard.check(ip, locked_at + LOCK_DURATION), None);
        assert_eq!(guard.failed(ip, locked_at + LOCK_DURATION), None);
    }

    #[test]
    fn old_failures_expire_and_success_resets_count() {
        let guard = AdminAuthGuard::new();
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        let start = Instant::now();
        for second in 0..4 {
            assert_eq!(guard.failed(ip, start + Duration::from_secs(second)), None);
        }
        assert_eq!(guard.failed(ip, start + FAILURE_WINDOW), None);
        guard.clear(ip);
        for second in 0..4 {
            assert_eq!(
                guard.failed(ip, start + FAILURE_WINDOW + Duration::from_secs(second)),
                None
            );
        }
    }
}
