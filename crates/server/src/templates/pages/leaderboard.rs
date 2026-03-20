use maud::{html, Markup};

use crate::domain::ScoreWithUsername;
use crate::templates::fragments::leaderboard_rows::leaderboard_rows;
use crate::templates::layouts::base::{base, PageConfig};

pub fn leaderboard_page(config: &PageConfig, scores: &[ScoreWithUsername]) -> Markup {
    base(config, leaderboard_content(scores))
}

pub fn leaderboard_content(scores: &[ScoreWithUsername]) -> Markup {
    html! {
        div class="leaderboard-container nes-container is-dark" {
            h2 class="nes-text is-primary" { "LEADERBOARD" }

            div class="nav-buttons" {
                a href="/"
                  class="nes-btn is-warning"
                  hx-get="/"
                  hx-target="#main-content"
                  hx-push-url="true" {
                    "Back to Home"
                }
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
