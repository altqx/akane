use std::{ net::SocketAddr, path::PathBuf };

use anyhow::{ Context, Result };
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

#[derive(Clone)]
struct AppState {
    s3: S3Client,
    bucket: String,
    public_base_url: String,
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

    let state = AppState {
        s3,
        bucket: r2_bucket,
        public_base_url,
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

    while
        let Some(field) = multipart
            .next_field().await
            .map_err(|e| internal_err(anyhow::anyhow!(e)))?
    {
        let name = field.name().map(|s| s.to_string());
        if name.as_deref() != Some("file") {
            continue;
        }

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
        break;
    }

    let video_path = video_path.ok_or_else(|| {
        (axum::http::StatusCode::BAD_REQUEST, "missing file field 'file'".to_string())
    })?;

    // Encode to HLS (playlist + segments) into a temp directory
    let output_id = Uuid::new_v4().to_string();
    let hls_dir = std::env::temp_dir().join(format!("hls-{}", &output_id));
    fs::create_dir_all(&hls_dir).await.map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    encode_to_hls(&video_path, &hls_dir).await.map_err(|e| internal_err(e))?;

    // Upload HLS to R2
    let prefix = format!("{}/", output_id);
    let playlist_key = upload_hls_to_r2(&state, &hls_dir, &prefix).await.map_err(|e|
        internal_err(e)
    )?;

    // Build public URL (you should front this with your CDN/domain)
    let playlist_url = format!("{}/{}", state.public_base_url.trim_end_matches('/'), playlist_key);

    // Cleanup (ignore errors)
    let _ = fs::remove_file(&video_path).await;
    let _ = fs::remove_dir_all(&hls_dir).await;

    Ok(Json(UploadResponse { playlist_url }))
}

async fn encode_to_hls(input: &PathBuf, out_dir: &PathBuf) -> Result<()> {
    use tokio::process::Command;

    fs::create_dir_all(out_dir).await?;
    let playlist_path = out_dir.join("index.m3u8");

    // Select encoder based on env var (default: libx264 CPU)
    // Supported values:
    //   CPU: "libx264" (default)
    //   NVIDIA: "h264_nvenc"
    //   AMD: "h264_amf"
    let video_codec = std::env
        ::var("ENCODER")
        .unwrap_or_else(|_| "libx264".to_string());

    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(input)
        .arg("-codec:v")
        .arg(&video_codec)
        .arg("-codec:a")
        .arg("aac")
        .arg("-start_number")
        .arg("0")
        .arg("-hls_time")
        .arg("4")
        .arg("-hls_list_size")
        .arg("0")
        .arg("-f")
        .arg("hls")
        .arg(&playlist_path)
        .status().await
        .context("failed to run ffmpeg")?;

    if !status.success() {
        anyhow::bail!("ffmpeg exited with status: {}", status);
    }

    Ok(())
}

async fn upload_hls_to_r2(state: &AppState, hls_dir: &PathBuf, prefix: &str) -> Result<String> {
    let mut read_dir = fs::read_dir(&hls_dir).await.context("read hls dir")?;

    let mut playlist_key = None;

    while let Some(entry) = read_dir.next_entry().await.context("iterate hls dir")? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy().into_owned();

        let key = format!("{}{}", prefix, file_name);

        if file_name.ends_with(".m3u8") {
            playlist_key = Some(key.clone());
        }

        let body_bytes = fs::read(&path).await.with_context(|| format!("read {}", file_name))?;

        state.s3
            .put_object()
            .bucket(&state.bucket)
            .key(&key)
            .body(body_bytes.into())
            .send().await
            .with_context(|| format!("upload {}", key))?;
    }

    let playlist_key = playlist_key.ok_or_else(||
        anyhow::anyhow!("no playlist (.m3u8) generated")
    )?;

    Ok(playlist_key)
}

fn internal_err(e: anyhow::Error) -> (axum::http::StatusCode, String) {
    error!(error = ?e, "internal error");
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "internal server error".to_string())
}
