use maud::{html, Markup};

use crate::domain::ScoreWithUsername;

pub fn leaderboard_rows(scores: &[ScoreWithUsername]) -> Markup {
    html! {
        @if scores.is_empty() {
            tr {
                td colspan="6" class="has-text-centered" {
                    "No scores available yet!"
                }
            }
        } @else {
            @for (index, score) in scores.iter().enumerate() {
                tr {
                    td class="has-text-centered" { (index + 1) }
                    td class="has-text-centered nes-text is-primary" { (&score.username) }
                    td class="has-text-centered nes-text is-success" { (score.score) }
                    td class="has-text-centered" { (score.level) }
                    td class="has-text-centered" { (format_play_time(score.play_time)) }
                    td class="has-text-centered" { (&score.created_at) }
                }
            }
        }
    }
}

fn format_play_time(seconds: i64) -> String {
    let minutes = seconds / 60;
    let secs = seconds % 60;
    format!("{}m:{:02}s", minutes, secs)
}
