use crate::database::{count_videos, list_videos as db_list_videos, save_video};
use crate::storage::upload_hls_to_r2;
use crate::types::{
    AppState, ProgressResponse, ProgressUpdate, UploadResponse, VideoListResponse, VideoQuery,
};
use crate::video::{encode_to_hls, get_variants_for_height, get_video_duration, get_video_height};
// use aws_sdk_s3::types::{Delete, ObjectIdentifier};
use axum::{
    Json,
    extract::{ConnectInfo, Multipart, Path, Query, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use minify_js::{Session, TopLevelMode, minify};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::{fs, io::AsyncWriteExt};
use tracing::error;
use uuid::Uuid;

pub async fn upload_video(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, (axum::http::StatusCode, String)> {
    let mut video_path: Option<PathBuf> = None;
    let mut video_name: Option<String> = None;
    let mut tags: Vec<String> = Vec::new();

    while let Some(field) = multipart
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

                let mut bytes = field
                    .bytes()
                    .await
                    .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

                file.write_all_buf(&mut bytes)
                    .await
                    .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

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

    // Create a unique upload ID for progress tracking
    let upload_id = Uuid::new_v4().to_string();

    // Initialize progress
    let initial_progress = ProgressUpdate {
        stage: "Uploading to server".to_string(),
        current_chunk: 0,
        total_chunks: 1,
        percentage: 0,
    };
    state
        .progress
        .write()
        .await
        .insert(upload_id.clone(), initial_progress);

    // Encode to HLS (playlist + segments) into a temp directory
    let output_id = Uuid::new_v4().to_string();
    let hls_dir = std::env::temp_dir().join(format!("hls-{}", &output_id));
    fs::create_dir_all(&hls_dir)
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    // Get video metadata before encoding (parallel)
    let (video_duration, original_height) = tokio::join!(
        get_video_duration(&video_path),
        get_video_height(&video_path)
    );
    let video_duration = video_duration.map_err(|e| internal_err(e))?;
    let original_height = original_height.map_err(|e| internal_err(e))?;
    let variants = get_variants_for_height(original_height);
    let available_resolutions: Vec<String> = variants.iter().map(|v| v.label.clone()).collect();

    // Update progress: FFmpeg processing stage
    let encoding_progress = ProgressUpdate {
        stage: "FFmpeg processing".to_string(),
        current_chunk: 0,
        total_chunks: variants.len() as u32,
        percentage: 0,
    };
    state
        .progress
        .write()
        .await
        .insert(upload_id.clone(), encoding_progress);

    encode_to_hls(&video_path, &hls_dir, &state.progress, &upload_id)
        .await
        .map_err(|e| internal_err(e))?;

    // Update progress: Upload to R2 stage
    let upload_progress = ProgressUpdate {
        stage: "Upload to R2".to_string(),
        current_chunk: 0,
        total_chunks: 1,
        percentage: 0,
    };
    state
        .progress
        .write()
        .await
        .insert(upload_id.clone(), upload_progress);

    // Upload HLS to R2
    let prefix = format!("{}/", output_id);
    // Build public URL (pointing to our proxy)
    let playlist_key = upload_hls_to_r2(&state, &hls_dir, &prefix)
        .await
        .map_err(|e| internal_err(e))?;

    // Save to database
    let thumbnail_key = format!("{}/thumbnail.jpg", output_id);
    let entrypoint = playlist_key.clone();

    save_video(
        &state.db_pool,
        &output_id,
        &video_name,
        &tags,
        &available_resolutions,
        video_duration,
        &thumbnail_key,
        &entrypoint,
    )
    .await
    .map_err(|e| internal_err(e))?;

    // Cleanup (ignore errors)
    let _ = fs::remove_file(&video_path).await;
    let _ = fs::remove_dir_all(&hls_dir).await;

    // Return player URL
    let player_url = format!("/player/{}", output_id);

    Ok(Json(UploadResponse {
        player_url,
        upload_id,
    }))
}

pub async fn get_progress(
    State(state): State<AppState>,
    Path(upload_id): Path<String>,
) -> Json<Option<ProgressResponse>> {
    let progress_map = state.progress.read().await;
    let progress = progress_map.get(&upload_id).cloned();
    Json(progress.map(|p| ProgressResponse {
        stage: p.stage,
        current_chunk: p.current_chunk,
        total_chunks: p.total_chunks,
        percentage: p.percentage,
    }))
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
        &state.public_base_url,
    )
    .await
    .map_err(|e| internal_err(e))?;

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

    let payload = format!("{}:{}:{}:{}", video_id, expiration, ip, user_agent);

    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    let signature = hex::encode(result.into_bytes());

    format!("{}:{}", expiration, signature)
}

// Helper to verify a signed token
fn verify_token(
    video_id: &str,
    token: &str,
    secret: &str,
    ip: &str,
    user_agent: &str,
) -> bool {
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
    let payload = format!("{}:{}:{}:{}", video_id, expiration, ip, user_agent);
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

        let ip = addr.ip().to_string();
        let user_agent = headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !verify_token(&id, token, &state.secret_key, &ip, user_agent) {
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
        .bucket(&state.bucket)
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

    // Generate token
    let token = generate_token(&id, &state.secret_key, &ip, user_agent);

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
            }} catch (e) {{
                onError(e);
            }}
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
        id
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

    // Set cookie
    let cookie = format!(
        "token={}; Path=/; HttpOnly; Max-Age=3600; SameSite=Lax",
        token
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
            .as_secs() - 100; // Expired

        let payload = format!("{}:{}:{}:{}", video_id, expiration, ip, ua);
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
        mac.update(payload.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());
        let token = format!("{}:{}", expiration, signature);

        assert!(!verify_token(video_id, &token, secret, ip, ua));
    }
}
