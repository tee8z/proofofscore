use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use time::OffsetDateTime;

use crate::{
    lightning::get_invoice_from_lightning_address, map_error, nostr_extractor::NostrAuth,
    startup::AppState,
};

// Get the status of a payment
pub async fn check_payment_status(
    auth: NostrAuth,
    Path(payment_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    info!(
        "Checking payment status: {} for user: {}",
        payment_id, pubkey
    );

    // Find user
    let user = match state.user_store.find_by_pubkey(pubkey).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err((StatusCode::UNAUTHORIZED, "User not found").into_response()),
        Err(e) => return Err(map_error(e)),
    };

    // Check if the payment belongs to this user
    let payment = match state.payment_store.get_payment_by_id(&payment_id).await {
        Ok(Some(payment)) => {
            if payment.user_id != user.id {
                return Err(
                    (StatusCode::FORBIDDEN, "Payment belongs to another user").into_response()
                );
            }
            payment
        }
        Ok(None) => return Err((StatusCode::NOT_FOUND, "Payment not found").into_response()),
        Err(e) => return Err(map_error(e)),
    };

    // If payment is already marked as paid in our database
    if payment.status == "paid" {
        return Ok((
            StatusCode::OK,
            Json(json!({
                "status": "paid",
                "payment_id": payment.payment_id
            })),
        ));
    }

    // Check with Lightning provider
    match state
        .lightning_provider
        .check_payment_status(&payment_id)
        .await
    {
        Ok(result) => match result.status.as_str() {
            "paid" => {
                if let Err(e) = state
                    .payment_store
                    .update_payment_status(&payment_id, "paid")
                    .await
                {
                    error!("Failed to update payment status: {}", e);
                }

                // Grant plays if not already granted (idempotent — invoice
                // watcher may have already done this)
                if payment.plays_remaining == 0 {
                    let comp = &state.settings.competition_settings;
                    if let Err(e) = state
                        .payment_store
                        .set_plays_remaining(
                            &payment_id,
                            comp.plays_per_payment,
                            comp.plays_ttl_minutes,
                        )
                        .await
                    {
                        error!("Failed to set plays_remaining: {}", e);
                    }
                }

                if let Err(e) = state
                    .ledger_service
                    .publish_game_entry(
                        &user.nostr_pubkey,
                        &payment_id,
                        payment.amount_sats,
                        "",
                        &OffsetDateTime::now_utc().date().to_string(),
                    )
                    .await
                {
                    warn!("Failed to publish game entry to ledger: {}", e);
                }

                Ok((
                    StatusCode::OK,
                    Json(json!({
                        "status": "paid",
                        "payment_id": payment_id
                    })),
                ))
            }
            "failed" => {
                if let Err(e) = state
                    .payment_store
                    .update_payment_status(&payment_id, "failed")
                    .await
                {
                    error!("Failed to update payment status: {}", e);
                }

                Ok((
                    StatusCode::OK,
                    Json(json!({
                        "status": "failed",
                        "payment_id": payment_id
                    })),
                ))
            }
            _ => Ok((
                StatusCode::OK,
                Json(json!({
                    "status": "pending",
                    "payment_id": payment_id
                })),
            )),
        },
        Err(e) => {
            error!(
                "Error checking payment status with Lightning provider: {}",
                e
            );

            Ok((
                StatusCode::OK,
                Json(json!({
                    "status": payment.status,
                    "payment_id": payment_id,
                    "error": "Could not verify payment status with payment provider."
                })),
            ))
        }
    }
}

// Structure for the winning player information
#[derive(Debug, Serialize, Deserialize)]
pub struct DailyWinnerInfo {
    pub eligible: bool,
    pub date: String,
    pub amount: i64,
    pub message: String,
}

// Structure for claiming a prize
#[derive(Debug, Deserialize)]
pub struct ClaimPrizeRequest {
    /// Bolt11 invoice — optional if user has a lightning address set.
    pub invoice: Option<String>,
    pub date: String,
}

