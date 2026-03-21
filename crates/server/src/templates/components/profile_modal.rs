use maud::{html, Markup};

pub fn profile_modal() -> Markup {
    html! {
        div id="profileModal" class="modal" {
            div class="modal-content" {
                span class="modal-close" id="closeProfileModal" { "\u{00d7}" }
                h2 class="nes-text is-primary" { "Player Profile" }

                // Stats section
                div class="nes-container is-dark" style="margin-bottom: 16px;" {
                    h3 class="nes-text is-warning" style="margin-bottom: 8px;" { "Stats" }
                    div class="profile-stats" {
                        div class="stat-row" {
                            span class="stat-label" { "High Score:" }
                            span class="stat-value nes-text is-success" id="profileHighScore" { "-" }
                        }
                        div class="stat-row" {
                            span class="stat-label" { "Total Plays:" }
                            span class="stat-value" id="profileTotalPlays" { "-" }
                        }
                        div class="stat-row" {
                            span class="stat-label" { "Games Purchased:" }
                            span class="stat-value" id="profileGamesPurchased" { "-" }
                        }
                        div class="stat-row" {
                            span class="stat-label" { "Total Spent:" }
                            span class="stat-value nes-text is-error" id="profileTotalSpent" { "-" }
                        }
                        div class="stat-row" {
                            span class="stat-label" { "Prizes Won:" }
                            span class="stat-value nes-text is-success" id="profilePrizesWon" { "-" }
                        }
                        div class="stat-row" {
                            span class="stat-label" { "Total Earned:" }
                            span class="stat-value nes-text is-success" id="profileTotalEarned" { "-" }
                        }
                    }
                }

                // Lightning address section
                div class="nes-container is-dark" {
                    h3 class="nes-text is-warning" style="margin-bottom: 8px;" { "Lightning Address" }
                    p style="font-size: 0.75em; margin-bottom: 8px;" {
                        "Set your lightning address so prizes are paid to you "
                        strong { "automatically" }
                        " when you win. Without one, you'll need to manually submit "
                        "an invoice to claim prizes."
                    }
                    p style="font-size: 0.7em; margin-bottom: 12px; opacity: 0.8;" {
                        "Works with CashApp ($cashtag), Wallet of Satoshi, "
                        "Strike, Primal, or any lightning wallet that gives you "
                        "an address like you@wallet.com"
                    }

                    div class="nes-field" {
                        label for="lightningAddressInput" { "Lightning Address:" }
                        input type="text" id="lightningAddressInput" class="nes-input"
                            placeholder="$cashtag or you@wallet.com";
                    }
                    p id="lightningAddressStatus" class="help-text" style="margin-top: 4px;" {}

                    div style="margin-top: 10px; display: flex; gap: 8px;" {
                        button id="saveLightningAddress" class="nes-btn is-success" {
                            "Save"
                        }
                        button id="clearLightningAddress" class="nes-btn" {
                            "Clear"
                        }
                    }
                }
            }
        }
    }
}
