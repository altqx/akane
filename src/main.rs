use std::{ net::SocketAddr, path::PathBuf, sync::Arc };

use anyhow::{ Context, Result };
use sqlx::{ SqlitePool, migrate::MigrateDatabase, Sqlite };
use futures::{ stream::{ self, StreamExt }, future::try_join_all };
use tokio::sync::Semaphore;
use axum::{ extract::{ State, Multipart }, routing::post, Router, Json };
use axum::extract::DefaultBodyLimit;
use aws_sdk_s3::{ config::Region, Client as S3Client };
use serde::Serialize;
use tokio::{ fs, io::AsyncWriteExt };
use tower_http::services::ServeDir;
use tracing::{ error, info };
use tracing_subscriber::{ layer::SubscriberExt, util::SubscriberInitExt };
use uuid::Uuid;
use dotenv::dotenv;

#[derive(Clone, Debug)]
struct VideoVariant {
    label: String,
    height: u32,
    bitrate: String,
}

#[derive(Clone)]
struct AppState {
    s3: S3Client,
    bucket: String,
    public_base_url: String,
    db_pool: SqlitePool,
}

#[derive(Serialize)]
struct UploadResponse {
    playlist_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber
        ::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenv().ok();
    dotenv().expect("Failed to load .env file");

    let r2_endpoint = std::env
        ::var("R2_ENDPOINT")
        .context(
            "R2_ENDPOINT env var required (e.g. https://<accountid>.r2.cloudflarestorage.com)"
        )?;
    let r2_bucket = std::env::var("R2_BUCKET").context("R2_BUCKET env var required")?;
    let r2_access_key = std::env
        ::var("R2_ACCESS_KEY_ID")
        .context("R2_ACCESS_KEY_ID env var required")?;
    let r2_secret_key = std::env
        ::var("R2_SECRET_ACCESS_KEY")
        .context("R2_SECRET_ACCESS_KEY env var required")?;
    let public_base_url = std::env
        ::var("R2_PUBLIC_BASE_URL")
        .unwrap_or_else(|_| format!("{}/{}", r2_endpoint, r2_bucket));

    let s3_config = aws_sdk_s3::config::Builder
        ::new()
        .endpoint_url(r2_endpoint)
        .region(Region::new("auto"))
        .credentials_provider(
            aws_sdk_s3::config::Credentials::new(r2_access_key, r2_secret_key, None, None, "r2")
        )
        .build();
    let s3 = S3Client::from_conf(s3_config);

    // Initialize SQLite database
    let database_url = "sqlite://videos.db";
    if !Sqlite::database_exists(database_url).await.unwrap_or(false) {
        info!("Creating database: {}", database_url);
        Sqlite::create_database(database_url).await.context("Failed to create database")?;
    }

    let db_pool = SqlitePool::connect(database_url).await.context("Failed to connect to database")?;
    
    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .context("Failed to run migrations")?;
    
    info!("Database initialized successfully");

    let state = AppState {
        s3,
        bucket: r2_bucket,
        public_base_url,
        db_pool,
    };

    let app = Router::new()
        .route("/upload", post(upload_video))
        .fallback_service(ServeDir::new("public"))
        // e.g. 1 GB body limit
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024))
        .with_state(state);

    let addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();
    info!("listening on {}", addr);

    axum
        ::serve(tokio::net::TcpListener::bind(addr).await?, app.into_make_service()).await
        .context("server error")?;
    Ok(())
}

