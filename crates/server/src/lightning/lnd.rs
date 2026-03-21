use base64::Engine;
use log::{error, info, warn};
use reqwest_middleware::reqwest;
use serde::{Deserialize, Serialize};

use super::models::LightningError;

// ── LND response types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LndInvoiceResponse {
    pub payment_request: String,
    pub r_hash: String, // base64 encoded
    pub add_index: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LndInvoiceLookup {
    pub state: String, // "OPEN", "SETTLED", "CANCELED", "ACCEPTED"
    pub r_hash: String,
    pub value: String,
    pub settled: bool,
    pub payment_request: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LndPaymentResponse {
    pub payment_hash: String,
    pub status: String, // "SUCCEEDED", "FAILED", "IN_FLIGHT"
    pub payment_preimage: Option<String>,
    pub failure_reason: Option<String>,
}

/// Intermediate type used when reading streamed payment status lines from LND.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LndStreamedPayment {
    result: Option<LndPaymentResponse>,
    error: Option<serde_json::Value>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn hex_to_base64url(hex_str: &str) -> Result<String, LightningError> {
    let bytes = hex::decode(hex_str)
        .map_err(|e| LightningError::ApiError(format!("invalid hex: {}", e)))?;
    Ok(base64::engine::general_purpose::URL_SAFE.encode(&bytes))
}

pub fn base64_to_hex(b64: &str) -> Result<String, LightningError> {
    // LND may return standard base64 or URL-safe base64; try both.
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b64))
        .map_err(|e| LightningError::ApiError(format!("invalid base64: {}", e)))?;
    Ok(hex::encode(&bytes))
}

fn load_macaroon(path: &str) -> Result<String, anyhow::Error> {
    let bytes = std::fs::read(path)?;
    Ok(hex::encode(&bytes))
}

// ── LndClient ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LndClient {
    base_url: String,
    client: reqwest::Client,
    macaroon: String, // hex-encoded macaroon
}

impl LndClient {
    pub fn new(
        base_url: &str,
        macaroon_path: &str,
        tls_cert_path: Option<&str>,
    ) -> Result<Self, anyhow::Error> {
        let macaroon = load_macaroon(macaroon_path)?;

        let mut builder = reqwest::Client::builder();

        // If a TLS cert is provided (self-signed LND node), add it and disable
        // default system root verification so the self-signed cert is trusted.
        if let Some(cert_path) = tls_cert_path {
            let cert_pem = std::fs::read(cert_path)?;
            let cert = reqwest::Certificate::from_pem(&cert_pem)?;
            builder = builder
                .add_root_certificate(cert)
                .danger_accept_invalid_certs(true);
        }

        let client = builder.build()?;

        // Normalise base_url: strip trailing slash so we can append paths consistently.
        let base_url = base_url.trim_end_matches('/').to_string();

        Ok(Self {
            base_url,
            client,
            macaroon,
        })
    }

    // ── Create Invoice (receive payment) ─────────────────────────────────

    pub async fn create_invoice(
        &self,
        amount_sats: i64,
        memo: &str,
    ) -> Result<LndInvoiceResponse, LightningError> {
        info!("LND: creating invoice for {} sats", amount_sats);

        let url = format!("{}/v1/invoices", self.base_url);
        let body = serde_json::json!({
            "value": amount_sats.to_string(),
            "expiry": "3600",
            "memo": memo,
        });

        let response = self
            .client
            .post(&url)
            .header("Grpc-Metadata-macaroon", &self.macaroon)
            .json(&body)
            .send()
            .await
            .map_err(|e| LightningError::ApiError(format!("LND request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("LND create_invoice error: {} - {}", status, text);
            return Err(LightningError::ApiError(format!(
                "LND create_invoice failed: {} - {}",
                status, text
            )));
        }

        let invoice: LndInvoiceResponse = response.json().await.map_err(|e| {
            LightningError::InvalidResponse(format!("Failed to parse LND invoice response: {}", e))
        })?;

        info!(
            "LND: invoice created, payment_request starts with {}...",
            &invoice.payment_request[..20.min(invoice.payment_request.len())]
        );
        Ok(invoice)
    }

    // ── Lookup Invoice ───────────────────────────────────────────────────

    pub async fn lookup_invoice(
        &self,
        r_hash_hex: &str,
    ) -> Result<LndInvoiceLookup, LightningError> {
        let hash_b64 = hex_to_base64url(r_hash_hex)?;
        let url = format!(
            "{}/v2/invoices/lookup?payment_hash={}",
            self.base_url, hash_b64
        );

        info!("LND: looking up invoice {}", r_hash_hex);

        let response = self
            .client
            .get(&url)
            .header("Grpc-Metadata-macaroon", &self.macaroon)
            .send()
            .await
            .map_err(|e| LightningError::ApiError(format!("LND request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(LightningError::ApiError(format!(
                "LND lookup_invoice failed: {} - {}",
                status, text
            )));
        }

        let lookup: LndInvoiceLookup = response.json().await.map_err(|e| {
            LightningError::InvalidResponse(format!("Failed to parse LND lookup response: {}", e))
        })?;

        info!("LND: invoice {} state = {}", r_hash_hex, lookup.state);
        Ok(lookup)
    }

    // ── Send Payment (streaming) ─────────────────────────────────────────

    pub async fn send_payment(
        &self,
        payment_request: &str,
        amount_sats: i64,
        fee_limit_sats: i64,
    ) -> Result<LndPaymentResponse, LightningError> {
        info!(
            "LND: sending payment, amount={} sats, fee_limit={} sats",
            amount_sats, fee_limit_sats
        );

        let url = format!("{}/v2/router/send", self.base_url);
        let body = serde_json::json!({
            "payment_request": payment_request,
            "timeout_seconds": 60,
            "fee_limit_sat": fee_limit_sats.to_string(),
        });

        let response = self
            .client
            .post(&url)
            .header("Grpc-Metadata-macaroon", &self.macaroon)
            .json(&body)
            .send()
            .await
            .map_err(|e| LightningError::ApiError(format!("LND request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(LightningError::ApiError(format!(
                "LND send_payment failed: {} - {}",
                status, text
            )));
        }

        // The /v2/router/send endpoint streams JSONL responses.
        // Read the full body and parse each line until we find a terminal status.
        let body_text = response.text().await.map_err(|e| {
            LightningError::InvalidResponse(format!("Failed to read LND stream: {}", e))
        })?;

        let mut last_payment: Option<LndPaymentResponse> = None;

        for line in body_text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<LndStreamedPayment>(line) {
                Ok(streamed) => {
                    if let Some(err) = streamed.error {
                        return Err(LightningError::PaymentError(format!(
                            "LND payment stream error: {}",
                            err
                        )));
                    }
                    if let Some(payment) = streamed.result {
                        match payment.status.as_str() {
                            "SUCCEEDED" | "FAILED" => return Ok(payment),
                            _ => {
                                info!("LND: payment in-flight, status={}", payment.status);
                                last_payment = Some(payment);
                            }
                        }
                    }
                }
                Err(e) => {
                    // Try parsing as a direct LndPaymentResponse (some LND versions)
                    match serde_json::from_str::<LndPaymentResponse>(line) {
                        Ok(payment) => match payment.status.as_str() {
                            "SUCCEEDED" | "FAILED" => return Ok(payment),
                            _ => {
                                last_payment = Some(payment);
                            }
                        },
                        Err(_) => {
                            warn!("LND: could not parse stream line: {} ({})", line, e);
                        }
                    }
                }
            }
        }

