use maud::{html, Markup, DOCTYPE};

use crate::templates::components::auth_modals::auth_modals;
use crate::templates::components::profile_modal::profile_modal;
use crate::templates::layouts::navbar::navbar;

pub struct PageConfig<'a> {
    pub title: &'a str,
    pub api_base: &'a str,
    pub default_relays: &'a [String],
    pub entry_fee_sats: i64,
    pub plays_per_payment: i32,
    pub plays_ttl_minutes: i64,
    pub prize_pool_pct: u8,
}

fn how_to_play_modal(entry_fee_sats: i64, plays_per_payment: i32, prize_pool_pct: u8) -> Markup {
    html! {
        div id="howToPlayModal" class="modal" {
            div class="modal-content" {
                span class="modal-close" id="closeHowToPlay" { "\u{00d7}" }
                h2 class="nes-text is-primary" { "How to Play" }

                div style="font-size: 0.8em;" {
                    div class="nes-container is-dark" style="margin-bottom: 12px;" {
                        p class="nes-text is-warning" style="margin-bottom: 8px;" { "THE GAME" }
                        p { "Asteroid-style arcade shooter. Destroy asteroids, enemies, and bosses to rack up points." }
                        p style="margin-top: 6px;" {
                            "Controls: " strong { "Arrows" } " to move, " strong { "Space" } " to fire."
                        }
                    }

                    div class="nes-container is-dark" style="margin-bottom: 12px;" {
                        p class="nes-text is-success" style="margin-bottom: 8px;" { "PAY & PLAY" }
                        p { "1. Pay " (entry_fee_sats) " sats for " (plays_per_payment) " plays" }
                        p { "2. Compete for the daily high score" }
                        p { "3. Top scorer wins " (prize_pool_pct) "% of all entry fees!" }
                    }

                    div class="nes-container is-dark" {
                        p class="nes-text is-primary" style="margin-bottom: 8px;" { "PAYOUTS" }
                        p { "Set a lightning address in your " strong { "Profile" } " and prizes are paid out automatically when you win." }
                        p style="margin-top: 6px;" { "Works with CashApp, Wallet of Satoshi, Strike, or any LNURL-enabled lightning wallet." }
                        p class="nes-text" style="margin-top: 8px; font-size: 0.85em; opacity: 0.8;" {
                            "Lightning address is optional — you can always pay and claim prizes with a regular bolt11 invoice instead."
                        }
                    }
                }
            }
        }
    }
}

pub fn base(config: &PageConfig, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover";
                meta name="theme-color" content="#212529";
                meta name="apple-mobile-web-app-capable" content="yes";
                meta name="apple-mobile-web-app-status-bar-style" content="black-translucent";
                meta name="format-detection" content="telephone=no";
                title { (config.title) }

                link rel="stylesheet" href="https://unpkg.com/nes.css@latest/css/nes.min.css";
                link href="https://fonts.googleapis.com/css?family=Press+Start+2P" rel="stylesheet";
                link rel="stylesheet" href="/static/styles.min.css";

                script src="https://unpkg.com/htmx.org@1.9.10" {}
                script type="module" src="https://unpkg.com/bitcoin-qr@1.4.1/dist/bitcoin-qr/bitcoin-qr.esm.js" {}
                script nomodule src="https://unpkg.com/bitcoin-qr@1.4.1/dist/bitcoin-qr/bitcoin-qr.js" {}
            }
            body data-api-base=(config.api_base) data-default-relays=(config.default_relays.join(",")) data-plays-per-payment=(config.plays_per_payment) {
                (navbar())

                div class="container" {
                    div id="main-content" {
                        (content)
                    }
                }

                (auth_modals())
                (profile_modal())
                (how_to_play_modal(config.entry_fee_sats, config.plays_per_payment, config.prize_pool_pct))

                script type="module" src="/static/loader.js" {}
            }
        }
    }
}
