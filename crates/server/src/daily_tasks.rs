use log::{error, info, warn};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use tokio::time as tokio_time;

use crate::startup::AppState;

// Process to run daily to find winners and set up prizes
pub async fn run_daily_tasks(app_state: Arc<AppState>) {
    info!("Starting daily tasks runner");

    let mut interval = tokio_time::interval(tokio_time::Duration::from_secs(3600)); // Run every hour

    loop {
        interval.tick().await;

        // Check if it's the right time to run (e.g., 00:05 AM)
        let now = OffsetDateTime::now_utc();
        if now.hour() == 0 && now.minute() >= 5 && now.minute() < 15 {
            info!("Running daily tasks to find winners");

            // Calculate yesterday's date
            let yesterday = (now - Duration::days(1))
                .format(&time::format_description::well_known::Iso8601::DEFAULT)
                .unwrap_or_else(|_| {
                    error!("Failed to format date");
                    String::from("unknown")
                })
                .chars()
                .take(10) // Get just the YYYY-MM-DD part
                .collect::<String>();

            info!("Processing winners for date: {}", yesterday);

            // Find top scorer for yesterday
            match app_state
                .payment_store
                .get_top_scorer_for_date(&yesterday)
                .await
            {
                Ok(Some(scorer)) => {
                    info!(
                        "Found top scorer for {}: user_id={}, score={}",
                        yesterday, scorer.user_id, scorer.score
                    );

                    // Count the number of games played that day
                    match app_state
                        .payment_store
                        .count_games_for_date(&yesterday)
                        .await
                    {
                        Ok(games_count) => {
                            if games_count > 0 {
                                // Calculate prize (90% of collected fees)
                                let prize_amount = games_count * 450; // 90% of 500 sats per game

                                // Record the winner
                                match app_state
                                    .payment_store
                                    .record_daily_winner(
                                        scorer.user_id,
                                        &yesterday,
                                        scorer.score,
                                        prize_amount,
                                    )
                                    .await
                                {
                                    Ok(_) => {
                                        info!("Recorded daily winner for {}: user_id={}, prize={} sats",
                                              yesterday, scorer.user_id, prize_amount);

                                        // Publish competition result to audit ledger
                                        if let Ok(Some(winner_user)) =
                                            app_state.user_store.find_by_id(scorer.user_id).await
                                        {
                                            if let Err(e) = app_state
                                                .ledger_service
                                                .publish_competition_result(
                                                    &yesterday,
                                                    &winner_user.nostr_pubkey,
                                                    scorer.score,
                                                    games_count,
                                                    games_count * 500, // total pool (500 sats per game)
                                                    prize_amount,
                                                )
                                                .await
                                            {
                                                warn!("Failed to publish competition result to ledger: {}", e);
                                            }
                                        } else {
                                            warn!("Failed to find user for ledger competition result: user_id={}", scorer.user_id);
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to record daily winner: {}", e);
                                    }
                                }
                            } else {
                                info!("No paid games found for {}, no prize to award", yesterday);
                            }
                        }
                        Err(e) => {
                            error!("Failed to count games for {}: {}", yesterday, e);
                        }
                    }
                }
                Ok(None) => {
                    info!("No scores found for {}, no winner to announce", yesterday);
                }
                Err(e) => {
                    error!("Failed to find top scorer for {}: {}", yesterday, e);
                }
            }
        }
    }
}
