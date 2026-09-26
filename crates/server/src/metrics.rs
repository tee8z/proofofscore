//! Prometheus metrics, served on a separate, optional listener.
//!
//! Counters are incremented where things happen. Gauges derived from game.db
//! are computed when scraped and cached for [`DB_GAUGE_TTL`], so frequent
//! scrapes cost at most one set of cheap queries per interval.

use std::{
    net::SocketAddr,
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use anyhow::anyhow;
use axum::{
    extract::State,
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use log::{error, info, warn};
use prometheus::{
    Encoder, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts, Registry, TextEncoder,
};
use sqlx::{Pool, Row, Sqlite};
use tokio::{net::TcpListener, sync::Mutex};

use crate::config::MetricsSettings;

/// How long database-derived gauges are reused between scrapes.
pub const DB_GAUGE_TTL: Duration = Duration::from_secs(15);

const PREFIX: &str = "proofofscore";

/// Outcome labels for submitted scores.
pub const SCORE_ACCEPTED: &str = "accepted";
pub const SCORE_REJECTED: &str = "rejected";
pub const SCORE_FLAGGED: &str = "flagged";

/// Outcome labels for invoice creation.
pub const RESULT_OK: &str = "ok";
pub const RESULT_ERROR: &str = "error";

/// Outcome labels for prize payouts.
pub const PAYOUT_SUCCEEDED: &str = "succeeded";
pub const PAYOUT_FAILED: &str = "failed";

/// Every metric the server exports, registered in its own registry.
pub struct Metrics {
    registry: Registry,
    pub game_sessions_started: IntCounter,
    scores_submitted: IntCounterVec,
    invoices_created: IntCounterVec,
    pub invoices_paid: IntCounter,
    payouts: IntCounterVec,
    pub lnd_invoice_stream_errors: IntCounter,
    users: IntGauge,
    sessions_today: IntGauge,
    scores_today: IntGauge,
    payments: IntGaugeVec,
    prize_payouts: IntGaugeVec,
}

fn opts(name: &str, help: &str) -> Opts {
    Opts::new(name, help).namespace(PREFIX)
}

impl Metrics {
    fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();

        let game_sessions_started = IntCounter::with_opts(opts(
            "game_sessions_started_total",
            "Game sessions started.",
        ))?;
        let scores_submitted = IntCounterVec::new(
            opts(
                "scores_submitted_total",
                "Score submissions by outcome: accepted, rejected or flagged (accepted with bot-detection flags).",
            ),
            &["result"],
        )?;
        let invoices_created = IntCounterVec::new(
            opts(
                "invoices_created_total",
                "Entry-fee invoice creation attempts by result.",
            ),
            &["result"],
        )?;
        let invoices_paid = IntCounter::with_opts(opts(
            "invoices_paid_total",
            "Entry-fee invoices seen settled and granted plays.",
        ))?;
        let payouts = IntCounterVec::new(
            opts("payouts_total", "Prize payout attempts by result."),
            &["result"],
        )?;
        let lnd_invoice_stream_errors = IntCounter::with_opts(opts(
            "lnd_invoice_stream_errors_total",
            "Times the LND invoice subscription ended or failed and was restarted.",
        ))?;
        let build_info = IntGaugeVec::new(
            opts("build_info", "Build information; always 1."),
            &["version"],
        )?;
        let users = IntGauge::with_opts(opts("users", "Registered users."))?;
        let sessions_today = IntGauge::with_opts(opts(
            "sessions_today",
            "Game sessions started since midnight UTC.",
        ))?;
        let scores_today =
            IntGauge::with_opts(opts("scores_today", "Scores recorded since midnight UTC."))?;
        let payments = IntGaugeVec::new(
            opts("payments", "Entry-fee payments by status."),
            &["status"],
        )?;
        let prize_payouts = IntGaugeVec::new(
            opts("prize_payouts", "Prize payouts by status."),
            &["status"],
        )?;

        registry.register(Box::new(game_sessions_started.clone()))?;
        registry.register(Box::new(scores_submitted.clone()))?;
        registry.register(Box::new(invoices_created.clone()))?;
        registry.register(Box::new(invoices_paid.clone()))?;
        registry.register(Box::new(payouts.clone()))?;
        registry.register(Box::new(lnd_invoice_stream_errors.clone()))?;
        registry.register(Box::new(build_info.clone()))?;
        registry.register(Box::new(users.clone()))?;
        registry.register(Box::new(sessions_today.clone()))?;
        registry.register(Box::new(scores_today.clone()))?;
        registry.register(Box::new(payments.clone()))?;
        registry.register(Box::new(prize_payouts.clone()))?;

        // Initialise every known label so each series is exported from zero.
        for result in [SCORE_ACCEPTED, SCORE_REJECTED, SCORE_FLAGGED] {
            scores_submitted.with_label_values(&[result]);
        }
        for result in [RESULT_OK, RESULT_ERROR] {
            invoices_created.with_label_values(&[result]);
        }
        for result in [PAYOUT_SUCCEEDED, PAYOUT_FAILED] {
            payouts.with_label_values(&[result]);
        }
        build_info
            .with_label_values(&[env!("CARGO_PKG_VERSION")])
            .set(1);

        Ok(Self {
            registry,
            game_sessions_started,
            scores_submitted,
            invoices_created,
            invoices_paid,
            payouts,
            lnd_invoice_stream_errors,
            users,
            sessions_today,
            scores_today,
            payments,
            prize_payouts,
        })
    }

    /// Count a score submission with one of the `SCORE_*` outcomes.
    pub fn score_submitted(&self, result: &str) {
        self.scores_submitted.with_label_values(&[result]).inc();
    }

    /// Count an entry-fee invoice creation with `RESULT_OK` or `RESULT_ERROR`.
    pub fn invoice_created(&self, result: &str) {
        self.invoices_created.with_label_values(&[result]).inc();
    }

    /// Count a prize payout attempt with one of the `PAYOUT_*` outcomes.
    pub fn payout(&self, result: &str) {
        self.payouts.with_label_values(&[result]).inc();
    }

    /// Render every registered family in the Prometheus text format.
    pub fn render(&self) -> Result<String, prometheus::Error> {
        let mut buffer = Vec::new();
        TextEncoder::new().encode(&self.registry.gather(), &mut buffer)?;
        String::from_utf8(buffer).map_err(|e| prometheus::Error::Msg(e.to_string()))
    }

    /// Recompute the database-derived gauges.
    async fn refresh_db_gauges(&self, pool: &Pool<Sqlite>) -> Result<(), sqlx::Error> {
        let today = time::OffsetDateTime::now_utc().date().to_string();

        let users: i64 = sqlx::query("SELECT COUNT(*) AS c FROM users")
            .fetch_one(pool)
            .await?
            .get("c");
        let sessions_today: i64 =
            sqlx::query("SELECT COUNT(*) AS c FROM game_sessions WHERE start_time >= ?")
                .bind(&today)
                .fetch_one(pool)
                .await?
                .get("c");
        let scores_today: i64 =
            sqlx::query("SELECT COUNT(*) AS c FROM scores WHERE created_at >= ?")
                .bind(&today)
                .fetch_one(pool)
                .await?
                .get("c");
        let payments = status_counts(pool, "game_payments").await?;
        let prize_payouts = status_counts(pool, "prize_payouts").await?;

        self.users.set(users);
        self.sessions_today.set(sessions_today);
        self.scores_today.set(scores_today);
        set_status_gauges(&self.payments, &payments);
        set_status_gauges(&self.prize_payouts, &prize_payouts);
        Ok(())
    }
}

async fn status_counts(
    pool: &Pool<Sqlite>,
    table: &str,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    // `table` is always one of this module's constant table names.
    let rows = sqlx::query(&format!(
        "SELECT status, COUNT(*) AS c FROM {table} GROUP BY status"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.get::<String, _>("status"), row.get::<i64, _>("c")))
        .collect())
}

