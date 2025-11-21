use crate::types::AppState;
use anyhow::{Context, Result};
use aws_sdk_s3::presigning::PresigningConfig;
use futures::stream::{self, StreamExt};
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
use tracing::info;

pub async fn upload_hls_to_r2(state: &AppState, hls_dir: &PathBuf, prefix: &str) -> Result<String> {
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
    let max_concurrent_uploads = std::env::var("MAX_CONCURRENT_UPLOADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(30);

    let upload_results: Vec<Result<String>> = stream::iter(files_to_upload)
        .map(|(path, key)| {
            let state = state.clone();
            async move {
                let body_bytes = fs::read(&path)
                    .await
                    .with_context(|| format!("read {:?}", path))?;

                state
                    .s3
                    .put_object()
                    .bucket(&state.bucket)
                    .key(&key)
                    .body(body_bytes.into())
                    .send()
                    .await
                    .with_context(|| format!("upload {}", key))?;

                info!("Uploaded: {}", key);
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

pub async fn generate_presigned_url(state: &AppState, key: &str) -> Result<String> {
    let presigning_config = PresigningConfig::expires_in(Duration::from_secs(3600))?;

    let presigned_request = state
        .s3
        .get_object()
        .bucket(&state.bucket)
        .key(key)
        .presigned(presigning_config)
        .await?;

    Ok(presigned_request.uri().to_string())
}
