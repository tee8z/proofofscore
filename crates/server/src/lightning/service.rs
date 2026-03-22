use hyper::StatusCode;
use log::{error, info, warn};
use reqwest_middleware::ClientWithMiddleware;
use serde_json::Value;
use std::time::Duration;
use tokio::time;
use uuid::Uuid;

use super::models::LightningError;

#[derive(Debug, Clone)]
pub struct LightningService {
    client: ClientWithMiddleware,
    api_url: String,
    api_key: String,
    organization_id: String,
    environment_id: String,
    wallet_id: String,
}

impl LightningService {
    pub fn new(
        client: ClientWithMiddleware,
        api_url: String,
        api_key: String,
        organization_id: String,
        environment_id: String,
        wallet_id: String,
    ) -> Self {
        Self {
            client,
            api_url,
            api_key,
            organization_id,
            environment_id,
            wallet_id,
        }
    }

    // Create a Lightning invoice (bolt11) to receive payment for game entry
    pub async fn create_game_invoice(
        &self,
        amount_sats: i64,
        description: Option<&str>,
    ) -> Result<String, LightningError> {
        info!("Creating Lightning invoice for {} sats", amount_sats);

        let payment_id = Uuid::now_v7().to_string();
        let amount_msats = amount_sats * 1000; // Convert sats to msats

        // Create request payload according to Voltage API spec
        let request = serde_json::json!({
            "id": payment_id.clone(),
            "wallet_id": self.wallet_id,
            "currency": "btc",
            "amount_msats": amount_msats,
            "payment_kind": "bolt11",
            "description": description.unwrap_or("Proof of Score Entry Fee")
        });

        let url = format!(
            "{}organizations/{}/environments/{}/payments",
            self.api_url, self.organization_id, self.environment_id
        );

        info!("Sending request to: {}", url);

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&request)
            .send()
            .await
            .map_err(LightningError::RequestError)?;

