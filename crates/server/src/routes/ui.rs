use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
};
use log::warn;
use std::sync::Arc;

use crate::{
    startup::AppState,
    templates::{fragments, layouts::base::PageConfig, pages},
};

/// Home page handler
pub async fn home_handler(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Html<String> {
    let scores = state
        .game_store
        .get_top_scores(5)
        .await
        .unwrap_or_else(|e| {
            warn!("Failed to load top scores: {}", e);
            vec![]
        });
    let config = PageConfig {
        title: "Proof of Score",
        api_base: &state.remote_url,
        default_relays: &state.settings.ui_settings.default_relays,
        entry_fee_sats: state.settings.competition_settings.entry_fee_sats,
        plays_per_payment: state.settings.competition_settings.plays_per_payment,
        plays_ttl_minutes: state.settings.competition_settings.plays_ttl_minutes,
        prize_pool_pct: state.settings.competition_settings.prize_pool_pct,
        tip_address: state.settings.competition_settings.tip_address.as_deref(),
    };

    if headers.contains_key("hx-request") {
        Html(
            pages::home::home_content(
                config.entry_fee_sats,
                config.plays_per_payment,
                config.plays_ttl_minutes,
                config.prize_pool_pct,
                &scores,
            )
            .into_string(),
        )
    } else {
        Html(pages::home::home_page(&config, &scores).into_string())
    }
}

/// Game page handler
pub async fn game_handler(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Html<String> {
    let config = PageConfig {
        title: "Play",
        api_base: &state.remote_url,
        default_relays: &state.settings.ui_settings.default_relays,
        entry_fee_sats: state.settings.competition_settings.entry_fee_sats,
        plays_per_payment: state.settings.competition_settings.plays_per_payment,
        plays_ttl_minutes: state.settings.competition_settings.plays_ttl_minutes,
        prize_pool_pct: state.settings.competition_settings.prize_pool_pct,
        tip_address: state.settings.competition_settings.tip_address.as_deref(),
    };

    if headers.contains_key("hx-request") {
        Html(
            pages::game::game_content(
                config.entry_fee_sats,
                config.plays_per_payment,
                config.plays_ttl_minutes,
                config.prize_pool_pct,
            )
            .into_string(),
        )
    } else {
        Html(pages::game::game_page(&config).into_string())
    }
}

/// Leaderboard page handler
pub async fn leaderboard_handler(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Html<String> {
    let scores = state
        .game_store
        .get_top_scores(50)
        .await
        .unwrap_or_else(|e| {
            warn!("Failed to load scores: {}", e);
            vec![]
        });

    // Calculate current prize pool
    let today = time::OffsetDateTime::now_utc().date().to_string();
    let comp = &state.settings.competition_settings;
    let today_games = state
        .payment_store
        .count_games_for_date(&today)
        .await
        .unwrap_or(0);
    let prize_pool_sats = today_games * comp.entry_fee_sats * (comp.prize_pool_pct as i64) / 100;

    let config = PageConfig {
        title: "Leaderboard",
        api_base: &state.remote_url,
        default_relays: &state.settings.ui_settings.default_relays,
        entry_fee_sats: state.settings.competition_settings.entry_fee_sats,
        plays_per_payment: state.settings.competition_settings.plays_per_payment,
        plays_ttl_minutes: state.settings.competition_settings.plays_ttl_minutes,
        prize_pool_pct: state.settings.competition_settings.prize_pool_pct,
        tip_address: state.settings.competition_settings.tip_address.as_deref(),
    };

    if headers.contains_key("hx-request") {
        Html(
            pages::leaderboard::leaderboard_content(prize_pool_sats, comp.prize_pool_pct, &scores)
                .into_string(),
        )
    } else {
        Html(pages::leaderboard::leaderboard_page(&config, prize_pool_sats, &scores).into_string())
    }
}

/// Fragment: leaderboard rows (for HTMX polling)
pub async fn leaderboard_rows_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let scores = state
        .game_store
        .get_top_scores(50)
        .await
        .unwrap_or_else(|e| {
            warn!("Failed to load scores: {}", e);
            vec![]
        });
    Html(fragments::leaderboard_rows::leaderboard_rows(&scores).into_string())
}

/// Serve the service worker from root path (SW scope must be /)
pub async fn serve_service_worker(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sw_path = std::path::Path::new(&state.settings.ui_settings.static_dir).join("sw.js");
    match tokio::fs::read_to_string(&sw_path).await {
        Ok(content) => (
            StatusCode::OK,
            [
                ("content-type", "application/javascript"),
                ("service-worker-allowed", "/"),
            ],
            content,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "Service worker not found").into_response(),
    }
}

/// Fragment: navbar (for HTMX swap after auth state change)
pub async fn nav_fragment_handler() -> Html<String> {
    Html(fragments::nav::nav_fragment().into_string())
}
