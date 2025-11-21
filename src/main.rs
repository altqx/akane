mod database;
mod handlers;
mod storage;
mod types;
mod video;

use anyhow::{Context, Result};
use aws_sdk_s3::{Client as S3Client, config::Region};
use axum::extract::DefaultBodyLimit;
use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::Redirect,
    response::Response,
    routing::{get, post},
};
use dotenv::dotenv;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

use types::AppState;

async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok());

    let expected_auth = format!("Bearer {}", state.admin_password);

    match auth_header {
        Some(auth) if auth == expected_auth => Ok(next.run(req).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

async fn check_auth() -> Result<(), StatusCode> {
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,akane=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let r2_endpoint = std::env::var("R2_ENDPOINT").context(
        "R2_ENDPOINT env var required (e.g. https://<accountid>.r2.cloudflarestorage.com)",
    )?;
    let r2_bucket = std::env::var("R2_BUCKET").context("R2_BUCKET env var required")?;
    let r2_access_key =
        std::env::var("R2_ACCESS_KEY_ID").context("R2_ACCESS_KEY_ID env var required")?;
    let r2_secret_key =
        std::env::var("R2_SECRET_ACCESS_KEY").context("R2_SECRET_ACCESS_KEY env var required")?;
    let public_base_url = std::env::var("R2_PUBLIC_BASE_URL")
        .unwrap_or_else(|_| format!("{}/{}", r2_endpoint, r2_bucket));

    let s3_config = aws_sdk_s3::config::Builder::new()
        .endpoint_url(r2_endpoint)
        .region(Region::new("auto"))
        .credentials_provider(aws_sdk_s3::config::Credentials::new(
            r2_access_key,
            r2_secret_key,
            None,
            None,
            "r2",
        ))
        .build();
    let s3 = S3Client::from_conf(s3_config);

    let database_url = "sqlite://videos.db";
    let db_pool = database::initialize_database(database_url).await?;

    let progress = Arc::new(RwLock::new(HashMap::new()));

    let secret_key = std::env::var("SECRET_KEY").unwrap_or_else(|_| {
        // Generate a random key if not provided (for dev)
        Uuid::new_v4().to_string()
    });

    let admin_password = std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| {
        let pass = Uuid::new_v4().to_string();
        info!(
            "ADMIN_PASSWORD not set, generated random password: {}",
            pass
        );
        pass
    });

    // Limit concurrent FFmpeg processes (configurable via env, default 1)
    let max_concurrent = std::env::var("MAX_CONCURRENT_ENCODES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1);
    let ffmpeg_semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));

    let state = AppState {
        s3,
        bucket: r2_bucket,
        public_base_url,
        db_pool,
        progress: progress.clone(),
        secret_key,
        admin_password,
        active_viewers: Arc::new(RwLock::new(HashMap::new())),
        ffmpeg_semaphore,
    };

    let public_routes = Router::new()
        .route("/videos/{id}/heartbeat", post(handlers::heartbeat))
        .route("/analytics/realtime", get(handlers::get_realtime_analytics))
        .route("/analytics/history", get(handlers::get_analytics_history))
        .route("/analytics/videos", get(handlers::get_analytics_videos));

    let protected_routes = Router::new()
        .route("/upload", post(handlers::upload_video))
        .route("/progress/{upload_id}", get(handlers::get_progress))
        .route("/videos", get(handlers::list_videos))
        .route("/auth/check", get(check_auth))
        //.route("/purge", delete(handlers::purge_bucket))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let api_routes = Router::new().merge(public_routes).merge(protected_routes);

    let app = Router::new()
        .nest("/api", api_routes)
        .route("/hls/{id}/{*file}", get(handlers::get_hls_file))
        .route("/player/{id}", get(handlers::get_player))
        .nest_service(
            "/admin-webui",
            ServeDir::new("webui")
                .append_index_html_on_directories(false)
                .fallback(ServeFile::new("webui/index.html")),
        )
        .route(
            "/",
            get(|| async { Redirect::permanent("https://altqx.com/") }),
        )
        // e.g. 1 GB body limit
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();
    info!("listening on {}", addr);

    axum::serve(
        tokio::net::TcpListener::bind(addr).await?,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("server error")?;
    Ok(())
}
