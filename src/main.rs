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
    routing::{delete, get, post},
};
use dotenv::dotenv;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tower_http::services::ServeDir;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

use types::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenv().ok();
    dotenv().expect("Failed to load .env file");

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

    let state = AppState {
        s3,
        bucket: r2_bucket,
        public_base_url,
        db_pool,
        progress: progress.clone(),
        secret_key,
    };

    let app = Router::new()
        .route("/api/upload", post(handlers::upload_video))
        .route("/api/progress/{upload_id}", get(handlers::get_progress))
        .route("/api/videos", get(handlers::list_videos))
        //.route("/api/purge", delete(handlers::purge_bucket))
        .route("/hls/{id}/{*file}", get(handlers::get_hls_file))
        .route("/player/{id}", get(handlers::get_player))
        .fallback_service(ServeDir::new("public"))
        // e.g. 1 GB body limit
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
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
