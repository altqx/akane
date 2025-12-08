use crate::database::{save_attachment, save_chapter, save_subtitle, save_video};
use crate::handlers::common::{internal_err, now_millis};
use crate::storage::upload_hls_to_r2;
use crate::types::{
    AppState, ChunkUploadResponse, ChunkedUpload, FinalizeUploadRequest, ProgressMap,
    ProgressResponse, ProgressUpdate, QueueItem, QueueListResponse, UploadAccepted, UploadResponse,
};
use crate::video::{
    encode_to_hls, extract_all_attachments, extract_subtitle, get_attachments, get_audio_streams,
    get_chapters, get_subtitle_streams, get_variants_for_height, get_video_duration,
    get_video_height,
};

use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, Sse},
};
use futures::stream::Stream;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::{fs, io::AsyncReadExt, io::AsyncWriteExt};
use tracing::{error, info, warn};
use uuid::Uuid;

// Stale upload timeout: 30 minutes of inactivity
const STALE_UPLOAD_TIMEOUT_MS: u64 = 30 * 60 * 1000;

/// Clean up stale chunked uploads that have been inactive for too long
async fn cleanup_stale_uploads(state: &AppState) {
    let now = now_millis();
    let mut to_remove = Vec::new();

    {
        let uploads = state.chunked_uploads.read().await;
        for (id, upload) in uploads.iter() {
            if now.saturating_sub(upload.last_activity) > STALE_UPLOAD_TIMEOUT_MS {
                to_remove.push((id.clone(), upload.temp_dir.clone()));
            }
        }
    }

    if !to_remove.is_empty() {
        let mut uploads = state.chunked_uploads.write().await;
        let mut progress_map = state.progress.write().await;

        for (id, temp_dir) in to_remove {
            uploads.remove(&id);
            progress_map.remove(&id);
            let _ = fs::remove_dir_all(&temp_dir).await;
            warn!("Cleaned up stale chunked upload: {}", id);
        }
    }
}

async fn update_progress(progress_map: &ProgressMap, upload_id: &str, mut update: ProgressUpdate) {
    let mut map = progress_map.write().await;
    if let Some(existing) = map.get(upload_id) {
        update.created_at = existing.created_at;
    }
    map.insert(upload_id.to_string(), update);
}

