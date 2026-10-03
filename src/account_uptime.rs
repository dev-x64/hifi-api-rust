//! Availability is the last known enabled/disabled pool state, not a probe of Tidal.
//! Only observed history is counted; existing accounts are never backfilled as up.
use serde::Serialize;
use sqlx::SqlitePool;

pub const WINDOW_SECS: i64 = 7 * 24 * 60 * 60;

#[derive(Clone, Debug)]
struct Event {
    at: i64,
    active: bool,
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
}

#[derive(Debug, Serialize)]
pub struct UptimeSummary {
    pub window_start: i64,
    pub window_end: i64,
    pub up_seconds: i64,
    pub down_seconds: i64,
    pub unknown_seconds: i64,
    /// Percentage of the observed period; None until any time is observed.
    pub percentage: Option<f64>,
    pub segments: Vec<UptimeSegment>,
}

impl AccountUptime {
    pub async fn load(db: &SqlitePool, id: &str, now: i64) -> Result<Self, sqlx::Error> {
        let rows: Vec<(i64, bool)> = sqlx::query_as(
            "SELECT observed_at, is_active FROM account_uptime_events
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
                .map(|(at, active)| Event { at, active })
                .collect(),
        })
    }

    /// Called under the per-account mutex so rapid transitions keep their order.
    pub async fn observe(
        &mut self,
        db: Option<&SqlitePool>,
        id: &str,
        active: bool,
        now: i64,
    ) -> Result<(), sqlx::Error> {
        self.prune(now);
        if self.events.last().is_some_and(|e| e.active == active) {
            return Ok(());
        }
        // Do not let a backwards wall-clock adjustment reorder transitions.
        let at = self.events.last().map_or(now, |e| now.max(e.at));
        self.events.push(Event { at, active });
        if let Some(db) = db {
            let mut tx = db.begin().await?;
            sqlx::query(
                "INSERT INTO account_uptime_events (account_id, observed_at, is_active) VALUES (?, ?, ?)",
            )
            .bind(id)
            .bind(at)
            .bind(active)
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
        let mut active = None;
        for event in &self.events {
            if event.at > now {
                break;
            }
            if event.at > cursor {
                segments.push(UptimeSegment {
                    start: cursor,
                    end: event.at,
                    active,
                });
                cursor = event.at;
            }
            active = Some(event.active);
        }
        if cursor < now {
            segments.push(UptimeSegment {
                start: cursor,
                end: now,
                active,
            });
        }
        let mut up = 0;
        let mut down = 0;
        for segment in &segments {
            match segment.active {
                Some(true) => up += segment.end - segment.start,
                Some(false) => down += segment.end - segment.start,
                None => {}
            }
        }
        UptimeSummary {
            window_start: start,
            window_end: now,
            up_seconds: up,
            down_seconds: down,
            unknown_seconds: WINDOW_SECS - up - down,
            percentage: (up + down > 0).then(|| up as f64 * 100.0 / (up + down) as f64),
            segments,
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
                },
                Event {
                    at: now - 100,
                    active: false,
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
                },
                Event {
                    at: WINDOW_SECS,
                    active: false,
                },
                Event {
                    at: now - 60,
                    active: true,
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
            history.observe(Some(&db), "a", active, at).await.unwrap();
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
            .observe(Some(&db), "a", true, 1300 + WINDOW_SECS)
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
}
