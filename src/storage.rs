use crate::types::{AppState, ProgressUpdate};
use anyhow::{Context, Result};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::CompletedMultipartUpload;
use aws_sdk_s3::types::CompletedPart;
use futures::stream::{self, StreamExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;
use tracing::{error, info, warn};

// 100 MB threshold for multipart upload
const MULTIPART_THRESHOLD: u64 = 100 * 1024 * 1024;
// 100 MB part size for multipart upload (minimum is 5MB for S3)
const MULTIPART_PART_SIZE: usize = 100 * 1024 * 1024;
// Retry configuration for R2 uploads
const MAX_UPLOAD_RETRIES: u32 = 3;
const INITIAL_RETRY_DELAY_MS: u64 = 1000;
const MAX_RETRY_DELAY_MS: u64 = 10000;

/// Upload a large file to R2/S3 using multipart upload to avoid Windows I/O buffer limits.
/// This streams the file in chunks instead of loading the entire file into memory.
#[allow(dead_code)]
pub async fn upload_large_file_to_r2(
    state: &AppState,
    file_path: &PathBuf,
    key: &str,
) -> Result<()> {
    let file_metadata = fs::metadata(file_path)
        .await
        .with_context(|| format!("Failed to get metadata for {:?}", file_path))?;
    let file_size = file_metadata.len();

    // For smaller files, use simple put_object but read in chunks to avoid Windows I/O limits
    if file_size < MULTIPART_THRESHOLD {
        let body_bytes = read_file_chunked(file_path).await?;
        state
            .s3
            .put_object()
            .bucket(&state.config.r2.bucket)
            .key(key)
            .body(body_bytes.into())
            .send()
            .await
            .with_context(|| format!("Failed to upload {}", key))?;
        return Ok(());
    }

    // For larger files, use multipart upload
    info!(
        "Using multipart upload for large file: {} ({} bytes)",
        key, file_size
    );

    // Initiate multipart upload
    let create_response = state
        .s3
        .create_multipart_upload()
        .bucket(&state.config.r2.bucket)
        .key(key)
        .send()
        .await
        .context("Failed to initiate multipart upload")?;

    let upload_id = create_response
        .upload_id()
        .ok_or_else(|| anyhow::anyhow!("No upload_id returned from create_multipart_upload"))?;

    let mut file = File::open(file_path)
        .await
        .with_context(|| format!("Failed to open {:?}", file_path))?;

    let mut part_number = 1i32;
    let mut completed_parts: Vec<CompletedPart> = Vec::new();
    let mut bytes_uploaded: u64 = 0;

    loop {
        // Read chunk - use a smaller buffer size to avoid Windows I/O limits
        let mut buffer = vec![0u8; MULTIPART_PART_SIZE];
        let mut bytes_read = 0;

        // Read in smaller sub-chunks to avoid Windows I/O buffer limit
        const SUB_CHUNK_SIZE: usize = 64 * 1024 * 1024; // 64 MB sub-chunks
        while bytes_read < MULTIPART_PART_SIZE {
            let remaining = MULTIPART_PART_SIZE - bytes_read;
            let to_read = remaining.min(SUB_CHUNK_SIZE);
            let n = file
                .read(&mut buffer[bytes_read..bytes_read + to_read])
                .await?;
            if n == 0 {
                break; // EOF
            }
            bytes_read += n;
        }

        if bytes_read == 0 {
            break; // No more data
        }

        buffer.truncate(bytes_read);
        bytes_uploaded += bytes_read as u64;

        info!(
            "Uploading part {} ({} bytes, {:.1}% complete)",
            part_number,
            bytes_read,
            (bytes_uploaded as f64 / file_size as f64) * 100.0
        );

        // Upload part
        let upload_part_response = state
            .s3
            .upload_part()
            .bucket(&state.config.r2.bucket)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_number)
            .body(ByteStream::from(buffer))
            .send()
            .await;

        match upload_part_response {
            Ok(response) => {
                let e_tag = response.e_tag().map(|s| s.to_string());
                completed_parts.push(
                    CompletedPart::builder()
                        .part_number(part_number)
                        .set_e_tag(e_tag)
                        .build(),
                );
            }
            Err(e) => {
                // Abort the multipart upload on failure
                error!("Failed to upload part {}: {}", part_number, e);
                let _ = state
                    .s3
                    .abort_multipart_upload()
                    .bucket(&state.config.r2.bucket)
                    .key(key)
                    .upload_id(upload_id)
                    .send()
                    .await;
                return Err(anyhow::anyhow!(
                    "Failed to upload part {}: {}",
                    part_number,
                    e
                ));
            }
        }

        part_number += 1;
    }

    // Complete multipart upload
    let completed_upload = CompletedMultipartUpload::builder()
        .set_parts(Some(completed_parts))
        .build();

    state
        .s3
        .complete_multipart_upload()
        .bucket(&state.config.r2.bucket)
        .key(key)
        .upload_id(upload_id)
        .multipart_upload(completed_upload)
        .send()
        .await
        .context("Failed to complete multipart upload")?;

    info!(
        "Multipart upload completed: {} ({} bytes in {} parts)",
        key,
        file_size,
        part_number - 1
    );

    Ok(())
}

