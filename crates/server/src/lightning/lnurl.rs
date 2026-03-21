use log::{error, info};
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Serialize};

use super::models::LightningError;

/// Metadata returned by the LNURL-pay first request (LUD-06 / LUD-16).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LnurlPayParams {
    pub callback: String,
    pub min_sendable: i64,  // millisats
    pub max_sendable: i64,  // millisats
    pub metadata: String,
    pub tag: Option<String>,
}

/// Response from the LNURL-pay callback containing the bolt11 invoice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LnurlPayInvoice {
    pub pr: String, // bolt11 payment request
    #[serde(default)]
    pub routes: Vec<serde_json::Value>,
}

/// LNURL error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LnurlError {
    pub status: String,
    pub reason: Option<String>,
}

/// Validate a lightning address format (user@domain).
/// Also accepts CashApp `$cashtag` shorthand → `cashtag@cash.app`.
pub fn normalize_lightning_address(input: &str) -> Result<String, LightningError> {
    let trimmed = input.trim();

    // CashApp shorthand: $cashtag → cashtag@cash.app
    if trimmed.starts_with('$') {
        let cashtag = &trimmed[1..];
        if cashtag.is_empty()
            || !cashtag
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(LightningError::ApiError(
                "Invalid CashApp cashtag".to_string(),
            ));
        }
        return Ok(format!("{}@cash.app", cashtag));
    }

    // Standard lightning address: user@domain
    let parts: Vec<&str> = trimmed.split('@').collect();
    if parts.len() != 2 {
        return Err(LightningError::ApiError(
            "Lightning address must be in user@domain format".to_string(),
        ));
    }

    let user = parts[0];
    let domain = parts[1];

    if user.is_empty() || domain.is_empty() {
        return Err(LightningError::ApiError(
            "Lightning address must be in user@domain format".to_string(),
        ));
    }

    // Basic domain validation — allow localhost for development
    let domain_host = domain.split(':').next().unwrap_or(domain);
    if !domain_host.contains('.') && domain_host != "localhost" {
        return Err(LightningError::ApiError(
            "Invalid domain in lightning address".to_string(),
        ));
    }

    Ok(trimmed.to_lowercase())
}

/// Resolve a lightning address to its LNURL-pay parameters.
///
/// Lightning address `user@domain` maps to:
/// `https://domain/.well-known/lnurlp/user`
pub async fn resolve_lightning_address(
    client: &ClientWithMiddleware,
    address: &str,
) -> Result<LnurlPayParams, LightningError> {
    let normalized = normalize_lightning_address(address)?;
    let parts: Vec<&str> = normalized.split('@').collect();
    let user = parts[0];
    let domain = parts[1];

    // Use http for localhost (development), https for everything else
    let domain_host = domain.split(':').next().unwrap_or(domain);
    let scheme = if domain_host == "localhost" || domain_host == "127.0.0.1" {
        "http"
    } else {
        "https"
    };
    let url = format!("{}://{}/.well-known/lnurlp/{}", scheme, domain, user);
    info!("LNURL: resolving lightning address {} → {}", address, url);

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(LightningError::RequestError)?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        error!("LNURL: resolution failed: {} - {}", status, text);
        return Err(LightningError::ApiError(format!(
            "LNURL resolution failed: {} - {}",
            status, text
        )));
    }

    let body: serde_json::Value = response.json().await.map_err(|e| {
        LightningError::InvalidResponse(format!("Failed to parse LNURL response: {}", e))
    })?;

    // Check for LNURL error response
    if body.get("status").and_then(|s| s.as_str()) == Some("ERROR") {
        let reason = body
            .get("reason")
            .and_then(|r| r.as_str())
            .unwrap_or("Unknown error");
        return Err(LightningError::ApiError(format!(
            "LNURL error: {}",
            reason
        )));
    }

    let params: LnurlPayParams = serde_json::from_value(body).map_err(|e| {
        LightningError::InvalidResponse(format!("Failed to parse LNURL-pay params: {}", e))
    })?;

    info!(
        "LNURL: resolved {} — sendable range: {}-{} msats",
        address, params.min_sendable, params.max_sendable
    );

    Ok(params)
}

/// Request a bolt11 invoice from a LNURL-pay callback.
///
/// `amount_msats` must be between the endpoint's min_sendable and max_sendable.
pub async fn request_invoice(
    client: &ClientWithMiddleware,
    params: &LnurlPayParams,
    amount_msats: i64,
) -> Result<String, LightningError> {
    if amount_msats < params.min_sendable || amount_msats > params.max_sendable {
        return Err(LightningError::ApiError(format!(
            "Amount {} msats outside sendable range ({}-{})",
            amount_msats, params.min_sendable, params.max_sendable
        )));
    }

    // Append amount to callback URL
    let separator = if params.callback.contains('?') {
        "&"
    } else {
        "?"
    };
    let url = format!("{}{}amount={}", params.callback, separator, amount_msats);

    info!("LNURL: requesting invoice for {} msats", amount_msats);

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(LightningError::RequestError)?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(LightningError::ApiError(format!(
            "LNURL invoice request failed: {} - {}",
            status, text
        )));
    }

    let body: serde_json::Value = response.json().await.map_err(|e| {
        LightningError::InvalidResponse(format!("Failed to parse LNURL invoice response: {}", e))
    })?;

    // Check for error
    if body.get("status").and_then(|s| s.as_str()) == Some("ERROR") {
        let reason = body
            .get("reason")
            .and_then(|r| r.as_str())
            .unwrap_or("Unknown error");
        return Err(LightningError::ApiError(format!(
            "LNURL invoice error: {}",
            reason
        )));
    }

    let invoice: LnurlPayInvoice = serde_json::from_value(body).map_err(|e| {
        LightningError::InvalidResponse(format!("Failed to parse LNURL invoice: {}", e))
    })?;

    info!("LNURL: obtained invoice");
    Ok(invoice.pr)
}

/// End-to-end: resolve a lightning address and get a bolt11 invoice for the
/// given amount in sats.
pub async fn get_invoice_from_lightning_address(
    client: &ClientWithMiddleware,
    address: &str,
    amount_sats: i64,
) -> Result<String, LightningError> {
    let params = resolve_lightning_address(client, address).await?;
    let amount_msats = amount_sats * 1000;
    request_invoice(client, &params, amount_msats).await
}

/// Detect if a lightning address is a CashApp address.
pub fn is_cashapp_address(address: &str) -> bool {
    let normalized = normalize_lightning_address(address)
        .unwrap_or_default()
        .to_lowercase();
    normalized.ends_with("@cash.app")
}
