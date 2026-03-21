use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use log::{error, info};
use nostr_sdk::PublicKey;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, sync::Arc};

use super::password::{hash_password, verify_password};
use crate::{
    lightning::normalize_lightning_address, map_error, nostr_extractor::NostrAuth,
    startup::AppState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterPayload {
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub session_id: String,
    pub username: String,
    pub pubkey: String,
    pub lightning_address: Option<String>,
}

// --- Extension-based auth (existing) ---

pub async fn login(
    auth: NostrAuth,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    info!("Login request from pubkey: {}", pubkey);

    match state.user_store.login(pubkey).await {
        Ok(user_info) => {
            let response = LoginResponse {
                session_id: user_info.session_id,
                username: user_info.username,
                pubkey: user_info.pubkey,
                lightning_address: user_info.lightning_address,
            };
            Ok((StatusCode::OK, Json(response)))
        }
        Err(e) => {
            error!("Login error: {}", e);
            Err(map_error(e))
        }
    }
}

pub async fn register(
    auth: NostrAuth,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegisterPayload>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    info!("Register request from pubkey: {}", pubkey);

    match state.user_store.register(pubkey, payload).await {
        Ok(user_info) => {
            let response = LoginResponse {
                session_id: user_info.session_id,
                username: user_info.username,
                pubkey: user_info.pubkey,
                lightning_address: user_info.lightning_address,
            };
            Ok((StatusCode::CREATED, Json(response)))
        }
        Err(e) => {
            error!("Registration error: {}", e);
            Err(map_error(e))
        }
    }
}

// --- Username/password auth ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsernameRegisterPayload {
    pub username: String,
    pub password: String,
    pub encrypted_nsec: String,
    pub nostr_pubkey: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsernameRegisterResponse {
    pub nostr_pubkey: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsernameLoginPayload {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsernameLoginResponse {
    pub encrypted_nsec: String,
    pub nostr_pubkey: String,
}

fn validate_username(username: &str) -> Result<(), String> {
    if username.len() < 3 || username.len() > 32 {
        return Err("Username must be 3-32 characters".into());
    }
    if !username.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Err("Username must start with a letter".into());
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Username can only contain letters, numbers, underscores, and hyphens".into());
    }
    Ok(())
}

fn validate_password_strength(password: &str) -> Result<(), String> {
    if password.len() < 8 {
        return Err("Password must be at least 8 characters".into());
    }
    Ok(())
}

pub async fn register_username(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UsernameRegisterPayload>,
) -> Result<impl IntoResponse, Response> {
    info!("Username register request for: {}", payload.username);

    if let Err(msg) = validate_username(&payload.username) {
        return Err((StatusCode::BAD_REQUEST, msg).into_response());
    }

    if let Err(msg) = validate_password_strength(&payload.password) {
        return Err((StatusCode::BAD_REQUEST, msg).into_response());
    }

    // Convert pubkey to hex format (NostrAuth extractor uses hex, JS sends bech32)
    let nostr_pubkey = PublicKey::from_str(&payload.nostr_pubkey)
        .map(|pk| pk.to_string())
        .unwrap_or_else(|_| payload.nostr_pubkey.clone());

    match state.user_store.username_exists(&payload.username).await {
        Ok(true) => {
            return Err((StatusCode::CONFLICT, "Username already taken").into_response());
        }
        Ok(false) => {}
        Err(e) => {
            error!("Database error checking username: {}", e);
            return Err(map_error(e));
        }
    }

    let password_hash = hash_password(&payload.password).map_err(|e| {
        error!("Password hash error: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
    })?;

    match state
        .user_store
        .register_username_user(
            nostr_pubkey.clone(),
            payload.username.clone(),
            password_hash,
            payload.encrypted_nsec,
        )
        .await
    {
        Ok(_user) => {
            let response = UsernameRegisterResponse {
                nostr_pubkey,
                username: payload.username,
            };
            Ok((StatusCode::CREATED, Json(response)))
        }
        Err(e) => {
            error!("Registration error: {}", e);
            Err(map_error(e))
        }
    }
}

pub async fn login_username(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UsernameLoginPayload>,
) -> Result<impl IntoResponse, Response> {
    info!("Username login request for: {}", payload.username);

    // Timing-safe: always verify even if user not found
    let dummy_hash = "$argon2id$v=19$m=19456,t=2,p=1$dW5rbm93bg$YWxzb191bmtub3du";

    let user = state
        .user_store
        .find_by_username(&payload.username)
        .await
        .map_err(|e| {
            error!("Database error: {}", e);
            map_error(e)
        })?;

    let (hash_to_verify, found_user) = match &user {
        Some(u) => (u.password_hash.as_deref().unwrap_or(dummy_hash), true),
        None => (dummy_hash, false),
    };

    let password_valid = verify_password(&payload.password, hash_to_verify).unwrap_or(false);

    if !found_user || !password_valid {
        return Err((StatusCode::UNAUTHORIZED, "Invalid username or password").into_response());
    }

    let user = user.unwrap();
    let encrypted_nsec = user.encrypted_nsec.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Account missing encrypted key",
        )
            .into_response()
    })?;

    Ok((
        StatusCode::OK,
        Json(UsernameLoginResponse {
            encrypted_nsec,
            nostr_pubkey: user.nostr_pubkey,
        }),
    ))
}

// --- Lightning address management ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLightningAddressPayload {
    pub lightning_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfileResponse {
    pub username: String,
    pub pubkey: String,
    pub lightning_address: Option<String>,
    pub stats: crate::domain::payments::UserStats,
}

pub async fn get_user_profile(
    auth: NostrAuth,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();

    let user = match state.user_store.find_by_pubkey(pubkey).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err((StatusCode::NOT_FOUND, "User not found").into_response()),
        Err(e) => return Err(map_error(e)),
    };

    let stats = state
        .payment_store
        .get_user_stats(user.id)
        .await
        .map_err(map_error)?;

    Ok((
        StatusCode::OK,
        Json(UserProfileResponse {
            username: user.username,
            pubkey: user.nostr_pubkey,
            lightning_address: user.lightning_address,
            stats,
        }),
    ))
}

pub async fn update_lightning_address(
    auth: NostrAuth,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdateLightningAddressPayload>,
) -> Result<impl IntoResponse, Response> {
    let pubkey = auth.pubkey.to_string();
    info!("Update lightning address for pubkey: {}", pubkey);

    let user = match state.user_store.find_by_pubkey(pubkey).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err((StatusCode::NOT_FOUND, "User not found").into_response()),
        Err(e) => return Err(map_error(e)),
    };

    // Validate the lightning address if provided
    let normalized = match &payload.lightning_address {
        Some(addr) if !addr.trim().is_empty() => {
            let normalized = normalize_lightning_address(addr).map_err(|e| {
                (StatusCode::BAD_REQUEST, format!("Invalid lightning address: {}", e))
                    .into_response()
            })?;
            Some(normalized)
        }
        _ => None,
    };

    state
        .user_store
        .update_lightning_address(user.id, normalized.as_deref())
        .await
        .map_err(map_error)?;

    info!(
        "Lightning address updated for user {}: {:?}",
        user.id, normalized
    );

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "lightning_address": normalized,
        })),
    ))
}