pub async fn upload_video(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<UploadAccepted>, (StatusCode, String)> {
    let mut video_path: Option<PathBuf> = None;
    let mut video_name: Option<String> = None;
    let mut tags: Vec<String> = Vec::new();

    let upload_id = headers
        .get("X-Upload-ID")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

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
            video_name: None,
            created_at: now_millis(),
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
                            video_name: None,
                            created_at: 0,
                        };
                        update_progress(&state.progress, &upload_id, progress_update).await;
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
            StatusCode::BAD_REQUEST,
            "missing file field 'file'".to_string(),
        )
    })?;

    let video_name =
        video_name.ok_or_else(|| (StatusCode::BAD_REQUEST, "missing field 'name'".to_string()))?;

    let initial_progress = ProgressUpdate {
        stage: "Queued for processing".to_string(),
        current_chunk: 0,
        total_chunks: 1,
        percentage: 0,
        details: None,
        status: "processing".to_string(),
        result: None,
        error: None,
        video_name: Some(video_name.clone()),
        created_at: 0,
    };
    update_progress(&state.progress, &upload_id, initial_progress).await;

    let state_clone = state.clone();
    let upload_id_clone = upload_id.clone();
    let video_path_clone = video_path.clone();
    let video_name_clone = video_name.clone();
    let tags_clone = tags.clone();

    tokio::spawn(async move {
        let result = async {
            let output_id = Uuid::new_v4().to_string();
            let hls_dir = std::env::temp_dir().join(format!("hls-{}", &output_id));
            fs::create_dir_all(&hls_dir)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;

            let (video_duration, original_height) = tokio::join!(
                get_video_duration(&video_path_clone),
                get_video_height(&video_path_clone)
            );
            let video_duration = video_duration?;
            let original_height = original_height?;
            let variants = get_variants_for_height(original_height);
            let available_resolutions: Vec<String> =
                variants.iter().map(|v| v.label.clone()).collect();

            let encoding_progress = ProgressUpdate {
                stage: "FFmpeg processing".to_string(),
                current_chunk: 0,
                total_chunks: variants.len() as u32,
                percentage: 0,
                details: Some("Starting encoding...".to_string()),
                status: "processing".to_string(),
                result: None,
                error: None,
                video_name: Some(video_name_clone.clone()),
                created_at: 0,
            };
            update_progress(&state_clone.progress, &upload_id_clone, encoding_progress).await;

            // Get audio streams for multi-audio encoding
            let audio_streams = get_audio_streams(&video_path_clone)
                .await
                .unwrap_or_default();

            encode_to_hls(
                &video_path_clone,
                &hls_dir,
                &state_clone.progress,
                &upload_id_clone,
                state_clone.ffmpeg_semaphore.clone(),
                &state_clone.config.video.encoder,
                video_duration,
                &audio_streams,
            )
            .await?;

            // Extract subtitles and attachments from the source video
            let subtitle_streams = get_subtitle_streams(&video_path_clone)
                .await
                .unwrap_or_default();
            let attachment_streams = get_attachments(&video_path_clone).await.unwrap_or_default();

            // Create directories for subtitles and fonts
            let subtitles_dir = hls_dir.join("subtitles");
            let fonts_dir = hls_dir.join("fonts");

            if !subtitle_streams.is_empty() {
                fs::create_dir_all(&subtitles_dir).await?;
            }
            if !attachment_streams.is_empty() {
                fs::create_dir_all(&fonts_dir).await?;
                // Extract all font attachments
                extract_all_attachments(&video_path_clone, &fonts_dir).await?;
            }

            // Extract each subtitle stream
            for (idx, sub) in subtitle_streams.iter().enumerate() {
                let ext = match sub.codec_name.as_str() {
                    "ass" | "ssa" => "ass",
                    "subrip" | "srt" => "srt",
                    _ => "ass", // Default to ASS
                };
                let sub_filename = format!("track_{}.{}", idx, ext);
                let sub_path = subtitles_dir.join(&sub_filename);

                // Use enumerate index (idx) as relative subtitle stream index
                if let Err(e) =
                    extract_subtitle(&video_path_clone, idx as i32, &sub_path, &sub.codec_name)
                        .await
                {
                    error!(
                        "Failed to extract subtitle stream {} (track {}): {}",
                        sub.stream_index, idx, e
                    );
                }
            }

            let upload_progress = ProgressUpdate {
                stage: "Upload to R2".to_string(),
                current_chunk: 0,
                total_chunks: 1,
                percentage: 0,
                details: Some("Uploading segments to storage...".to_string()),
                status: "processing".to_string(),
                result: None,
                error: None,
                video_name: Some(video_name_clone.clone()),
                created_at: 0,
            };
            update_progress(&state_clone.progress, &upload_id_clone, upload_progress).await;

            let prefix = format!("{}/", output_id);
            let playlist_key =
                upload_hls_to_r2(&state_clone, &hls_dir, &prefix, Some(&upload_id_clone)).await?;

            let thumbnail_key = format!("{}/thumbnail.jpg", output_id);
            let sprites_key = format!("{}/sprites.jpg", output_id);
            let entrypoint = playlist_key.clone();

            save_video(
                &state_clone.db_pool,
                &output_id,
                &video_name_clone,
                &tags_clone,
                &available_resolutions,
                video_duration,
                &thumbnail_key,
                &sprites_key,
                &entrypoint,
            )
            .await?;

            // Save subtitle metadata to database
            for (idx, sub) in subtitle_streams.iter().enumerate() {
                let ext = match sub.codec_name.as_str() {
                    "ass" | "ssa" => "ass",
                    "subrip" | "srt" => "srt",
                    _ => "ass",
                };
                let storage_key = format!("{}/subtitles/track_{}.{}", output_id, idx, ext);

                if let Err(e) = save_subtitle(
                    &state_clone.db_pool,
                    &output_id,
                    idx as i32,
                    sub.language.as_deref(),
                    sub.title.as_deref(),
                    &sub.codec_name,
                    &storage_key,
                    None, // idx_storage_key for VobSub
                    sub.is_default,
                    sub.is_forced,
                )
                .await
                {
                    error!("Failed to save subtitle metadata for track {}: {}", idx, e);
                }
            }

            // Save attachment metadata to database
            for att in &attachment_streams {
                let storage_key = format!("{}/fonts/{}", output_id, att.filename);

                if let Err(e) = save_attachment(
                    &state_clone.db_pool,
                    &output_id,
                    &att.filename,
                    &att.mimetype,
                    &storage_key,
                )
                .await
                {
                    error!(
                        "Failed to save attachment metadata for {}: {}",
                        att.filename, e
                    );
                }
            }

            // Extract and save chapters from video
            let chapter_streams = get_chapters(&video_path_clone).await.unwrap_or_default();
            for (idx, chapter) in chapter_streams.iter().enumerate() {
                if let Err(e) = save_chapter(
                    &state_clone.db_pool,
                    &output_id,
                    idx as i32,
                    chapter.start_time,
                    chapter.end_time,
                    &chapter.title,
                )
                .await
                {
                    error!("Failed to save chapter metadata for index {}: {}", idx, e);
                }
            }

            let _ = fs::remove_file(&video_path_clone).await;
            let _ = fs::remove_dir_all(&hls_dir).await;

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
                    video_name: Some(video_name_clone.clone()),
                    created_at: 0,
                };
                update_progress(&state_clone.progress, &upload_id_clone, completion_progress).await;
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
                    video_name: Some(video_name_clone.clone()),
                    created_at: 0,
                };
                update_progress(&state_clone.progress, &upload_id_clone, error_progress).await;
            }
        }

        tokio::time::sleep(Duration::from_secs(10)).await;
        let mut progress_map = state_clone.progress.write().await;
        if let Some(entry) = progress_map.get(&upload_id_clone)
            && (entry.status == "completed" || entry.status == "failed")
        {
            progress_map.remove(&upload_id_clone);
        }
    });

    Ok(Json(UploadAccepted {
        upload_id,
        message: "File uploaded successfully, processing started in background".to_string(),
    }))
}

