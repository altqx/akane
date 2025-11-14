use crate::database::save_video;
use crate::storage::upload_hls_to_r2;
use crate::types::{AppState, ProgressResponse, ProgressUpdate, UploadResponse};
use crate::video::{encode_to_hls, get_variants_for_height, get_video_duration, get_video_height};
use axum::{
    Json,
    extract::{Multipart, Path, State},
};
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
    let playlist_key = upload_hls_to_r2(&state, &hls_dir, &prefix)
        .await
        .map_err(|e| internal_err(e))?;

    // Build public URL (you should front this with your CDN/domain)
    let playlist_url = format!(
        "{}/{}",
        state.public_base_url.trim_end_matches('/'),
        playlist_key
    );

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

    Ok(Json(UploadResponse {
        playlist_url,
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

fn internal_err(e: anyhow::Error) -> (axum::http::StatusCode, String) {
    error!(error = ?e, "internal error");
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "internal server error".to_string(),
    )
}
