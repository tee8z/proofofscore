use maud::{html, Markup};

#[allow(dead_code)]
pub fn payment_pending(invoice: &str) -> Markup {
    html! {
        div class="payment-status-fragment" {
            div class="spinner" {}
            p { "Waiting for payment..." }
            p class="nes-text is-primary" { "Amount: 500 sats" }
            div class="qr-container" id="qrContainer" {
                // QR code rendered client-side
            }
            div class="nes-field" {
                label { "Lightning Invoice:" }
                input type="text" class="nes-input" value=(invoice) readonly;
            }
        }
    }
}

#[allow(dead_code)]
pub fn payment_confirmed() -> Markup {
    html! {
        div class="payment-status-fragment" {
            p class="nes-text is-success" { "Payment confirmed!" }
            p { "Starting game..." }
        }
    }
}
