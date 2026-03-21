use maud::{html, Markup};

use crate::domain::ScoreWithUsername;
use crate::templates::layouts::base::{base, PageConfig};

pub fn home_page(config: &PageConfig, scores: &[ScoreWithUsername]) -> Markup {
    base(config, home_content(config.entry_fee_sats, config.plays_per_payment, config.plays_ttl_minutes, config.prize_pool_pct, scores))
}

pub fn home_content(entry_fee_sats: i64, plays_per_payment: i32, plays_ttl_minutes: i64, prize_pool_pct: u8, scores: &[ScoreWithUsername]) -> Markup {
    html! {
        div id="welcome-screen" class="nes-container is-dark" {
            h1 class="nes-text is-primary" { "Proof of Play" }
            p class="nes-text is-success" style="font-size: 0.7em; margin-top: 4px;" {
                (entry_fee_sats) " sats = " (plays_per_payment) " plays"
                @if plays_ttl_minutes > 0 {
                    " (" (plays_ttl_minutes) " min window)"
                }
                " — " (prize_pool_pct) "% goes to the prize pool!"
            }

            // Replay viewer
            div class="replay-container" style="margin-top: 16px;" {
                p id="replayLabel" class="nes-text is-primary" style="display: none; font-size: 0.7em; margin-bottom: 8px;" {}
                canvas id="replayCanvas" width="800" height="600" style="display: none; width: 100%; max-width: 800px; border: 2px solid #333;" {}
            }

            div class="start-leaderboard" {
                h3 class="nes-text is-primary" { "TODAY'S TOP SCORES" }
                table class="leaderboard-table" {
                    thead {
                        tr {
                            th class="has-text-centered" { "Rank" }
                            th class="has-text-centered" { "Player" }
                            th class="has-text-centered" { "Score" }
                            th class="has-text-centered" { "Level" }
                        }
                    }
                    tbody id="start-scores-body"
                          hx-get="/fragments/leaderboard-rows"
                          hx-trigger="every 30s"
                          hx-swap="innerHTML" {
                        @if scores.is_empty() {
                            tr {
                                td colspan="4" class="has-text-centered" {
                                    "No scores available yet!"
                                }
                            }
                        } @else {
                            @for (index, score) in scores.iter().take(5).enumerate() {
                                tr {
                                    td class="has-text-centered" { (index + 1) }
                                    td class="has-text-centered nes-text is-primary" { (score.username) }
                                    td class="has-text-centered nes-text is-success" { (score.score) }
                                    td class="has-text-centered" { (score.level) }
                                }
                            }
                        }
                    }
                }
            }

            p class="nes-text is-warning" style="margin-top: 20px;" {
                "Please login or register to play!"
            }
            div class="auth-buttons-container" id="home-auth-cta" {
                button class="nes-btn is-primary" id="startLoginBtn" {
                    "Login"
                }
                button class="nes-btn is-success" id="startRegisterBtn" {
                    "Sign Up"
                }
            }
            div class="auth-buttons-container is-hidden" id="home-play-cta" {
                a href="/play"
                  class="nes-btn is-success"
                  hx-get="/play"
                  hx-target="#main-content"
                  hx-push-url="true" {
                    "Play Game"
                }
            }
        }
    }
}