fn set_status_gauges(gauges: &IntGaugeVec, counts: &[(String, i64)]) {
    // Drop statuses that no longer have rows before setting the current ones.
    gauges.reset();
    for (status, count) in counts {
        gauges.with_label_values(&[status.as_str()]).set(*count);
    }
}

static METRICS: LazyLock<Metrics> =
    LazyLock::new(|| Metrics::new().expect("metric definitions are valid"));

/// The process-wide metrics.
pub fn metrics() -> &'static Metrics {
    &METRICS
}

/// Shared state of the metrics listener.
#[derive(Clone)]
pub struct MetricsState {
    pool: Pool<Sqlite>,
    last_refresh: Arc<Mutex<Option<Instant>>>,
}

impl MetricsState {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self {
            pool,
            last_refresh: Arc::new(Mutex::new(None)),
        }
    }

    async fn refresh_if_stale(&self) {
        let mut last = self.last_refresh.lock().await;
        if last.is_some_and(|at| at.elapsed() < DB_GAUGE_TTL) {
            return;
        }
        match metrics().refresh_db_gauges(&self.pool).await {
            Ok(()) => *last = Some(Instant::now()),
            Err(e) => warn!("Failed to refresh database metrics: {}", e),
        }
    }
}

/// The metrics router: `GET /metrics` and nothing else.
pub fn metrics_router(state: MetricsState) -> Router {
    Router::new()
        .route("/metrics", get(metrics_handler))
        .fallback(|| async { StatusCode::NOT_FOUND })
        .with_state(state)
}

