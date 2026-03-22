use maud::{html, Markup, DOCTYPE};

use crate::asset_hashes;
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
    pub tip_address: Option<&'a str>,
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
                link rel="manifest" href="/static/manifest.json";
                link rel="icon" type="image/svg+xml" href="/static/icons/favicon.svg";
                link rel="apple-touch-icon" href="/static/icons/icon-192x192.png";
                title { (config.title) }

                link rel="stylesheet" href="https://unpkg.com/nes.css@latest/css/nes.min.css";
                link href="https://fonts.googleapis.com/css?family=Press+Start+2P" rel="stylesheet";
                link rel="stylesheet" href=(format!("/static/{}", asset_hashes::CSS_HASHED));

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

                @if let Some(tip_addr) = config.tip_address {
                    footer class="site-footer" {
                        span { "Enjoy the game? " }
                        span class="nes-text is-success tip-link" data-address=(tip_addr) title="Click to tip" style="cursor: pointer;" {
                            "Tip the devs ⚡"
                        }
                    }

                    // Tip modal
                    div id="tipModal" class="modal" {
                        div class="modal-content" {
                            span class="modal-close" id="closeTipModal" { "\u{00d7}" }
                            h2 class="nes-text is-success" { "Tip the Devs ⚡" }

                            div id="tipAddressStep" {
                                p style="font-size: 0.8em; margin-bottom: 12px;" {
                                    "Copy the lightning address and paste it into your wallet:"
                                }
                                div class="nes-container is-rounded" style="text-align: center; padding: 12px; margin-bottom: 12px; word-break: break-all;" {
                                    span class="nes-text is-primary" id="tipAddressDisplay" style="font-size: 0.9em;" { (tip_addr) }
                                }
                                div style="display: flex; gap: 8px; justify-content: center; margin-bottom: 16px;" {
                                    button class="nes-btn is-success" id="tipCopyAddressBtn" { "Copy Address" }
                                }
                                span id="tipAddressCopied" class="nes-text is-success" style="display: none;" { "Copied!" }

                                div style="border-top: 2px dashed #444; margin-top: 16px; padding-top: 12px;" {
                                    p style="font-size: 0.7em; opacity: 0.7; margin-bottom: 8px;" {
                                        "Or generate a specific invoice:"
                                    }
                                    div style="display: flex; gap: 8px; justify-content: center; flex-wrap: wrap; margin-bottom: 8px;" {
                                        button class="nes-btn tip-preset" data-amount="100" { "100" }
                                        button class="nes-btn tip-preset" data-amount="500" { "500" }
                                        button class="nes-btn tip-preset" data-amount="1000" { "1k" }
                                        button class="nes-btn tip-preset" data-amount="5000" { "5k" }
                                    }
                                    div style="display: flex; gap: 8px; align-items: flex-end; justify-content: center;" {
                                        div class="nes-field" style="flex: 1; max-width: 180px;" {
                                            input type="number" id="tipAmountInput" class="nes-input" min="1" max="1000000" placeholder="sats";
                                        }
                                        button class="nes-btn is-primary" id="tipSendBtn" { "Get Invoice" }
                                    }
                                }
                            }

                            div id="tipInvoiceStep" style="display: none;" {
                                div id="tipQrContainer" style="text-align: center; margin-bottom: 12px;" {}
                                div class="nes-field" {
                                    label for="tipInvoiceInput" { "Lightning Invoice:" }
                                    input type="text" id="tipInvoiceInput" class="nes-input" readonly;
                                }
                                div style="display: flex; gap: 8px; justify-content: center; margin-top: 12px;" {
                                    button class="nes-btn is-primary" id="tipCopyInvoiceBtn" { "Copy Invoice" }
                                    button class="nes-btn is-warning" id="tipBackBtn" { "Back" }
                                }
                                span id="tipInvoiceCopied" class="nes-text is-success" style="display: none;" { "Copied!" }
                                p id="tipStatus" class="nes-text" style="font-size: 0.8em; margin-top: 8px;" {}
                            }
                        }
                    }
                }

                (auth_modals())
                (profile_modal())
                (how_to_play_modal(config.entry_fee_sats, config.plays_per_payment, config.prize_pool_pct))

                script { (maud::PreEscaped(format!("window.APP_JS_PATH='/static/{}';", asset_hashes::JS_HASHED))) }
                script type="module" src="/static/loader.js" {}
            }
        }
    }
}