// Return info about prize eligibility for the most recently completed competition.
pub async fn check_prize_eligibility(
    auth: NostrAuth,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    info!("Checking prize eligibility for user: {}", pubkey);

    let user = match state.user_store.find_by_pubkey(pubkey).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err((StatusCode::UNAUTHORIZED, "User not found").into_response()),
        Err(e) => return Err(map_error(e)),
    };

    // The target date is today — the competition window that just closed.
    let target_date = OffsetDateTime::now_utc().date().to_string();

    // Check if user was the top scorer
    let was_top_scorer = match state
        .payment_store
        .check_top_scorer(user.id, &target_date)
        .await
    {
        Ok(is_top) => is_top,
        Err(e) => {
            error!("Failed to check top scorer: {}", e);
            return Err(map_error(e));
        }
    };

    if !was_top_scorer {
        return Ok((
            StatusCode::OK,
            Json(json!({
                "eligible": false,
                "message": "You were not the top scorer for this competition"
            })),
        ));
    }

    // Check if prize was already claimed
    let already_claimed = match state
        .payment_store
        .check_prize_claimed(user.id, &target_date)
        .await
    {
        Ok(claimed) => claimed,
        Err(e) => {
            error!("Failed to check if prize was claimed: {}", e);
            return Err(map_error(e));
        }
    };

    if already_claimed {
        let prize = match state
            .payment_store
            .get_pending_prize_for_user(user.id, &target_date)
            .await
        {
            Ok(Some(prize)) => prize,
            Ok(None) => {
                return Ok((
                    StatusCode::OK,
                    Json(json!({
                        "eligible": false,
                        "message": "Your prize has already been processed"
                    })),
                ));
            }
            Err(e) => {
                error!("Failed to get pending prize: {}", e);
                return Err(map_error(e));
            }
        };

        if prize.status == "paid" {
            return Ok((
                StatusCode::OK,
                Json(json!({
                    "eligible": false,
                    "message": "Your prize has already been paid"
                })),
            ));
        } else {
            return Ok((
                StatusCode::OK,
                Json(json!({
                    "eligible": true,
                    "date": target_date,
                    "amount": prize.amount_sats,
                    "message": "You can claim your prize by submitting a Lightning invoice",
                    "status": "pending",
                    "has_payment_request": prize.payment_request.is_some()
                })),
            ));
        }
    }

    // Calculate prize amount
    let total_games = match state.payment_store.count_games_for_date(&target_date).await {
        Ok(count) => count,
        Err(e) => {
            error!("Failed to count games: {}", e);
            return Err(map_error(e));
        }
    };

    let comp = &state.settings.competition_settings;
    let prize_per_game = comp.entry_fee_sats * (comp.prize_pool_pct as i64) / 100;
    let prize_amount = total_games * prize_per_game;

    if prize_amount <= 0 {
        return Ok((
            StatusCode::OK,
            Json(json!({
                "eligible": false,
                "message": "No prize pool available for this competition"
            })),
        ));
    }

    // Record the winner if not already recorded
    match state
        .payment_store
        .record_daily_winner(
            user.id,
            &target_date,
            0,
            prize_amount,
        )
        .await
    {
        Ok(_) => (),
        Err(e) => {
            error!("Failed to record winner: {}", e);
        }
    };

    Ok((
        StatusCode::OK,
        Json(json!({
            "eligible": true,
            "date": target_date,
            "amount": prize_amount,
            "message": "You can claim your prize by submitting a Lightning invoice"
        })),
    ))
}

