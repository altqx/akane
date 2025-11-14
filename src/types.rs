use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use aws_sdk_s3::Client as S3Client;
use sqlx::SqlitePool;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct ProgressUpdate {
    pub stage: String,
    pub current_chunk: u32,
    pub total_chunks: u32,
    pub percentage: u32,
}

pub type ProgressMap = Arc<RwLock<HashMap<String, ProgressUpdate>>>;

#[derive(Clone, Debug)]
pub struct VideoVariant {
    pub label: String,
    pub height: u32,
    pub bitrate: String,
}

#[derive(Clone)]
pub struct AppState {
    pub s3: S3Client,
    pub bucket: String,
    pub public_base_url: String,
    pub db_pool: SqlitePool,
    pub progress: ProgressMap,
}

#[derive(Serialize)]
pub struct UploadResponse {
    pub playlist_url: String,
    pub upload_id: String,
}

#[derive(Serialize)]
pub struct ProgressResponse {
    pub stage: String,
    pub current_chunk: u32,
    pub total_chunks: u32,
    pub percentage: u32,
}
