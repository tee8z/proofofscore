use log::{error, info, warn};
use std::sync::Arc;
use time::OffsetDateTime;

use crate::{lightning::get_invoice_from_lightning_address, startup::AppState};

/// Runs the competition lifecycle: sleep until the window closes, then resolve the winner.
pub async fn run_competition_task(app_state: Arc<AppState>) {
    let comp = &app_state.settings.competition_settings;
    let (end_hour, end_minute) = comp.end_hour_minute();
    info!(
        "Competition: starts {} UTC, duration {}, closes {:02}:{:02} UTC",
        comp.start_time,
        comp.duration_display(),
        end_hour,
        end_minute,
    );

    let prize_per_game = comp.entry_fee_sats * (comp.prize_pool_pct as i64) / 100;
    let mut last_processed_date: Option<String> = None;

    loop {
        let now = OffsetDateTime::now_utc();
        let today = now.date().to_string();
        let now_secs = now.hour() as u64 * 3600 + now.minute() as u64 * 60 + now.second() as u64;
        let end_secs = end_hour as u64 * 3600 + end_minute as u64 * 60;
        let already_processed = last_processed_date.as_deref() == Some(&today);

        // Check if the window already closed today and we haven't processed it yet
        // (handles server restart after window close)
        if now_secs >= end_secs && !already_processed {
            info!(
                "Competition window already closed today — resolving winner for {}",
                today
            );
            resolve_winner(&app_state, &today, prize_per_game, comp).await;
            last_processed_date = Some(today);
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            continue;
        }

        if already_processed || now_secs >= end_secs {
            // Already processed today — sleep until tomorrow's close
            let sleep_secs = (24 * 3600 - now_secs) + end_secs;
            info!(
                "Today already processed — sleeping {}h {}m until tomorrow {:02}:{:02} UTC",
                sleep_secs / 3600,
                (sleep_secs % 3600) / 60,
                end_hour,
                end_minute,
            );
            tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
        } else {
            // Window closes later today — sleep until then
            let sleep_secs = end_secs - now_secs;
            info!(
                "Competition window closes in {}m {}s — sleeping until {:02}:{:02} UTC",
                sleep_secs / 60,
                sleep_secs % 60,
                end_hour,
                end_minute,
            );
            tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
        }

        // Woke up — resolve the winner
        let target_date = OffsetDateTime::now_utc().date().to_string();
        if last_processed_date.as_deref() != Some(&target_date) {
            info!("Competition window closed — resolving winner for {}", target_date);
            resolve_winner(&app_state, &target_date, prize_per_game, comp).await;
            last_processed_date = Some(target_date);
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn resolve_winner(
    app_state: &Arc<AppState>,
    target_date: &str,
    prize_per_game: i64,
    comp: &crate::config::CompetitionSettings,
) {
    match app_state
        .payment_store
        .get_top_scorer_for_date(target_date)
        .await
    {
        Ok(Some(scorer)) => {
            info!(
                "Found top scorer for {}: user_id={}, score={}",
                target_date, scorer.user_id, scorer.score
            );

            match app_state
                .payment_store
                .count_games_for_date(target_date)
                .await
            {
                Ok(games_count) if games_count > 0 => {
                    let prize_amount = games_count * prize_per_game;

                    match app_state
                        .payment_store
                        .record_daily_winner(
                            scorer.user_id,
                            target_date,
                            scorer.score,
                            prize_amount,
                        )
                        .await
                    {
                        Ok(_) => {
                            info!(
                                "Recorded winner for {}: user_id={}, prize={} sats",
                                target_date, scorer.user_id, prize_amount
                            );

                            if let Ok(Some(winner_user)) =
                                app_state.user_store.find_by_id(scorer.user_id).await
                            {
                                let total_pool = games_count * comp.entry_fee_sats;
                                if let Err(e) = app_state
                                    .ledger_service
                                    .publish_competition_result(
                                        target_date,
                                        &winner_user.nostr_pubkey,
                                        scorer.score,
                                        games_count,
                                        total_pool,
                                        prize_amount,
                                    )
                                    .await
                                {
                                    warn!("Failed to publish competition result to ledger: {}", e);
                                }

                                attempt_auto_payout(
                                    app_state,
                                    scorer.user_id,
                                    &winner_user.nostr_pubkey,
                                    winner_user.lightning_address.as_deref(),
                                    target_date,
                                    prize_amount,
                                )
                                .await;
                            } else {
                                warn!(
                                    "Failed to find user for competition result: user_id={}",
                                    scorer.user_id
                                );
                            }
                        }
                        Err(e) => {
                            error!("Failed to record winner: {}", e);
                        }
                    }
                }
                Ok(_) => {
                    info!(
                        "No paid games found for {}, no prize to award",
                        target_date
                    );
                }
                Err(e) => {
                    error!("Failed to count games for {}: {}", target_date, e);
                }
            }
        }
        Ok(None) => {
            info!(
                "No scores found for {}, no winner to announce",
                target_date
            );
        }
        Err(e) => {
            error!("Failed to find top scorer for {}: {}", target_date, e);
        }
    }
}

/// Try to auto-pay a prize via the winner's lightning address.
/// If they don't have one or LNURL resolution fails, the prize stays pending
/// for manual claim.
async fn attempt_auto_payout(
    state: &Arc<AppState>,
    user_id: i64,
    nostr_pubkey: &str,
    lightning_address: Option<&str>,
    date: &str,
    amount_sats: i64,
) {
    let ln_addr = match lightning_address {
        Some(addr) if !addr.is_empty() => addr,
        _ => {
            info!(
                "User {} has no lightning address — prize for {} stays pending for manual claim",
                user_id, date
            );
            return;
        }
    };

    info!(
        "Auto-paying prize of {} sats to {} (user_id={}) for {}",
        amount_sats, ln_addr, user_id, date
    );

    let http_client = crate::startup::build_reqwest_client();
    let invoice = match get_invoice_from_lightning_address(&http_client, ln_addr, amount_sats).await
    {
        Ok(inv) => inv,
        Err(e) => {
            warn!(
                "LNURL resolution failed for {} — prize stays pending: {}",
                ln_addr, e
            );
            return;
        }
    };

    if let Err(e) = state
        .payment_store
        .update_prize_with_invoice(user_id, date, &invoice)
        .await
    {
        error!("Failed to store resolved invoice on prize: {}", e);
        return;
    }

    match state
        .lightning_provider
        .send_payment(&invoice, amount_sats)
        .await
    {
        Ok(payment_id) => {
            if let Ok(Some(prize)) = state
                .payment_store
                .get_pending_prize_for_user(user_id, date)
                .await
            {
                if let Err(e) = state
                    .payment_store
                    .update_prize_status(prize.id, "paid", Some(&payment_id))
                    .await
                {
                    error!("Failed to update prize status after auto-pay: {}", e);
                }
            }

            info!(
                "Auto-payout successful: {} sats → {} (payment_id={})",
                amount_sats, ln_addr, payment_id
            );

            if let Err(e) = state
                .ledger_service
                .publish_prize_payout(nostr_pubkey, date, amount_sats, &payment_id)
                .await
            {
                warn!("Failed to publish auto-payout to ledger: {}", e);
            }
        }
        Err(e) => {
            error!(
                "Auto-payout failed for {} ({} sats): {} — prize stays pending for manual claim",
                ln_addr, amount_sats, e
            );
        }
    }
}
