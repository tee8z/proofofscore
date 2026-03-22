use maud::{html, Markup, PreEscaped};

use crate::templates::layouts::base::{base, PageConfig};

pub fn game_page(config: &PageConfig) -> Markup {
    base(
        config,
        game_content(
            config.entry_fee_sats,
            config.plays_per_payment,
            config.plays_ttl_minutes,
            config.prize_pool_pct,
        ),
    )
}

pub fn game_content(
    entry_fee_sats: i64,
    plays_per_payment: i32,
    plays_ttl_minutes: i64,
    prize_pool_pct: u8,
) -> Markup {
    // Serialize the default engine config for practice mode
    let default_engine_config =
        serde_json::to_string(&game_engine::config::GameConfig::default_config())
            .unwrap_or_else(|_| "{}".to_string());

    html! {
        // Embed default config for practice mode JS
        script {
            (PreEscaped(format!("window.DEFAULT_ENGINE_CONFIG = '{}';", default_engine_config.replace('\'', "\\'"))))
        }
        div id="game-section" {
            div id="start-screen" class="nes-container is-dark" {
                div class="start-content" {
                    h1 class="nes-text is-primary" { "Proof of Score" }
                    p class="nes-text is-warning" { "Get ready to play!" }
                    p id="playsRemainingDisplay" class="nes-text is-success" style="display: none;" {}
                    div style="display: flex; gap: 8px; justify-content: center; flex-wrap: wrap;" {
                        button class="nes-btn is-primary" id="startGameBtn" {
                            "Start Game"
                        }
                        button class="nes-btn" id="practiceBtn" {
                            "Practice"
                        }
                    }
                }
            }

            div class="game-container nes-container is-dark" style="display: none;" {
                p id="practiceModeIndicator" class="nes-text is-warning" style="display: none; text-align: center; font-size: 0.7em; margin-bottom: 4px;" {
                    "PRACTICE MODE — scores will not be submitted"
                }
                div class="game-ui nes-container is-rounded is-dark" {
                    div class="nes-text is-primary" {
                        "LIVES: " span id="lives" { "♦ ♦ ♦" }
                    }
                    div class="nes-text is-warning" {
                        "SCORE: " span id="score" { "0" }
                    }
                    div class="nes-text is-success" {
                        "LEVEL: " span id="level" { "1" }
                    }
                    div class="nes-text is-error" {
                        "TIME: " span id="time" { "0" }
                    }
                }

                canvas id="gameCanvas" width="800" height="600" {}

                // Touch controls — below canvas, visible only on touch devices
                div class="touch-controls" id="touchControls" {
                    div class="touch-controls-left" {
                        div class="touch-dpad" {
                            button class="touch-btn touch-thrust" id="touchThrust" { "▲" }
                            div class="touch-dpad-row" {
                                button class="touch-btn touch-rotate-left" id="touchLeft" { "◄" }
                                button class="touch-btn touch-rotate-right" id="touchRight" { "►" }
                            }
                        }
                    }
                    div class="touch-controls-right" {
                        button class="touch-btn touch-fire" id="touchFire" { "FIRE" }
                    }
                }

                div class="controls nes-container is-rounded" id="keyboardControls" {
                    p class="nes-text is-primary" { "CONTROLS:" }
                    ul class="nes-list is-disc" {
                        li { "ARROWS: Move ship" }
                        li { "SPACE: Fire" }
                    }
                }
            }

            div id="game-over-dialog" class="nes-dialog game-over-overlay" style="display: none;" {
                div class="dialog-content" {
                    h2 class="nes-text is-error" { "GAME OVER" }
                    p { "Final Score: " span id="final-score" { "0" } }
                    p id="gameOverPlaysRemaining" class="nes-text is-success" style="display: none;" {}
                    div style="display: flex; gap: 8px; justify-content: center; flex-wrap: wrap;" {
                        button type="button" id="restart-button" class="nes-btn is-primary" {
                            "Play Again"
                        }
                        button type="button" id="play-for-real-button" class="nes-btn is-success" style="display: none;" {
                            "Play for Real"
                        }
                        a href="/leaderboard"
                          class="nes-btn is-warning"
                          id="view-leaderboard-button"
                          hx-get="/leaderboard"
                          hx-target="#main-content"
                          hx-push-url="true" {
                            "View Leaderboard"
                        }
                    }
                }
            }

            // Payment Modal
            div id="paymentModal" class="payment-modal-overlay" {
                div class="payment-modal-content" {
                    h2 class="nes-text is-primary" { "Pay to Play" }
                    p { (entry_fee_sats) " sats = " (plays_per_payment) " plays" }
                    @if plays_ttl_minutes > 0 {
                        p class="nes-text is-warning" style="font-size: 0.8em;" {
                            "Plays expire " (plays_ttl_minutes) " min after purchase"
                        }
                    }
                    p class="nes-text is-success" style="font-size: 0.8em;" {
                        (prize_pool_pct) "% of each entry goes to the prize pool!"
                    }

                    div class="qr-container" id="qrContainer" {}

                    div class="nes-field" {
                        label for="paymentRequest" { "Lightning Invoice:" }
                        input type="text" id="paymentRequest" class="nes-input" readonly;
                        div id="copyFeedback" class="nes-text is-success copy-feedback" {
                            "Copied to clipboard!"
                        }
                    }

                    div class="payment-buttons" {
                        button id="copyInvoiceBtn" class="nes-btn is-primary" { "Copy Invoice" }
                        button id="checkPaymentBtn" class="nes-btn is-warning" { "Check Payment" }
                        button id="cancelPaymentBtn" class="nes-btn is-error" { "Cancel" }
                    }

                    div id="paymentStatus" class="payment-status nes-container is-dark" {
                        p { "Waiting for payment..." }
                    }
                }
            }
        }
    }
}
