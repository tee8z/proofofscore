use maud::{html, Markup};

use crate::templates::layouts::base::{base, PageConfig};

pub fn game_page(config: &PageConfig) -> Markup {
    base(config, game_content())
}

pub fn game_content() -> Markup {
    html! {
        div id="game-section" {
            div id="start-screen" class="nes-container is-dark" {
                div class="start-content" {
                    h1 class="nes-text is-primary" { "ASTEROIDS" }
                    p class="nes-text is-warning" { "Get ready to play!" }
                    button class="nes-btn is-primary" id="startGameBtn" {
                        "Start Game"
                    }
                }
            }

            div class="game-container nes-container is-dark" style="display: none;" {
                div class="game-ui nes-container is-rounded is-dark" {
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

                div class="controls nes-container is-rounded" {
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
                    button type="button" id="restart-button" class="nes-btn is-primary" {
                        "Play Again"
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

            // Payment Modal
            div id="paymentModal" class="payment-modal-overlay" {
                div class="payment-modal-content" {
                    h2 class="nes-text is-primary" { "Pay to Play" }
                    p { "Please pay 500 sats to start the game:" }

                    div class="qr-container" id="qrContainer" {}

                    div class="nes-field" {
                        label for="paymentRequest" { "Lightning Invoice:" }
                        input type="text" id="paymentRequest" class="nes-input" readonly;
                        div id="copyFeedback" class="nes-text is-success copy-feedback" {
                            "Copied to clipboard!"
                        }
                    }

                    div class="nes-container" style="margin-top: 15px;" {
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
