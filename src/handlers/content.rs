use crate::database::{
    get_attachment_by_filename, get_attachments_for_video, get_chapters_for_video,
    get_subtitle_by_track, get_subtitles_for_video,
};
use crate::types::{AppState, AttachmentListResponse, ChapterListResponse, SubtitleListResponse};
use crate::handlers::common::{internal_err, verify_token};

use axum::{
    Json,
    body::Body,
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use std::net::SocketAddr;
use tracing::error;

#[derive(serde::Deserialize)]
pub struct TokenQuery {
    pub token: Option<String>,
}

pub async fn get_video_subtitles(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(video_id): Path<String>,
    Query(query): Query<TokenQuery>,
) -> Result<Json<SubtitleListResponse>, (StatusCode, String)> {
    // Extract token from Cookie header or query parameter
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let mut token = query.token.as_deref().unwrap_or("");
    if token.is_empty() {
        for cookie in cookie_header.split(';') {
            let cookie = cookie.trim();
            if let Some(val) = cookie.strip_prefix("token=") {
                token = val;
                break;
            }
        }
    }

    // Extract client IP from X-Forwarded-For header, fallback to addr.ip()
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

    if !verify_token(&video_id, token, &state.config.server.secret_key, &ip, user_agent) {
        error!(
            video_id = %video_id,
            ip = %ip,
            "Subtitle list access denied: invalid or expired token"
        );
        return Err((
            StatusCode::FORBIDDEN,
            "Access denied: Invalid or expired token".to_string(),
        ));
    }

    let subtitles = get_subtitles_for_video(&state.db_pool, &video_id)
        .await
        .map_err(internal_err)?;

    Ok(Json(SubtitleListResponse { subtitles }))
}

pub async fn get_subtitle_file(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((video_id, track_with_ext)): Path<(String, String)>,
    Query(query): Query<TokenQuery>,
) -> Result<Response, (StatusCode, String)> {
    // Extract token from Cookie header or query parameter
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let mut token = query.token.as_deref().unwrap_or("");
    if token.is_empty() {
        for cookie in cookie_header.split(';') {
            let cookie = cookie.trim();
            if let Some(val) = cookie.strip_prefix("token=") {
                token = val;
                break;
            }
        }
    }

    // Extract client IP from X-Forwarded-For header, fallback to addr.ip()
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

    if !verify_token(&video_id, token, &state.config.server.secret_key, &ip, user_agent) {
        error!(
            video_id = %video_id,
            track = %track_with_ext,
            ip = %ip,
            "Subtitle file access denied: invalid or expired token"
        );
        return Err((
            StatusCode::FORBIDDEN,
            "Access denied: Invalid or expired token".to_string(),
        ));
    }

    // Parse track index from "0.ass" or "1.srt" format
    let track_index: i32 = track_with_ext
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Invalid track format".to_string()))?;

    let subtitle = get_subtitle_by_track(&state.db_pool, &video_id, track_index)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Subtitle not found".to_string()))?;

    // Fetch from R2
    let content = state
        .s3
        .get_object()
        .bucket(&state.config.r2.bucket)
        .key(&subtitle.storage_key)
        .send()
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    let reader = content.body.into_async_read();
    let stream = tokio_util::io::ReaderStream::new(reader);
    let body_stream = stream.map(|result| result.map_err(std::io::Error::other));
    let body = Body::from_stream(body_stream);

    // Determine content type based on codec
    let content_type = match subtitle.codec.as_str() {
        "ass" | "ssa" => "text/x-ssa",
        "subrip" | "srt" => "text/plain",
        _ => "text/plain",
    };

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        ],
        body,
    )
        .into_response())
}

pub async fn get_video_attachments(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(video_id): Path<String>,
    Query(query): Query<TokenQuery>,
) -> Result<Json<AttachmentListResponse>, (StatusCode, String)> {
    // Extract token from Cookie header or query parameter
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let mut token = query.token.as_deref().unwrap_or("");
    if token.is_empty() {
        for cookie in cookie_header.split(';') {
            let cookie = cookie.trim();
            if let Some(val) = cookie.strip_prefix("token=") {
                token = val;
                break;
            }
        }
    }

    // Extract client IP from X-Forwarded-For header, fallback to addr.ip()
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

    if !verify_token(&video_id, token, &state.config.server.secret_key, &ip, user_agent) {
        error!(
            video_id = %video_id,
            ip = %ip,
            "Attachment list access denied: invalid or expired token"
        );
        return Err((
            StatusCode::FORBIDDEN,
            "Access denied: Invalid or expired token".to_string(),
        ));
    }

    let attachments = get_attachments_for_video(&state.db_pool, &video_id)
        .await
        .map_err(internal_err)?;

    Ok(Json(AttachmentListResponse { attachments }))
}