pub async fn upload_chunk(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<ChunkUploadResponse>, (StatusCode, String)> {
    let upload_id = headers
        .get("X-Upload-ID")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "Missing X-Upload-ID header".to_string(),
            )
        })?;

    let mut chunk_data: Option<Vec<u8>> = None;
    let mut chunk_index: Option<u32> = None;
    let mut total_chunks: Option<u32> = None;
    let mut file_name: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?
    {
        let field_name = field.name().map(|s| s.to_string());

        match field_name.as_deref() {
            Some("chunk") => {
                chunk_data = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| internal_err(anyhow::anyhow!(e)))?
                        .to_vec(),
                );
            }
            Some("chunk_index") => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| internal_err(anyhow::anyhow!(e)))?;
                chunk_index =
                    Some(text.parse().map_err(|_| {
                        (StatusCode::BAD_REQUEST, "Invalid chunk_index".to_string())
                    })?);
            }
            Some("total_chunks") => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| internal_err(anyhow::anyhow!(e)))?;
                total_chunks =
                    Some(text.parse().map_err(|_| {
                        (StatusCode::BAD_REQUEST, "Invalid total_chunks".to_string())
                    })?);
            }
            Some("file_name") => {
                file_name = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| internal_err(anyhow::anyhow!(e)))?,
                );
            }
            _ => continue,
        }
    }

    let chunk_data =
        chunk_data.ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing chunk data".to_string()))?;
    let chunk_index =
        chunk_index.ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing chunk_index".to_string()))?;
    let total_chunks = total_chunks
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing total_chunks".to_string()))?;
    let file_name =
        file_name.ok_or_else(|| (StatusCode::BAD_REQUEST, "Missing file_name".to_string()))?;

    info!(
        "Received chunk {}/{} for upload {} (file: {})",
        chunk_index + 1,
        total_chunks,
        upload_id,
        file_name
    );

    let temp_dir = {
        let mut uploads = state.chunked_uploads.write().await;

        if !uploads.contains_key(&upload_id) {
            let temp_dir = std::env::temp_dir().join(format!("chunked-{}", upload_id));
            fs::create_dir_all(&temp_dir)
                .await
                .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

            uploads.insert(
                upload_id.clone(),
                ChunkedUpload {
                    file_name: file_name.clone(),
                    total_chunks,
                    received_chunks: vec![false; total_chunks as usize],
                    temp_dir: temp_dir.clone(),
                    last_activity: now_millis(),
                },
            );

            let progress = ProgressUpdate {
                stage: "Receiving chunks".to_string(),
                current_chunk: 0,
                total_chunks,
                percentage: 0,
                details: Some(format!("Receiving chunk 1 of {}", total_chunks)),
                status: "processing".to_string(),
                result: None,
                error: None,
                video_name: Some(file_name.replace(&['.'][..], "_")),
                created_at: now_millis(),
            };
            state
                .progress
                .write()
                .await
                .insert(upload_id.clone(), progress);
        }

        uploads.get(&upload_id).unwrap().temp_dir.clone()
    };

    let chunk_path = temp_dir.join(format!("chunk_{:06}", chunk_index));
    fs::write(&chunk_path, &chunk_data)
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    {
        let mut uploads = state.chunked_uploads.write().await;
        if let Some(upload) = uploads.get_mut(&upload_id) {
            upload.received_chunks[chunk_index as usize] = true;
            upload.last_activity = now_millis();
        }
    }

    // Periodically cleanup stale uploads (every ~10 chunks received)
    if chunk_index % 10 == 0 {
        let state_clone = state.clone();
        tokio::spawn(async move {
            cleanup_stale_uploads(&state_clone).await;
        });
    }

    let received_count = {
        let uploads = state.chunked_uploads.read().await;
        uploads
            .get(&upload_id)
            .map(|u| u.received_chunks.iter().filter(|&&r| r).count() as u32)
            .unwrap_or(0)
    };

    let progress = ProgressUpdate {
        stage: "Receiving chunks".to_string(),
        current_chunk: received_count,
        total_chunks,
        percentage: (received_count * 100) / total_chunks,
        details: Some(format!(
            "Received chunk {} of {}",
            received_count, total_chunks
        )),
        status: "processing".to_string(),
        result: None,
        error: None,
        video_name: Some(file_name.replace(&['.'][..], "_")),
        created_at: 0,
    };
    update_progress(&state.progress, &upload_id, progress).await;

    Ok(Json(ChunkUploadResponse {
        upload_id,
        chunk_index,
        received: true,
    }))
}

