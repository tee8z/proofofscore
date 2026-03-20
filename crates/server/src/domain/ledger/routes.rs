use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::{map_error, startup::AppState};

#[derive(Debug, Deserialize)]
pub struct LedgerQuery {
    pub date: Option<String>,
    #[serde(rename = "type")]
    pub event_type: Option<String>,
}

/// GET /api/v1/ledger/events?date=YYYY-MM-DD&type=game_entry
pub async fn get_ledger_events(
    Query(query): Query<LedgerQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let events = if let Some(date) = &query.date {
        let mut events = state
            .ledger_service
            .store()
            .get_events_by_date(date)
            .await
            .map_err(map_error)?;

        // If both date and type are specified, filter further
        if let Some(event_type) = &query.event_type {
            events.retain(|e| &e.event_type == event_type);
        }

        events
    } else if let Some(event_type) = &query.event_type {
        state
            .ledger_service
            .store()
            .get_events_by_type(event_type)
            .await
            .map_err(map_error)?
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            "Either 'date' or 'type' query parameter is required",
        )
            .into_response());
    };

    // Return the raw signed Nostr event JSON objects
    let event_jsons: Vec<serde_json::Value> = events
        .iter()
        .filter_map(|e| serde_json::from_str(&e.event_json).ok())
        .collect();

    Ok((StatusCode::OK, Json(json!({ "events": event_jsons }))))
}

/// GET /api/v1/ledger/pubkey
pub async fn get_server_pubkey(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({
        "pubkey": state.ledger_service.server_pubkey()
    }))
}

#[derive(Debug, Deserialize)]
pub struct SummaryQuery {
    pub date: String,
}

/// GET /api/v1/ledger/summary?date=YYYY-MM-DD
pub async fn get_ledger_summary(
    Query(query): Query<SummaryQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, Response> {
    let events = state
        .ledger_service
        .store()
        .get_events_by_date(&query.date)
        .await
        .map_err(map_error)?;

    let total_entries = events
        .iter()
        .filter(|e| e.event_type == "game_entry")
        .count();
    let total_scores = events
        .iter()
        .filter(|e| e.event_type == "score_verified")
        .count();

    // Sum entry amounts from game_entry events
    let mut pool_sats: i64 = 0;
    for event in events.iter().filter(|e| e.event_type == "game_entry") {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&event.event_json) {
            if let Some(tags) = parsed["tags"].as_array() {
                for tag in tags {
                    if let Some(arr) = tag.as_array() {
                        if arr.len() >= 2 && arr[0].as_str() == Some("amount") {
                            if let Some(amount_str) = arr[1].as_str() {
                                if let Ok(amount) = amount_str.parse::<i64>() {
                                    pool_sats += amount;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Extract competition result info if available
    let competition = events.iter().find(|e| e.event_type == "competition_result");
    let mut winner_pubkey: Option<String> = None;
    let mut winning_score: Option<i64> = None;
    let mut prize_sats: Option<i64> = None;

    if let Some(comp_event) = competition {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&comp_event.event_json) {
            if let Some(tags) = parsed["tags"].as_array() {
                for tag in tags {
                    if let Some(arr) = tag.as_array() {
                        if arr.len() >= 2 {
                            match arr[0].as_str() {
                                Some("winner") => {
                                    winner_pubkey = arr[1].as_str().map(|s| s.to_string());
                                }
                                Some("winning_score") => {
                                    winning_score =
                                        arr[1].as_str().and_then(|s| s.parse::<i64>().ok());
                                }
                                Some("prize_sats") => {
                                    prize_sats =
                                        arr[1].as_str().and_then(|s| s.parse::<i64>().ok());
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    let has_payout = events.iter().any(|e| e.event_type == "prize_payout");

    Ok((
        StatusCode::OK,
        Json(json!({
            "date": query.date,
            "total_entries": total_entries,
            "total_scores_verified": total_scores,
            "pool_sats": pool_sats,
            "winner_pubkey": winner_pubkey,
            "winning_score": winning_score,
            "prize_sats": prize_sats,
            "payout_completed": has_payout,
        })),
    ))
}