        // If we exhausted the stream without a terminal status, return last known or error.
        last_payment.ok_or_else(|| {
            LightningError::PaymentError(
                "LND payment stream ended without a terminal status".to_string(),
            )
        })
    }

    // ── Track Payment ────────────────────────────────────────────────────

    pub async fn track_payment(
        &self,
        payment_hash_hex: &str,
    ) -> Result<LndPaymentResponse, LightningError> {
        let hash_b64 = hex_to_base64url(payment_hash_hex)?;
        let url = format!(
            "{}/v2/router/track/{}?no_inflight_updates=true",
            self.base_url, hash_b64
        );

        info!("LND: tracking payment {}", payment_hash_hex);

        let response = self
            .client
            .get(&url)
            .header("Grpc-Metadata-macaroon", &self.macaroon)
            .send()
            .await
            .map_err(|e| LightningError::ApiError(format!("LND request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(LightningError::ApiError(format!(
                "LND track_payment failed: {} - {}",
                status, text
            )));
        }

        // Streaming endpoint — read lines and return the first terminal result.
        let body_text = response.text().await.map_err(|e| {
            LightningError::InvalidResponse(format!("Failed to read LND stream: {}", e))
        })?;

        for line in body_text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Ok(streamed) = serde_json::from_str::<LndStreamedPayment>(line) {
                if let Some(payment) = streamed.result {
                    return Ok(payment);
                }
            } else if let Ok(payment) = serde_json::from_str::<LndPaymentResponse>(line) {
                return Ok(payment);
            }
        }

        Err(LightningError::PaymentError(
            "LND track_payment stream ended with no result".to_string(),
        ))
    }

    // ── Subscribe to Invoice Updates (streaming) ──────────────────────────

    /// Opens a streaming connection to LND's SubscribeInvoices endpoint.
    /// Returns a response whose body can be read line-by-line for settled invoices.
    ///
    /// uri:/lnrpc.Lightning/SubscribeInvoices
    pub async fn subscribe_invoices(&self) -> Result<reqwest::Response, LightningError> {
        let url = format!("{}/v1/invoices/subscribe?settle_only=true", self.base_url);

        info!("LND: subscribing to invoice updates");

        let response = self
            .client
            .get(&url)
            .header("Grpc-Metadata-macaroon", &self.macaroon)
            .send()
            .await
            .map_err(|e| {
                LightningError::ApiError(format!("LND subscribe_invoices failed: {}", e))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(LightningError::ApiError(format!(
                "LND subscribe_invoices failed: {} - {}",
                status, text
            )));
        }

        Ok(response)
    }

    // ── Ping / health check ──────────────────────────────────────────────

    pub async fn ping(&self) -> Result<(), LightningError> {
        let url = format!("{}/v1/getinfo", self.base_url);

        let response = self
            .client
            .get(&url)
            .header("Grpc-Metadata-macaroon", &self.macaroon)
            .send()
            .await
            .map_err(|e| LightningError::ApiError(format!("LND ping failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(LightningError::ApiError(format!(
                "LND getinfo failed: {} - {}",
                status, text
            )));
        }

        info!("LND: ping successful");
        Ok(())
    }
}
