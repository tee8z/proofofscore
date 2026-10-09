use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use game_engine::{config::GameConfig, engine::replay_iter, state::input_frames};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

// About 1.5 MB encoded, below the HTTP JSON body's 2 MiB limit even after
// base64 encoding. The bound accommodates over thirteen hours at 60 fps.
const MAX_REPLAY_FRAMES: u32 = 3_000_000;
const REPLAY_TIME_LIMIT: Duration = Duration::from_secs(10);
const REPLAY_WORKERS: usize = 2;

#[derive(Debug, PartialEq, Eq)]
pub enum ReplayError {
    InvalidInput,
    Busy,
    TimedOut,
    WorkerFailed,
}

impl IntoResponse for ReplayError {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidInput => (
                StatusCode::BAD_REQUEST,
                "Replay frame count or input length is invalid",
            )
                .into_response(),
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
            .map_err(|_| ReplayError::Busy)
    }
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

    pub async fn verify(
        self,
        seed: u64,
        config: GameConfig,
        input_log: Vec<u8>,
        frame_count: u32,
        claimed_score: u32,
    ) -> Result<ReplayResult, ReplayError> {
        self.run(move || verify_replay(seed, &config, &input_log, frame_count, claimed_score))
            .await?
    }
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
    if frame_count > MAX_REPLAY_FRAMES || input_log.len() != (frame_count as usize).div_ceil(2) {
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
    use game_engine::{
        engine::replay,
        state::{encode_inputs, FrameInput},
    };

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
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while verifier.0.available_permits() != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            second.run(|| panic!("test worker failure")).await,
            Err(ReplayError::WorkerFailed)
        ));
        assert_eq!(verifier.0.available_permits(), REPLAY_WORKERS);
        assert!(
            verifier
                .admit()
                .unwrap()
                .verify(42, GameConfig::default_config(), vec![0, 0], 3, 0)
                .await
                .unwrap()
                .verified
        );
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