async fn upload_video(
    State(state): State<AppState>,
    mut multipart: Multipart
) -> Result<Json<UploadResponse>, (axum::http::StatusCode, String)> {
    let mut video_path: Option<PathBuf> = None;
    let mut video_name: Option<String> = None;
    let mut tags: Vec<String> = Vec::new();

    while
        let Some(field) = multipart
            .next_field().await
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

                let mut file = fs::File
                    ::create(&tmp_file).await
                    .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

                let mut bytes = field.bytes().await.map_err(|e| internal_err(anyhow::anyhow!(e)))?;

                file.write_all_buf(&mut bytes).await.map_err(|e| internal_err(anyhow::anyhow!(e)))?;

                video_path = Some(tmp_file);
            },
            Some("name") => {
                let text = field.text().await.map_err(|e| internal_err(anyhow::anyhow!(e)))?;
                video_name = Some(text);
            },
            Some("tags") => {
                let text = field.text().await.map_err(|e| internal_err(anyhow::anyhow!(e)))?;
                // Parse tags as JSON array or comma-separated
                if let Ok(parsed_tags) = serde_json::from_str::<Vec<String>>(&text) {
                    tags = parsed_tags;
                } else {
                    tags = text.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                }
            },
            _ => continue,
        }
    }

    let video_path = video_path.ok_or_else(|| {
        (axum::http::StatusCode::BAD_REQUEST, "missing file field 'file'".to_string())
    })?;

    let video_name = video_name.ok_or_else(|| {
        (axum::http::StatusCode::BAD_REQUEST, "missing field 'name'".to_string())
    })?;

    // Encode to HLS (playlist + segments) into a temp directory
    let output_id = Uuid::new_v4().to_string();
    let hls_dir = std::env::temp_dir().join(format!("hls-{}", &output_id));
    fs::create_dir_all(&hls_dir).await.map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    // Get video metadata before encoding (parallel)
    let (video_duration, original_height) = tokio::join!(
        get_video_duration(&video_path),
        get_video_height(&video_path)
    );
    let video_duration = video_duration.map_err(|e| internal_err(e))?;
    let original_height = original_height.map_err(|e| internal_err(e))?;
    let variants = get_variants_for_height(original_height);
    let available_resolutions: Vec<String> = variants.iter().map(|v| v.label.clone()).collect();

    encode_to_hls(&video_path, &hls_dir).await.map_err(|e| internal_err(e))?;

    // Upload HLS to R2
    let prefix = format!("{}/", output_id);
    let playlist_key = upload_hls_to_r2(&state, &hls_dir, &prefix).await.map_err(|e|
        internal_err(e)
    )?;

    // Build public URL (you should front this with your CDN/domain)
    let playlist_url = format!("{}/{}", state.public_base_url.trim_end_matches('/'), playlist_key);

    // Save to database
    let thumbnail_key = format!("{}/thumbnail.jpg", output_id);
    let entrypoint = playlist_key.clone();
    let tags_json = serde_json::to_string(&tags).map_err(|e| internal_err(anyhow::anyhow!(e)))?;
    let resolutions_json = serde_json::to_string(&available_resolutions).map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    sqlx::query(
        "INSERT INTO videos (id, name, tags, available_resolutions, duration, thumbnail_key, entrypoint) VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&output_id)
    .bind(&video_name)
    .bind(&tags_json)
    .bind(&resolutions_json)
    .bind(video_duration as i64)
    .bind(&thumbnail_key)
    .bind(&entrypoint)
    .execute(&state.db_pool)
    .await
    .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    info!("Video saved to database: id={}, name={}", output_id, video_name);

    // Cleanup (ignore errors)
    let _ = fs::remove_file(&video_path).await;
    let _ = fs::remove_dir_all(&hls_dir).await;

    Ok(Json(UploadResponse { playlist_url }))
}

async fn get_video_height(input: &PathBuf) -> Result<u32> {
    use tokio::process::Command;

    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=height")
        .arg("-of")
        .arg("csv=p=0")
        .arg(input)
        .output().await
        .context("failed to run ffprobe")?;

    if !output.status.success() {
        anyhow::bail!("ffprobe failed to get video height");
    }

    let height_str = String::from_utf8(output.stdout)?
        .trim()
        .to_string();
    let height: u32 = height_str.parse().context("failed to parse video height")?;

    Ok(height)
}

