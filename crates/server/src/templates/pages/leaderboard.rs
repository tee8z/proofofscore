use maud::{html, Markup};

use crate::domain::ScoreWithUsername;
use crate::templates::fragments::leaderboard_rows::leaderboard_rows;
use crate::templates::layouts::base::{base, PageConfig};

pub fn leaderboard_page(config: &PageConfig, prize_pool_sats: i64, scores: &[ScoreWithUsername]) -> Markup {
    base(config, leaderboard_content(prize_pool_sats, config.prize_pool_pct, scores))
}

pub fn leaderboard_content(prize_pool_sats: i64, prize_pool_pct: u8, scores: &[ScoreWithUsername]) -> Markup {
    html! {
        div class="leaderboard-container nes-container is-dark" {
            h2 class="nes-text is-primary" { "LEADERBOARD" }

            div style="display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-bottom: 12px;" {
                a href="/"
                  class="nes-btn is-warning"
                  hx-get="/"
                  hx-target="#main-content"
                  hx-push-url="true" {
                    "Back to Home"
                }

                div class="nes-container is-rounded" style="padding: 8px 16px; display: inline-block;" {
                    span class="nes-text is-success" style="font-size: 1.1em;" {
                        "Prize Pool: " (prize_pool_sats) " sats"
                    }
                    span class="nes-text" style="font-size: 0.7em; margin-left: 8px; opacity: 0.7;" {
                        "(" (prize_pool_pct) "% of entries)"
                    }
                }
            }

            div class="replay-container" style="margin-bottom: 15px;" {
                p id="replayLabel" class="nes-text is-primary" style="display: none; font-size: 0.7em; margin-bottom: 8px;" {}
                canvas id="replayCanvas" width="800" height="600" style="display: none; width: 100%; max-width: 800px; border: 2px solid #333;" {}
            }

            table class="leaderboard-table" {
                thead {
                    tr {
                        th class="has-text-centered" { "Rank" }
                        th class="has-text-centered" { "Player" }
                        th class="has-text-centered" { "Score" }
                        th class="has-text-centered" { "Level" }
                        th class="has-text-centered" { "Time" }
                        th class="has-text-centered" { "Date" }
                        th class="has-text-centered" { "" }
                    }
                }
                tbody hx-get="/fragments/leaderboard-rows"
                      hx-trigger="every 30s"
                      hx-swap="innerHTML" {
                    (leaderboard_rows(scores))
                }
            }
        }
    }
}
