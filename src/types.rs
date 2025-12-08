use crate::config::Config;
use aws_sdk_s3::Client as S3Client;
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
    pub created_at: u64,
}

pub type ProgressMap = Arc<RwLock<HashMap<String, ProgressUpdate>>>;

#[derive(Clone, Debug)]
pub struct VideoVariant {
    pub label: String,
    pub height: u32,
    pub bitrate: u32, // in kbps
}

impl VideoVariant {
    /// Create a new variant with dynamically calculated bitrate based on resolution
    /// Uses bits-per-pixel (BPP) formula for optimal quality/size balance
    pub fn new(label: &str, height: u32) -> Self {
        Self {
            label: label.to_string(),
            height,
            bitrate: Self::calculate_bitrate(height),
        }
    }

    /// Calculate optimal bitrate based on resolution using BPP (bits per pixel)
    /// BPP of 0.1 is good for H.264 with motion (anime/live action)
    /// Formula: bitrate = width * height * fps * bpp
    pub fn calculate_bitrate(height: u32) -> u32 {
        // Assume 16:9 aspect ratio and 24fps (common for anime/movies)
        let width = (height as f64 * 16.0 / 9.0).round() as u32;
        let fps = 24.0;

        // BPP values tuned for H.264 encoding quality
        // Higher resolutions can use lower BPP due to better compression efficiency
        let bpp = match height {
            0..=480 => 0.12,     // SD needs higher BPP for quality
            481..=720 => 0.10,   // HD sweet spot
            721..=1080 => 0.08,  // FHD - good compression
            1081..=1440 => 0.07, // QHD - efficient at scale
            _ => 0.06,           // 4K+ - very efficient
        };

        let bitrate_bps = (width as f64) * (height as f64) * fps * bpp;
        let bitrate_kbps = (bitrate_bps / 1000.0).round() as u32;

        // Clamp to reasonable bounds
        bitrate_kbps.clamp(500, 20000)
    }

    /// Get bitrate as formatted string (e.g., "2500k")
    #[inline]
    pub fn bitrate_str(&self) -> String {
        format!("{}k", self.bitrate)
    }

    /// Get max bitrate (1.5x target) for VBR headroom
    #[inline]
    pub fn max_bitrate(&self) -> u32 {
        self.bitrate * 3 / 2
    }

    /// Get buffer size (2x target) for smooth streaming
    #[inline]
    pub fn bufsize(&self) -> u32 {
        self.bitrate * 2
    }

    /// Get bandwidth in bps for HLS manifest
    #[inline]
    pub fn bandwidth(&self) -> u32 {
        self.bitrate * 1000
    }
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
    pub sprites_url: Option<String>,
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
    pub created_at: u64,
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
    pub last_activity: u64,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubtitleTrack {
    pub id: i64,
    pub video_id: String,
    pub track_index: i32,
    pub language: Option<String>,
    pub title: Option<String>,
    pub codec: String,
    pub storage_key: String,
    pub idx_storage_key: Option<String>, // For VobSub subtitles (.idx file)
    pub is_default: bool,
    pub is_forced: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attachment {
    pub id: i64,
    pub video_id: String,
    pub filename: String,
    pub mimetype: String,
    pub storage_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioTrack {
    pub id: i64,
    pub video_id: String,
    pub track_index: i32,
    pub language: Option<String>,
    pub title: Option<String>,
    pub codec: String,
    pub channels: Option<i32>,
    pub sample_rate: Option<i32>,
    pub bit_rate: Option<i64>,
    pub is_default: bool,
}

#[derive(Serialize)]
pub struct AudioTrackListResponse {
    pub items: Vec<AudioTrack>,
}

#[derive(Clone, Debug)]
pub struct SubtitleStreamInfo {
    pub stream_index: i32,
    pub codec_name: String,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct AudioStreamInfo {
    pub stream_index: i32,
    pub codec_name: String,
    pub language: Option<String>,
    pub title: Option<String>,
    pub channels: Option<i32>,
    pub sample_rate: Option<i32>,
    pub bit_rate: Option<i64>,
    pub is_default: bool,
}

#[derive(Clone, Debug)]
pub struct AttachmentInfo {
    pub filename: String,
    pub mimetype: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chapter {
    pub id: i64,
    pub video_id: String,
    pub chapter_index: i32,
    pub start_time: f64,
    pub end_time: f64,
    pub title: String,
}

#[derive(Clone, Debug)]
pub struct ChapterInfo {
    pub start_time: f64,
    pub end_time: f64,
    pub title: String,
}

#[derive(Serialize)]
pub struct SubtitleListResponse {
    pub subtitles: Vec<SubtitleTrack>,
}

#[derive(Serialize)]
pub struct AttachmentListResponse {
    pub attachments: Vec<Attachment>,
}

#[derive(Serialize)]
pub struct ChapterListResponse {
    pub chapters: Vec<Chapter>,
}
