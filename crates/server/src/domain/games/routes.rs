use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use time::OffsetDateTime;

/// Extract the client IP from X-Forwarded-For (leftmost/originating IP),
/// falling back to the direct connection address.
fn extract_client_ip(headers: &HeaderMap, addr: SocketAddr) -> String {
    if let Some(forwarded_for) = headers.get("x-forwarded-for") {
        if let Ok(value) = forwarded_for.to_str() {
            if let Some(first_ip) = value.split(',').next() {
                let trimmed = first_ip.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }
    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(value) = real_ip.to_str() {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    addr.ip().to_string()
}

use crate::{map_error, nostr_extractor::NostrAuth, startup::AppState};

use super::bot_detection::{
    analyze_frame_timings, analyze_ip_activity, analyze_server_timing, cross_reference_timings,
    extract_timing_signals, IpAnalysis,
};
use super::store::GameConfigResponse;
use super::store::ScoreMetadata;
use super::verify::verify_replay;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
pub struct ConfigQuery {
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NewSessionResponse {
    pub config: GameConfigResponse,
    pub plays_remaining: i64,
}

#[derive(Debug, Deserialize)]
pub struct ScoreSubmission {
    pub score: i64,
    pub level: i64,
    pub play_time: i64,
    pub session_id: String,
    pub input_log: String,
    pub input_hash: String,
    pub frames: u32,
    pub frame_timings: Option<String>,
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
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(query): Query<ConfigQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    let client_ip = extract_client_ip(&headers, addr);
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
        match state.game_store.create_session(user.id, &client_ip).await {
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
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    let client_ip = extract_client_ip(&headers, addr);
    info!(
        "New session request from pubkey: {}, ip: {}",
        pubkey, client_ip
    );

    // Find user
    let user = match state.user_store.find_by_pubkey(pubkey.clone()).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err((StatusCode::UNAUTHORIZED, "User not found").into_response()),
        Err(e) => return Err(map_error(e)),
    };

    // Check bans
    if user.banned != 0 {
        return Err((StatusCode::FORBIDDEN, "Account suspended").into_response());
    }
    if let Ok(true) = state.game_store.is_ip_banned(&client_ip).await {
        return Err((StatusCode::FORBIDDEN, "Access denied").into_response());
    }

    // Check if the user has remaining plays from a previous payment
    let remaining_plays = match state.payment_store.get_remaining_plays(user.id).await {
        Ok(plays) => plays,
        Err(e) => return Err(map_error(e)),
    };

    if remaining_plays > 0 {
        // User has plays remaining, use one and create a new session
        match state.game_store.create_session(user.id, &client_ip).await {
            Ok(session) => match state.game_store.create_game_config(&session).await {
                Ok(config) => {
                    let plays_remaining = state
                        .payment_store
                        .use_one_play(user.id)
                        .await
                        .map_err(map_error)?;
                    return Ok((
                        StatusCode::CREATED,
                        Json(NewSessionResponse {
                            config,
                            plays_remaining,
                        }),
                    ));
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
            // No pending payment, create a new invoice via the unified provider
            return create_and_return_invoice(&state, user.id, &pubkey).await;
        }
        Err(e) => return Err(map_error(e)),
    };

    // Check payment status for existing pending payment
    info!(
        "Checking status of existing payment: {}",
        pending_payment.payment_id
    );

    let status_result = state
        .lightning_provider
        .check_payment_status(&pending_payment.payment_id)
        .await;

    match status_result {
        Ok(result) => match result.status.as_str() {
            "paid" => {
                info!(
                    "Payment {} is paid, updating status",
                    pending_payment.payment_id
                );

                let comp = &state.settings.competition_settings;

                if let Err(e) = state
                    .payment_store
                    .update_payment_status(&pending_payment.payment_id, "paid")
                    .await
                {
                    error!("Failed to update payment status: {}", e);
                }

                // Grant plays for this payment with expiry
                if let Err(e) = state
                    .payment_store
                    .set_plays_remaining(
                        &pending_payment.payment_id,
                        comp.plays_per_payment,
                        comp.plays_ttl_minutes,
                    )
                    .await
                {
                    error!("Failed to set plays_remaining: {}", e);
                }

                // Create a new session (uses one play)
                match state.game_store.create_session(user.id, &client_ip).await {
                    Ok(session) => match state.game_store.create_game_config(&session).await {
                        Ok(config) => {
                            let plays_remaining = state
                                .payment_store
                                .use_one_play(user.id)
                                .await
                                .map_err(map_error)?;
                            Ok((
                                StatusCode::CREATED,
                                Json(NewSessionResponse {
                                    config,
                                    plays_remaining,
                                }),
                            ))
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

                if let Err(e) = state
                    .payment_store
                    .update_payment_status(&pending_payment.payment_id, "failed")
                    .await
                {
                    error!("Failed to update payment status: {}", e);
                }

                create_and_return_invoice(&state, user.id, &pubkey).await
            }
            _ => {
                info!("Payment {} is still pending", pending_payment.payment_id);

                Err((
                    StatusCode::PAYMENT_REQUIRED,
                    Json(json!({
                        "payment_required": true,
                        "invoice": pending_payment.invoice,
                        "payment_id": pending_payment.payment_id,
                        "amount_sats": pending_payment.amount_sats,
                        "created_at": pending_payment.created_at,
                        "lightning_address": user.lightning_address
                    })),
                )
                    .into_response())
            }
        },
        Err(e) => {
            error!("Failed to check payment status: {}", e);

            Err((
                StatusCode::PAYMENT_REQUIRED,
                Json(json!({
                    "payment_required": true,
                    "invoice": pending_payment.invoice,
                    "payment_id": pending_payment.payment_id,
                    "amount_sats": pending_payment.amount_sats,
                    "created_at": pending_payment.created_at,
                    "lightning_address": user.lightning_address,
                    "error": "Could not verify payment status. Please try again."
                })),
            )
                .into_response())
        }
    }
}

/// Helper: create a Lightning invoice and return a 402 Payment Required response.
/// Includes user's lightning address info so the frontend can show wallet deep-links.
async fn create_and_return_invoice(
    state: &Arc<AppState>,
    user_id: i64,
    pubkey: &str,
) -> Result<(StatusCode, Json<NewSessionResponse>), Response> {
    let entry_fee = state.settings.competition_settings.entry_fee_sats;
    let description = format!("Proof of Score Entry Fee - User:{}", pubkey);

    let invoice_result = state
        .lightning_provider
        .create_invoice(entry_fee, &description)
        .await
        .map_err(|e| {
            error!("Failed to create invoice: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create payment invoice",
            )
                .into_response()
        })?;

    // For LND the invoice is returned immediately; for Voltage it needs polling
    let invoice_str = match invoice_result.invoice {
        Some(inv) => inv,
        None => {
            // Voltage path: poll for the invoice
            let mut invoice = None;
            for attempt in 0..10 {
                info!("Poll attempt {} for invoice", attempt + 1);
                match state
                    .lightning_provider
                    .check_payment_status(&invoice_result.payment_id)
                    .await
                {
                    Ok(status) if status.invoice.is_some() => {
                        invoice = status.invoice;
                        break;
                    }
                    _ => {
                        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                    }
                }
            }
            invoice.ok_or_else(|| {
                error!("Failed to get invoice after polling");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to generate Lightning invoice. Please try again.",
                )
                    .into_response()
            })?
        }
    };

    info!("Successfully obtained invoice: {}", invoice_str);

    let payment = state
        .payment_store
        .create_game_payment(user_id, &invoice_result.payment_id, &invoice_str, entry_fee)
        .await
        .map_err(map_error)?;

    // Look up user's lightning address for frontend payment UX
    let lightning_address = state
        .user_store
        .find_by_id(user_id)
        .await
        .ok()
        .flatten()
        .and_then(|u| u.lightning_address);

    Err((
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({
            "payment_required": true,
            "invoice": payment.invoice,
            "payment_id": payment.payment_id,
            "amount_sats": payment.amount_sats,
            "created_at": payment.created_at,
            "lightning_address": lightning_address
        })),
    )
        .into_response())
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

    info!("Looking for session ID: {}", submission.session_id);

    // Verify session
    let session = match state.game_store.find_session(&submission.session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => {
            info!("Session not found: {}", submission.session_id);
            return Err((StatusCode::NOT_FOUND, "Session not found").into_response());
        }
        Err(e) => return Err(map_error(e)),
    };

    if session.user_id != user.id {
        return Err((StatusCode::FORBIDDEN, "Session belongs to a different user").into_response());
    }

    // Decode input log from base64
    let input_bytes = BASE64.decode(&submission.input_log).map_err(|e| {
        error!("Invalid base64 input_log: {}", e);
        (StatusCode::BAD_REQUEST, "Invalid input_log encoding").into_response()
    })?;

    // Verify input hash
    let computed_hash = hex::encode(Sha256::digest(&input_bytes));
    if computed_hash != submission.input_hash {
        error!(
            "Input hash mismatch: computed={}, submitted={}",
            computed_hash, submission.input_hash
        );
        return Err((StatusCode::BAD_REQUEST, "Input hash mismatch").into_response());
    }

    // Get seed and engine config from session
    let seed_hex = session.seed.as_deref().unwrap_or("");
    let seed = u64::from_str_radix(seed_hex, 16).map_err(|_| {
        error!("Invalid seed in session: {:?}", session.seed);
        (StatusCode::INTERNAL_SERVER_ERROR, "Invalid session seed").into_response()
    })?;

    let engine_config_str = session.engine_config.as_deref().unwrap_or("{}");
    let engine_config: game_engine::config::GameConfig = serde_json::from_str(engine_config_str)
        .map_err(|e| {
            error!("Invalid engine config in session: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Invalid session config").into_response()
        })?;

    // Replay and verify
    let result = verify_replay(
        seed,
        &engine_config,
        &input_bytes,
        submission.frames,
        submission.score as u32,
    );

    if !result.verified {
        error!(
            "Score verification failed: claimed={}, replayed={}, frames={}/{}",
            submission.score, result.score, submission.frames, result.frames
        );
        return Err((StatusCode::BAD_REQUEST, "Score verification failed").into_response());
    }

    info!(
        "Score verified: score={}, level={}, frames={}",
        result.score, result.level, result.frames
    );

    // Bot detection checks + signal collection for dashboard
    let mut bot_flags: Vec<String> = Vec::new();
    let mut ip_session_count: Option<i64> = None;
    let mut ip_account_count: Option<i64> = None;
    let mut server_elapsed: f64 = 0.0;
    let mut timing_signals = None;

    if state.settings.bot_detection.enabled {
        // IP-based analysis
        if let Some(ip) = &session.client_ip {
            match state.game_store.get_ip_activity(ip).await {
                Ok((sc, ac)) => {
                    ip_session_count = Some(sc);
                    ip_account_count = Some(ac);
                    let ip_result = analyze_ip_activity(
                        &IpAnalysis {
                            session_count: sc,
                            account_count: ac,
                        },
                        &state.settings.bot_detection,
                    );
                    if ip_result.reject {
                        warn!(
                            "Bot detection rejected score from IP {}: {:?}",
                            ip, ip_result.flags
                        );
                        return Err((StatusCode::FORBIDDEN, "Submission rejected").into_response());
                    }
                    bot_flags.extend(ip_result.flags);
                }
                Err(e) => warn!("Failed to check IP activity: {}", e),
            }
        }

        // Frame timing analysis (client-reported, fakeable)
        if let Some(ref timings_b64) = submission.frame_timings {
            if let Ok(timing_bytes) = BASE64.decode(timings_b64) {
                let timing_result =
                    analyze_frame_timings(&timing_bytes, &state.settings.bot_detection);
                bot_flags.extend(timing_result.flags);
            }
        }

        // Server-side timing check (unforgeable — uses server timestamps)
        if let Ok(session_time) = time::OffsetDateTime::parse(
            &session.start_time,
            &time::format_description::well_known::Iso8601::DEFAULT,
        ) {
            let now = OffsetDateTime::now_utc();
            server_elapsed = (now.unix_timestamp() - session_time.unix_timestamp()) as f64;

            let timing_result = analyze_server_timing(
                submission.frames,
                session_time.unix_timestamp(),
                now.unix_timestamp(),
            );
            if timing_result.reject {
                warn!(
                    "Server timing rejected: session={}, frames={}, elapsed={}s, flags={:?}",
                    submission.session_id, submission.frames, server_elapsed, timing_result.flags
                );
                return Err((StatusCode::FORBIDDEN, "Submission rejected").into_response());
            }
            bot_flags.extend(timing_result.flags);

            // Cross-reference client timing with server timing (catches faked timing data)
            if let Some(ref timings_b64) = submission.frame_timings {
                if let Ok(timing_bytes) = BASE64.decode(timings_b64) {
                    timing_signals = extract_timing_signals(&timing_bytes);

                    let xref = cross_reference_timings(&timing_bytes, server_elapsed);
                    if xref.reject {
                        warn!(
                            "Timing cross-reference rejected: session={}, flags={:?}",
                            submission.session_id, xref.flags
                        );
                        return Err((StatusCode::FORBIDDEN, "Submission rejected").into_response());
                    }
                    bot_flags.extend(xref.flags);
                }
            }
        }

        if !bot_flags.is_empty() {
            warn!(
                "Bot flags for session {}: {:?}",
                submission.session_id, bot_flags
            );
        }
    }

    // Save input log
    if let Err(e) = state
        .game_store
        .save_input_log(&submission.session_id, &input_bytes, &submission.input_hash)
        .await
    {
        warn!("Failed to save input log: {}", e);
    }

    // Submit the verified score
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
                    seed_hex,
                    submission.score,
                    submission.level,
                    submission.frames,
                    &submission.input_hash,
                    &OffsetDateTime::now_utc().date().to_string(),
                )
                .await
            {
                warn!("Failed to publish score verification to ledger: {}", e);
            }

            // Save score metadata for dashboard
            let expected_play_secs = submission.frames as f64 / 60.0;
            let timing_ratio = if expected_play_secs > 0.0 {
                server_elapsed / expected_play_secs
            } else {
                1.0
            };
            let xref_ratio = timing_signals.as_ref().map(|ts| {
                if server_elapsed > 0.0 {
                    ts.client_claimed_secs / server_elapsed
                } else {
                    1.0
                }
            });

            let meta = ScoreMetadata {
                score_id: score.id,
                session_id: submission.session_id.clone(),
                user_id: user.id,
                username: user.username.clone(),
                client_ip: session.client_ip.clone(),
                score: submission.score,
                level: submission.level,
                frames: submission.frames,
                play_time: submission.play_time,
                server_elapsed_secs: server_elapsed,
                expected_play_secs,
                server_timing_ratio: timing_ratio,
                client_claimed_secs: timing_signals.as_ref().map(|ts| ts.client_claimed_secs),
                timing_cross_ref_ratio: xref_ratio,
                timing_variance_us2: timing_signals.as_ref().map(|ts| ts.variance_us2),
                timing_mean_offset_us: timing_signals.as_ref().map(|ts| ts.mean_offset_us),
                ip_session_count,
                ip_account_count,
                flags: bot_flags,
                rejected: false,
            };
            if let Err(e) = state.game_store.save_score_metadata(&meta).await {
                warn!("Failed to save score metadata: {}", e);
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

// Get top replay data for today (for home page replay viewer)
pub async fn get_top_replays(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    info!("Get top replays request");

    match state.game_store.get_top_replays(3).await {
        Ok(replays) => Ok((StatusCode::OK, Json(replays))),
        Err(e) => Err(map_error(e)),
    }
}

// Get replay data for a specific score
pub async fn get_replay_by_score(
    Path(score_id): Path<i64>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    info!("Get replay for score_id: {}", score_id);

    match state.game_store.get_replay_by_score_id(score_id).await {
        Ok(Some(replay)) => Ok((StatusCode::OK, Json(replay))),
        Ok(None) => Err((StatusCode::NOT_FOUND, "Replay not found").into_response()),
        Err(e) => Err(map_error(e)),
    }
}

// Get competition info (window, entry fee, prize split) for countdown display
pub async fn get_competition_info(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let comp = &state.settings.competition_settings;
    let (end_h, end_m) = comp.end_hour_minute();
    (
        StatusCode::OK,
        Json(json!({
            "start_time": comp.start_time,
            "duration_secs": comp.duration_secs,
            "duration_display": comp.duration_display(),
            "end_time": format!("{:02}:{:02}", end_h, end_m),
            "entry_fee_sats": comp.entry_fee_sats,
            "plays_per_payment": comp.plays_per_payment,
            "prize_pool_pct": comp.prize_pool_pct,
        })),
    )
}
