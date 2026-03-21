use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};
use time::OffsetDateTime;

use crate::domain::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePayment {
    pub id: i64,
    pub user_id: i64,
    pub payment_id: String,
    pub invoice: String,
    pub amount_sats: i64,
    pub status: String,
    pub plays_remaining: i64,
    pub created_at: String,
    pub updated_at: String,
    pub paid_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrizePayout {
    pub id: i64,
    pub user_id: i64,
    pub date: String,
    pub score: i64,
    pub amount_sats: i64,
    pub payment_request: Option<String>,
    pub payment_id: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub paid_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopScorer {
    pub user_id: i64,
    pub score: i64,
    pub games_played: i64,
    pub username: String,
}

#[derive(Debug, Clone)]
pub struct PaymentStore {
    db: Pool<Sqlite>,
}

impl PaymentStore {
    pub fn new(db: Pool<Sqlite>) -> Self {
        Self { db }
    }

    pub async fn ping(&self) -> Result<(), Error> {
        sqlx::query!("SELECT 1 as ping").fetch_one(&self.db).await?;
        Ok(())
    }

    // Create a new payment record for a game
    pub async fn create_game_payment(
        &self,
        user_id: i64,
        payment_id: &str,
        invoice: &str,
        amount_sats: i64,
    ) -> Result<GamePayment, Error> {
        let now = OffsetDateTime::now_utc().to_string();

        let id = sqlx::query!(
            r#"
            INSERT INTO game_payments
            (user_id, payment_id, invoice, amount_sats, status, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            user_id,
            payment_id,
            invoice,
            amount_sats,
            "pending", // Initial status
            now,
            now
        )
        .execute(&self.db)
        .await?
        .last_insert_rowid();

        Ok(GamePayment {
            id,
            user_id,
            payment_id: payment_id.to_string(),
            invoice: invoice.to_string(),
            amount_sats,
            status: "pending".to_string(),
            plays_remaining: 0,
            created_at: now.clone(),
            updated_at: now,
            paid_at: None,
        })
    }

    // Get a payment by its payment_id
    pub async fn get_payment_by_id(&self, payment_id: &str) -> Result<Option<GamePayment>, Error> {
        let payment = sqlx::query_as!(
            GamePayment,
            r#"
            SELECT id, user_id, payment_id, invoice, amount_sats, status, plays_remaining, created_at, updated_at, paid_at
            FROM game_payments
            WHERE payment_id = ?
            "#,
            payment_id
        )
        .fetch_optional(&self.db)
        .await?;

        Ok(payment)
    }

    // Update a payment status
    pub async fn update_payment_status(
        &self,
        payment_id: &str,
        status: &str,
    ) -> Result<Option<GamePayment>, Error> {
        let now = OffsetDateTime::now_utc().to_string();
        let paid_at = if status == "paid" {
            Some(now.clone())
        } else {
            None
        };

        let result = sqlx::query!(
            r#"
            UPDATE game_payments
            SET status = ?, updated_at = ?, paid_at = ?
            WHERE payment_id = ?
            "#,
            status,
            now,
            paid_at,
            payment_id
        )
        .execute(&self.db)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        self.get_payment_by_id(payment_id).await
    }

    // Get pending payment for a user
    pub async fn get_pending_payment_for_user(
        &self,
        user_id: i64,
    ) -> Result<Option<GamePayment>, Error> {
        let payment = sqlx::query_as!(
            GamePayment,
            r#"
            SELECT id, user_id, payment_id, invoice, amount_sats, status, plays_remaining, created_at, updated_at, paid_at
            FROM game_payments
            WHERE user_id = ? AND status = 'pending'
            ORDER BY created_at DESC
            LIMIT 1
            "#,
            user_id
        )
        .fetch_optional(&self.db)
        .await?;

        Ok(payment)
    }

    /// Check if a user has a paid payment with remaining plays.
    /// Returns the number of plays remaining, or 0 if none.
    pub async fn get_remaining_plays(&self, user_id: i64) -> Result<i64, Error> {
        let result = sqlx::query!(
            r#"
            SELECT COALESCE(SUM(plays_remaining), 0) as total
            FROM game_payments
            WHERE user_id = ? AND status = 'paid' AND plays_remaining > 0
            "#,
            user_id,
        )
        .fetch_one(&self.db)
        .await?;

        Ok(result.total)
    }

    /// Decrement plays_remaining on the oldest paid payment with plays left.
    /// Returns the new total remaining plays for this user.
    pub async fn use_one_play(&self, user_id: i64) -> Result<i64, Error> {
        let now = OffsetDateTime::now_utc().to_string();

        // Decrement the oldest payment that still has plays
        sqlx::query!(
            r#"
            UPDATE game_payments
            SET plays_remaining = plays_remaining - 1, updated_at = ?
            WHERE id = (
                SELECT id FROM game_payments
                WHERE user_id = ? AND status = 'paid' AND plays_remaining > 0
                ORDER BY paid_at ASC
                LIMIT 1
            )
            "#,
            now,
            user_id,
        )
        .execute(&self.db)
        .await?;

        self.get_remaining_plays(user_id).await
    }

    /// Set plays_remaining when a payment is confirmed.
    pub async fn set_plays_remaining(&self, payment_id: &str, plays: i32) -> Result<(), Error> {
        let now = OffsetDateTime::now_utc().to_string();
        sqlx::query!(
            r#"
            UPDATE game_payments
            SET plays_remaining = ?, updated_at = ?
            WHERE payment_id = ?
            "#,
            plays,
            now,
            payment_id,
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }

    // Get the count of paid games for a specific date
    pub async fn count_games_for_date(&self, date: &str) -> Result<i64, Error> {
        // Create date range for the given date
        let start_date = format!("{} 00:00:00", date);
        let end_date = format!("{} 23:59:59", date);

        let result = sqlx::query!(
            r#"
            SELECT COUNT(*) as count
            FROM game_payments
            WHERE status = 'paid' AND paid_at >= ? AND paid_at <= ?
            "#,
            start_date,
            end_date
        )
        .fetch_one(&self.db)
        .await?;

        Ok(result.count)
    }

    // Find the top scorer for a given date
    // TODO( @tee8z): clean up query
    pub async fn get_top_scorer_for_date(&self, date: &str) -> Result<Option<TopScorer>, Error> {
        // Create date range for the given date
        let start_date = format!("{} 00:00:00", date);
        let end_date = format!("{} 23:59:59", date);

        // Use raw query to avoid macro issues
        let query = format!(
            "SELECT
                s.user_id,
                COALESCE(MAX(s.score), 0) as top_score,
                COUNT(*) as games_played,
                u.username
            FROM scores s
            JOIN users u ON s.user_id = u.id
            WHERE s.created_at >= '{}' AND s.created_at <= '{}'
            GROUP BY s.user_id
            ORDER BY top_score DESC
            LIMIT 1",
            start_date, end_date
        );

        let row = sqlx::query(&query).fetch_optional(&self.db).await?;

        match row {
            Some(row) => {
                let user_id: i64 = row.try_get("user_id")?;
                let top_score: i64 = row.try_get("top_score")?;
                let games_played: i64 = row.try_get("games_played")?;
                let username: String = row.try_get("username")?;

                Ok(Some(TopScorer {
                    user_id,
                    score: top_score,
                    games_played,
                    username,
                }))
            }
            None => Ok(None),
        }
    }

    // Check if a user was the top scorer for a given date
    pub async fn check_top_scorer(&self, user_id: i64, date: &str) -> Result<bool, Error> {
        let top_scorer = self.get_top_scorer_for_date(date).await?;

        match top_scorer {
            Some(ts) => Ok(ts.user_id == user_id),
            None => Ok(false),
        }
    }

    // Check if a prize has already been claimed for a user and date
    pub async fn check_prize_claimed(&self, user_id: i64, date: &str) -> Result<bool, Error> {
        let result = sqlx::query!(
            r#"
            SELECT COUNT(*) as count
            FROM prize_payouts
            WHERE user_id = ? AND date = ?
            "#,
            user_id,
            date
        )
        .fetch_one(&self.db)
        .await?;

        Ok(result.count > 0)
    }

    // Record a daily winner
    pub async fn record_daily_winner(
        &self,
        user_id: i64,
        date: &str,
        score: i64,
        amount_sats: i64,
    ) -> Result<PrizePayout, Error> {
        let now = OffsetDateTime::now_utc().to_string();

        let id = sqlx::query!(
            r#"
            INSERT INTO prize_payouts
            (user_id, date, score, amount_sats, status, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id, date) DO NOTHING
            "#,
            user_id,
            date,
            score,
            amount_sats,
            "pending",
            now,
            now
        )
        .execute(&self.db)
        .await?
        .last_insert_rowid();

        Ok(PrizePayout {
            id,
            user_id,
            date: date.to_string(),
            score,
            amount_sats,
            payment_request: None,
            payment_id: None,
            status: "pending".to_string(),
            created_at: now.clone(),
            updated_at: now,
            paid_at: None,
        })
    }

    // Update a prize payout with an invoice
    pub async fn update_prize_with_invoice(
        &self,
        user_id: i64,
        date: &str,
        invoice: &str,
    ) -> Result<Option<PrizePayout>, Error> {
        let now = OffsetDateTime::now_utc().to_string();

        let result = sqlx::query!(
            r#"
            UPDATE prize_payouts
            SET payment_request = ?, updated_at = ?
            WHERE user_id = ? AND date = ? AND status = 'pending'
            "#,
            invoice,
            now,
            user_id,
            date
        )
        .execute(&self.db)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        let payout = sqlx::query_as!(
            PrizePayout,
            r#"
            SELECT id, user_id, date, score, amount_sats, payment_request, payment_id, status, created_at, updated_at, paid_at
            FROM prize_payouts
            WHERE user_id = ? AND date = ?
            "#,
            user_id,
            date
        )
        .fetch_optional(&self.db)
        .await?;

        Ok(payout)
    }

    // Update a prize payout status
    pub async fn update_prize_status(
        &self,
        id: i64,
        status: &str,
        payment_id: Option<&str>,
    ) -> Result<Option<PrizePayout>, Error> {
        let now = OffsetDateTime::now_utc().to_string();
        let paid_at = if status == "paid" {
            Some(now.clone())
        } else {
            None
        };

        let result = sqlx::query!(
            r#"
            UPDATE prize_payouts
            SET status = ?, payment_id = ?, updated_at = ?, paid_at = ?
            WHERE id = ?
            "#,
            status,
            payment_id,
            now,
            paid_at,
            id
        )
        .execute(&self.db)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        let payout = sqlx::query_as!(
            PrizePayout,
            r#"
            SELECT id, user_id, date, score, amount_sats, payment_request, payment_id, status, created_at, updated_at, paid_at
            FROM prize_payouts
            WHERE id = ?
            "#,
            id
        )
        .fetch_optional(&self.db)
        .await?;

        Ok(payout)
    }

    // Get pending prize for a user
    pub async fn get_pending_prize_for_user(
        &self,
        user_id: i64,
        date: &str,
    ) -> Result<Option<PrizePayout>, Error> {
        let payout = sqlx::query_as!(
            PrizePayout,
            r#"
            SELECT id, user_id, date, score, amount_sats, payment_request, payment_id, status, created_at, updated_at, paid_at
            FROM prize_payouts
            WHERE user_id = ? AND date = ? AND status = 'pending'
            "#,
            user_id,
            date
        )
        .fetch_optional(&self.db)
        .await?;

        Ok(payout)
    }
}