async fn metrics_handler(State(state): State<MetricsState>) -> Response {
    state.refresh_if_stale().await;
    match metrics().render() {
        Ok(body) => (
            StatusCode::OK,
            [(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
            body,
        )
            .into_response(),
        Err(e) => {
            error!("Failed to render metrics: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Start the metrics listener when `[metrics] bind` is set.
///
/// Returns the bound address, or `None` when metrics are disabled.
pub async fn start_metrics_listener(
    settings: &MetricsSettings,
    pool: Pool<Sqlite>,
) -> Result<Option<SocketAddr>, anyhow::Error> {
    let Some(bind) = settings.bind.as_deref().filter(|b| !b.trim().is_empty()) else {
        info!("Metrics listener disabled");
        return Ok(None);
    };
    let listener = TcpListener::bind(bind)
        .await
        .map_err(|e| anyhow!("Failed to bind metrics listener on {}: {}", bind, e))?;
    let addr = listener.local_addr()?;
    let app = metrics_router(MetricsState::new(pool));
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("Metrics listener stopped: {}", e);
        }
    });
    info!("Metrics listening @: http://{}/metrics", addr);
    Ok(Some(addr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest_middleware::reqwest;

    async fn test_pool() -> Pool<Sqlite> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        sqlx::migrate::Migrator::new(migrations)
            .await
            .expect("migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        pool
    }

    const FAMILIES: &[&str] = &[
        "proofofscore_game_sessions_started_total",
        "proofofscore_scores_submitted_total{result=\"accepted\"}",
        "proofofscore_scores_submitted_total{result=\"rejected\"}",
        "proofofscore_scores_submitted_total{result=\"flagged\"}",
        "proofofscore_invoices_created_total{result=\"ok\"}",
        "proofofscore_invoices_created_total{result=\"error\"}",
        "proofofscore_invoices_paid_total",
        "proofofscore_payouts_total{result=\"succeeded\"}",
        "proofofscore_payouts_total{result=\"failed\"}",
        "proofofscore_lnd_invoice_stream_errors_total",
        "proofofscore_users",
        "proofofscore_sessions_today",
        "proofofscore_scores_today",
    ];

    #[test]
    fn renders_expected_families() {
        let body = metrics().render().expect("render");
        for family in FAMILIES {
            assert!(body.contains(family), "missing {family} in:\n{body}");
        }
        assert!(body.contains(&format!(
            "proofofscore_build_info{{version=\"{}\"}} 1",
            env!("CARGO_PKG_VERSION")
        )));
    }

    #[tokio::test]
    async fn status_gauges_follow_the_database() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO users (nostr_pubkey, username) VALUES ('a', 'a'), ('b', 'b')")
            .execute(&pool)
            .await
            .expect("insert users");
        sqlx::query(
            "INSERT INTO game_payments (user_id, payment_id, invoice, amount_sats, status) \
             VALUES (1, 'p1', 'i1', 100, 'paid'), (1, 'p2', 'i2', 100, 'paid'), \
                    (2, 'p3', 'i3', 100, 'pending')",
        )
        .execute(&pool)
        .await
        .expect("insert payments");
        sqlx::query(
            "INSERT INTO prize_payouts (user_id, date, score, amount_sats, status) \
             VALUES (1, '2026-01-01', 10, 500, 'failed')",
        )
        .execute(&pool)
        .await
        .expect("insert payouts");

        // A private instance, so concurrent tests cannot change these gauges.
        let metrics = Metrics::new().expect("metrics");
        metrics.refresh_db_gauges(&pool).await.expect("refresh");
        let body = metrics.render().expect("render");
        assert!(body.contains("proofofscore_users 2"), "{body}");
        assert!(
            body.contains("proofofscore_payments{status=\"paid\"} 2"),
            "{body}"
        );
        assert!(
            body.contains("proofofscore_payments{status=\"pending\"} 1"),
            "{body}"
        );
        assert!(
            body.contains("proofofscore_prize_payouts{status=\"failed\"} 1"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn unset_bind_disables_the_listener() {
        let pool = test_pool().await;
        let disabled = MetricsSettings::default();
        assert!(disabled.bind.is_none());
        assert_eq!(
            start_metrics_listener(&disabled, pool.clone())
                .await
                .unwrap(),
            None
        );
        let blank = MetricsSettings {
            bind: Some(String::new()),
        };
        assert_eq!(start_metrics_listener(&blank, pool).await.unwrap(), None);
    }

    #[tokio::test]
    async fn listener_serves_metrics_only() {
        let pool = test_pool().await;
        let settings = MetricsSettings {
            bind: Some("127.0.0.1:0".to_string()),
        };
        let addr = start_metrics_listener(&settings, pool)
            .await
            .unwrap()
            .expect("listener enabled");

        let response = reqwest::get(format!("http://{addr}/metrics"))
            .await
            .expect("scrape");
        assert_eq!(response.status(), 200);
        let content_type = response.headers()[CONTENT_TYPE.as_str()]
            .to_str()
            .unwrap()
            .to_string();
        assert!(content_type.starts_with("text/plain"));
        let body = response.text().await.unwrap();
        assert!(body.contains("proofofscore_users"));

        let other = reqwest::get(format!("http://{addr}/"))
            .await
            .expect("request");
        assert_eq!(other.status(), 404);
    }
}
