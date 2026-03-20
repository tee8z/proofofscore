use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};

use crate::domain::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEvent {
    pub id: i64,
    pub event_id: String,
    pub event_type: String,
    pub event_json: String,
    pub related_user_id: Option<i64>,
    pub related_date: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct GameInputLog {
    pub id: i64,
    pub session_id: String,
    pub input_log: Vec<u8>,
    pub input_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct LedgerStore {
    db: Pool<Sqlite>,
}

impl LedgerStore {
    pub fn new(db: Pool<Sqlite>) -> Self {
        Self { db }
    }

    pub async fn save_event(
        &self,
        event_id: &str,
        event_type: &str,
        event_json: &str,
        user_id: Option<i64>,
        date: Option<&str>,
    ) -> Result<(), Error> {
        sqlx::query(
            r#"
            INSERT INTO ledger_events (event_id, event_type, event_json, related_user_id, related_date)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(event_id)
        .bind(event_type)
        .bind(event_json)
        .bind(user_id)
        .bind(date)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    pub async fn get_events_by_date(&self, date: &str) -> Result<Vec<LedgerEvent>, Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, event_id, event_type, event_json, related_user_id, related_date, created_at
            FROM ledger_events
            WHERE related_date = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(date)
        .fetch_all(&self.db)
        .await?;

        let events = rows
            .into_iter()
            .map(|row| LedgerEvent {
                id: row.get("id"),
                event_id: row.get("event_id"),
                event_type: row.get("event_type"),
                event_json: row.get("event_json"),
                related_user_id: row.get("related_user_id"),
                related_date: row.get("related_date"),
                created_at: row.get("created_at"),
            })
            .collect();

        Ok(events)
    }

    pub async fn get_events_by_type(&self, event_type: &str) -> Result<Vec<LedgerEvent>, Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, event_id, event_type, event_json, related_user_id, related_date, created_at
            FROM ledger_events
            WHERE event_type = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(event_type)
        .fetch_all(&self.db)
        .await?;

        let events = rows
            .into_iter()
            .map(|row| LedgerEvent {
                id: row.get("id"),
                event_id: row.get("event_id"),
                event_type: row.get("event_type"),
                event_json: row.get("event_json"),
                related_user_id: row.get("related_user_id"),
                related_date: row.get("related_date"),
                created_at: row.get("created_at"),
            })
            .collect();

        Ok(events)
    }

    pub async fn get_events_by_user(&self, user_id: i64) -> Result<Vec<LedgerEvent>, Error> {
        let rows = sqlx::query(
            r#"
            SELECT id, event_id, event_type, event_json, related_user_id, related_date, created_at
            FROM ledger_events
            WHERE related_user_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.db)
        .await?;

        let events = rows
            .into_iter()
            .map(|row| LedgerEvent {
                id: row.get("id"),
                event_id: row.get("event_id"),
                event_type: row.get("event_type"),
                event_json: row.get("event_json"),
                related_user_id: row.get("related_user_id"),
                related_date: row.get("related_date"),
                created_at: row.get("created_at"),
            })
            .collect();

        Ok(events)
    }

    pub async fn get_event_by_id(&self, event_id: &str) -> Result<Option<LedgerEvent>, Error> {
        let row = sqlx::query(
            r#"
            SELECT id, event_id, event_type, event_json, related_user_id, related_date, created_at
            FROM ledger_events
            WHERE event_id = ?
            "#,
        )
        .bind(event_id)
        .fetch_optional(&self.db)
        .await?;

        Ok(row.map(|row| LedgerEvent {
            id: row.get("id"),
            event_id: row.get("event_id"),
            event_type: row.get("event_type"),
            event_json: row.get("event_json"),
            related_user_id: row.get("related_user_id"),
            related_date: row.get("related_date"),
            created_at: row.get("created_at"),
        }))
    }

    pub async fn save_input_log(
        &self,
        session_id: &str,
        input_log: &[u8],
        input_hash: &str,
    ) -> Result<(), Error> {
        sqlx::query(
            r#"
            INSERT INTO game_input_logs (session_id, input_log, input_hash)
            VALUES (?, ?, ?)
            "#,
        )
        .bind(session_id)
        .bind(input_log)
        .bind(input_hash)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    pub async fn get_input_log(&self, session_id: &str) -> Result<Option<GameInputLog>, Error> {
        let row = sqlx::query(
            r#"
            SELECT id, session_id, input_log, input_hash, created_at
            FROM game_input_logs
            WHERE session_id = ?
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.db)
        .await?;

        Ok(row.map(|row| GameInputLog {
            id: row.get("id"),
            session_id: row.get("session_id"),
            input_log: row.get("input_log"),
            input_hash: row.get("input_hash"),
            created_at: row.get("created_at"),
        }))
    }
}