/// Read a file in chunks to avoid Windows I/O buffer limits (4GB max)
async fn read_file_chunked(path: &PathBuf) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).await?;
    let file_size = metadata.len() as usize;

    let mut file = File::open(path).await?;
    let mut buffer = Vec::with_capacity(file_size);

    // Read in 64MB chunks to stay well under Windows' 4GB limit
    const CHUNK_SIZE: usize = 64 * 1024 * 1024;
    let mut temp_buf = vec![0u8; CHUNK_SIZE];

    loop {
        let n = file.read(&mut temp_buf).await?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&temp_buf[..n]);
    }

    Ok(buffer)
}

pub async fn upload_hls_to_r2(
    state: &AppState,
    hls_dir: &PathBuf,
    prefix: &str,
    upload_id: Option<&str>,
) -> Result<String> {
    let mut master_playlist_key = None;
    let mut files_to_upload = Vec::new();

    // Collect all files to upload
    async fn collect_files(
        dir: &PathBuf,
        prefix: &str,
        files: &mut Vec<(PathBuf, String)>,
        master_key: &mut Option<String>,
    ) -> Result<()> {
        let mut read_dir = fs::read_dir(dir).await.context("read dir")?;

        while let Some(entry) = read_dir.next_entry().await.context("iterate dir")? {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().into_owned();

            if path.is_dir() {
                let sub_prefix = format!("{}{}/", prefix, file_name);
                Box::pin(collect_files(&path, &sub_prefix, files, master_key)).await?;
            } else if path.is_file() {
                let key = format!("{}{}", prefix, file_name);

                // Track master playlist
                if file_name == "index.m3u8" && prefix.matches('/').count() == 1 {
                    *master_key = Some(key.clone());
                }

                files.push((path, key));
            }
        }

        Ok(())
    }

    collect_files(
        hls_dir,
        prefix,
        &mut files_to_upload,
        &mut master_playlist_key,
    )
    .await?;

    // Upload all files in parallel with concurrency limit
    let max_concurrent_uploads = state.config.server.max_concurrent_uploads;

    let total_files = files_to_upload.len() as u32;
    let uploaded_count = Arc::new(AtomicU32::new(0));

    let upload_results: Vec<Result<String>> = stream::iter(files_to_upload)
        .map(|(path, key)| {
            let state = state.clone();
            let uploaded_count = Arc::clone(&uploaded_count);
            let upload_id = upload_id.map(|s| s.to_string());
            async move {
                let body_bytes = fs::read(&path)
                    .await
                    .with_context(|| format!("read {:?}", path))?;

                // Retry logic with exponential backoff for connection errors
                let mut last_error = None;
                for attempt in 0..MAX_UPLOAD_RETRIES {
                    match state
                        .s3
                        .put_object()
                        .bucket(&state.config.r2.bucket)
                        .key(&key)
                        .body(body_bytes.clone().into())
                        .send()
                        .await
                    {
                        Ok(_) => {
                            if attempt > 0 {
                                info!("Uploaded {} after {} retries", key, attempt);
                            }
                            last_error = None;
                            break;
                        }
                        Err(e) => {
                            last_error = Some(e);
                            if attempt < MAX_UPLOAD_RETRIES - 1 {
                                let delay = std::cmp::min(
                                    INITIAL_RETRY_DELAY_MS * 2u64.pow(attempt),
                                    MAX_RETRY_DELAY_MS,
                                );
                                warn!(
                                    "Upload failed for {}, attempt {}/{}, retrying in {}ms: {:?}",
                                    key,
                                    attempt + 1,
                                    MAX_UPLOAD_RETRIES,
                                    delay,
                                    last_error
                                );
                                tokio::time::sleep(Duration::from_millis(delay)).await;
                            }
                        }
                    }
                }

                if let Some(e) = last_error {
                    return Err(anyhow::anyhow!(
                        "upload {}: {} (after {} retries)",
                        key,
                        e,
                        MAX_UPLOAD_RETRIES
                    ));
                }

                info!("Uploaded: {}", key);

                // Update progress
                let current = uploaded_count.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(id) = upload_id {
                    let percentage = ((current as f32 / total_files as f32) * 100.0) as u32;
                    // Preserve video_name and created_at from existing progress
                    let (existing_video_name, existing_created_at) = {
                        let progress_map = state.progress.read().await;
                        progress_map
                            .get(&id)
                            .map(|p| (p.video_name.clone(), p.created_at))
                            .unwrap_or((None, 0))
                    };
                    let progress_update = ProgressUpdate {
                        stage: "Upload to R2".to_string(),
                        current_chunk: current,
                        total_chunks: total_files,
                        percentage,
                        details: Some(format!("Uploaded {}/{} files", current, total_files)),
                        status: "processing".to_string(),
                        result: None,
                        error: None,
                        video_name: existing_video_name,
                        created_at: existing_created_at,
                        variant_percentage: None,
                    };
                    state.progress.write().await.insert(id, progress_update);
                }

                Ok::<_, anyhow::Error>(key)
            }
        })
        .buffer_unordered(max_concurrent_uploads)
        .collect()
        .await;

    // Check for any upload errors
    for result in upload_results {
        result?;
    }

    let playlist_key = master_playlist_key
        .ok_or_else(|| anyhow::anyhow!("no master playlist (index.m3u8) generated"))?;

    Ok(playlist_key)
}
