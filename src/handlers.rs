use crate::clickhouse;
use crate::database::{
    /*clear_database,*/ count_videos, list_videos as db_list_videos, save_video,
};
use crate::storage::upload_hls_to_r2;
use crate::types::{
    AppState, ProgressResponse, ProgressUpdate, UploadAccepted, UploadResponse, VideoListResponse,
    VideoQuery,
};
use crate::video::{encode_to_hls, get_variants_for_height, get_video_duration, get_video_height};
// use aws_sdk_s3::types::{Delete, ObjectIdentifier};
use axum::{
    Json,
    extract::{ConnectInfo, Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{
        Html, IntoResponse, Response,
        sse::{Event, Sse},
    },
};
use futures::stream::Stream;
use minify_js::{Session, TopLevelMode, minify};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use tokio::{fs, io::AsyncWriteExt};
use tracing::error;
use uuid::Uuid;

pub async fn upload_video(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<UploadAccepted>, (axum::http::StatusCode, String)> {
    let mut video_path: Option<PathBuf> = None;
    let mut video_name: Option<String> = None;
    let mut tags: Vec<String> = Vec::new();

    // Create a unique upload ID for progress tracking, or use provided one
    let upload_id = headers
        .get("X-Upload-ID")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Initialize progress immediately to avoid race condition with SSE
    {
        let initial_progress = ProgressUpdate {
            stage: "Initializing upload".to_string(),
            current_chunk: 0,
            total_chunks: 1,
            percentage: 0,
            details: Some("Waiting for file data...".to_string()),
            status: "initializing".to_string(),
            result: None,
            error: None,
        };
        state
            .progress
            .write()
            .await
            .insert(upload_id.clone(), initial_progress);
    }

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?
    {
        let field_name = field.name().map(|s| s.to_string());

        match field_name.as_deref() {
            Some("file") => {
                let file_name = field
                    .file_name()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "upload.mp4".to_string());

                let tmp_dir = std::env::temp_dir();
                let tmp_file = tmp_dir.join(format!("{}-{}", Uuid::new_v4(), file_name));

                let mut file = fs::File::create(&tmp_file)
                    .await
                    .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

                // Stream the file to disk and update progress
                let mut total_bytes = 0;
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|e| internal_err(anyhow::anyhow!(e)))?
                {
                    total_bytes += chunk.len();
                    file.write_all(&chunk)
                        .await
                        .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

                    if !upload_id.is_empty() {
                        let progress_update = ProgressUpdate {
                            stage: "Uploading to server".to_string(),
                            current_chunk: 0,
                            total_chunks: 1,
                            percentage: 0,
                            details: Some(format!("Uploaded {} bytes", total_bytes)),
                            status: "processing".to_string(),
                            result: None,
                            error: None,
                        };
                        state
                            .progress
                            .write()
                            .await
                            .insert(upload_id.clone(), progress_update);
                    }
                }

                video_path = Some(tmp_file);
            }
            Some("name") => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| internal_err(anyhow::anyhow!(e)))?;
                video_name = Some(text);
            }
            Some("tags") => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| internal_err(anyhow::anyhow!(e)))?;
                // Parse tags as JSON array or comma-separated
                if let Ok(parsed_tags) = serde_json::from_str::<Vec<String>>(&text) {
                    tags = parsed_tags;
                } else {
                    tags = text
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
            _ => {
                continue;
            }
        }
    }

    let video_path = video_path.ok_or_else(|| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "missing file field 'file'".to_string(),
        )
    })?;

    let video_name = video_name.ok_or_else(|| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "missing field 'name'".to_string(),
        )
    })?;

    // Initialize progress
    let initial_progress = ProgressUpdate {
        stage: "Queued for processing".to_string(),
        current_chunk: 0,
        total_chunks: 1,
        percentage: 0,
        details: None,
        status: "processing".to_string(),
        result: None,
        error: None,
    };
    state
        .progress
        .write()
        .await
        .insert(upload_id.clone(), initial_progress);

    // Spawn background task for processing
    let state_clone = state.clone();
    let upload_id_clone = upload_id.clone();
    let video_path_clone = video_path.clone();
    let video_name_clone = video_name.clone();
    let tags_clone = tags.clone();

    tokio::spawn(async move {
        let result = async {
            // Encode to HLS (playlist + segments) into a temp directory
            let output_id = Uuid::new_v4().to_string();
            let hls_dir = std::env::temp_dir().join(format!("hls-{}", &output_id));
            fs::create_dir_all(&hls_dir)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;

            // Get video metadata before encoding (parallel)
            let (video_duration, original_height) = tokio::join!(
                get_video_duration(&video_path_clone),
                get_video_height(&video_path_clone)
            );
            let video_duration = video_duration?;
            let original_height = original_height?;
            let variants = get_variants_for_height(original_height);
            let available_resolutions: Vec<String> =
                variants.iter().map(|v| v.label.clone()).collect();

            // Update progress: FFmpeg processing stage
            let encoding_progress = ProgressUpdate {
                stage: "FFmpeg processing".to_string(),
                current_chunk: 0,
                total_chunks: variants.len() as u32,
                percentage: 0,
                details: Some("Starting encoding...".to_string()),
                status: "processing".to_string(),
                result: None,
                error: None,
            };
            state_clone
                .progress
                .write()
                .await
                .insert(upload_id_clone.clone(), encoding_progress);

            encode_to_hls(
                &video_path_clone,
                &hls_dir,
                &state_clone.progress,
                &upload_id_clone,
                state_clone.ffmpeg_semaphore.clone(),
                &state_clone.config.video.encoder,
            )
            .await?;

            // Update progress: Upload to R2 stage
            let upload_progress = ProgressUpdate {
                stage: "Upload to R2".to_string(),
                current_chunk: 0,
                total_chunks: 1,
                percentage: 0,
                details: Some("Uploading segments to storage...".to_string()),
                status: "processing".to_string(),
                result: None,
                error: None,
            };
            state_clone
                .progress
                .write()
                .await
                .insert(upload_id_clone.clone(), upload_progress);

            // Upload HLS to R2
            let prefix = format!("{}/", output_id);
            // Build public URL (pointing to our proxy)
            let playlist_key =
                upload_hls_to_r2(&state_clone, &hls_dir, &prefix, Some(&upload_id_clone)).await?;

            // Save to database
            let thumbnail_key = format!("{}/thumbnail.jpg", output_id);
            let entrypoint = playlist_key.clone();

            save_video(
                &state_clone.db_pool,
                &output_id,
                &video_name_clone,
                &tags_clone,
                &available_resolutions,
                video_duration,
                &thumbnail_key,
                &entrypoint,
            )
            .await?;

            // Cleanup (ignore errors)
            let _ = fs::remove_file(&video_path_clone).await;
            let _ = fs::remove_dir_all(&hls_dir).await;

            // Return player URL
            let player_url = format!("/player/{}", output_id);
            Ok::<_, anyhow::Error>(UploadResponse {
                player_url,
                upload_id: upload_id_clone.clone(),
            })
        }
        .await;

        match result {
            Ok(response) => {
                let completion_progress = ProgressUpdate {
                    stage: "Completed".to_string(),
                    current_chunk: 1,
                    total_chunks: 1,
                    percentage: 100,
                    details: Some("Upload and processing complete".to_string()),
                    status: "completed".to_string(),
                    result: Some(response),
                    error: None,
                };
                state_clone
                    .progress
                    .write()
                    .await
                    .insert(upload_id_clone, completion_progress);
            }
            Err(e) => {
                error!("Background processing failed: {:?}", e);
                let error_progress = ProgressUpdate {
                    stage: "Failed".to_string(),
                    current_chunk: 0,
                    total_chunks: 1,
                    percentage: 0,
                    details: Some(format!("Processing failed: {}", e)),
                    status: "failed".to_string(),
                    result: None,
                    error: Some(e.to_string()),
                };
                state_clone
                    .progress
                    .write()
                    .await
                    .insert(upload_id_clone, error_progress);
            }
        }
    });

    Ok(Json(UploadAccepted {
        upload_id,
        message: "File uploaded successfully, processing started in background".to_string(),
    }))
}

