//! Availability tracks enabled pool state and whether a usable token is present.
//! Only observed history is counted; existing accounts are never backfilled as up.
use serde::Serialize;
use sqlx::SqlitePool;

pub const WINDOW_SECS: i64 = 7 * 24 * 60 * 60;

#[derive(Clone, Debug)]
struct Event {
    at: i64,
    active: bool,
    token_ready: bool,
    token_expires_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UptimeStatus {
    Up,
    Down,
    Waiting,
}

impl Event {
    fn status(&self, now: i64) -> UptimeStatus {
        if !self.active {
            UptimeStatus::Down
        } else if !self.token_ready || self.token_expires_at.is_some_and(|expires| expires <= now) {
            UptimeStatus::Waiting
        } else {
            UptimeStatus::Up
        }
    }
}

#[derive(Default)]
pub struct AccountUptime {
    events: Vec<Event>,
}

#[derive(Debug, Serialize)]
pub struct UptimeSegment {
    pub start: i64,
    pub end: i64,
    /// None means no history was recorded yet.
    pub active: Option<bool>,
    pub status: Option<UptimeStatus>,
}

#[derive(Debug, Serialize)]
pub struct UptimeSummary {
    pub current_status: Option<UptimeStatus>,
    pub window_start: i64,
    pub window_end: i64,
    pub up_seconds: i64,
    pub down_seconds: i64,
    pub waiting_seconds: i64,
    pub unknown_seconds: i64,
    /// Percentage of the observed period; None until any time is observed.
    pub percentage: Option<f64>,
    pub segments: Vec<UptimeSegment>,
}

impl AccountUptime {
    pub async fn load(db: &SqlitePool, id: &str, now: i64) -> Result<Self, sqlx::Error> {
        let rows: Vec<(i64, bool, bool, Option<i64>)> = sqlx::query_as(
            "SELECT observed_at, is_active, token_ready, token_expires_at FROM account_uptime_events
             WHERE account_id = ? AND (observed_at >= ? OR id = (
                 SELECT id FROM account_uptime_events WHERE account_id = ? AND observed_at < ?
                 ORDER BY observed_at DESC, id DESC LIMIT 1
             )) ORDER BY observed_at, id",
        )
        .bind(id)
        .bind(now - WINDOW_SECS)
        .bind(id)
        .bind(now - WINDOW_SECS)
        .fetch_all(db)
        .await?;
        Ok(Self {
            events: rows
                .into_iter()
                .map(|(at, active, token_ready, token_expires_at)| Event {
                    at,
                    active,
                    token_ready,
                    token_expires_at,
                })
                .collect(),
        })
    }

