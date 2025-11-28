use crate::config::Config;
use aws_sdk_s3::Client as S3Client;
use clickhouse;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};

#[derive(Clone, Debug, Serialize)]
pub struct ProgressUpdate {
    pub stage: String,
    pub current_chunk: u32,
    pub total_chunks: u32,
    pub percentage: u32,
    pub details: Option<String>,
    pub status: String, // "processing", "completed", "failed"
    pub result: Option<UploadResponse>,
    pub error: Option<String>,
    pub video_name: Option<String>,
    pub created_at: u64, // Unix timestamp in milliseconds for queue ordering
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
    pub config: Config,
    pub s3: S3Client,
    pub db_pool: SqlitePool,
    pub progress: ProgressMap,
    pub active_viewers: Arc<RwLock<HashMap<String, HashMap<String, std::time::Instant>>>>,
    pub ffmpeg_semaphore: Arc<Semaphore>,
    pub clickhouse: clickhouse::Client,
    pub chunked_uploads: ChunkedUploadsMap,
}

#[derive(Serialize, Clone, Debug)]
pub struct UploadResponse {
    pub player_url: String,
    pub upload_id: String,
}

#[derive(Serialize)]
pub struct UploadAccepted {
    pub upload_id: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct ProgressResponse {
    pub stage: String,
    pub current_chunk: u32,
    pub total_chunks: u32,
    pub percentage: u32,
    pub details: Option<String>,
    pub status: String,
    pub result: Option<UploadResponse>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct VideoQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub name: Option<String>,
    pub tag: Option<String>,
}

#[derive(Serialize)]
pub struct VideoDto {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub available_resolutions: Vec<String>,
    pub duration: u32,
    pub thumbnail_url: String,
    pub player_url: String,
    pub view_count: i64,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct VideoListResponse {
    pub items: Vec<VideoDto>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
    pub has_next: bool,
    pub has_prev: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct QueueItem {
    pub upload_id: String,
    pub stage: String,
    pub current_chunk: u32,
    pub total_chunks: u32,
    pub percentage: u32,
    pub details: Option<String>,
    pub status: String,
    pub video_name: Option<String>,
    pub created_at: u64, // Unix timestamp in milliseconds for queue ordering
}

#[derive(Serialize)]
pub struct QueueListResponse {
    pub items: Vec<QueueItem>,
    pub active_count: u32,
    pub completed_count: u32,
    pub failed_count: u32,
}

#[derive(Clone, Debug)]
pub struct ChunkedUpload {
    pub file_name: String,
    pub total_chunks: u32,
    pub received_chunks: Vec<bool>,
    pub temp_dir: std::path::PathBuf,
}

pub type ChunkedUploadsMap = Arc<RwLock<HashMap<String, ChunkedUpload>>>;

#[derive(Serialize)]
pub struct ChunkUploadResponse {
    pub upload_id: String,
    pub chunk_index: u32,
    pub received: bool,
}

#[derive(Deserialize)]
pub struct FinalizeUploadRequest {
    pub name: String,
    pub tags: Option<String>,
}
