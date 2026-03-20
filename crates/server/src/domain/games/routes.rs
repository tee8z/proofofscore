use axum::{
    extract::{Query, State},
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

use super::store::GameConfigResponse;

#[derive(Debug, Deserialize)]
pub struct ConfigQuery {
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NewSessionResponse {
    pub config: GameConfigResponse,
}

#[derive(Debug, Deserialize)]
pub struct ScoreSubmission {
    pub score: i64,
    pub level: i64,
    pub play_time: i64,
    pub session_id: String,
}

#[derive(Debug, Serialize)]
pub struct ScoreResponse {
    pub id: i64,
    pub score: i64,
    pub level: i64,
    pub play_time: i64,
    pub created_at: String,
}

// Health check endpoint
pub async fn health() -> impl IntoResponse {
    "OK"
}

// Create a new game session or get config for existing session
pub async fn get_game_config(
    auth: NostrAuth,
    Query(query): Query<ConfigQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    info!("Game config request from pubkey: {}", pubkey);

    // Find or create user
    let user = match state.user_store.find_by_pubkey(pubkey.clone()).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return Err((StatusCode::NOT_FOUND, "User not found").into_response());
        }
        Err(e) => return Err(map_error(e)),
    };

    // Use existing session or create new one
    if let Some(session_id) = query.session_id {
        // Update existing session
        match state.game_store.update_session_activity(&session_id).await {
            Ok(session) => {
                if session.user_id != user.id {
                    return Err(
                        (StatusCode::FORBIDDEN, "Session belongs to a different user")
                            .into_response(),
                    );
                }

                // Get config for this session
                match state.game_store.create_game_config(&session).await {
                    Ok(config) => Ok((StatusCode::OK, Json(config))),
                    Err(e) => Err(map_error(e)),
                }
            }
            Err(e) => Err(map_error(e)),
        }
    } else {
        // Create a new session
        match state.game_store.create_session(user.id).await {
            Ok(session) => match state.game_store.create_game_config(&session).await {
                Ok(config) => Ok((StatusCode::OK, Json(config))),
                Err(e) => Err(map_error(e)),
            },
            Err(e) => Err(map_error(e)),
        }
    }
}

