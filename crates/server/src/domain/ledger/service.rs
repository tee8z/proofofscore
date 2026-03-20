use log::info;
use nostr_sdk::{EventBuilder, Keys, Kind, PublicKey, Tag, TagKind};
use std::str::FromStr;

use crate::domain::Error;

use super::store::LedgerStore;

#[derive(Clone)]
pub struct LedgerService {
    keys: Keys,
    store: LedgerStore,
}

impl LedgerService {
    pub fn new(keys: Keys, store: LedgerStore) -> Self {
        Self { keys, store }
    }

    pub fn server_pubkey(&self) -> String {
        self.keys.public_key().to_string()
    }

    pub fn store(&self) -> &LedgerStore {
        &self.store
    }

    pub async fn publish_game_entry(
        &self,
        user_pubkey: &str,
        payment_id: &str,
        amount_sats: i64,
        session_id: &str,
        date: &str,
    ) -> Result<String, Error> {
        let pubkey = PublicKey::from_str(user_pubkey)
            .map_err(|e| Error::InvalidInput(format!("Invalid pubkey: {}", e)))?;

        let tags = vec![
            Tag::custom(TagKind::custom("t"), vec!["game_entry"]),
            Tag::public_key(pubkey),
            Tag::custom(TagKind::custom("payment_id"), vec![payment_id]),
            Tag::custom(TagKind::custom("amount"), vec![&amount_sats.to_string()]),
            Tag::custom(TagKind::custom("session_id"), vec![session_id]),
            Tag::custom(TagKind::custom("date"), vec![date]),
        ];

        let event = EventBuilder::new(Kind::Custom(10100), "")
            .tags(tags)
            .sign_with_keys(&self.keys)
            .map_err(|e| Error::InvalidInput(format!("Failed to sign event: {}", e)))?;

        let event_id = event.id.to_hex();
        let event_json = serde_json::to_string(&event)
            .map_err(|e| Error::InvalidInput(format!("Failed to serialize event: {}", e)))?;

        self.store
            .save_event(&event_id, "game_entry", &event_json, None, Some(date))
            .await?;

        info!("Published game_entry ledger event: {}", event_id);
        Ok(event_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn publish_score_verified(
        &self,
        user_pubkey: &str,
        session_id: &str,
        seed: &str,
        score: i64,
        level: i64,
        frames: u32,
        input_hash: &str,
        date: &str,
    ) -> Result<String, Error> {
        let pubkey = PublicKey::from_str(user_pubkey)
            .map_err(|e| Error::InvalidInput(format!("Invalid pubkey: {}", e)))?;

        let tags = vec![
            Tag::custom(TagKind::custom("t"), vec!["score_verified"]),
            Tag::public_key(pubkey),
            Tag::custom(TagKind::custom("session_id"), vec![session_id]),
            Tag::custom(TagKind::custom("seed"), vec![seed]),
            Tag::custom(TagKind::custom("score"), vec![&score.to_string()]),
            Tag::custom(TagKind::custom("level"), vec![&level.to_string()]),
            Tag::custom(TagKind::custom("frames"), vec![&frames.to_string()]),
            Tag::custom(TagKind::custom("input_hash"), vec![input_hash]),
            Tag::custom(TagKind::custom("date"), vec![date]),
        ];

        let event = EventBuilder::new(Kind::Custom(10100), "")
            .tags(tags)
            .sign_with_keys(&self.keys)
            .map_err(|e| Error::InvalidInput(format!("Failed to sign event: {}", e)))?;

        let event_id = event.id.to_hex();
        let event_json = serde_json::to_string(&event)
            .map_err(|e| Error::InvalidInput(format!("Failed to serialize event: {}", e)))?;

        self.store
            .save_event(&event_id, "score_verified", &event_json, None, Some(date))
            .await?;

        info!("Published score_verified ledger event: {}", event_id);
        Ok(event_id)
    }

    pub async fn publish_competition_result(
        &self,
        date: &str,
        winner_pubkey: &str,
        winning_score: i64,
        total_games: i64,
        pool_sats: i64,
        prize_sats: i64,
    ) -> Result<String, Error> {
        let tags = vec![
            Tag::custom(TagKind::custom("t"), vec!["competition_result"]),
            Tag::custom(TagKind::custom("date"), vec![date]),
            Tag::custom(TagKind::custom("winner"), vec![winner_pubkey]),
            Tag::custom(
                TagKind::custom("winning_score"),
                vec![&winning_score.to_string()],
            ),
            Tag::custom(
                TagKind::custom("total_games"),
                vec![&total_games.to_string()],
            ),
            Tag::custom(TagKind::custom("pool_sats"), vec![&pool_sats.to_string()]),
            Tag::custom(TagKind::custom("prize_sats"), vec![&prize_sats.to_string()]),
        ];

        let event = EventBuilder::new(Kind::Custom(10100), "")
            .tags(tags)
            .sign_with_keys(&self.keys)
            .map_err(|e| Error::InvalidInput(format!("Failed to sign event: {}", e)))?;

        let event_id = event.id.to_hex();
        let event_json = serde_json::to_string(&event)
            .map_err(|e| Error::InvalidInput(format!("Failed to serialize event: {}", e)))?;

        self.store
            .save_event(
                &event_id,
                "competition_result",
                &event_json,
                None,
                Some(date),
            )
            .await?;

        info!("Published competition_result ledger event: {}", event_id);
        Ok(event_id)
    }

    pub async fn publish_prize_payout(
        &self,
        winner_pubkey: &str,
        date: &str,
        amount_sats: i64,
        payment_id: &str,
    ) -> Result<String, Error> {
        let pubkey = PublicKey::from_str(winner_pubkey)
            .map_err(|e| Error::InvalidInput(format!("Invalid pubkey: {}", e)))?;

        let tags = vec![
            Tag::custom(TagKind::custom("t"), vec!["prize_payout"]),
            Tag::public_key(pubkey),
            Tag::custom(TagKind::custom("date"), vec![date]),
            Tag::custom(TagKind::custom("amount"), vec![&amount_sats.to_string()]),
            Tag::custom(TagKind::custom("payment_id"), vec![payment_id]),
        ];

        let event = EventBuilder::new(Kind::Custom(10100), "")
            .tags(tags)
            .sign_with_keys(&self.keys)
            .map_err(|e| Error::InvalidInput(format!("Failed to sign event: {}", e)))?;

        let event_id = event.id.to_hex();
        let event_json = serde_json::to_string(&event)
            .map_err(|e| Error::InvalidInput(format!("Failed to serialize event: {}", e)))?;

        self.store
            .save_event(&event_id, "prize_payout", &event_json, None, Some(date))
            .await?;

        info!("Published prize_payout ledger event: {}", event_id);
        Ok(event_id)
    }
}
