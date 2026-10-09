use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use game_engine::{config::GameConfig, engine::replay_iter, state::input_frames};
use log::error;
use sha2::{Digest, Sha256};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::metrics::{
    metrics, REPLAY_BUSY, REPLAY_HASH_MISMATCH, REPLAY_INVALID_ENCODING, REPLAY_INVALID_INPUT,
    REPLAY_TIMED_OUT, REPLAY_WORKER_FAILED,
};

// About 1.5 MB encoded, below the HTTP JSON body's 2 MiB limit even after
// base64 encoding. The bound accommodates over thirteen hours at 60 fps.
const MAX_REPLAY_FRAMES: u32 = 3_000_000;
const REPLAY_TIME_LIMIT: Duration = Duration::from_secs(10);
const REPLAY_WORKERS: usize = 2;

#[derive(Debug, PartialEq, Eq)]
pub enum ReplayError {
    InvalidInput,
    InvalidEncoding,
    HashMismatch,
    Busy,
    TimedOut,
    WorkerFailed,
}

impl ReplayError {
    /// Counts this rejection in the exported metrics.
    fn counted(self) -> Self {
        let reason = match self {
            Self::InvalidInput => REPLAY_INVALID_INPUT,
            Self::InvalidEncoding => REPLAY_INVALID_ENCODING,
            Self::HashMismatch => REPLAY_HASH_MISMATCH,
            Self::Busy => REPLAY_BUSY,
            Self::TimedOut => REPLAY_TIMED_OUT,
            Self::WorkerFailed => REPLAY_WORKER_FAILED,
        };
        metrics().replay_rejected(reason);
        self
    }
}

impl IntoResponse for ReplayError {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidInput => (
                StatusCode::BAD_REQUEST,
                "Replay frame count or input length is invalid",
            )
                .into_response(),
            Self::InvalidEncoding => {
                (StatusCode::BAD_REQUEST, "Invalid input_log encoding").into_response()
            }
            Self::HashMismatch => (StatusCode::BAD_REQUEST, "Input hash mismatch").into_response(),
            Self::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                [("retry-after", "1")],
                "Replay verification is busy; try again shortly",
            )
                .into_response(),
            Self::TimedOut => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Replay exceeds the verification work limit",
            )
                .into_response(),
            Self::WorkerFailed => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Replay verification failed",
            )
                .into_response(),
        }
    }
}

#[derive(Clone)]
pub struct ReplayVerifier(Arc<Semaphore>);

impl Default for ReplayVerifier {
    fn default() -> Self {
        Self(Arc::new(Semaphore::new(REPLAY_WORKERS)))
    }
}

pub struct ReplayPermit(OwnedSemaphorePermit);

impl ReplayVerifier {
    pub fn admit(&self) -> Result<ReplayPermit, ReplayError> {
        self.0
            .clone()
            .try_acquire_owned()
            .map(ReplayPermit)
            .map_err(|_| ReplayError::Busy.counted())
    }
}

/// Rejects a base64 input log whose length cannot hold `frame_count` frames,
/// without decoding it, so the submission never takes a verification slot.
/// The standard engine requires canonical padding, which makes the encoded
/// length of a valid log exact.
pub fn check_encoded_length(input_log: &str, frame_count: u32) -> Result<(), ReplayError> {
    let expected = input_log_len(frame_count).and_then(|len| base64::encoded_len(len, true));
    if expected != Some(input_log.len()) {
        return Err(ReplayError::InvalidInput.counted());
    }
    Ok(())
}

/// The bytes that hold `frame_count` frames at two frames per byte, or `None`
/// above the frame cap.
fn input_log_len(frame_count: u32) -> Option<usize> {
    (frame_count <= MAX_REPLAY_FRAMES).then_some((frame_count as usize).div_ceil(2))
}

impl ReplayPermit {
    async fn run<T: Send + 'static>(
        self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, ReplayError> {
        tokio::task::spawn_blocking(move || {
            // Keep the lease through the actual work, even after HTTP cancellation.
            let _slot = self.0;
            work()
        })
        .await
        .map_err(|_| ReplayError::WorkerFailed)
    }