        // As long as the request is accepted (2xx status), we consider this successful
        if response.status().is_success() {
            info!("Payment request initiated with payment_id: {}", payment_id);
            Ok(payment_id)
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("Failed to create invoice: {}", error_text);
            Err(LightningError::ApiError(format!(
                "Failed to create invoice: {}",
                error_text
            )))
        }
    }

    // Get payment status - used to check if payment has been received
    pub async fn get_payment_status(
        &self,
        payment_id: &str,
    ) -> Result<Option<Value>, LightningError> {
        info!("Checking payment status for: {}", payment_id);

        let url = format!(
            "{}organizations/{}/environments/{}/payments/{}",
            self.api_url, self.organization_id, self.environment_id, payment_id
        );

        let response = self
            .client
            .get(&url)
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(LightningError::RequestError)?;

        // Handle 404 specially - payment might be in the process of being created
        if response.status() == StatusCode::NOT_FOUND {
            info!("Payment {} not found yet (still being created)", payment_id);

            // Return a placeholder "in progress" payment
            return Ok(None);
        }

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            return Err(LightningError::ApiError(format!(
                "Failed to get payment status: {} - {}",
                status, error_text
            )));
        }

        let payment_json: Value = response.json().await.map_err(|e| {
            LightningError::InvalidResponse(format!(
                "Failed to parse payment status response: {}",
                e
            ))
        })?;

        info!("Payment details: {}", payment_json);

        Ok(Some(payment_json))
    }

    pub async fn get_payment_invoice(
        &self,
        payment_id: &str,
    ) -> Result<Option<String>, LightningError> {
        info!("Getting invoice for payment_id: {}", payment_id);

        // Try to get the payment status
        let response = self
            .client
            .get(format!(
                "{}organizations/{}/environments/{}/payments/{}",
                self.api_url, self.organization_id, self.environment_id, payment_id
            ))
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(LightningError::RequestError)?;

        // Handle 404 responses specially - the payment exists but isn't ready yet
        if response.status() == StatusCode::NOT_FOUND {
            info!("Payment {} not found yet (still being created)", payment_id);
            return Ok(None); // Return None to indicate "not ready yet"
        }

        // Handle other error responses
        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            warn!("Error getting payment details: {}", error_text);
            return Err(LightningError::ApiError(format!(
                "Failed to get payment details: {}",
                error_text
            )));
        }

        // Log response status and headers before consuming the body
        info!(
            "Response status: {}, headers: {:?}",
            response.status(),
            response.headers()
        );

        // Parse the successful response
        let payment: Value = response.json().await.map_err(|e| {
            LightningError::InvalidResponse(format!("Failed to parse payment response: {}", e))
        })?;
        info!("Payment details: {:?}", payment);

        // Extract the payment request
        self.extract_invoice_from_payment(&payment)
            .map(Some)
            .or(Ok(None))
    }

    fn extract_invoice_from_payment(&self, payment: &Value) -> Result<String, LightningError> {
        // Check payment type
        let payment_type = payment["type"].as_str().ok_or_else(|| {
            LightningError::PaymentError("Payment type not found or not a string".to_string())
        })?;

        // Check payment direction
        let direction = payment["direction"].as_str().ok_or_else(|| {
            LightningError::PaymentError("Payment direction not found or not a string".to_string())
        })?;

        // Verify that it's a BOLT11 receive payment
        if payment_type == "bolt11" && direction == "receive" {
            // Try to extract the payment request from the data
            if let Some(data) = payment.get("data") {
                if let Some(payment_request) = data.get("payment_request") {
                    if let Some(request_str) = payment_request.as_str() {
                        return Ok(request_str.to_string());
                    }
                }
            }
        }

        // If we reach here, we couldn't find a payment request
        Err(LightningError::PaymentError(
            "No invoice found in payment data".to_string(),
        ))
    }

    // Wait for payment to be received with timeout
    pub async fn wait_for_payment(
        &self,
        payment_id: &str,
        timeout_secs: u64,
    ) -> Result<Value, LightningError> {
        info!(
            "Waiting for payment {} with timeout {}s",
            payment_id, timeout_secs
        );

        // Setup timeout
        let timeout = time::Duration::from_secs(timeout_secs);
        let start = time::Instant::now();

        // Poll for payment status
        loop {
            if start.elapsed() >= timeout {
                return Err(LightningError::PaymentTimeout(format!(
                    "Timeout waiting for payment {}",
                    payment_id
                )));
            }

            match self.get_payment_status(payment_id).await {
                Ok(Some(payment)) => {
                    // Check status
                    if let Some(status) = payment["status"].as_str() {
                        match status {
                            "completed" => {
                                info!("Payment {} completed", payment_id);
                                return Ok(payment);
                            }
                            "failed" => {
                                return Err(LightningError::PaymentError(format!(
                                    "Payment {} failed",
                                    payment_id
                                )));
                            }
                            _ => {
                                // Payment still in progress, continue polling
                                info!("Payment {} status: {}", payment_id, status);
                            }
                        }
                    }
                }
                Ok(None) => {
                    info!("Payment {} not found yet, will retry", payment_id);
                }
                Err(e) => {
                    warn!("Error checking payment status: {}", e);
                    // Continue polling on errors, as they might be transient
                }
            }

            // Wait before next poll
            time::sleep(Duration::from_secs(3)).await;
        }
    }

    // Send a payment to a winner using their invoice
    pub async fn pay_winner_invoice(
        &self,
        invoice: &str,
        amount_msats: i64,
    ) -> Result<Value, LightningError> {
        info!(
            "Sending payment for invoice, amount: {} msats",
            amount_msats
        );

        let payment_id = Uuid::now_v7().to_string();

        let request = serde_json::json!({
            "id": payment_id,
            "wallet_id": self.wallet_id,
            "currency": "btc",
            "type": "bolt11",
            "data": {
                "payment_request": invoice,
                "max_fee_msats": amount_msats / 100, // 1% fee limit
            }
        });

        let url = format!(
            "{}organizations/{}/environments/{}/payments",
            self.api_url, self.organization_id, self.environment_id
        );

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&request)
            .send()
            .await
            .map_err(LightningError::RequestError)?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(LightningError::ApiError(format!(
                "Failed to send payment: {} - {}",
                status, error_text
            )));
        }

        let payment: Value = response.json().await.map_err(|e| {
            LightningError::InvalidResponse(format!("Failed to parse payment response: {}", e))
        })?;

        info!("Payment initiated with id: {}", payment["id"]);

        // Wait for payment to complete
        self.wait_for_payment(payment["id"].as_str().unwrap_or(&payment_id), 60)
            .await
    }
}