pub async fn get_attachment_file(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((video_id, filename)): Path<(String, String)>,
    Query(query): Query<TokenQuery>,
) -> Result<Response, (StatusCode, String)> {
    // Extract token from Cookie header or query parameter
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let mut token = query.token.as_deref().unwrap_or("");
    if token.is_empty() {
        for cookie in cookie_header.split(';') {
            let cookie = cookie.trim();
            if let Some(val) = cookie.strip_prefix("token=") {
                token = val;
                break;
            }
        }
    }

    // Extract client IP from X-Forwarded-For header, fallback to addr.ip()
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

    if !verify_token(&video_id, token, &state.config.server.secret_key, &ip, user_agent) {
        error!(
            video_id = %video_id,
            filename = %filename,
            ip = %ip,
            "Attachment file access denied: invalid or expired token"
        );
        return Err((
            StatusCode::FORBIDDEN,
            "Access denied: Invalid or expired token".to_string(),
        ));
    }

    let attachment = get_attachment_by_filename(&state.db_pool, &video_id, &filename)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Attachment not found".to_string()))?;

    // Fetch from R2
    let content = state
        .s3
        .get_object()
        .bucket(&state.config.r2.bucket)
        .key(&attachment.storage_key)
        .send()
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    let reader = content.body.into_async_read();
    let stream = tokio_util::io::ReaderStream::new(reader);
    let body_stream = stream.map(|result| result.map_err(std::io::Error::other));
    let body = Body::from_stream(body_stream);

    Ok((
        [
            (header::CONTENT_TYPE, attachment.mimetype.as_str()),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            (header::CACHE_CONTROL, "public, max-age=31536000"), // Cache fonts for 1 year
        ],
        body,
    )
        .into_response())
}

pub async fn get_video_chapters(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(video_id): Path<String>,
    Query(query): Query<TokenQuery>,
) -> Result<Json<ChapterListResponse>, (StatusCode, String)> {
    // Extract token from Cookie header or query parameter
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let mut token = query.token.as_deref().unwrap_or("");
    if token.is_empty() {
        for cookie in cookie_header.split(';') {
            let cookie = cookie.trim();
            if let Some(val) = cookie.strip_prefix("token=") {
                token = val;
                break;
            }
        }
    }

    // Extract client IP from X-Forwarded-For header, fallback to addr.ip()
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

    if !verify_token(&video_id, token, &state.config.server.secret_key, &ip, user_agent) {
        error!(
            video_id = %video_id,
            ip = %ip,
            "Chapter list access denied: invalid or expired token"
        );
        return Err((
            StatusCode::FORBIDDEN,
            "Access denied: Invalid or expired token".to_string(),
        ));
    }

    let chapters = get_chapters_for_video(&state.db_pool, &video_id)
        .await
        .map_err(internal_err)?;

    Ok(Json(ChapterListResponse { chapters }))
}

pub async fn get_jassub_worker(
    Path(filename): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    // Only allow specific JASSUB files
    let url = match filename.as_str() {
        "jassub-worker.js" => "https://cdn.jsdelivr.net/npm/jassub/dist/jassub-worker.js",
        "jassub-worker.wasm" => "https://cdn.jsdelivr.net/npm/jassub/dist/jassub-worker.wasm",
        _ => return Err((StatusCode::NOT_FOUND, "File not found".to_string())),
    };

    // Fetch from CDN
    let response = reqwest::get(url)
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    let content_type = if filename.ends_with(".wasm") {
        "application/wasm"
    } else {
        "application/javascript"
    };

    let bytes = response
        .bytes()
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=86400"), // Cache for 1 day
        ],
        bytes.to_vec(),
    )
        .into_response())
}
