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

/// Request a tip invoice for the dev's lightning address via LNURL.
/// No auth required — anyone can tip.
#[derive(Debug, Deserialize)]
pub struct TipRequest {
    pub amount_sats: i64,
}

pub async fn create_tip_invoice(
    State(state): State<Arc<AppState>>,
    Json(request): Json<TipRequest>,
) -> Result<impl IntoResponse, Response> {
    let tip_address = state
        .settings
        .competition_settings
        .tip_address
        .as_deref()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Tipping is not configured").into_response())?;

    if request.amount_sats < 1 || request.amount_sats > 1_000_000 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Amount must be between 1 and 1,000,000 sats",
        )
            .into_response());
    }

    info!(
        "Tip invoice request: {} sats to {}",
        request.amount_sats, tip_address
    );

    let http_client = crate::startup::build_reqwest_client();
    let invoice =
        get_invoice_from_lightning_address(&http_client, tip_address, request.amount_sats)
            .await
            .map_err(|e| {
                error!("Failed to resolve tip address {}: {}", tip_address, e);
                (
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to create tip invoice: {}", e),
                )
                    .into_response()
            })?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "invoice": invoice,
            "amount_sats": request.amount_sats,
            "address": tip_address,
        })),
    ))
}

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
                } else {
                    crate::metrics::metrics().invoices_paid.inc();
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

// Return all pending (unclaimed) prizes for this user.
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

    // Get all claimable prizes (pending + failed) for this user
    let pending = match state.payment_store.get_claimable_prizes(user.id).await {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to get pending prizes: {}", e);
            return Err(map_error(e));
        }
    };

    if pending.is_empty() {
        return Ok((
            StatusCode::OK,
            Json(json!({
                "pending_prizes": [],
                "message": "No prizes to claim"
            })),
        ));
    }

    let prizes: Vec<serde_json::Value> = pending
        .iter()
        .map(|p| {
            json!({
                "date": p.date,
                "amount": p.amount_sats,
                "score": p.score,
                "status": p.status,
            })
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(json!({
            "pending_prizes": prizes,
            "message": format!("You have {} unclaimed prize(s)", prizes.len())
        })),
    ))
}

