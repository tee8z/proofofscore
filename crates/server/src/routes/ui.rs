use axum::{extract::State, http::HeaderMap, response::Html};
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
        title: "ASTEROIDS",
        api_base: &state.remote_url,
    };

    if headers.contains_key("hx-request") {
        Html(pages::home::home_content(&scores).into_string())
    } else {
        Html(pages::home::home_page(&config, &scores).into_string())
    }
}

/// Game page handler
pub async fn game_handler(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Html<String> {
    let config = PageConfig {
        title: "ASTEROIDS - Play",
        api_base: &state.remote_url,
    };

    if headers.contains_key("hx-request") {
        Html(pages::game::game_content().into_string())
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
    let config = PageConfig {
        title: "ASTEROIDS - Leaderboard",
        api_base: &state.remote_url,
    };

    if headers.contains_key("hx-request") {
        Html(pages::leaderboard::leaderboard_content(&scores).into_string())
    } else {
        Html(pages::leaderboard::leaderboard_page(&config, &scores).into_string())
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

/// Fragment: navbar (for HTMX swap after auth state change)
pub async fn nav_fragment_handler() -> Html<String> {
    Html(fragments::nav::nav_fragment().into_string())
}
