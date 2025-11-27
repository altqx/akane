mod clickhouse;
mod config;
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
    http::{StatusCode, header, Method},
    middleware::{self, Next},
    response::Redirect,
    response::Response,
    routing::{/*delete,*/ get, post},
};
use config::Config;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

    let expected_auth = format!("Bearer {}", state.config.server.admin_password);

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
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,akane=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::load("config.yml").await?;

    let s3_config = aws_sdk_s3::config::Builder::new()
        .endpoint_url(&config.r2.endpoint)
        .region(Region::new("auto"))
        .credentials_provider(aws_sdk_s3::config::Credentials::new(
            &config.r2.access_key_id,
            &config.r2.secret_access_key,
            None,
            None,
            "r2",
        ))
        .build();
    let s3 = S3Client::from_conf(s3_config);

    let database_url = "sqlite://videos.db";
    let db_pool = database::initialize_database(database_url).await?;

    let clickhouse_client = clickhouse::initialize_client(&config.clickhouse);
    clickhouse::create_schema(&clickhouse_client, &config.clickhouse).await?;

    let progress = Arc::new(RwLock::new(HashMap::new()));

    let ffmpeg_semaphore = Arc::new(tokio::sync::Semaphore::new(
        config.server.max_concurrent_encodes,
    ));

    let host = config.server.host.clone();
    let port = config.server.port;

    let state = AppState {
        config,
        s3,
        db_pool,
        progress: progress.clone(),
        active_viewers: Arc::new(RwLock::new(HashMap::new())),
        ffmpeg_semaphore,
        clickhouse: clickhouse_client,
    };

    let public_routes = Router::new()
        .route("/videos/{id}/heartbeat", post(handlers::heartbeat))
        .route("/analytics/realtime", get(handlers::get_realtime_analytics))
        .route("/analytics/history", get(handlers::get_analytics_history))
        .route("/analytics/videos", get(handlers::get_analytics_videos))
        .route("/progress/{upload_id}", get(handlers::get_progress));

    let protected_routes = Router::new()
        .route("/upload", post(handlers::upload_video))
        .route("/videos", get(handlers::list_videos))
        .route("/queues", get(handlers::list_queues))
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
        // CORS layer for development (Next.js dev server on different port)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers(Any)
                .expose_headers(Any),
        )
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", host, port).parse().unwrap();
    info!("listening on {}", addr);

    axum::serve(
        tokio::net::TcpListener::bind(addr).await?,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("server error")?;
    Ok(())
}
