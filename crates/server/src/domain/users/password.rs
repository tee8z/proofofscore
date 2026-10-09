use std::sync::{Arc, LazyLock};

use argon2::{
    password_hash::{
        self, rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
    },
    Argon2,
};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

// Argon2's default needs 19 MiB and tens of milliseconds per job. Every
// password route shares these workers, with no waiting queue.
const PASSWORD_WORKERS: usize = 2;

// Unknown usernames verify against this hash so they cost the same Argon2 pass
// as a wrong password. It is built on first use, inside a blocking job, from a
// random password that is never kept.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    hash_password(&hex::encode(rand::random::<[u8; 32]>()))
        .expect("default Argon2 parameters hash any password")
});

#[derive(Debug, Error)]
pub enum PasswordError {
    #[error("Password work is busy")]
    Busy,
    #[error("Password worker failed")]
    WorkerFailed,
    #[error("Hash failed: {0}")]
    HashError(String),
    #[error("Verify failed: {0}")]
    VerifyError(String),
}

impl IntoResponse for PasswordError {
    fn into_response(self) -> Response {
        match self {
            Self::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                [("retry-after", "1")],
                "Password check is busy; try again shortly",
            )
                .into_response(),
            Self::WorkerFailed | Self::HashError(_) | Self::VerifyError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
            }
        }
    }
}

#[derive(Clone)]
pub struct PasswordWork(Arc<Semaphore>);

impl Default for PasswordWork {
    fn default() -> Self {
        Self(Arc::new(Semaphore::new(PASSWORD_WORKERS)))
    }
}

pub struct PasswordPermit(OwnedSemaphorePermit);

impl PasswordWork {
    pub fn admit(&self) -> Result<PasswordPermit, PasswordError> {
        self.0
            .clone()
            .try_acquire_owned()
            .map(PasswordPermit)
            .map_err(|_| PasswordError::Busy)
    }
}

impl PasswordPermit {
    async fn run<T: Send + 'static>(
        self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, PasswordError> {
        tokio::task::spawn_blocking(move || {
            // Keep the lease through the actual work, even after HTTP cancellation.
            let _slot = self.0;
            work()
        })
        .await
        .map_err(|_| PasswordError::WorkerFailed)
    }

    pub async fn hash(self, password: String) -> Result<String, PasswordError> {
        self.run(move || hash_password(&password)).await?
    }

    /// Without a stored hash this still runs a real verification, against a
    /// dummy hash, and always answers `false`.
    pub async fn verify(
        self,
        password: String,
        stored_hash: Option<String>,
    ) -> Result<bool, PasswordError> {
        self.run(move || match stored_hash {
            Some(hash) => verify_password(&password, &hash),
            None => verify_password(&password, &DUMMY_HASH).map(|_| false),
        })
        .await?
    }
}

fn hash_password(password: &str) -> Result<String, PasswordError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| PasswordError::HashError(e.to_string()))?;
    Ok(hash.to_string())
}

fn verify_password(password: &str, hash: &str) -> Result<bool, PasswordError> {
    let parsed_hash =
        PasswordHash::new(hash).map_err(|e| PasswordError::VerifyError(e.to_string()))?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(password_hash::Error::Password) => Ok(false),
        Err(e) => Err(PasswordError::VerifyError(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};
    use tokio::sync::oneshot;

    #[test]
    fn unknown_user_verification_runs_a_real_argon2_pass() {
        assert!(matches!(
            verify_password("any guess", &DUMMY_HASH),
            Ok(false)
        ));
    }

    #[test]
    fn malformed_stored_hash_is_an_error_not_a_mismatch() {
        // Argon2 refuses this seven-byte salt before hashing anything.
        let short_salt = "$argon2id$v=19$m=19456,t=2,p=1$dW5rbm93bg$YWxzb191bmtub3du";
        for hash in [short_salt, "not a password hash"] {
            assert!(matches!(
                verify_password("any guess", hash),
                Err(PasswordError::VerifyError(_))
            ));
        }
    }

    #[tokio::test]
    async fn stored_hash_accepts_only_its_password_and_unknown_users_never_pass() {
        let work = PasswordWork::default();
        let hash = work
            .admit()
            .unwrap()
            .hash("correct horse".into())
            .await
            .unwrap();
        for (guess, stored, expected) in [
            ("correct horse", Some(hash.clone()), true),
            ("battery staple", Some(hash), false),
            ("correct horse", None, false),
        ] {
            let valid = work
                .admit()
                .unwrap()
                .verify(guess.into(), stored)
                .await
                .unwrap();
            assert_eq!(valid, expected, "{guess}");
        }
    }

    #[tokio::test]
    async fn saturated_work_refuses_until_jobs_end_even_after_cancellation() {
        let work = PasswordWork::default();
        let mut releases = vec![];
        let mut callers = vec![];
        for _ in 0..PASSWORD_WORKERS {
            let permit = work.admit().unwrap();
            let (started, ready) = oneshot::channel();
            let (release, wait) = mpsc::channel::<()>();
            callers.push(tokio::spawn(permit.run(move || {
                started.send(()).unwrap();
                let _ = wait.recv();
            })));
            tokio::time::timeout(Duration::from_secs(5), ready)
                .await
                .unwrap()
                .unwrap();
            releases.push(release);
        }
        assert!(matches!(work.admit(), Err(PasswordError::Busy)));
        let response = PasswordError::Busy.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()["retry-after"], "1");

        // A disconnected caller keeps its slot until its blocking job ends.
        let cancelled = callers.remove(0);
        cancelled.abort();
        assert!(cancelled.await.unwrap_err().is_cancelled());
        assert!(matches!(work.admit(), Err(PasswordError::Busy)));
        releases.remove(0).send(()).unwrap();
        let returned = tokio::time::timeout(Duration::from_secs(5), work.0.clone().acquire_owned())
            .await
            .unwrap()
            .unwrap();
        drop(returned);
        assert!(work.admit().is_ok());

        // A caller that waits has its slot back once it has the result.
        releases.remove(0).send(()).unwrap();
        callers.remove(0).await.unwrap().unwrap();
        assert_eq!(work.0.available_permits(), PASSWORD_WORKERS);
    }

    #[tokio::test]
    async fn failed_worker_returns_its_slot() {
        let work = PasswordWork::default();
        assert!(matches!(
            work.admit()
                .unwrap()
                .run(|| panic!("test worker failure"))
                .await,
            Err(PasswordError::WorkerFailed)
        ));
        assert_eq!(work.0.available_permits(), PASSWORD_WORKERS);
    }
}