async fn get_video_duration(input: &PathBuf) -> Result<u32> {
    use tokio::process::Command;

    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("csv=p=0")
        .arg(input)
        .output().await
        .context("failed to run ffprobe")?;

    if !output.status.success() {
        anyhow::bail!("ffprobe failed to get video duration");
    }

    let duration_str = String::from_utf8(output.stdout)?
        .trim()
        .to_string();
    let duration: f64 = duration_str.parse().context("failed to parse video duration")?;

    Ok(duration.round() as u32)
}

fn get_variants_for_height(original_height: u32) -> Vec<VideoVariant> {
    let all_variants = vec![
        VideoVariant { label: "480p".to_string(), height: 480, bitrate: "1000k".to_string() },
        VideoVariant { label: "720p".to_string(), height: 720, bitrate: "2500k".to_string() },
        VideoVariant { label: "1080p".to_string(), height: 1080, bitrate: "5000k".to_string() },
        VideoVariant { label: "1440p".to_string(), height: 1440, bitrate: "8000k".to_string() },
    ];

    // Only include variants at or below the original resolution
    all_variants
        .into_iter()
        .filter(|v| v.height <= original_height)
        .collect()
}

async fn encode_to_hls(input: &PathBuf, out_dir: &PathBuf) -> Result<()> {
    use tokio::process::Command;

    fs::create_dir_all(out_dir).await?;

    // Get original video height to determine appropriate variants
    let original_height = get_video_height(input).await?;
    let variants = get_variants_for_height(original_height);

    if variants.is_empty() {
        anyhow::bail!("No suitable variants for video height {}", original_height);
    }

    let video_codec = std::env
        ::var("ENCODER")
        .unwrap_or_else(|_| "libx264".to_string());
    let gop = 48;

    // Limit concurrent FFmpeg processes (configurable via env, default 3)
    let max_concurrent = std::env
        ::var("MAX_CONCURRENT_ENCODES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(3);
    let semaphore = Arc::new(Semaphore::new(max_concurrent));

    // Encode all variants in parallel
    let input = Arc::new(input.clone());
    let out_dir = Arc::new(out_dir.clone());
    let video_codec = Arc::new(video_codec);

    let mut encode_tasks = Vec::new();
    
    for variant in variants.clone() {
        let input = Arc::clone(&input);
        let out_dir = Arc::clone(&out_dir);
        let video_codec = Arc::clone(&video_codec);
        let semaphore = Arc::clone(&semaphore);
        
        let task = tokio::task::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            
            let seg_dir = out_dir.join(&variant.label);
            fs::create_dir_all(&seg_dir).await?;
            let playlist_path = seg_dir.join("index.m3u8");
            let segment_pattern = seg_dir.join("segment_%03d.ts");

            info!("Encoding variant: {} at {}p with bitrate {}", variant.label, variant.height, variant.bitrate);

            let scale_filter = format!("scale=-2:{}", variant.height);

            let status = Command::new("ffmpeg")
                .arg("-y")
                .arg("-i")
                .arg(input.as_ref())
                .arg("-c:v")
                .arg(video_codec.as_ref())
                .arg("-profile:v")
                .arg("main")
                .arg("-level:v")
                .arg("4.0")
                .arg("-preset")
                .arg("veryfast")
                .arg("-b:v")
                .arg(&variant.bitrate)
                .arg("-vf")
                .arg(&scale_filter)
                .arg("-pix_fmt")
                .arg("yuv420p")
                .arg("-g")
                .arg(gop.to_string())
                .arg("-keyint_min")
                .arg(gop.to_string())
                .arg("-sc_threshold")
                .arg("0")
                .arg("-force_key_frames")
                .arg("expr:gte(t,n_forced*4)")
                .arg("-c:a")
                .arg("aac")
                .arg("-b:a")
                .arg("128k")
                .arg("-ac")
                .arg("2")
                .arg("-hls_time")
                .arg("4")
                .arg("-hls_list_size")
                .arg("0")
                .arg("-hls_playlist_type")
                .arg("vod")
                .arg("-hls_segment_type")
                .arg("mpegts")
                .arg("-start_number")
                .arg("0")
                .arg("-hls_segment_filename")
                .arg(&segment_pattern)
                .arg(&playlist_path)
                .status().await
                .context("failed to run ffmpeg")?;

            if !status.success() {
                anyhow::bail!("ffmpeg exited with status: {} for variant {}", status, variant.label);
            }
            
            Ok::<_, anyhow::Error>(())
        });
        
        encode_tasks.push(task);
    }

    // Spawn thumbnail generation in parallel with encoding
    let input_thumb = Arc::clone(&input);
    let out_dir_thumb = Arc::clone(&out_dir);
    let thumb_task = tokio::task::spawn(async move {
        let thumb_path = out_dir_thumb.join("thumbnail.jpg");
        info!("Generating thumbnail: {:?}", thumb_path);
        
        let thumb_status = Command::new("ffmpeg")
            .arg("-y")
            .arg("-ss")
            .arg("0")
            .arg("-i")
            .arg(input_thumb.as_ref())
            .arg("-vframes")
            .arg("1")
            .arg("-q:v")
            .arg("20")
            .arg(&thumb_path)
            .status().await
            .context("failed to generate thumbnail")?;

        if !thumb_status.success() {
            error!("Thumbnail generation failed, but continuing...");
        }
        
        Ok::<_, anyhow::Error>(())
    });
    
    encode_tasks.push(thumb_task);

    // Wait for all encoding and thumbnail tasks to complete
    let results: Result<Vec<_>, _> = try_join_all(
        encode_tasks.into_iter().map(|handle| async move {
            handle.await.context("task panicked")?
        })
    ).await;
    
    results?;

    // Create master playlist
    let master_playlist_path = out_dir.join("index.m3u8");
    let mut master_content = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");

    for variant in &variants {
        let bandwidth = variant.bitrate.trim_end_matches('k').parse::<u32>().unwrap_or(1000) * 1000;
        master_content.push_str(&format!(
            "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{}\n",
            bandwidth,
            (variant.height as f32 * 16.0 / 9.0) as u32, // Approximate width for display
            variant.height
        ));
        master_content.push_str(&format!("{}/index.m3u8\n", variant.label));
    }

    fs::write(&master_playlist_path, master_content).await
        .context("failed to write master playlist")?;

    Ok(())
}