// Finalize chunked upload - assembles chunks and starts processing
pub async fn finalize_chunked_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<FinalizeUploadRequest>,
) -> Result<Json<UploadAccepted>, (StatusCode, String)> {
    let upload_id = headers
        .get("X-Upload-ID")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "Missing X-Upload-ID header".to_string(),
            )
        })?;

    info!("Finalizing chunked upload: {}", upload_id);

    let chunked_upload = {
        let mut uploads = state.chunked_uploads.write().await;
        uploads.remove(&upload_id).ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "Upload ID not found or already finalized".to_string(),
            )
        })?
    };

    if !chunked_upload.received_chunks.iter().all(|&r| r) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Not all chunks have been received".to_string(),
        ));
    }

    let progress = ProgressUpdate {
        stage: "Assembling file".to_string(),
        current_chunk: chunked_upload.total_chunks,
        total_chunks: chunked_upload.total_chunks,
        percentage: 100,
        details: Some("Assembling chunks into final file...".to_string()),
        status: "processing".to_string(),
        result: None,
        error: None,
        video_name: Some(body.name.clone()),
        created_at: 0,
    };
    update_progress(&state.progress, &upload_id, progress).await;
    let final_path =
        std::env::temp_dir().join(format!("{}-{}", Uuid::new_v4(), chunked_upload.file_name));
    let mut final_file = fs::File::create(&final_path)
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    for i in 0..chunked_upload.total_chunks {
        let chunk_path = chunked_upload.temp_dir.join(format!("chunk_{:06}", i));
        let mut chunk_file = fs::File::open(&chunk_path)
            .await
            .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

        let mut buffer = Vec::new();
        chunk_file
            .read_to_end(&mut buffer)
            .await
            .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

        final_file
            .write_all(&buffer)
            .await
            .map_err(|e| internal_err(anyhow::anyhow!(e)))?;
    }

    let _ = fs::remove_dir_all(&chunked_upload.temp_dir).await;

    let tags: Vec<String> = body
        .tags
        .map(|t| {
            t.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let video_name = body.name;

    let progress = ProgressUpdate {
        stage: "Queued for processing".to_string(),
        current_chunk: 0,
        total_chunks: 1,
        percentage: 0,
        details: None,
        status: "processing".to_string(),
        result: None,
        error: None,
        video_name: Some(video_name.clone()),
        created_at: 0,
    };
    update_progress(&state.progress, &upload_id, progress).await;
    let state_clone = state.clone();
    let upload_id_clone = upload_id.clone();
    let video_path_clone = final_path.clone();
    let video_name_clone = video_name.clone();
    let tags_clone = tags.clone();

    tokio::spawn(async move {
        let result = async {
            let output_id = Uuid::new_v4().to_string();
            let hls_dir = std::env::temp_dir().join(format!("hls-{}", &output_id));
            fs::create_dir_all(&hls_dir)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;

            let (video_duration, original_height) = tokio::join!(
                get_video_duration(&video_path_clone),
                get_video_height(&video_path_clone)
            );
            let video_duration = video_duration?;
            let original_height = original_height?;
            let variants = get_variants_for_height(original_height);
            let available_resolutions: Vec<String> =
                variants.iter().map(|v| v.label.clone()).collect();

            let encoding_progress = ProgressUpdate {
                stage: "FFmpeg processing".to_string(),
                current_chunk: 0,
                total_chunks: variants.len() as u32,
                percentage: 0,
                details: Some("Starting encoding...".to_string()),
                status: "processing".to_string(),
                result: None,
                error: None,
                video_name: Some(video_name_clone.clone()),
                created_at: 0,
            };
            update_progress(&state_clone.progress, &upload_id_clone, encoding_progress).await;

            // Get audio streams for multi-audio encoding
            let audio_streams = get_audio_streams(&video_path_clone)
                .await
                .unwrap_or_default();

            encode_to_hls(
                &video_path_clone,
                &hls_dir,
                &state_clone.progress,
                &upload_id_clone,
                state_clone.ffmpeg_semaphore.clone(),
                &state_clone.config.video.encoder,
                video_duration,
                &audio_streams,
            )
            .await?;

            // Extract subtitles and attachments from the source video
            let subtitle_streams = get_subtitle_streams(&video_path_clone)
                .await
                .unwrap_or_default();
            let attachment_streams = get_attachments(&video_path_clone).await.unwrap_or_default();

            // Create directories for subtitles and fonts
            let subtitles_dir = hls_dir.join("subtitles");
            let fonts_dir = hls_dir.join("fonts");

            if !subtitle_streams.is_empty() {
                fs::create_dir_all(&subtitles_dir).await?;
            }
            if !attachment_streams.is_empty() {
                fs::create_dir_all(&fonts_dir).await?;
                // Extract all font attachments
                extract_all_attachments(&video_path_clone, &fonts_dir).await?;
            }

            // Extract each subtitle stream
            for (idx, sub) in subtitle_streams.iter().enumerate() {
                let ext = match sub.codec_name.as_str() {
                    "ass" | "ssa" => "ass",
                    "subrip" | "srt" => "srt",
                    _ => "ass", // Default to ASS
                };
                let sub_filename = format!("track_{}.{}", idx, ext);
                let sub_path = subtitles_dir.join(&sub_filename);

                // Use enumerate index (idx) as relative subtitle stream index
                if let Err(e) =
                    extract_subtitle(&video_path_clone, idx as i32, &sub_path, &sub.codec_name)
                        .await
                {
                    error!(
                        "Failed to extract subtitle stream {} (track {}): {}",
                        sub.stream_index, idx, e
                    );
                }
            }

            let upload_progress = ProgressUpdate {
                stage: "Upload to R2".to_string(),
                current_chunk: 0,
                total_chunks: 1,
                percentage: 0,
                details: Some("Uploading segments to storage...".to_string()),
                status: "processing".to_string(),
                result: None,
                error: None,
                video_name: Some(video_name_clone.clone()),
                created_at: 0,
            };
            update_progress(&state_clone.progress, &upload_id_clone, upload_progress).await;

            let prefix = format!("{}/", output_id);
            let playlist_key =
                upload_hls_to_r2(&state_clone, &hls_dir, &prefix, Some(&upload_id_clone)).await?;

            let thumbnail_key = format!("{}/thumbnail.jpg", output_id);
            let sprites_key = format!("{}/sprites.jpg", output_id);
            let entrypoint = playlist_key.clone();

            save_video(
                &state_clone.db_pool,
                &output_id,
                &video_name_clone,
                &tags_clone,
                &available_resolutions,
                video_duration,
                &thumbnail_key,
                &sprites_key,
                &entrypoint,
            )
            .await?;

            // Save subtitle metadata to database
            for (idx, sub) in subtitle_streams.iter().enumerate() {
                let ext = match sub.codec_name.as_str() {
                    "ass" | "ssa" => "ass",
                    "subrip" | "srt" => "srt",
                    _ => "ass",
                };
                let storage_key = format!("{}/subtitles/track_{}.{}", output_id, idx, ext);

                if let Err(e) = save_subtitle(
                    &state_clone.db_pool,
                    &output_id,
                    idx as i32,
                    sub.language.as_deref(),
                    sub.title.as_deref(),
                    &sub.codec_name,
                    &storage_key,
                    None, // idx_storage_key for VobSub
                    sub.is_default,
                    sub.is_forced,
                )
                .await
                {
                    error!("Failed to save subtitle metadata for track {}: {}", idx, e);
                }
            }

            // Save attachment metadata to database
            for att in &attachment_streams {
                let storage_key = format!("{}/fonts/{}", output_id, att.filename);

                if let Err(e) = save_attachment(
                    &state_clone.db_pool,
                    &output_id,
                    &att.filename,
                    &att.mimetype,
                    &storage_key,
                )
                .await
                {
                    error!(
                        "Failed to save attachment metadata for {}: {}",
                        att.filename, e
                    );
                }
            }

            // Extract and save chapters from video
            let chapter_streams = get_chapters(&video_path_clone).await.unwrap_or_default();
            for (idx, chapter) in chapter_streams.iter().enumerate() {
                if let Err(e) = save_chapter(
                    &state_clone.db_pool,
                    &output_id,
                    idx as i32,
                    chapter.start_time,
                    chapter.end_time,
                    &chapter.title,
                )
                .await
                {
                    error!("Failed to save chapter metadata for index {}: {}", idx, e);
                }
            }

            let _ = fs::remove_file(&video_path_clone).await;
            let _ = fs::remove_dir_all(&hls_dir).await;

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
                    video_name: Some(video_name_clone.clone()),
                    created_at: 0,
                };
                update_progress(&state_clone.progress, &upload_id_clone, completion_progress).await;
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
                    video_name: Some(video_name_clone.clone()),
                    created_at: 0,
                };
                update_progress(&state_clone.progress, &upload_id_clone, error_progress).await;
            }
        }

        tokio::time::sleep(Duration::from_secs(10)).await;
        let mut progress_map = state_clone.progress.write().await;
        if let Some(entry) = progress_map.get(&upload_id_clone)
            && (entry.status == "completed" || entry.status == "failed")
        {
            progress_map.remove(&upload_id_clone);
        }
    });

    Ok(Json(UploadAccepted {
        upload_id,
        message: "Chunked upload finalized, processing started in background".to_string(),
    }))
}

