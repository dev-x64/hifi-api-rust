CREATE TABLE IF NOT EXISTS account_uptime_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    observed_at INTEGER NOT NULL,
    is_active INTEGER NOT NULL CHECK (is_active IN (0, 1))
);
CREATE INDEX IF NOT EXISTS account_uptime_events_account_time
    ON account_uptime_events(account_id, observed_at, id);
