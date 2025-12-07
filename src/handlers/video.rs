use crate::clickhouse;
use crate::database::{
    count_videos, delete_videos as db_delete_videos, get_video_ids_with_prefix,
    list_videos as db_list_videos, update_video as db_update_video,
};
use crate::handlers::common::internal_err;
use crate::types::{AppState, VideoListResponse, VideoQuery};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use std::collections::HashMap;
use tracing::info;

#[derive(serde::Deserialize)]
pub struct UpdateVideoRequest {
    pub name: String,
    pub tags: Vec<String>,
}

#[derive(serde::Deserialize)]
pub struct DeleteVideosRequest {
    pub ids: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct DeleteVideosResponse {
    pub deleted: u64,
    pub message: String,
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
        .map_err(internal_err)?;

    let items = db_list_videos(
        &state.db_pool,
        &filters,
        page,
        page_size,
        &state.config.r2.public_base_url,
        &HashMap::new(), // View counts are fetched separately from ClickHouse below
    )
    .await
    .map_err(internal_err)?;

    // Uses safe version - returns empty map if ClickHouse is unavailable
    let video_ids: Vec<String> = items.iter().map(|v| v.id.clone()).collect();
    let view_counts = clickhouse::get_view_counts_safe(&state.clickhouse, &video_ids).await;

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

pub async fn delete_videos(
    State(state): State<AppState>,
    Json(body): Json<DeleteVideosRequest>,
) -> Result<Json<DeleteVideosResponse>, (StatusCode, String)> {
    if body.ids.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No video IDs provided".to_string()));
    }

    info!("Deleting {} videos: {:?}", body.ids.len(), body.ids);

    // First, verify videos exist and get their IDs (also acts as validation)
    let existing_ids = get_video_ids_with_prefix(&state.db_pool, &body.ids)
        .await
        .map_err(internal_err)?;

    if existing_ids.is_empty() {
        return Err((StatusCode::NOT_FOUND, "No videos found".to_string()));
    }

    // Delete from R2 storage (each video has a folder with its ID as prefix)
    for video_id in &existing_ids {
        let prefix = format!("{}/", video_id);

        // List all objects with this prefix
        let mut continuation_token: Option<String> = None;
        loop {
            let list_resp = state
                .s3
                .list_objects_v2()
                .bucket(&state.config.r2.bucket)
                .prefix(&prefix)
                .set_continuation_token(continuation_token.clone())
                .send()
                .await
                .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

            if let Some(contents) = list_resp.contents {
                for obj in contents {
                    if let Some(key) = obj.key {
                        state
                            .s3
                            .delete_object()
                            .bucket(&state.config.r2.bucket)
                            .key(&key)
                            .send()
                            .await
                            .map_err(|e| internal_err(anyhow::anyhow!(e)))?;
                        info!("Deleted from R2: {}", key);
                    }
                }
            }

            if list_resp.is_truncated.unwrap_or(false) {
                continuation_token = list_resp.next_continuation_token;
            } else {
                break;
            }
        }
    }

    // Delete from database
    let deleted = db_delete_videos(&state.db_pool, &existing_ids)
        .await
        .map_err(internal_err)?;

    Ok(Json(DeleteVideosResponse {
        deleted,
        message: format!("Successfully deleted {} video(s)", deleted),
    }))
}

pub async fn update_video(
    State(state): State<AppState>,
    Path(video_id): Path<String>,
    Json(body): Json<UpdateVideoRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    db_update_video(&state.db_pool, &video_id, &body.name, &body.tags)
        .await
        .map_err(|e| {
            if e.to_string().contains("Video not found") {
                (StatusCode::NOT_FOUND, "Video not found".to_string())
            } else {
                internal_err(e)
            }
        })?;

    Ok(StatusCode::OK)
}