pub async fn list_queues(State(state): State<AppState>) -> Json<QueueListResponse> {
    let progress_map = state.progress.read().await;

    let mut items: Vec<QueueItem> = progress_map
        .iter()
        .map(|(id, p)| QueueItem {
            upload_id: id.clone(),
            stage: p.stage.clone(),
            current_chunk: p.current_chunk,
            total_chunks: p.total_chunks,
            percentage: p.percentage,
            details: p.details.clone(),
            status: p.status.clone(),
            video_name: p.video_name.clone(),
            created_at: p.created_at,
        })
        .collect();

    // Sort by created_at to maintain consistent queue order (oldest first)
    items.sort_by_key(|item| item.created_at);

    let active_count = items
        .iter()
        .filter(|i| i.status == "processing" || i.status == "initializing")
        .count() as u32;
    let completed_count = items.iter().filter(|i| i.status == "completed").count() as u32;
    let failed_count = items.iter().filter(|i| i.status == "failed").count() as u32;

    Json(QueueListResponse {
        items,
        active_count,
        completed_count,
        failed_count,
    })
}

#[derive(serde::Serialize)]
pub struct CancelQueueResponse {
    pub cancelled: bool,
    pub message: String,
}

pub async fn cancel_queue(
    State(state): State<AppState>,
    Path(upload_id): Path<String>,
) -> Result<Json<CancelQueueResponse>, (StatusCode, String)> {
    info!("Attempting to cancel queue: {}", upload_id);

    // Check if the queue item exists and is in a cancellable state
    let mut progress_map = state.progress.write().await;

    if let Some(progress) = progress_map.get(&upload_id) {
        // Only allow cancellation of items that are "initializing" or in early "processing" stages
        // We cannot cancel items that are actively being encoded by FFmpeg
        let cancellable_stages = [
            "Initializing upload",
            "Queued for processing",
            "Receiving chunks",
        ];
        let is_cancellable = progress.status == "initializing"
            || (progress.status == "processing"
                && cancellable_stages.contains(&progress.stage.as_str()));

        if !is_cancellable {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "Cannot cancel: video is already being processed (stage: {})",
                    progress.stage
                ),
            ));
        }

        // Mark as cancelled (we'll use "failed" status with a specific message)
        let cancelled_progress = ProgressUpdate {
            stage: "Cancelled".to_string(),
            current_chunk: 0,
            total_chunks: progress.total_chunks,
            percentage: 0,
            details: Some("Cancelled by user".to_string()),
            status: "failed".to_string(),
            result: None,
            error: Some("Cancelled by user".to_string()),
            video_name: progress.video_name.clone(),
            created_at: progress.created_at,
        };
        progress_map.insert(upload_id.clone(), cancelled_progress);

        // Also clean up any chunked upload data if it exists
        drop(progress_map); // Release the lock before acquiring another
        let mut chunked_uploads = state.chunked_uploads.write().await;
        if let Some(chunked) = chunked_uploads.remove(&upload_id) {
            // Clean up temp directory
            let _ = fs::remove_dir_all(&chunked.temp_dir).await;
            info!("Cleaned up chunked upload temp files for {}", upload_id);
        }

        Ok(Json(CancelQueueResponse {
            cancelled: true,
            message: "Queue item cancelled successfully".to_string(),
        }))
    } else {
        Err((StatusCode::NOT_FOUND, "Queue item not found".to_string()))
    }
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

