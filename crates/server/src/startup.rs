use anyhow::anyhow;
use axum::{
    body::Body,
    extract::{connect_info::IntoMakeServiceWithConnectInfo, ConnectInfo, Request},
    http::Extensions,
    middleware::{self, AddExtension, Next},
    response::IntoResponse,
    routing::{get, post},
    serve::Serve,
    Router,
};
use hyper::{
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    Method,
};
use log::{error, info, warn};
use reqwest_middleware::{
    reqwest::{self, Client, Response},
    ClientBuilder, ClientWithMiddleware, Middleware,
};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use std::sync::Arc;
use std::{net::SocketAddr, str::FromStr};
use tokio::{net::TcpListener, select};
use tokio::{
    signal::unix::{signal, SignalKind},
    spawn,
};
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
};

use nostr_sdk::Keys;

use crate::{
    check_payment_status, check_prize_eligibility, claim_prize, config::Settings,
    file_utils::create_folder, game_handler, get_competition_info, get_game_config,
    get_ledger_events, get_ledger_summary, get_replay_by_score, get_server_pubkey,
    get_top_replays, get_top_scores, get_user_profile, get_user_scores, health_check,
    home_handler, index_handler, leaderboard_handler, leaderboard_rows_handler, login,
    login_username, nav_fragment_handler, register, register_username,
    routes::admin::admin_dashboard, run_competition_task, secrets::get_key, start_new_session,
    submit_score, update_lightning_address, GameStore, LedgerService, LedgerStore,
    LightningProvider, LightningService, LndClient, PaymentStore, UserStore,
};
pub struct Application {
    server: Serve<
        TcpListener,
        IntoMakeServiceWithConnectInfo<Router, SocketAddr>,
        AddExtension<Router, ConnectInfo<SocketAddr>>,
    >,
}

impl Application {
    pub async fn build(config: Settings) -> Result<Self, anyhow::Error> {
        let address = format!(
            "{}:{}",
            config.api_settings.domain, config.api_settings.port
        );
        let listener = SocketAddr::from_str(&address)?;
        let (app_state, serve_dir) = build_app(config).await?;
        let server = build_server(listener, app_state, serve_dir).await?;
        Ok(Self { server })
    }

    pub async fn run_until_stopped(self) -> Result<(), anyhow::Error> {
        info!("Starting server...");
        match self.server.with_graceful_shutdown(shutdown_signal()).await {
            Ok(_) => {
                info!("Shutdown complete");
                Ok(())
            }
            Err(e) => {
                error!("Server shutdown error: {}", e);
                Err(anyhow!("Error during server shutdown: {}", e))
            }
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub ui_dir: String,
    pub remote_url: String,
    pub user_store: UserStore,
    pub game_store: GameStore,
    pub payment_store: PaymentStore,
    /// Voltage-specific client. Existing route handlers use this directly.
    pub lightning_service: LightningService,
    /// Unified provider that delegates to Voltage or LND based on config.
    /// New or migrated route handlers should prefer this field.
    pub lightning_provider: LightningProvider,
    pub ledger_service: LedgerService,
}

pub async fn build_app(config: Settings) -> Result<(AppState, ServeDir), anyhow::Error> {
    // The ui folder needs to be generated and have this relative path from where the binary is being run
    let ui_path = std::path::Path::new(&config.ui_settings.ui_dir)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(&config.ui_settings.ui_dir));
    info!("Serving UI files from: {:?}", ui_path);
    let serve_dir = ServeDir::new(ui_path);
    info!("Public UI configured");

    create_folder(&config.db_settings.data_folder.clone());

    let db_path = format!("{}/game.db", config.db_settings.data_folder);
    let database_url = format!("sqlite:{}?mode=rwc", db_path);
    info!("Connecting to database at {}", database_url);

    let db_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .map_err(|e| anyhow!("Failed to connect to database: {}", e))?;

    info!("Running database migrations");
    let migrations_path = std::path::Path::new(&config.db_settings.migrations_folder);
    sqlx::migrate::Migrator::new(migrations_path)
        .await
        .map_err(|e| anyhow!("Failed to prepare migrations: {}", e))?
        .run(&db_pool)
        .await
        .map_err(|e| anyhow!("Failed to run migrations: {}", e))?;

    info!("Database migrations completed successfully");

    let lightning_service = LightningService::new(
        build_reqwest_client(),
        config.api_settings.voltage_api_url.clone(),
        config.api_settings.voltage_api_key.clone(),
        config.api_settings.voltage_org_id.clone(),
        config.api_settings.voltage_env_id.clone(),
        config.api_settings.voltage_wallet_id.clone(),
    );

    // Build the unified lightning provider based on config.
    let lightning_provider = match config.ln_settings.provider.as_str() {
        "lnd" => {
            info!("Lightning provider: LND");
            let lnd_client = LndClient::new(
                config
                    .ln_settings
                    .lnd_base_url
                    .as_deref()
                    .unwrap_or("https://localhost:8080"),
                config
                    .ln_settings
                    .lnd_macaroon_path
                    .as_deref()
                    .unwrap_or("./creds/admin.macaroon"),
                config.ln_settings.lnd_tls_cert_path.as_deref(),
            )?;
            LightningProvider::Lnd(lnd_client)
        }
        "stub" => {
            info!("Lightning provider: Stub (testing mode — all invoices auto-settle)");
            LightningProvider::Stub
        }
        _ => {
            info!("Lightning provider: Voltage");
            LightningProvider::Voltage(lightning_service.clone())
        }
    };

    let secret_key: nostr_sdk::secp256k1::SecretKey =
        get_key(&config.api_settings.private_key_file)?;
    let keys = Keys::parse(&hex::encode(secret_key.secret_bytes()))
        .map_err(|e| anyhow!("Failed to parse server keys: {}", e))?;
    info!("Server Nostr pubkey: {}", keys.public_key());

    let ledger_store = LedgerStore::new(db_pool.clone());
    let ledger_service = LedgerService::new(keys, ledger_store);

    let app_state = AppState {
        ui_dir: config.ui_settings.ui_dir.clone(),
        remote_url: config.ui_settings.remote_url.clone(),
        user_store: UserStore::new(db_pool.clone()),
        game_store: GameStore::new(db_pool.clone()),
        payment_store: PaymentStore::new(db_pool.clone()),
        lightning_service,
        lightning_provider,
        ledger_service,
        settings: config,
    };
    Ok((app_state, serve_dir))
}

pub async fn build_server(
    socket_addr: SocketAddr,
    app_state: AppState,
    serve_dir: ServeDir,
) -> Result<
    Serve<
        TcpListener,
        IntoMakeServiceWithConnectInfo<Router, SocketAddr>,
        AddExtension<Router, ConnectInfo<SocketAddr>>,
    >,
    anyhow::Error,
> {
    let listener = TcpListener::bind(socket_addr).await?;

    info!("Setting up service");
    let app = app(app_state.clone(), serve_dir);

    // Spawn background tasks
    // TODO have these close down with the server gracefully
    let shared_state = Arc::new(app_state);
    spawn(run_competition_task(shared_state.clone()));
    spawn(crate::run_invoice_watcher(shared_state.clone()));

    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    );
    info!(
        "Service running @: http://{}:{}",
        socket_addr.ip(),
        socket_addr.port()
    );
    Ok(server)
}