    /// Called under the per-account mutex so rapid transitions keep their order.
    pub async fn observe(
        &mut self,
        db: Option<&SqlitePool>,
        id: &str,
        active: bool,
        token_ready: bool,
        token_expires_at: Option<i64>,
        now: i64,
    ) -> Result<(), sqlx::Error> {
        self.prune(now);
        let token_ready = active && token_ready;
        let token_expires_at = if token_ready { token_expires_at } else { None };
        if self.events.last().is_some_and(|e| {
            e.active == active
                && e.token_ready == token_ready
                && e.token_expires_at == token_expires_at
        }) {
            return Ok(());
        }
        // Do not let a backwards wall-clock adjustment reorder transitions.
        let at = self.events.last().map_or(now, |e| now.max(e.at));
        self.events.push(Event {
            at,
            active,
            token_ready,
            token_expires_at,
        });
        if let Some(db) = db {
            let mut tx = db.begin().await?;
            sqlx::query(
                "INSERT INTO account_uptime_events (account_id, observed_at, is_active, token_ready, token_expires_at) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(at)
            .bind(active)
            .bind(token_ready)
            .bind(token_expires_at)
            .execute(&mut *tx)
            .await?;
            // Keep the last event before the window as its initial state.
            sqlx::query(
                "DELETE FROM account_uptime_events WHERE account_id = ? AND observed_at < ?
                 AND id != (SELECT id FROM account_uptime_events WHERE account_id = ? AND observed_at < ?
                            ORDER BY observed_at DESC, id DESC LIMIT 1)",
            )
            .bind(id)
            .bind(now - WINDOW_SECS)
            .bind(id)
            .bind(now - WINDOW_SECS)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
        }
        Ok(())
    }

    fn prune(&mut self, now: i64) {
        let old = self.events.partition_point(|e| e.at < now - WINDOW_SECS);
        if old > 1 {
            self.events.drain(..old - 1);
        }
    }

    pub fn summary(&mut self, now: i64) -> UptimeSummary {
        self.prune(now);
        let start = now - WINDOW_SECS;
        let mut segments = Vec::new();
        let mut cursor = start;
        let mut previous: Option<&Event> = None;
        for event in &self.events {
            if event.at > now {
                break;
            }
            if event.at > cursor {
                Self::append_interval(&mut segments, cursor, event.at, previous);
                cursor = event.at;
            }
            previous = Some(event);
        }
        if cursor < now {
            Self::append_interval(&mut segments, cursor, now, previous);
        }
        let mut up = 0;
        let mut down = 0;
        let mut waiting = 0;
        for segment in &segments {
            match segment.status {
                Some(UptimeStatus::Up) => up += segment.end - segment.start,
                Some(UptimeStatus::Down) => down += segment.end - segment.start,
                Some(UptimeStatus::Waiting) => waiting += segment.end - segment.start,
                None => {}
            }
        }
        UptimeSummary {
            current_status: previous.map(|event| event.status(now)),
            window_start: start,
            window_end: now,
            up_seconds: up,
            down_seconds: down,
            waiting_seconds: waiting,
            unknown_seconds: WINDOW_SECS - up - down - waiting,
            percentage: (up + down + waiting > 0)
                .then(|| up as f64 * 100.0 / (up + down + waiting) as f64),
            segments,
        }
    }

    fn append_interval(
        segments: &mut Vec<UptimeSegment>,
        start: i64,
        end: i64,
        event: Option<&Event>,
    ) {
        if let Some(event) = event.filter(|e| e.active && e.token_ready) {
            if let Some(expires) = event
                .token_expires_at
                .filter(|expires| *expires > start && *expires < end)
            {
                Self::append_segment(segments, start, expires, Some(event));
                Self::append_segment(segments, expires, end, Some(event));
                return;
            }
        }
        Self::append_segment(segments, start, end, event);
    }

    fn append_segment(
        segments: &mut Vec<UptimeSegment>,
        start: i64,
        end: i64,
        event: Option<&Event>,
    ) {
        let status = event.map(|e| e.status(start));
        if let Some(last) = segments
            .last_mut()
            .filter(|s| s.end == start && s.status == status)
        {
            last.end = end;
        } else {
            segments.push(UptimeSegment {
                start,
                end,
                active: event.map(|e| e.active),
                status,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_history_does_not_invent_uptime() {
        let now = WINDOW_SECS + 1000;
        let mut history = AccountUptime {
            events: vec![
                Event {
                    at: now - 300,
                    active: true,
                    token_ready: true,
                    token_expires_at: None,
                },
                Event {
                    at: now - 100,
                    active: false,
                    token_ready: false,
                    token_expires_at: None,
                },
            ],
        };
        let summary = history.summary(now);
        assert_eq!(summary.up_seconds, 200);
        assert_eq!(summary.down_seconds, 100);
        assert_eq!(summary.unknown_seconds, WINDOW_SECS - 300);
        assert!((summary.percentage.unwrap() - 200.0 / 3.0).abs() < 0.001);
        assert_eq!(summary.segments[0].active, None);
    }

    #[test]
    fn rolling_window_keeps_its_initial_state() {
        let now = WINDOW_SECS * 3;
        let mut history = AccountUptime {
            events: vec![
                Event {
                    at: 1,
                    active: true,
                    token_ready: true,
                    token_expires_at: None,
                },
                Event {
                    at: WINDOW_SECS,
                    active: false,
                    token_ready: false,
                    token_expires_at: None,
                },
                Event {
                    at: now - 60,
                    active: true,
                    token_ready: true,
                    token_expires_at: None,
                },
            ],
        };
        let summary = history.summary(now);
        assert_eq!(summary.down_seconds, WINDOW_SECS - 60);
        assert_eq!(summary.up_seconds, 60);
        assert_eq!(summary.unknown_seconds, 0);
        assert_eq!(summary.segments[0].start, now - WINDOW_SECS);
        assert_eq!(history.events.len(), 2);
    }

    #[tokio::test]
    async fn history_survives_reload_and_preserves_same_second_transitions() {
        let db = crate::db::init_pool("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO accounts (id, client_id, client_secret, refresh_token, created_at, updated_at) VALUES ('a', 'c', 's', 'r', 1, 1)")
            .execute(&db).await.unwrap();
        let mut history = AccountUptime::default();
        for (at, active) in [
            (1000, true),
            (1100, false),
            (1100, true),
            (1200, false),
            (1200, false),
        ] {
            history
                .observe(Some(&db), "a", active, active, None, at)
                .await
                .unwrap();
        }
        let mut restored = AccountUptime::load(&db, "a", 1300).await.unwrap();
        let summary = restored.summary(1300);
        assert_eq!(summary.up_seconds, 200);
        assert_eq!(summary.down_seconds, 100);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM account_uptime_events")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count, 4);

        // A later transition prunes old rows but retains the boundary state.
        restored
            .observe(Some(&db), "a", true, true, None, 1300 + WINDOW_SECS)
            .await
            .unwrap();
        let mut restored = AccountUptime::load(&db, "a", 1400 + WINDOW_SECS)
            .await
            .unwrap();
        let summary = restored.summary(1400 + WINDOW_SECS);
        assert_eq!(summary.up_seconds, 100);
        assert_eq!(summary.down_seconds, WINDOW_SECS - 100);
        assert_eq!(summary.unknown_seconds, 0);
        db.close().await;
    }

    #[test]
    fn empty_history_is_entirely_unknown() {
        let summary = AccountUptime::default().summary(WINDOW_SECS);
        assert_eq!(summary.unknown_seconds, WINDOW_SECS);
        assert_eq!(summary.percentage, None);
        assert_eq!(summary.segments.len(), 1);
    }

    #[tokio::test]
    async fn waiting_and_expired_tokens_do_not_count_as_uptime() {
        let mut history = AccountUptime::default();
        history
            .observe(None, "a", true, false, None, 1000)
            .await
            .unwrap();
        history
            .observe(None, "a", true, true, Some(1300), 1100)
            .await
            .unwrap();
        // Expiration changes the timeline even without a request or observer tick.
        let summary = history.summary(1400);
        assert_eq!(summary.up_seconds, 200);
        assert_eq!(summary.waiting_seconds, 200);
        assert_eq!(summary.down_seconds, 0);
        assert_eq!(summary.percentage, Some(50.0));
        assert_eq!(summary.current_status, Some(UptimeStatus::Waiting));
        assert_eq!(summary.segments.last().unwrap().start, 1300);

        history
            .observe(None, "a", true, true, Some(2000), 1500)
            .await
            .unwrap();
        let summary = history.summary(1600);
        assert_eq!(summary.up_seconds, 300);
        assert_eq!(summary.waiting_seconds, 300);
        assert_eq!(summary.current_status, Some(UptimeStatus::Up));
    }

    #[tokio::test]
    async fn renewal_before_expiry_keeps_the_timeline_green() {
        let mut history = AccountUptime::default();
        history
            .observe(None, "a", true, true, Some(1300), 1000)
            .await
            .unwrap();
        history
            .observe(None, "a", true, true, Some(1800), 1200)
            .await
            .unwrap();
        let summary = history.summary(1400);
        assert_eq!(summary.up_seconds, 400);
        assert_eq!(summary.waiting_seconds, 0);
        assert_eq!(summary.percentage, Some(100.0));
    }

    #[tokio::test]
    async fn yellow_history_and_expiration_survive_reload() {
        let db = crate::db::init_pool("sqlite::memory:").await.unwrap();
        sqlx::query("INSERT INTO accounts (id, client_id, client_secret, refresh_token, created_at, updated_at) VALUES ('a', 'c', 's', 'r', 1, 1)")
            .execute(&db).await.unwrap();
        let mut history = AccountUptime::default();
        history
            .observe(Some(&db), "a", true, false, None, 1000)
            .await
            .unwrap();
        history
            .observe(Some(&db), "a", true, true, Some(1300), 1100)
            .await
            .unwrap();
        let mut restored = AccountUptime::load(&db, "a", 1400).await.unwrap();
        let summary = restored.summary(1400);
        assert_eq!(summary.up_seconds, 200);
        assert_eq!(summary.waiting_seconds, 200);
        assert_eq!(summary.percentage, Some(50.0));
        db.close().await;
    }

    #[tokio::test]
    async fn old_database_history_is_migrated_without_recoloring_the_past() {
        let path =
            std::env::temp_dir().join(format!("hifi-old-uptime-{}.db", uuid::Uuid::new_v4()));
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let old_db = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::query("CREATE TABLE account_uptime_events (id INTEGER PRIMARY KEY, account_id TEXT NOT NULL, observed_at INTEGER NOT NULL, is_active INTEGER NOT NULL CHECK (is_active IN (0, 1)))")
            .execute(&old_db).await.unwrap();
        sqlx::query("INSERT INTO account_uptime_events VALUES (1, 'a', 1000, 1)")
            .execute(&old_db)
            .await
            .unwrap();
        old_db.close().await;
        let db = crate::db::init_pool(&url).await.unwrap();
        let mut history = AccountUptime::load(&db, "a", 1100).await.unwrap();
        assert_eq!(history.summary(1100).up_seconds, 100);
        // Starting the app again must not attempt to add the columns twice.
        let second = crate::db::init_pool(&url).await.unwrap();
        second.close().await;
        db.close().await;
        std::fs::remove_file(path).unwrap();
    }
}