// Claim a prize — manual fallback when auto-payout didn't happen.
// If user has a lightning address, resolve via LNURL.
// Otherwise, user must provide a bolt11 invoice.
pub async fn claim_prize(
    auth: NostrAuth,
    State(state): State<Arc<AppState>>,
    Json(request): Json<ClaimPrizeRequest>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    info!(
        "Prize claim request from pubkey: {}, date: {}",
        pubkey, request.date
    );

    // Find user
    let user = match state.user_store.find_by_pubkey(pubkey).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err((StatusCode::UNAUTHORIZED, "User not found").into_response()),
        Err(e) => return Err(map_error(e)),
    };

    // Verify eligibility
    let was_top_scorer = match state
        .payment_store
        .check_top_scorer(user.id, &request.date)
        .await
    {
        Ok(is_top) => is_top,
        Err(e) => {
            error!("Failed to check top scorer: {}", e);
            return Err(map_error(e));
        }
    };

    if !was_top_scorer {
        return Err((
            StatusCode::FORBIDDEN,
            "You were not the top scorer for this date",
        )
            .into_response());
    }

    // Get the pending prize record
    let prize = match state
        .payment_store
        .get_pending_prize_for_user(user.id, &request.date)
        .await
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                "No eligible prize found for this date (may already be paid)",
            )
                .into_response());
        }
        Err(e) => {
            error!("Failed to get pending prize: {}", e);
            return Err(map_error(e));
        }
    };

    if prize.status == "paid" {
        return Err((StatusCode::FORBIDDEN, "Prize has already been paid").into_response());
    }

    // Resolve the bolt11 invoice:
    // 1. If user provided one explicitly, use it
    // 2. Else if user has a lightning address, resolve via LNURL
    // 3. Else error — they need to set one up
    let invoice = if let Some(ref provided) = request.invoice {
        if !provided.starts_with("lnbc")
            && !provided.starts_with("lnbcrt")
            && !provided.starts_with("lntbs")
            && !provided.starts_with("lntb")
        {
            return Err((StatusCode::BAD_REQUEST, "Invalid Lightning invoice").into_response());
        }
        provided.clone()
    } else if let Some(ref ln_addr) = user.lightning_address {
        let http_client = crate::startup::build_reqwest_client();
        get_invoice_from_lightning_address(&http_client, ln_addr, prize.amount_sats)
            .await
            .map_err(|e| {
                error!("LNURL resolution failed for {}: {}", ln_addr, e);
                (
                    StatusCode::BAD_GATEWAY,
                    format!(
                        "Failed to resolve lightning address '{}': {}. \
                         You can provide a bolt11 invoice directly instead.",
                        ln_addr, e
                    ),
                )
                    .into_response()
            })?
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            "No invoice provided and no lightning address on profile. \
             Set a lightning address in your profile or provide a bolt11 invoice.",
        )
            .into_response());
    };

    // Store the invoice on the prize
    let updated_prize = match state
        .payment_store
        .update_prize_with_invoice(user.id, &request.date, &invoice)
        .await
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return Err(
                (StatusCode::NOT_FOUND, "Failed to update prize with invoice").into_response(),
            );
        }
        Err(e) => {
            error!("Failed to update prize with invoice: {}", e);
            return Err(map_error(e));
        }
    };

    // Send the payment
    match state
        .lightning_provider
        .send_payment(&invoice, updated_prize.amount_sats)
        .await
    {
        Ok(payment_id) => {
            if let Err(e) = state
                .payment_store
                .update_prize_status(updated_prize.id, "paid", Some(&payment_id))
                .await
            {
                error!("Failed to update prize status: {}", e);
            }

            info!(
                "Prize payment successful for user_id: {}, amount: {}",
                user.id, updated_prize.amount_sats
            );

            if let Err(e) = state
                .ledger_service
                .publish_prize_payout(
                    &user.nostr_pubkey,
                    &request.date,
                    updated_prize.amount_sats,
                    &payment_id,
                )
                .await
            {
                warn!("Failed to publish prize payout to ledger: {}", e);
            }

            Ok((
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": "Prize payment sent successfully",
                    "payment_id": payment_id,
                    "amount": updated_prize.amount_sats
                })),
            ))
        }
        Err(e) => {
            error!("Failed to send prize payment: {}", e);

            if let Err(update_err) = state
                .payment_store
                .update_prize_status(updated_prize.id, "failed", None)
                .await
            {
                error!(
                    "Failed to update prize status after payment failure: {}",
                    update_err
                );
            }

            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to send payment: {}", e),
            )
                .into_response())
        }
    }
}