#[derive(serde::Serialize)]
pub struct CleanupResponse {
    pub cleaned_uploads: usize,
    pub cleaned_progress: usize,
    pub message: String,
}

/// Clean up all stale/stuck uploads - useful when uploads get into a bad state
pub async fn cleanup_uploads(
    State(state): State<AppState>,
) -> Result<Json<CleanupResponse>, (StatusCode, String)> {
    info!("Manual cleanup of stale uploads requested");

    let now = now_millis();
    let mut cleaned_uploads = 0;
    let mut cleaned_progress = 0;

    // Clean up chunked uploads that are older than timeout or not in progress map
    {
        let progress_map = state.progress.read().await;
        let mut uploads = state.chunked_uploads.write().await;

        let mut to_remove = Vec::new();
        for (id, upload) in uploads.iter() {
            // Remove if stale OR if there's no corresponding progress entry
            let is_stale = now.saturating_sub(upload.last_activity) > STALE_UPLOAD_TIMEOUT_MS;
            let no_progress = !progress_map.contains_key(id);

            if is_stale || no_progress {
                to_remove.push((id.clone(), upload.temp_dir.clone()));
            }
        }

        for (id, temp_dir) in to_remove {
            uploads.remove(&id);
            let _ = fs::remove_dir_all(&temp_dir).await;
            cleaned_uploads += 1;
            info!("Cleaned up stale chunked upload: {}", id);
        }
    }

    // Clean up progress entries that are stuck (not completed/failed and older than 1 hour)
    {
        let mut progress_map = state.progress.write().await;
        let hour_ago = now.saturating_sub(60 * 60 * 1000);

        let mut to_remove = Vec::new();
        for (id, progress) in progress_map.iter() {
            if progress.status != "completed"
                && progress.status != "failed"
                && progress.created_at < hour_ago
            {
                to_remove.push(id.clone());
            }
        }

        for id in to_remove {
            progress_map.remove(&id);
            cleaned_progress += 1;
            info!("Cleaned up stuck progress entry: {}", id);
        }
    }

    // Also clean up temp directories on disk that don't have corresponding entries
    if let Ok(mut entries) = tokio::fs::read_dir(std::env::temp_dir()).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("chunked-") {
                    let upload_id = name.trim_start_matches("chunked-");
                    let uploads = state.chunked_uploads.read().await;
                    if !uploads.contains_key(upload_id) {
                        let _ = fs::remove_dir_all(entry.path()).await;
                        cleaned_uploads += 1;
                        info!("Cleaned up orphaned temp directory: {}", name);
                    }
                }
            }
        }
    }

    Ok(Json(CleanupResponse {
        cleaned_uploads,
        cleaned_progress,
        message: format!(
            "Cleanup complete: {} uploads, {} progress entries removed",
            cleaned_uploads, cleaned_progress
        ),
    }))
}
