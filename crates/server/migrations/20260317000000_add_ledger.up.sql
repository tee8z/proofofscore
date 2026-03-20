CREATE TABLE ledger_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    event_type TEXT NOT NULL,
    event_json TEXT NOT NULL,
    related_user_id INTEGER,
    related_date TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_ledger_events_type ON ledger_events(event_type);
CREATE INDEX idx_ledger_events_date ON ledger_events(related_date);
CREATE INDEX idx_ledger_events_user ON ledger_events(related_user_id);

ALTER TABLE game_sessions ADD COLUMN seed TEXT;

CREATE TABLE game_input_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL UNIQUE,
    input_log BLOB NOT NULL,
    input_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
