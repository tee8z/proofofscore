use std::sync::Arc;

use axum::{extract::State, response::Html};

use crate::startup::AppState;
use crate::templates::layouts::base::{base, PageConfig};

pub async fn index_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let config = PageConfig {
        title: "Not Found - Proof of Score",
        api_base: &state.remote_url,
        default_relays: &state.settings.ui_settings.default_relays,
        entry_fee_sats: state.settings.competition_settings.entry_fee_sats,
        plays_per_payment: state.settings.competition_settings.plays_per_payment,
        plays_ttl_minutes: state.settings.competition_settings.plays_ttl_minutes,
        prize_pool_pct: state.settings.competition_settings.prize_pool_pct,
        tip_address: state.settings.competition_settings.tip_address.as_deref(),
    };
    let content = maud::html! {
        div class="nes-container is-dark" style="text-align: center; margin-top: 40px;" {
            h1 class="nes-text is-error" { "404" }
            p { "Page not found." }
            a href="/" class="nes-btn is-primary" { "Go Home" }
        }
    };
    Html(base(&config, content).into_string())
}
