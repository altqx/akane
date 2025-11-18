use crate::types::{ProgressMap, ProgressUpdate, VideoVariant};
use anyhow::{Context, Result};
use futures::future::try_join_all;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::{fs, process::Command};
use tracing::info;

pub async fn get_video_height(input: &PathBuf) -> Result<u32> {
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
        .output()
        .await
        .context("failed to run ffprobe")?;

    if !output.status.success() {
        anyhow::bail!("ffprobe failed to get video height");
    }

    let height_str = String::from_utf8(output.stdout)?.trim().to_string();
    let height: u32 = height_str.parse().context("failed to parse video height")?;

    Ok(height)
}

pub async fn get_video_duration(input: &PathBuf) -> Result<u32> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("csv=p=0")
        .arg(input)
        .output()
        .await
        .context("failed to run ffprobe")?;

    if !output.status.success() {
        anyhow::bail!("ffprobe failed to get video duration");
    }

    let duration_str = String::from_utf8(output.stdout)?.trim().to_string();
    let duration: f64 = duration_str
        .parse()
        .context("failed to parse video duration")?;

    Ok(duration.round() as u32)
}

pub fn get_variants_for_height(original_height: u32) -> Vec<VideoVariant> {
    let all_variants = vec![
        VideoVariant {
            label: "480p".to_string(),
            height: 480,
            bitrate: "1000k".to_string(),
        },
        VideoVariant {
            label: "720p".to_string(),
            height: 720,
            bitrate: "2500k".to_string(),
        },
        VideoVariant {
            label: "1080p".to_string(),
            height: 1080,
            bitrate: "5000k".to_string(),
        },
        VideoVariant {
            label: "1440p".to_string(),
            height: 1440,
            bitrate: "8000k".to_string(),
        },
    ];

    // Only include variants at or below the original resolution
    all_variants
        .into_iter()
        .filter(|v| v.height <= original_height)
        .collect()
}