// Create a new game session
pub async fn start_new_session(
    auth: NostrAuth,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    info!("New session request from pubkey: {}", pubkey);

    // Find user
    let user = match state.user_store.find_by_pubkey(pubkey.clone()).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err((StatusCode::UNAUTHORIZED, "User not found").into_response()),
        Err(e) => return Err(map_error(e)),
    };

    // Check if the user has a valid payment within the last hour
    let has_valid_payment = match state.payment_store.has_valid_payment(user.id).await {
        Ok(valid) => valid,
        Err(e) => return Err(map_error(e)),
    };

    if has_valid_payment {
        // User has a valid payment, create a new session
        match state.game_store.create_session(user.id).await {
            Ok(session) => match state.game_store.create_game_config(&session).await {
                Ok(config) => {
                    return Ok((StatusCode::CREATED, Json(NewSessionResponse { config })))
                }
                Err(e) => return Err(map_error(e)),
            },
            Err(e) => return Err(map_error(e)),
        }
    }

    // Check if user has a pending payment
    let pending_payment = match state
        .payment_store
        .get_pending_payment_for_user(user.id)
        .await
    {
        Ok(Some(payment)) => payment,
        Ok(None) => {
            // No pending payment, create a new invoice
            info!("Creating new payment invoice for user_id: {}", user.id);

            let description = format!("Asteroids Game Entry Fee - User:{}", pubkey);

            // Step 1: Request a new payment from Voltage
            let payment_id = match state
                .lightning_service
                .create_game_invoice(500, Some(&description))
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    error!("Failed to create invoice: {}", e);
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to create payment invoice",
                    )
                        .into_response());
                }
            };

            info!(
                "Created payment with ID: {}, polling for invoice",
                payment_id
            );

            // Step 2: Poll for the invoice
            let mut invoice: Option<String> = None;
            let max_attempts = 10;

            for attempt in 0..max_attempts {
                info!("Poll attempt {} for invoice", attempt + 1);

                match state
                    .lightning_service
                    .get_payment_invoice(&payment_id)
                    .await
                {
                    Ok(Some(payment_request)) => {
                        info!("Received invoice on attempt {}", attempt + 1);
                        invoice = Some(payment_request);
                        break;
                    }
                    Ok(None) => {
                        // Invoice not available yet, wait and retry
                        info!("Invoice not available yet, waiting");
                        tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
                    }
                    Err(e) => {
                        error!("Error getting payment invoice: {}", e);
                        return Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to retrieve payment invoice",
                        )
                            .into_response());
                    }
                }
            }

            // Step 3: Process the invoice result
            match invoice {
                Some(invoice_str) => {
                    info!("Successfully obtained invoice: {}", invoice_str);

                    // Store the payment in the database
                    match state
                        .payment_store
                        .create_game_payment(user.id, &payment_id, &invoice_str, 500)
                        .await
                    {
                        Ok(payment) => {
                            // Return payment required response
                            return Err((
                                StatusCode::PAYMENT_REQUIRED,
                                Json(json!({
                                    "payment_required": true,
                                    "invoice": payment.invoice,
                                    "payment_id": payment.payment_id,
                                    "amount_sats": payment.amount_sats,
                                    "created_at": payment.created_at
                                })),
                            )
                                .into_response());
                        }
                        Err(e) => return Err(map_error(e)),
                    }
                }
                None => {
                    error!("Failed to get invoice after {} attempts", max_attempts);
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to generate Lightning invoice. Please try again.",
                    )
                        .into_response());
                }
            }
        }
        Err(e) => return Err(map_error(e)),
    };

    // Check payment status for existing pending payment
    info!(
        "Checking status of existing payment: {}",
        pending_payment.payment_id
    );

    match state
        .lightning_service
        .get_payment_status(&pending_payment.payment_id)
        .await
    {
        Ok(Some(payment_status)) => {
            let status = payment_status["status"].as_str().unwrap_or("unknown");

            match status {
                "completed" => {
                    info!(
                        "Payment {} is completed, updating status",
                        pending_payment.payment_id
                    );

                    // Payment received, update our record
                    if let Err(e) = state
                        .payment_store
                        .update_payment_status(&pending_payment.payment_id, "paid")
                        .await
                    {
                        error!("Failed to update payment status: {}", e);
                    }

                    // Create a new session
                    match state.game_store.create_session(user.id).await {
                        Ok(session) => match state.game_store.create_game_config(&session).await {
                            Ok(config) => {
                                Ok((StatusCode::CREATED, Json(NewSessionResponse { config })))
                            }
                            Err(e) => Err(map_error(e)),
                        },
                        Err(e) => Err(map_error(e)),
                    }
                }
                "failed" => {
                    info!(
                        "Payment {} has failed, creating new invoice",
                        pending_payment.payment_id
                    );

                    // Payment failed, update our record
                    if let Err(e) = state
                        .payment_store
                        .update_payment_status(&pending_payment.payment_id, "failed")
                        .await
                    {
                        error!("Failed to update payment status: {}", e);
                    }

                    // Create a new invoice for the user
                    let description = format!("Asteroids Game Entry Fee - User:{}", pubkey);

                    // Step 1: Request a new payment from Voltage
                    let payment_id = match state
                        .lightning_service
                        .create_game_invoice(500, Some(&description))
                        .await
                    {
                        Ok(id) => id,
                        Err(e) => {
                            error!("Failed to create new invoice after payment failure: {}", e);
                            return Err((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Failed to create payment invoice",
                            )
                                .into_response());
                        }
                    };

                    // Step 2: Poll for the invoice
                    let mut invoice: Option<String> = None;
                    let max_attempts = 10;

                    for _attempt in 0..max_attempts {
                        match state
                            .lightning_service
                            .get_payment_invoice(&payment_id)
                            .await
                        {
                            Ok(Some(payment_request)) => {
                                invoice = Some(payment_request);
                                break;
                            }
                            Ok(None) => {
                                // Invoice not available yet, wait and retry
                                tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                            }
                            Err(e) => {
                                error!("Error getting payment invoice: {}", e);
                                return Err((
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    "Failed to retrieve payment invoice",
                                )
                                    .into_response());
                            }
                        }
                    }

                    // Step 3: Process the invoice result
                    match invoice {
                        Some(invoice_str) => {
                            // Store the payment in the database
                            match state
                                .payment_store
                                .create_game_payment(user.id, &payment_id, &invoice_str, 500)
                                .await
                            {
                                Ok(payment) => {
                                    // Return payment required response
                                    Err((
                                        StatusCode::PAYMENT_REQUIRED,
                                        Json(json!({
                                            "payment_required": true,
                                            "invoice": payment.invoice,
                                            "payment_id": payment.payment_id,
                                            "amount_sats": payment.amount_sats,
                                            "created_at": payment.created_at
                                        })),
                                    )
                                        .into_response())
                                }
                                Err(e) => Err(map_error(e)),
                            }
                        }
                        None => {
                            error!("Failed to get invoice after {} attempts", max_attempts);
                            Err((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Failed to generate Lightning invoice. Please try again.",
                            )
                                .into_response())
                        }
                    }
                }
                _ => {
                    info!("Payment {} is still pending", pending_payment.payment_id);

                    // Payment still pending
                    Err((
                        StatusCode::PAYMENT_REQUIRED,
                        Json(json!({
                            "payment_required": true,
                            "invoice": pending_payment.invoice,
                            "payment_id": pending_payment.payment_id,
                            "amount_sats": pending_payment.amount_sats,
                            "created_at": pending_payment.created_at
                        })),
                    )
                        .into_response())
                }
            }
        }
        Ok(None) => {
            // Payment not found in Lightning API yet, consider it still pending
            Err((
                StatusCode::PAYMENT_REQUIRED,
                Json(json!({
                    "payment_required": true,
                    "invoice": pending_payment.invoice,
                    "payment_id": pending_payment.payment_id,
                    "amount_sats": pending_payment.amount_sats,
                    "created_at": pending_payment.created_at,
                    "message": "Payment processing, please wait"
                })),
            )
                .into_response())
        }
        Err(e) => {
            error!("Failed to check payment status: {}", e);

            // Return the existing invoice in case of error checking status
            Err((
                StatusCode::PAYMENT_REQUIRED,
                Json(json!({
                    "payment_required": true,
                    "invoice": pending_payment.invoice,
                    "payment_id": pending_payment.payment_id,
                    "amount_sats": pending_payment.amount_sats,
                    "created_at": pending_payment.created_at,
                    "error": "Could not verify payment status. Please try again."
                })),
            )
                .into_response())
        }
    }
}