async fn upload_hls_to_r2(state: &AppState, hls_dir: &PathBuf, prefix: &str) -> Result<String> {
    let mut master_playlist_key = None;
    let mut files_to_upload = Vec::new();

    // Collect all files to upload
    async fn collect_files(
        dir: &PathBuf,
        prefix: &str,
        files: &mut Vec<(PathBuf, String)>,
        master_key: &mut Option<String>
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

    collect_files(hls_dir, prefix, &mut files_to_upload, &mut master_playlist_key).await?;

    // Upload all files in parallel with concurrency limit
    let max_concurrent_uploads = std::env
        ::var("MAX_CONCURRENT_UPLOADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(30);

    let upload_results: Vec<Result<String>> = stream::iter(files_to_upload)
        .map(|(path, key)| {
            let state = state.clone();
            async move {
                let body_bytes = fs::read(&path).await
                    .with_context(|| format!("read {:?}", path))?;

                state.s3
                    .put_object()
                    .bucket(&state.bucket)
                    .key(&key)
                    .body(body_bytes.into())
                    .send().await
                    .with_context(|| format!("upload {}", key))?;

                info!("Uploaded: {}", key);
                Ok::<_, anyhow::Error>(key)
            }
        })
        .buffer_unordered(max_concurrent_uploads)
        .collect().await;

    // Check for any upload errors
    for result in upload_results {
        result?;
    }

    let playlist_key = master_playlist_key.ok_or_else(||
        anyhow::anyhow!("no master playlist (index.m3u8) generated")
    )?;

    Ok(playlist_key)
}

fn internal_err(e: anyhow::Error) -> (axum::http::StatusCode, String) {
    error!(error = ?e, "internal error");
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "internal server error".to_string())
}