    /// Decodes, hashes and replays a submitted base64 input log on a blocking
    /// worker. Returns the decoded log with the result, so the caller can
    /// store it without keeping a second copy.
    pub async fn verify(
        self,
        seed: u64,
        config: GameConfig,
        input_log: String,
        input_hash: String,
        frame_count: u32,
        claimed_score: u32,
    ) -> Result<(Vec<u8>, ReplayResult), ReplayError> {
        self.run(move || -> Result<_, ReplayError> {
            let _timer = metrics().replay_verification_seconds.start_timer();
            let input_log = decode_input_log(input_log, &input_hash)?;
            let result = verify_replay(seed, &config, &input_log, frame_count, claimed_score)?;
            Ok((input_log, result))
        })
        .await
        .and_then(|verified| verified)
        .map_err(ReplayError::counted)
    }
}

/// Decodes a base64 input log and checks its SHA-256 hex digest. Takes the
/// encoded log by value so it is freed before the replay starts.
fn decode_input_log(encoded: String, input_hash: &str) -> Result<Vec<u8>, ReplayError> {
    let input_log = BASE64.decode(encoded).map_err(|e| {
        error!("Invalid base64 input_log: {}", e);
        ReplayError::InvalidEncoding
    })?;
    let computed_hash = hex::encode(Sha256::digest(&input_log));
    if computed_hash != input_hash {
        error!(
            "Input hash mismatch: computed={}, submitted={}",
            computed_hash, input_hash
        );
        return Err(ReplayError::HashMismatch);
    }
    Ok(input_log)
}

#[derive(Debug)]
pub struct ReplayResult {
    pub score: u32,
    pub level: u32,
    pub frames: u32,
    pub game_over: bool,
    pub verified: bool,
}

pub fn verify_replay(
    seed: u64,
    config: &GameConfig,
    input_log: &[u8],
    frame_count: u32,
    claimed_score: u32,
) -> Result<ReplayResult, ReplayError> {
    verify_with_budget(
        seed,
        config,
        input_log,
        frame_count,
        claimed_score,
        REPLAY_TIME_LIMIT,
    )
}