// Submit a score
pub async fn submit_score(
    auth: NostrAuth,
    State(state): State<Arc<AppState>>,
    Json(submission): Json<ScoreSubmission>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    info!("Score submission from pubkey: {}", pubkey);

    // Find user
    let user = match state.user_store.find_by_pubkey(pubkey).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err((StatusCode::UNAUTHORIZED, "User not found").into_response()),
        Err(e) => return Err(map_error(e)),
    };

    // Debug log the session ID we're looking for
    info!("Looking for session ID: {}", submission.session_id);

    // Verify session
    match state.game_store.find_session(&submission.session_id).await {
        Ok(Some(session)) => {
            if session.user_id != user.id {
                return Err(
                    (StatusCode::FORBIDDEN, "Session belongs to a different user").into_response(),
                );
            }

            // Submit the score
            match state
                .game_store
                .submit_score(
                    user.id,
                    submission.score,
                    submission.level,
                    submission.play_time,
                )
                .await
            {
                Ok(score) => {
                    // Publish verified score to audit ledger
                    if let Err(e) = state
                        .ledger_service
                        .publish_score_verified(
                            &user.nostr_pubkey,
                            &submission.session_id,
                            "", // seed - will be populated when replay verification is implemented
                            submission.score,
                            submission.level,
                            0, // frames - will be populated when replay verification is implemented
                            "", // input_hash - will be populated when replay verification is implemented
                            &OffsetDateTime::now_utc().date().to_string(),
                        )
                        .await
                    {
                        warn!("Failed to publish score verification to ledger: {}", e);
                    }

                    let response = ScoreResponse {
                        id: score.id,
                        score: score.score,
                        level: score.level,
                        play_time: score.play_time,
                        created_at: score.created_at,
                    };
                    Ok((StatusCode::CREATED, Json(response)))
                }
                Err(e) => Err(map_error(e)),
            }
        }
        Ok(None) => {
            info!("Session not found: {}", submission.session_id);
            Err((StatusCode::NOT_FOUND, "Session not found").into_response())
        }
        Err(e) => Err(map_error(e)),
    }
}

// Get top scores
pub async fn get_top_scores(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    info!("Get top scores request");

    match state.game_store.get_top_scores(10).await {
        Ok(scores) => Ok((StatusCode::OK, Json(scores))),
        Err(e) => Err(map_error(e)),
    }
}

// Get user scores
pub async fn get_user_scores(
    auth: NostrAuth,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    info!("Get user scores request from pubkey: {}", pubkey);

    // Find user
    let user = match state.user_store.find_by_pubkey(pubkey).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err((StatusCode::UNAUTHORIZED, "User not found").into_response()),
        Err(e) => return Err(map_error(e)),
    };

    // Get scores
    match state.game_store.get_user_scores(user.id, 10).await {
        Ok(scores) => {
            let response: Vec<ScoreResponse> = scores
                .into_iter()
                .map(|score| ScoreResponse {
                    id: score.id,
                    score: score.score,
                    level: score.level,
                    play_time: score.play_time,
                    created_at: score.created_at,
                })
                .collect();

            Ok((StatusCode::OK, Json(response)))
        }
        Err(e) => Err(map_error(e)),
    }
}
