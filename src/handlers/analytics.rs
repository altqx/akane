use crate::clickhouse;
use crate::types::AppState;
use crate::handlers::common::internal_err;

use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{
        sse::{Event, Sse},
    },
};
use futures::stream::Stream;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;
use tracing::{error, info};

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

pub async fn track_view(
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

    // Insert view into ClickHouse
    match crate::clickhouse::insert_view(&state.clickhouse, &video_id, &ip, user_agent).await {
        Ok(_) => {
            info!("View tracked for video {} from {}", video_id, ip);
            StatusCode::OK
        }
        Err(e) => {
            error!("Failed to track view: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
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
        .map_err(internal_err)?;
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
    let mut videos =
        crate::database::get_all_videos_summary(&state.db_pool, &HashMap::new(), Some(100))
            .await
            .map_err(internal_err)?;

    let video_ids: Vec<String> = videos.iter().map(|v| v.id.clone()).collect();
    let view_counts = clickhouse::get_view_counts(&state.clickhouse, &video_ids)
        .await
        .map_err(internal_err)?;

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