pub fn app(app_state: AppState, serve_dir: ServeDir) -> Router {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([ACCEPT, CONTENT_TYPE, AUTHORIZATION])
        .allow_origin(Any);

    let users_endpoints = Router::new()
        .route("/login", post(login))
        .route("/register", post(register))
        .route("/username/register", post(register_username))
        .route("/username/login", post(login_username))
        .route("/profile", get(get_user_profile))
        .route("/lightning-address", post(update_lightning_address));

    let game_endpoints = Router::new()
        .route("/config", get(get_game_config))
        .route("/session", post(start_new_session))
        .route("/score", post(submit_score))
        .route("/scores/top", get(get_top_scores))
        .route("/scores/user", get(get_user_scores))
        .route("/competition", get(get_competition_info))
        .route("/replays/top", get(get_top_replays))
        .route("/replay/{score_id}", get(get_replay_by_score));

    let payment_endpoints = Router::new().route("/status/{payment_id}", get(check_payment_status));

    let prize_endpoints = Router::new()
        .route("/check", get(check_prize_eligibility))
        .route("/claim", post(claim_prize));

    let ledger_endpoints = Router::new()
        .route("/events", get(get_ledger_events))
        .route("/pubkey", get(get_server_pubkey))
        .route("/summary", get(get_ledger_summary));

    // Serve bundled JS/CSS from the configured static directory
    let static_dir = std::path::Path::new(&app_state.settings.ui_settings.static_dir)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(&app_state.settings.ui_settings.static_dir));
    info!("Serving static files from: {:?}", static_dir);
    let static_serve = ServeDir::new(&static_dir);

    Router::new()
        .route("/", get(home_handler))
        .route("/play", get(game_handler))
        .route("/leaderboard", get(leaderboard_handler))
        .route("/fragments/leaderboard-rows", get(leaderboard_rows_handler))
        .route("/fragments/nav", get(nav_fragment_handler))
        .route("/admin", get(admin_dashboard))
        .route("/api/v1/health_check", get(health_check))
        .nest("/api/v1/users", users_endpoints)
        .nest("/api/v1/game", game_endpoints)
        .nest("/api/v1/payments", payment_endpoints)
        .nest("/api/v1/prizes", prize_endpoints)
        .nest("/api/v1/ledger", ledger_endpoints)
        .nest_service("/ui", serve_dir.clone())
        .nest_service("/static", static_serve)
        .fallback(index_handler)
        .layer(middleware::from_fn(log_request))
        .with_state(Arc::new(app_state))
        .layer(cors)
}

async fn log_request(request: Request<Body>, next: Next) -> impl IntoResponse {
    let now = time::OffsetDateTime::now_utc();
    let path = request
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or_default();
    info!(target: "http_request","new request, {} {}", request.method().as_str(), path);

    let response = next.run(request).await;
    let response_time = time::OffsetDateTime::now_utc() - now;
    info!(target: "http_response", "response, code: {}, time: {}", response.status().as_str(), response_time);

    response
}

pub fn build_reqwest_client() -> ClientWithMiddleware {
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
    ClientBuilder::new(Client::new())
        .with(LoggingMiddleware)
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build()
}

struct LoggingMiddleware;

#[async_trait::async_trait]
impl Middleware for LoggingMiddleware {
    async fn handle(
        &self,
        req: reqwest::Request,
        extensions: &mut Extensions,
        next: reqwest_middleware::Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        let method = req.method().clone();
        let url = req.url().clone();

        info!("Making {} request to: {}", method, url);

        let result = next.run(req, extensions).await;

        match &result {
            Ok(response) => {
                info!("{} {} -> Status: {}", method, url, response.status());
            }
            Err(error) => {
                warn!("{} {} -> Error: {:?}", method, url, error);
            }
        }

        result
    }
}

async fn shutdown_signal() {
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to install SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");

    select! {
        _ = sigint.recv() => info!("Received SIGINT signal"),
        _ = sigterm.recv() => info!("Received SIGTERM signal"),
    }
}
