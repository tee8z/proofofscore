use log::{error, info, warn};
use std::sync::Arc;

use crate::{
    lightning::{base64_to_hex, LightningProvider, LndInvoiceLookup},
    startup::AppState,
};

/// Background task that subscribes to LND invoice settlement events.
///
/// When an invoice is settled, this task immediately updates the payment
/// status in the database and grants plays — eliminating the need for the
/// client to poll. The client-side polling remains as a fallback.
pub async fn run_invoice_watcher(state: Arc<AppState>) {
    let lnd_client = match &state.lightning_provider {
        LightningProvider::Lnd(client) => client.clone(),
        _ => {
            info!("Invoice watcher: not using LND provider, skipping");
            return;
        }
    };

    info!("Invoice watcher: starting LND invoice subscription");

    loop {
        match watch_invoices(&lnd_client, &state).await {
            Ok(()) => {
                warn!("Invoice watcher: stream ended, reconnecting in 5s");
            }
            Err(e) => {
                error!("Invoice watcher: stream error: {}, reconnecting in 5s", e);
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

async fn watch_invoices(
    lnd_client: &crate::lightning::LndClient,
    state: &Arc<AppState>,
) -> Result<(), String> {
    let mut response = lnd_client
        .subscribe_invoices()
        .await
        .map_err(|e| format!("subscribe failed: {}", e))?;

    // Read the streaming response using chunk() — LND sends newline-delimited JSON
    let mut buffer = String::new();
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|e| format!("read error: {}", e))?;

        match chunk {
            Some(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                buffer.push_str(&text);

                // Process complete lines
                while let Some(newline_pos) = buffer.find('\n') {
                    let line: String = buffer.drain(..=newline_pos).collect();
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    if let Err(e) = handle_invoice_event(trimmed, state).await {
                        warn!("Invoice watcher: failed to handle event: {}", e);
                    }
                }
            }
            None => {
                // Stream closed
                return Ok(());
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct StreamedInvoice {
    result: Option<LndInvoiceLookup>,
}

async fn handle_invoice_event(json_line: &str, state: &Arc<AppState>) -> Result<(), String> {
    let invoice: LndInvoiceLookup =
        if let Ok(streamed) = serde_json::from_str::<StreamedInvoice>(json_line) {
            streamed
                .result
                .ok_or_else(|| "no result in streamed invoice".to_string())?
        } else if let Ok(inv) = serde_json::from_str::<LndInvoiceLookup>(json_line) {
            inv
        } else {
            return Err(format!("failed to parse invoice event: {}", json_line));
        };

    if invoice.state != "SETTLED" {
        return Ok(());
    }

    let r_hash_hex =
        base64_to_hex(&invoice.r_hash).map_err(|e| format!("invalid r_hash: {}", e))?;

    info!("Invoice watcher: invoice {} settled", r_hash_hex);

    // Look up the payment in our database
    let payment = match state.payment_store.get_payment_by_id(&r_hash_hex).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            // Not one of our game payments — ignore
            return Ok(());
        }
        Err(e) => {
            return Err(format!("db lookup failed: {}", e));
        }
    };

    if payment.status == "paid" {
        // Already processed (e.g. by polling)
        return Ok(());
    }

    // Update payment status to paid
    if let Err(e) = state
        .payment_store
        .update_payment_status(&r_hash_hex, "paid")
        .await
    {
        error!("Invoice watcher: failed to update payment status: {}", e);
    }

    // Grant plays
    let plays_per_payment = state.settings.competition_settings.plays_per_payment;
    if let Err(e) = state
        .payment_store
        .set_plays_remaining(&r_hash_hex, plays_per_payment)
        .await
    {
        error!("Invoice watcher: failed to set plays_remaining: {}", e);
    }

    // Publish to audit ledger
    if let Ok(Some(user)) = state.user_store.find_by_id(payment.user_id).await {
        if let Err(e) = state
            .ledger_service
            .publish_game_entry(
                &user.nostr_pubkey,
                &r_hash_hex,
                payment.amount_sats,
                "",
                &time::OffsetDateTime::now_utc().date().to_string(),
            )
            .await
        {
            warn!(
                "Invoice watcher: failed to publish game entry to ledger: {}",
                e
            );
        }
    }

    info!(
        "Invoice watcher: payment {} processed, {} plays granted",
        r_hash_hex, plays_per_payment
    );

    Ok(())
}