pub async fn get_progress(
    State(state): State<AppState>,
    Path(upload_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Sse<impl Stream<Item = Result<Event, anyhow::Error>> + Send> {
    // Check for token in query params (for EventSource which can't set headers)
    let is_authorized = if let Some(token) = params.get("token") {
        let expected_auth = format!("Bearer {}", state.config.server.admin_password);
        let provided_auth = format!("Bearer {}", token);
        provided_auth == expected_auth
    } else {
        false
    };

    let stream = async_stream::stream! {
        if !is_authorized {
            yield Ok(Event::default().event("error").data("Unauthorized"));
            return;
        }

        let start_time = std::time::Instant::now();
        let timeout = Duration::from_secs(60); // Wait up to 60s for upload to start

        loop {
            let progress = {
                let progress_map = state.progress.read().await;
                progress_map.get(&upload_id).cloned()
            };

            if let Some(p) = progress {
                // Only yield if changed or every few seconds to keep alive
                let json = serde_json::to_string(&ProgressResponse {
                    stage: p.stage.clone(),
                    current_chunk: p.current_chunk,
                    total_chunks: p.total_chunks,
                    percentage: p.percentage,
                    details: p.details.clone(),
                    status: p.status.clone(),
                    result: p.result.clone(),
                    error: p.error.clone(),
                })
                .unwrap_or_default();

                yield Ok(Event::default().data(json));

                if p.status == "completed" || p.status == "failed" {
                    // Wait a bit to ensure client receives the message before closing
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    break;
                }
            } else {
                // If not found, check if we timed out waiting for it to start
                if start_time.elapsed() > timeout {
                    yield Ok(Event::default().event("error").data("Upload ID not found (timeout)"));
                    break;
                }
                // Otherwise just wait and retry
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

pub async fn list_videos(
    State(state): State<AppState>,
    Query(query): Query<VideoQuery>,
) -> Result<Json<VideoListResponse>, (StatusCode, String)> {
    // Normalize page and page_size with defaults and limits
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);

    let filters = VideoQuery {
        page: Some(page),
        page_size: Some(page_size),
        name: query.name.clone(),
        tag: query.tag.clone(),
    };

    let total = count_videos(&state.db_pool, &filters)
        .await
        .map_err(|e| internal_err(e))?;

    let items = db_list_videos(
        &state.db_pool,
        &filters,
        page,
        page_size,
        &state.config.r2.public_base_url,
        &HashMap::new(), // TODO: Fetch view counts if needed for list, or just pass empty for now as list usually doesn't show precise view counts or we can fetch them
    )
    .await
    .map_err(|e| internal_err(e))?;

    // Optimization: Fetch view counts for the returned videos only
    let video_ids: Vec<String> = items.iter().map(|v| v.id.clone()).collect();
    let view_counts = clickhouse::get_view_counts(&state.clickhouse, &video_ids)
        .await
        .map_err(|e| internal_err(e))?;

    // Update items with view counts
    let items = items
        .into_iter()
        .map(|mut v| {
            if let Some(&count) = view_counts.get(&v.id) {
                v.view_count = count;
            }
            v
        })
        .collect();

    let total_u64 = total as u64;
    let page_u64 = page as u64;
    let page_size_u64 = page_size as u64;

    let has_prev = page > 1;
    let has_next = page_u64 * page_size_u64 < total_u64;

    Ok(Json(VideoListResponse {
        items,
        page,
        page_size,
        total: total_u64,
        has_next,
        has_prev,
    }))
}

pub async fn heartbeat(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(video_id): Path<String>,
) -> StatusCode {
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|xff| xff.split(',').next().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| addr.ip().to_string());

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    // Update active viewers in memory
    {
        let mut viewers = state.active_viewers.write().await;
        let video_viewers = viewers.entry(video_id.clone()).or_default();
        // Use IP + UserAgent as a simple unique identifier for now
        let viewer_id = format!("{}-{}", ip, user_agent);
        video_viewers.insert(viewer_id, std::time::Instant::now());
    }

    StatusCode::OK
}

pub async fn get_realtime_analytics(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, anyhow::Error>> + Send> {
    let stream = async_stream::stream! {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;

            let mut active_counts = HashMap::new();
            let now = std::time::Instant::now();

            {
                let mut viewers = state.active_viewers.write().await;
                // Remove expired viewers (no heartbeat in last 30 seconds)
                for (video_id, video_viewers) in viewers.iter_mut() {
                    video_viewers.retain(|_, last_seen| now.duration_since(*last_seen) < Duration::from_secs(30));
                    if !video_viewers.is_empty() {
                        active_counts.insert(video_id.clone(), video_viewers.len());
                    }
                }
                // Cleanup empty videos
                viewers.retain(|_, v| !v.is_empty());
            }

            let json = serde_json::to_string(&active_counts).unwrap_or_default();
            yield Ok(Event::default().data(json));
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

pub async fn get_analytics_history(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::clickhouse::HistoryItem>>, (StatusCode, String)> {
    let history = crate::clickhouse::get_analytics_history(&state.clickhouse)
        .await
        .map_err(|e| internal_err(e))?;
    Ok(Json(history))
}

#[derive(serde::Serialize)]
pub struct AnalyticsVideoDto {
    pub id: String,
    pub name: String,
    pub view_count: i64,
    pub created_at: String,
    pub thumbnail_url: String,
}

pub async fn get_analytics_videos(
    State(state): State<AppState>,
) -> Result<Json<Vec<AnalyticsVideoDto>>, (StatusCode, String)> {
    // First get all video IDs to fetch counts
    // We fetch summary from DB and then enrich with CH data
    let mut videos = crate::database::get_all_videos_summary(&state.db_pool, &HashMap::new())
        .await
        .map_err(|e| internal_err(e))?;

    let video_ids: Vec<String> = videos.iter().map(|v| v.id.clone()).collect();
    let view_counts = clickhouse::get_view_counts(&state.clickhouse, &video_ids)
        .await
        .map_err(|e| internal_err(e))?;

    for video in &mut videos {
        if let Some(&count) = view_counts.get(&video.id) {
            video.view_count = count;
        }
    }

    let base = state.config.r2.public_base_url.trim_end_matches('/');

    let dtos = videos
        .into_iter()
        .map(|v| AnalyticsVideoDto {
            id: v.id,
            name: v.name,
            view_count: v.view_count,
            created_at: v.created_at,
            thumbnail_url: format!("{}/{}", base, v.thumbnail_key),
        })
        .collect();

    Ok(Json(dtos))
}
/*
pub async fn purge_bucket(
    State(state): State<AppState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut continuation_token = None;

    loop {
        let list_resp = state
            .s3
            .list_objects_v2()
            .bucket(&state.bucket)
            .set_continuation_token(continuation_token)
            .send()
            .await
            .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

        if let Some(contents) = list_resp.contents {
            if !contents.is_empty() {
                let objects: Vec<ObjectIdentifier> = contents
                    .into_iter()
                    .filter_map(|o| {
                        o.key.and_then(|k| ObjectIdentifier::builder().key(k).build().ok())
                    })
                    .collect();

                if !objects.is_empty() {
                    // Delete in batches of 1000 (S3 limit)
                    for chunk in objects.chunks(1000) {
                        let delete = Delete::builder()
                            .set_objects(Some(chunk.to_vec()))
                            .build()
                            .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

                        state
                            .s3
                            .delete_objects()
                            .bucket(&state.bucket)
                            .delete(delete)
                            .send()
                            .await
                            .map_err(|e| internal_err(anyhow::anyhow!(e)))?;
                    }
                }
            }
        }

        if list_resp.is_truncated.unwrap_or(false) {
            continuation_token = list_resp.next_continuation_token;
        } else {
            break;
        }
    }

    clear_database(&state.db_pool)
        .await
        .map_err(|e| internal_err(e))?;

    Ok(StatusCode::OK)
}
*/
fn internal_err(e: anyhow::Error) -> (axum::http::StatusCode, String) {
    error!(error = ?e, "internal error");
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "internal server error".to_string(),
    )
}

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

// Helper to generate a signed token
fn generate_token(video_id: &str, secret: &str, ip: &str, user_agent: &str) -> String {
    // Token valid for 1 hour (3600 seconds)
    let expiration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;

    // Use ASCII Unit Separator (\x1F) as delimiter to avoid ambiguity with colons
    // that commonly appear in User-Agent strings (e.g., "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
    let payload = format!("{}\x1F{}\x1F{}\x1F{}", video_id, expiration, ip, user_agent);

    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    let signature = hex::encode(result.into_bytes());

    format!("{}:{}", expiration, signature)
}

// Helper to verify a signed token
fn verify_token(video_id: &str, token: &str, secret: &str, ip: &str, user_agent: &str) -> bool {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() != 2 {
        return false;
    }

    let expiration_str = parts[0];
    let signature = parts[1];

    // Check expiration
    let expiration: u64 = match expiration_str.parse() {
        Ok(ts) => ts,
        Err(_) => return false,
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if now > expiration {
        return false;
    }

    // Verify signature
    let payload = format!("{}\x1F{}\x1F{}\x1F{}", video_id, expiration, ip, user_agent);
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());

    let expected_signature = hex::encode(mac.finalize().into_bytes());

    expected_signature == signature
}

pub async fn get_hls_file(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Path((id, file)): Path<(String, String)>,
) -> Result<Response, (StatusCode, String)> {
    let key = format!("{}/{}", id, file);

    // Verify token for ALL HLS files (.m3u8, .ts, .vtt, .srt)
    if file.ends_with(".m3u8")
        || file.ends_with(".ts")
        || file.ends_with(".vtt")
        || file.ends_with(".srt")
    {
        // Extract token from Cookie header
        let cookie_header = headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let mut token = "";
        for cookie in cookie_header.split(';') {
            let cookie = cookie.trim();
            if let Some(val) = cookie.strip_prefix("token=") {
                token = val;
                break;
            }
        }

        // Try to get the real client IP from X-Forwarded-For header, fallback to addr.ip()
        let ip = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|xff| xff.split(',').next().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| addr.ip().to_string());

        // Extract User-Agent header
        let user_agent = headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !verify_token(&id, token, &state.config.server.secret_key, &ip, user_agent) {
            return Err((
                StatusCode::FORBIDDEN,
                "Access denied: Invalid or expired token".to_string(),
            ));
        }
    }

    // Fetch content from S3 for all file types (Proxy)
    let content = state
        .s3
        .get_object()
        .bucket(&state.config.r2.bucket)
        .key(&key)
        .send()
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    // Load content into memory (simplifies type handling for ByteStream)
    let body_bytes = content
        .body
        .collect()
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?
        .into_bytes();

    let body = axum::body::Body::from(body_bytes);

    // Determine Content-Type
    let content_type = if file.ends_with(".m3u8") {
        "application/vnd.apple.mpegurl"
    } else if file.ends_with(".ts") {
        "video/mp2t"
    } else if file.ends_with(".vtt") {
        "text/vtt"
    } else if file.ends_with(".srt") {
        "text/plain" // or application/x-subrip
    } else {
        "application/octet-stream"
    };

    Ok(([(header::CONTENT_TYPE, content_type)], body).into_response())
}

pub async fn get_player(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let ip = addr.ip().to_string();
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Increment view count on page load
    // Note: This is a simple implementation. In production, you'd want to debounce this
    // or use a more sophisticated method to prevent abuse.
    let _ = crate::clickhouse::insert_view(&state.clickhouse, &id, &ip, user_agent).await;

    // Generate token
    let token = generate_token(&id, &state.config.server.secret_key, &ip, user_agent);

    let js_code = format!(
        r#"
        async function init() {{
            const video = document.getElementById('video');
            const ui = video['ui'];
            const controls = ui.getControls();
            const player = controls.getPlayer();
            const config = {{
                'controlPanelElements': ['play_pause', 'time_and_duration', 'spacer', 'mute', 'volume', 'fullscreen', 'overflow_menu'],
                'overflowMenuButtons': ['quality', 'playback_rate', 'captions', 'picture_in_picture', 'cast'],
                'seekBarColors': {{
                    base: 'rgba(255, 255, 255, 0.3)',
                    buffered: 'rgba(255, 255, 255, 0.54)',
                    played: 'rgb(255, 0, 0)',
                }}
            }};
            
            ui.configure(config);
            window.player = player;
            window.ui = ui;
            player.addEventListener('error', onErrorEvent);

            try {{
                await player.load('/hls/{}/index.m3u8');
                startHeartbeat();
            }} catch (e) {{
                onError(e);
            }}
        }}

        function startHeartbeat() {{
            setInterval(() => {{
                fetch('/api/videos/{}/heartbeat', {{ method: 'POST' }});
            }}, 10000);
        }}

        function onErrorEvent(event) {{
            onError(event.detail);
        }}

        function onError(error) {{
            console.error('Error code', error.code, 'object', error);
        }}

        document.addEventListener('shaka-ui-loaded', init);
        document.addEventListener('shaka-ui-load-failed', initFailed);

        function initFailed() {{
            console.error('Unable to load the UI library!');
        }}
        "#,
        id, id
    );

    let session = Session::new();
    let mut out = Vec::new();
    minify(&session, TopLevelMode::Global, js_code.as_bytes(), &mut out).unwrap();
    let minified_js = String::from_utf8(out).unwrap();

    let html = format!(
        r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Video Player</title>
    <!-- Shaka Player UI CSS -->
    <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/shaka-player/4.7.11/controls.min.css" />
    <style>
        body, html {{ margin: 0; padding: 0; width: 100%; height: 100%; background: #000; overflow: hidden; }}
        #video-container {{ width: 100%; height: 100%; }}
        #video {{ width: 100%; height: 100%; }}
    </style>
    <!-- Shaka Player UI JS -->
    <script src="https://cdnjs.cloudflare.com/ajax/libs/shaka-player/4.7.11/shaka-player.ui.min.js"></script>
</head>
<body>
    <div id="video-container" data-shaka-player-container>
        <video id="video" autoplay data-shaka-player></video>
    </div>
    <script>{}</script>
</body>
</html>
"#,
        minified_js
    );

    // Determine cookie attributes based on protocol
    let is_https = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|proto| proto == "https")
        .unwrap_or(false);

    let cookie_attr = if is_https {
        "SameSite=None; Secure"
    } else {
        "SameSite=Lax"
    };

    // Set cookie
    let cookie = format!(
        "token={}; Path=/; HttpOnly; Max-Age=3600; {}",
        token, cookie_attr
    );

    ([(header::SET_COOKIE, cookie)], Html(html))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_verification_success() {
        let secret = "my_secret_key";
        let video_id = "video123";
        let ip = "127.0.0.1";
        let ua = "Mozilla/5.0";

        let token = generate_token(video_id, secret, ip, ua);
        assert!(verify_token(video_id, &token, secret, ip, ua));
    }

    #[test]
    fn test_token_verification_fail_wrong_ip() {
        let secret = "my_secret_key";
        let video_id = "video123";
        let ip = "127.0.0.1";
        let ua = "Mozilla/5.0";

        let token = generate_token(video_id, secret, ip, ua);
        assert!(!verify_token(video_id, &token, secret, "192.168.1.1", ua));
    }

    #[test]
    fn test_token_verification_fail_wrong_ua() {
        let secret = "my_secret_key";
        let video_id = "video123";
        let ip = "127.0.0.1";
        let ua = "Mozilla/5.0";

        let token = generate_token(video_id, secret, ip, ua);
        assert!(!verify_token(video_id, &token, secret, ip, "curl/7.68.0"));
    }

    #[test]
    fn test_token_verification_fail_wrong_secret() {
        let secret = "my_secret_key";
        let video_id = "video123";
        let ip = "127.0.0.1";
        let ua = "Mozilla/5.0";

        let token = generate_token(video_id, secret, ip, ua);
        assert!(!verify_token(video_id, &token, "wrong_secret", ip, ua));
    }

    #[test]
    fn test_token_verification_expired() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        use std::time::{SystemTime, UNIX_EPOCH};

        // Manual token construction with expired time
        let secret = "my_secret_key";
        let video_id = "video123";
        let ip = "127.0.0.1";
        let ua = "Mozilla/5.0";

        let expiration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 100; // Expired

        let payload = format!("{}\x1F{}\x1F{}\x1F{}", video_id, expiration, ip, ua);
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(payload.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());
        let token = format!("{}:{}", expiration, signature);

        assert!(!verify_token(video_id, &token, secret, ip, ua));
    }
}