fn verify_with_budget(
    seed: u64,
    config: &GameConfig,
    input_log: &[u8],
    frame_count: u32,
    claimed_score: u32,
    budget: Duration,
) -> Result<ReplayResult, ReplayError> {
    if input_log_len(frame_count) != Some(input_log.len()) {
        return Err(ReplayError::InvalidInput);
    }
    let started = Instant::now();
    let mut timed_out = false;
    let inputs = input_frames(input_log, frame_count)
        .enumerate()
        .take_while(|(index, _)| {
            if index % 1024 == 0 && started.elapsed() >= budget {
                timed_out = true;
                false
            } else {
                true
            }
        })
        .map(|(_, input)| input);
    let (score, level, frames, game_over) = replay_iter(seed, config.clone(), inputs);
    if timed_out {
        return Err(ReplayError::TimedOut);
    }
    Ok(ReplayResult {
        score,
        level,
        frames,
        game_over,
        verified: score == claimed_score,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use game_engine::{
        engine::replay,
        state::{encode_inputs, FrameInput},
    };

    /// The base64 input log and SHA-256 hex digest a client submits.
    fn encoded(input_log: &[u8]) -> (String, String) {
        (
            BASE64.encode(input_log),
            hex::encode(Sha256::digest(input_log)),
        )
    }

    #[test]
    fn rejects_impossible_lengths_before_replay() {
        let config = GameConfig::default_config();
        for (bytes, frames) in [
            (&[0][..], u32::MAX),
            (&[0][..], 3),
            (&[0, 0][..], 2),
            (&[][..], 1),
        ] {
            assert!(matches!(
                verify_replay(42, &config, bytes, frames, 0),
                Err(ReplayError::InvalidInput)
            ));
        }
        let over_limit = MAX_REPLAY_FRAMES + 1;
        assert!(matches!(
            verify_replay(
                42,
                &config,
                &vec![0; (over_limit as usize).div_ceil(2)],
                over_limit,
                0
            ),
            Err(ReplayError::InvalidInput)
        ));
    }

    #[test]
    fn encoded_length_check_rejects_logs_that_cannot_hold_the_frames() {
        for frames in [0, 1, 2, 3, 4, 5, 1_001, MAX_REPLAY_FRAMES] {
            let input_log = BASE64.encode(vec![0u8; (frames as usize).div_ceil(2)]);
            assert_eq!(check_encoded_length(&input_log, frames), Ok(()), "{frames}");
            let longer = format!("{input_log}A");
            assert_eq!(
                check_encoded_length(&longer, frames),
                Err(ReplayError::InvalidInput),
                "{frames}"
            );
            if let Some(shorter) = input_log.get(1..) {
                assert_eq!(
                    check_encoded_length(shorter, frames),
                    Err(ReplayError::InvalidInput),
                    "{frames}"
                );
            }
        }
        let over_limit = MAX_REPLAY_FRAMES + 1;
        let input_log = BASE64.encode(vec![0u8; (over_limit as usize).div_ceil(2)]);
        assert_eq!(
            check_encoded_length(&input_log, over_limit),
            Err(ReplayError::InvalidInput)
        );
    }

    #[tokio::test]
    async fn undecodable_or_mismatched_logs_keep_their_400_responses() {
        let verifier = ReplayVerifier::default();
        let (input_log, input_hash) = encoded(&[0, 0]);
        for (input_log, input_hash, error, message) in [
            (
                "AA*=".to_string(),
                input_hash,
                ReplayError::InvalidEncoding,
                "Invalid input_log encoding",
            ),
            (
                input_log,
                "0".repeat(64),
                ReplayError::HashMismatch,
                "Input hash mismatch",
            ),
        ] {
            let rejected = verifier
                .admit()
                .unwrap()
                .verify(
                    42,
                    GameConfig::default_config(),
                    input_log,
                    input_hash,
                    3,
                    0,
                )
                .await
                .unwrap_err();
            assert_eq!(rejected, error);
            let response = rejected.into_response();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert_eq!(body, message);
        }
    }

    #[test]
    fn accepts_odd_frame_replays_and_enforces_work_budget() {
        let config = GameConfig::default_config();
        let result = verify_replay(42, &config, &[0, 0], 3, 0).unwrap();
        assert_eq!(result.frames, 3);
        assert!(result.verified);
        // Even a matching partial score must not turn a timeout into a valid replay.
        assert!(matches!(
            verify_with_budget(42, &config, &[0, 0], 3, 0, Duration::ZERO),
            Err(ReplayError::TimedOut)
        ));
    }

    #[tokio::test]
    async fn admission_survives_cancellation_and_recovers_from_worker_failure() {
        let verifier = ReplayVerifier::default();
        let first = verifier.admit().unwrap();
        let second = verifier.admit().unwrap();
        assert!(matches!(verifier.admit(), Err(ReplayError::Busy)));
        let response = ReplayError::Busy.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()["retry-after"], "1");
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let caller = tokio::spawn(first.run(move || {
            started.send(()).unwrap();
            let _ = wait.recv();
        }));
        tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        assert!(matches!(verifier.admit(), Err(ReplayError::Busy)));
        assert!(matches!(
            second.run(|| panic!("test worker failure")).await,
            Err(ReplayError::WorkerFailed)
        ));
        release.send(()).unwrap();
        // Both slots come back: the failed worker's and the cancelled caller's.
        let slots = tokio::time::timeout(
            Duration::from_secs(5),
            verifier.0.clone().acquire_many_owned(REPLAY_WORKERS as u32),
        )
        .await
        .unwrap()
        .unwrap();
        drop(slots);
        let (input_log, input_hash) = encoded(&[0, 0]);
        let (decoded, result) = verifier
            .admit()
            .unwrap()
            .verify(
                42,
                GameConfig::default_config(),
                input_log,
                input_hash,
                3,
                0,
            )
            .await
            .unwrap();
        assert_eq!(decoded, [0, 0]);
        assert!(result.verified);
    }

    #[test]
    fn test_verify_replay_matches() {
        let config = GameConfig::default_config();
        let seed = 42u64;

        // Play a short game
        let inputs: Vec<FrameInput> = (0..100)
            .map(|i| FrameInput {
                thrust: i % 5 == 0,
                rotate_left: i % 7 == 0,
                rotate_right: i % 11 == 0,
                shoot: i % 13 == 0,
            })
            .collect();

        // Get the real score by replaying
        let (real_score, _, _, _) = replay(seed, config.clone(), &inputs);

        // Encode inputs
        let encoded = encode_inputs(&inputs);

        // Verify
        let result = verify_replay(seed, &config, &encoded, 100, real_score).unwrap();
        assert!(result.verified);
        assert_eq!(result.score, real_score);
    }

    #[test]
    fn test_verify_replay_rejects_fake_score() {
        let config = GameConfig::default_config();
        let seed = 42u64;

        let inputs: Vec<FrameInput> = (0..50)
            .map(|_| FrameInput {
                thrust: false,
                rotate_left: false,
                rotate_right: false,
                shoot: false,
            })
            .collect();

        let encoded = encode_inputs(&inputs);

        // Claim a fake score
        let result = verify_replay(seed, &config, &encoded, 50, 999999).unwrap();
        assert!(!result.verified);
    }
}
