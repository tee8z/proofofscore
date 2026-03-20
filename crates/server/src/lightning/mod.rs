pub mod lnd;
mod models;
mod service;

pub use lnd::{base64_to_hex, LndClient, LndInvoiceLookup, LndInvoiceResponse, LndPaymentResponse};
pub use models::*;
pub use service::*;

use log::info;

// ── Unified result types ─────────────────────────────────────────────────────

#[derive(Debug)]
pub struct InvoiceResult {
    /// For Voltage this is the payment UUID; for LND this is the r_hash in hex.
    pub payment_id: String,
    /// LND returns the bolt11 invoice immediately; Voltage requires polling.
    pub invoice: Option<String>,
}

#[derive(Debug)]
pub struct PaymentStatusResult {
    /// Normalised status: "paid", "pending", or "failed".
    pub status: String,
    /// The bolt11 payment_request, if available.
    pub invoice: Option<String>,
}

// ── LightningProvider ────────────────────────────────────────────────────────

/// Unified lightning provider that delegates to either the Voltage hosted wallet
/// API or a direct LND node connection.
///
/// Existing route handlers still reference `AppState.lightning_service` directly
/// (the Voltage client). New or migrated code should prefer this provider so
/// that either backend can be used transparently.
#[derive(Clone)]
pub enum LightningProvider {
    Voltage(LightningService),
    Lnd(LndClient),
}

impl std::fmt::Debug for LightningProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Voltage(_) => write!(f, "LightningProvider::Voltage"),
            Self::Lnd(_) => write!(f, "LightningProvider::Lnd"),
        }
    }
}

impl LightningProvider {
    /// Create a bolt11 invoice to receive a payment.
    pub async fn create_invoice(
        &self,
        amount_sats: i64,
        description: &str,
    ) -> Result<InvoiceResult, LightningError> {
        match self {
            Self::Voltage(svc) => {
                let payment_id = svc
                    .create_game_invoice(amount_sats, Some(description))
                    .await?;
                // Voltage creates the invoice asynchronously; callers must poll
                // for the bolt11 string via `check_payment_status`.
                Ok(InvoiceResult {
                    payment_id,
                    invoice: None,
                })
            }
            Self::Lnd(client) => {
                let resp = client.create_invoice(amount_sats, description).await?;
                let r_hash_hex = base64_to_hex(&resp.r_hash)?;
                Ok(InvoiceResult {
                    payment_id: r_hash_hex,
                    invoice: Some(resp.payment_request),
                })
            }
        }
    }

    /// Check whether an inbound payment has been settled.
    pub async fn check_payment_status(
        &self,
        payment_id: &str,
    ) -> Result<PaymentStatusResult, LightningError> {
        match self {
            Self::Voltage(svc) => {
                let maybe_payment = svc.get_payment_status(payment_id).await?;
                match maybe_payment {
                    Some(payment) => {
                        let status = payment["status"].as_str().unwrap_or("pending");
                        let normalised = match status {
                            "completed" => "paid",
                            "failed" => "failed",
                            _ => "pending",
                        };
                        let invoice = payment["data"]["payment_request"]
                            .as_str()
                            .map(|s| s.to_string());
                        Ok(PaymentStatusResult {
                            status: normalised.to_string(),
                            invoice,
                        })
                    }
                    None => Ok(PaymentStatusResult {
                        status: "pending".to_string(),
                        invoice: None,
                    }),
                }
            }
            Self::Lnd(client) => {
                let lookup = client.lookup_invoice(payment_id).await?;
                let status = match lookup.state.as_str() {
                    "SETTLED" => "paid",
                    "CANCELED" => "failed",
                    _ => "pending",
                };
                Ok(PaymentStatusResult {
                    status: status.to_string(),
                    invoice: Some(lookup.payment_request),
                })
            }
        }
    }

    /// Send an outbound payment via a bolt11 invoice.
    ///
    /// Returns the payment hash / id on success.
    pub async fn send_payment(
        &self,
        invoice: &str,
        amount_sats: i64,
    ) -> Result<String, LightningError> {
        match self {
            Self::Voltage(svc) => {
                let result = svc.pay_winner_invoice(invoice, amount_sats * 1000).await?;
                Ok(result["id"].as_str().unwrap_or("").to_string())
            }
            Self::Lnd(client) => {
                let fee_limit = std::cmp::max(amount_sats / 100, 10); // 1% or min 10 sats
                let resp = client.send_payment(invoice, amount_sats, fee_limit).await?;
                info!("LND payment status: {}", resp.status);
                if resp.status == "FAILED" {
                    return Err(LightningError::PaymentError(format!(
                        "LND payment failed: {:?}",
                        resp.failure_reason
                    )));
                }
                Ok(resp.payment_hash)
            }
        }
    }
}