/// Parse the amount from a bolt11 invoice's human-readable part.
/// Returns Some(sats) for invoices with an amount, None for zero-amount invoices.
///
/// Format: ln{bc,bcrt,tb,tbs}<amount><multiplier>1<data>
/// Multipliers: m=milli(0.001), u=micro(0.000001), n=nano, p=pico
fn parse_bolt11_amount(invoice: &str) -> Option<i64> {
    let lower = invoice.to_lowercase();

    // Strip the network prefix to get amount+multiplier+1+data
    let after_prefix = ["lnbcrt", "lntbs", "lnbc", "lntb"]
        .iter()
        .find_map(|prefix| lower.strip_prefix(prefix))?;

    // Find the "1" separator — the amount is everything before it
    let sep_pos = after_prefix.find('1')?;
    let amount_str = &after_prefix[..sep_pos];

    if amount_str.is_empty() {
        return None;
    }

    // Split into numeric part and multiplier suffix
    let (num_str, multiplier) = if let Some(n) = amount_str.strip_suffix('m') {
        (n, 100_000_000_i64) // mBTC → msats factor
    } else if let Some(n) = amount_str.strip_suffix('u') {
        (n, 100_000)
    } else if let Some(n) = amount_str.strip_suffix('n') {
        (n, 100)
    } else if let Some(n) = amount_str.strip_suffix('p') {
        let val: i64 = n.parse().ok()?;
        return Some(val / 100); // pico-BTC: 1p = 0.01 sat
    } else {
        (amount_str, 100_000_000_000) // BTC → msats
    };

    let n: i64 = num_str.parse().ok()?;
    Some(n * multiplier / 1_000)
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

    // Get a claimable prize (pending or failed)
    let prize = match state
        .payment_store
        .get_claimable_prize(user.id, &request.date)
        .await
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                "No claimable prize found for this date (may already be paid or in-flight)",
            )
                .into_response());
        }
        Err(e) => {
            error!("Failed to get claimable prize: {}", e);
            return Err(map_error(e));
        }
    };

    // If this is a retry of a failed prize, verify the previous attempt
    // didn't actually succeed or is still in-flight on our node.
    if prize.status == "failed" {
        if let Some(ref prev_payment_id) = prize.payment_id {
            info!(
                "Prize {} has a previous payment attempt ({}), checking with node...",
                prize.id, prev_payment_id
            );
            match state
                .lightning_provider
                .check_outbound_payment(prev_payment_id)
                .await
            {
                Ok(status) => match status.as_str() {
                    "SUCCEEDED" => {
                        // Payment actually landed — update status and return
                        info!("Previous payment {} actually succeeded!", prev_payment_id);
                        if let Err(e) = state
                            .payment_store
                            .update_prize_status(prize.id, "paid", Some(prev_payment_id))
                            .await
                        {
                            error!("Failed to update prize status: {}", e);
                        }
                        return Ok((
                            StatusCode::OK,
                            Json(json!({
                                "success": true,
                                "message": "Prize was already paid (previous attempt succeeded)",
                                "payment_id": prev_payment_id,
                                "amount": prize.amount_sats
                            })),
                        ));
                    }
                    "IN_FLIGHT" => {
                        return Err((
                            StatusCode::CONFLICT,
                            "Previous payment attempt is still in-flight. Please wait and try again.",
                        )
                            .into_response());
                    }
                    _ => {
                        // FAILED or NOT_FOUND — safe to retry
                        info!(
                            "Previous payment {} status: {} — safe to retry",
                            prev_payment_id, status
                        );
                    }
                },
                Err(e) => {
                    warn!(
                        "Could not verify previous payment status: {} — allowing retry",
                        e
                    );
                }
            }
        }
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

    // Validate invoice amount if manually provided.
    // Zero-amount invoices are fine (we specify the amount when paying).
    // Non-zero invoices must match the exact prize amount.
    if request.invoice.is_some() {
        if let Some(invoice_sats) = parse_bolt11_amount(&invoice) {
            if invoice_sats != prize.amount_sats {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!(
                        "Invoice amount ({} sats) doesn't match prize amount ({} sats). \
                         Use a zero-amount invoice or one for exactly {} sats.",
                        invoice_sats, prize.amount_sats, prize.amount_sats
                    ),
                )
                    .into_response());
            }
        }
        // None means zero-amount invoice — that's fine
    }

    // Store the invoice on the prize
    if let Err(e) = state
        .payment_store
        .update_prize_with_invoice(user.id, &request.date, &invoice)
        .await
    {
        error!("Failed to update prize with invoice: {}", e);
        return Err(map_error(e));
    }

    // Mark as "paying" before attempting — prevents concurrent retries
    if let Err(e) = state
        .payment_store
        .update_prize_status(prize.id, "paying", None)
        .await
    {
        error!("Failed to mark prize as paying: {}", e);
    }

    // Send the payment
    match state
        .lightning_provider
        .send_payment(&invoice, prize.amount_sats)
        .await
    {
        Ok(payment_id) => {
            crate::metrics::metrics().payout(crate::metrics::PAYOUT_SUCCEEDED);
            if let Err(e) = state
                .payment_store
                .update_prize_status(prize.id, "paid", Some(&payment_id))
                .await
            {
                error!("Failed to update prize status: {}", e);
            }

            info!(
                "Prize payment successful for user_id: {}, amount: {}",
                user.id, prize.amount_sats
            );

            if let Err(e) = state
                .ledger_service
                .publish_prize_payout(
                    &user.nostr_pubkey,
                    &request.date,
                    prize.amount_sats,
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
                    "amount": prize.amount_sats
                })),
            ))
        }
        Err(e) => {
            error!("Failed to send prize payment: {}", e);
            crate::metrics::metrics().payout(crate::metrics::PAYOUT_FAILED);

            // Record as failed — but extract and store the payment hash
            // from the error if possible, so we can verify it on retry.
            // The payment hash comes from the invoice, not the response,
            // so we store the invoice itself for re-verification.
            if let Err(update_err) = state
                .payment_store
                .update_prize_status(prize.id, "failed", prize.payment_id.as_deref())
                .await
            {
                error!(
                    "Failed to update prize status after payment failure: {}",
                    update_err
                );
            }

            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Payment failed: {}. You can retry from your profile.", e),
            )
                .into_response())
        }
    }
}
