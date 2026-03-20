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

use crate::{map_error, nostr_extractor::NostrAuth, startup::AppState};

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

    // Check with Lightning API
    match state
        .lightning_service
        .get_payment_status(&payment_id)
        .await
    {
        Ok(Some(api_payment)) => {
            let status = api_payment["status"].as_str().unwrap_or("unknown");

            match status {
                "completed" => {
                    // Update our record
                    if let Err(e) = state
                        .payment_store
                        .update_payment_status(&payment_id, "paid")
                        .await
                    {
                        error!("Failed to update payment status: {}", e);
                    }

                    // Publish game entry to audit ledger
                    if let Err(e) = state
                        .ledger_service
                        .publish_game_entry(
                            &user.nostr_pubkey,
                            &payment_id,
                            payment.amount_sats,
                            "", // session_id not available here, that's ok
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
                    // Update our record
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
            }
        }
        Ok(None) => {
            // Payment not found in Lightning API, consider it pending
            Ok((
                StatusCode::OK,
                Json(json!({
                    "status": "pending",
                    "payment_id": payment_id,
                    "message": "Payment not found in Lightning API yet"
                })),
            ))
        }
        Err(e) => {
            error!("Error checking payment status with Lightning API: {}", e);

            // Return current status from our database
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
    pub invoice: String,
    pub date: String,
}

// Return info about prize eligibility
pub async fn check_prize_eligibility(
    auth: NostrAuth,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    info!("Checking prize eligibility for user: {}", pubkey);

    // Find user
    let user = match state.user_store.find_by_pubkey(pubkey).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err((StatusCode::UNAUTHORIZED, "User not found").into_response()),
        Err(e) => return Err(map_error(e)),
    };

    // Get yesterday's date in YYYY-MM-DD format for completed day calculation
    let yesterday = (OffsetDateTime::now_utc() - time::Duration::days(1))
        .format(&time::format_description::well_known::Iso8601::DEFAULT)
        .unwrap()
        .chars()
        .take(10) // Take just YYYY-MM-DD part
        .collect::<String>();

    // Check if user was a top scorer for yesterday
    let was_top_scorer = match state
        .payment_store
        .check_top_scorer(user.id, &yesterday)
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
                "message": "You were not the top scorer for yesterday's games"
            })),
        ));
    }

    // Check if prize was already claimed
    let already_claimed = match state
        .payment_store
        .check_prize_claimed(user.id, &yesterday)
        .await
    {
        Ok(claimed) => claimed,
        Err(e) => {
            error!("Failed to check if prize was claimed: {}", e);
            return Err(map_error(e));
        }
    };

    if already_claimed {
        // Check if it was already paid or is pending
        let prize = match state
            .payment_store
            .get_pending_prize_for_user(user.id, &yesterday)
            .await
        {
            Ok(Some(prize)) => prize,
            Ok(None) => {
                return Ok((
                    StatusCode::OK,
                    Json(json!({
                        "eligible": false,
                        "message": "You have already claimed your prize for yesterday"
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
            // Prize is pending payment
            return Ok((
                StatusCode::OK,
                Json(json!({
                    "eligible": true,
                    "date": yesterday,
                    "amount": prize.amount_sats,
                    "message": "You can claim your prize by submitting a Lightning invoice",
                    "status": "pending",
                    "has_payment_request": prize.payment_request.is_some()
                })),
            ));
        }
    }

    // Calculate prize amount (90% of all entry fees for that day)
    let total_games = match state.payment_store.count_games_for_date(&yesterday).await {
        Ok(count) => count,
        Err(e) => {
            error!("Failed to count games: {}", e);
            return Err(map_error(e));
        }
    };

    let prize_amount = (total_games * 450) as i64; // 90% of 500 sats * number of games

    if prize_amount <= 0 {
        return Ok((
            StatusCode::OK,
            Json(json!({
                "eligible": false,
                "message": "No prize pool available for yesterday"
            })),
        ));
    }

    // Record the winner if not already recorded
    match state
        .payment_store
        .record_daily_winner(
            user.id,
            &yesterday,
            0, // We don't have the score here, it will be updated later
            prize_amount,
        )
        .await
    {
        Ok(_) => (),
        Err(e) => {
            error!("Failed to record daily winner: {}", e);
            // Continue anyway, it might already be recorded
        }
    };

    // Return eligibility info
    Ok((
        StatusCode::OK,
        Json(json!({
            "eligible": true,
            "date": yesterday,
            "amount": prize_amount,
            "message": "You can claim your prize by submitting a Lightning invoice"
        })),
    ))
}

// Claim a prize
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

    // Validate invoice
    if !request.invoice.starts_with("lnbc") {
        return Err((StatusCode::BAD_REQUEST, "Invalid Lightning invoice").into_response());
    }

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

    // Get or create the prize record
    let prize = match state
        .payment_store
        .get_pending_prize_for_user(user.id, &request.date)
        .await
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            // No pending prize found, check if one was already paid
            return Err((
                StatusCode::NOT_FOUND,
                "No eligible prize found for this date",
            )
                .into_response());
        }
        Err(e) => {
            error!("Failed to get pending prize: {}", e);
            return Err(map_error(e));
        }
    };

    // Check if prize has already been paid
    if prize.status == "paid" {
        return Err((StatusCode::FORBIDDEN, "Prize has already been paid").into_response());
    }

    // If prize already has an invoice but no payment yet, update it
    let updated_prize = if prize.payment_request.is_some() {
        match state
            .payment_store
            .update_prize_with_invoice(user.id, &request.date, &request.invoice)
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
        }
    } else {
        // First time adding an invoice
        match state
            .payment_store
            .update_prize_with_invoice(user.id, &request.date, &request.invoice)
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
        }
    };

    // Process payment (could be done asynchronously in production)
    // For now, we'll do it synchronously
    match state
        .lightning_service
        .pay_winner_invoice(
            &request.invoice,
            updated_prize.amount_sats * 1000, // Convert to msats
        )
        .await
    {
        Ok(payment_result) => {
            // Updated for Value type: extract payment ID
            let payment_id = payment_result["id"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();

            // Update the prize record
            match state
                .payment_store
                .update_prize_status(updated_prize.id, "paid", Some(&payment_id))
                .await
            {
                Ok(_) => {
                    info!(
                        "Prize payment successful for user_id: {}, amount: {}",
                        user.id, updated_prize.amount_sats
                    );

                    // Publish prize payout to audit ledger
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
                    error!("Failed to update prize status: {}", e);
                    Err(map_error(e))
                }
            }
        }
        Err(e) => {
            error!("Failed to send prize payment: {}", e);

            // Update the prize status to reflect the payment failure
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