pub async fn encode_to_hls(
    input: &PathBuf,
    out_dir: &PathBuf,
    progress: &ProgressMap,
    upload_id: &str,
) -> Result<()> {
    fs::create_dir_all(out_dir).await?;

    // Get original video height to determine appropriate variants
    let original_height = get_video_height(input).await?;
    let variants = get_variants_for_height(original_height);

    if variants.is_empty() {
        anyhow::bail!("No suitable variants for video height {}", original_height);
    }

    let video_codec = std::env::var("ENCODER").unwrap_or_else(|_| "libx264".to_string());
    let gop = 48;
    
    // Determine preset based on encoder
    let preset = if video_codec.contains("nvenc") {
        "p4" // p4 is medium preset for nvenc (p1=fastest, p7=slowest)
    } else {
        "veryfast" // libx264/libx265 preset
    };

    // Limit concurrent FFmpeg processes (configurable via env, default 3)
    let max_concurrent = std::env::var("MAX_CONCURRENT_ENCODES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(3);
    let semaphore = Arc::new(Semaphore::new(max_concurrent));

    // Encode all variants in parallel
    let input = Arc::new(input.clone());
    let out_dir = Arc::new(out_dir.clone());
    let video_codec = Arc::new(video_codec);
    let preset = Arc::new(preset.to_string());
    let progress = Arc::new(progress.clone());
    let upload_id = upload_id.to_string();

    let mut encode_tasks = Vec::new();
    let total_variants = variants.len() as u32;

    for (index, variant) in variants.clone().iter().enumerate() {
        let input = Arc::clone(&input);
        let out_dir = Arc::clone(&out_dir);
        let video_codec = Arc::clone(&video_codec);
        let preset = Arc::clone(&preset);
        let semaphore = Arc::clone(&semaphore);
        let progress = Arc::clone(&progress);
        let upload_id = upload_id.clone();
        let variant = variant.clone();

        let task = tokio::task::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            let seg_dir = out_dir.join(&variant.label);
            fs::create_dir_all(&seg_dir).await?;
            let playlist_path = seg_dir.join("index.m3u8");
            let segment_pattern = seg_dir.join("segment_%03d.ts");

            info!(
                "Encoding variant: {} at {}p with bitrate {}",
                variant.label, variant.height, variant.bitrate
            );

            let scale_filter = format!("scale=-2:{}", variant.height);

            let mut cmd = Command::new("ffmpeg");
            cmd.arg("-y")
                .arg("-i")
                .arg(input.as_ref())
                .arg("-c:v")
                .arg(video_codec.as_ref());

            // Add profile and level only for software encoders
            if !video_codec.contains("nvenc") && !video_codec.contains("_qsv") && !video_codec.contains("_amf") {
                cmd.arg("-profile:v").arg("main")
                   .arg("-level:v").arg("4.0");
            }

            cmd.arg("-preset")
                .arg(preset.as_ref())
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
                .arg("2");

            // Map all subtitle streams and convert to WebVTT
            cmd.arg("-c:s")
                .arg("webvtt");

            cmd.arg("-hls_time")
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
                .arg(&playlist_path);

            let status = cmd.status()
                .await
                .context("failed to run ffmpeg")?;

            if !status.success() {
                anyhow::bail!(
                    "ffmpeg exited with status: {} for variant {}",
                    status,
                    variant.label
                );
            }

            // Update progress for this variant
            let current_chunk = (index + 1) as u32;
            let percentage = (((current_chunk as f32) / (total_variants as f32)) * 100.0) as u32;
            let updated_progress = ProgressUpdate {
                stage: "FFmpeg processing".to_string(),
                current_chunk,
                total_chunks: total_variants,
                percentage,
            };
            progress
                .write()
                .await
                .insert(upload_id.clone(), updated_progress);

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
            .status()
            .await
            .context("failed to generate thumbnail")?;

        if !thumb_status.success() {
            tracing::error!("Thumbnail generation failed, but continuing...");
        }

        Ok::<_, anyhow::Error>(())
    });

    encode_tasks.push(thumb_task);

    // Wait for all encoding and thumbnail tasks to complete
    let results: Result<Vec<_>, _> = try_join_all(
        encode_tasks
            .into_iter()
            .map(|handle| async move { handle.await.context("task panicked")? }),
    )
    .await;

    results?;

    // Create master playlist
    let master_playlist_path = out_dir.join("index.m3u8");
    let mut master_content = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");

    // Check if any variant has subtitle files
    let mut has_subtitles = false;
    let variants_ref = get_variants_for_height(get_video_height(input.as_ref()).await?);
    
    for variant in &variants_ref {
        let subtitle_playlist = out_dir.join(&variant.label).join("index_vtt.m3u8");
        if subtitle_playlist.exists() {
            has_subtitles = true;
            break;
        }
    }

    // Add subtitle media group if subtitles exist
    if has_subtitles {
        // Add subtitle reference for each variant that has subtitles
        for variant in &variants_ref {
            let subtitle_playlist = out_dir.join(&variant.label).join("index_vtt.m3u8");
            if subtitle_playlist.exists() {
                master_content.push_str(&format!(
                    "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"{}\",DEFAULT=YES,AUTOSELECT=YES,FORCED=NO,LANGUAGE=\"en\",URI=\"{}/index_vtt.m3u8\"\n",
                    variant.label,
                    variant.label
                ));
            }
        }
        master_content.push_str("\n");
    }

    // Add video stream variants
    for variant in &variants_ref {
        let bandwidth = variant
            .bitrate
            .trim_end_matches('k')
            .parse::<u32>()
            .unwrap_or(1000)
            * 1000;
        
        let stream_inf = if has_subtitles {
            format!(
                "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{},SUBTITLES=\"subs\"\n",
                bandwidth,
                (((variant.height as f32) * 16.0) / 9.0) as u32,
                variant.height
            )
        } else {
            format!(
                "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{}\n",
                bandwidth,
                (((variant.height as f32) * 16.0) / 9.0) as u32,
                variant.height
            )
        };
        
        master_content.push_str(&stream_inf);
        master_content.push_str(&format!("{}/index.m3u8\n", variant.label));
    }

    fs::write(&master_playlist_path, master_content)
        .await
        .context("failed to write master playlist")?;

    Ok(())
}
