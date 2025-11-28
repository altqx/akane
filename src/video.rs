use crate::types::{ProgressMap, ProgressUpdate, VideoVariant};
use anyhow::{Context, Result};
use futures::future::try_join_all;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::{fs, process::Command};
use tracing::{error, info};

pub async fn get_video_metadata(input: &PathBuf) -> Result<(u32, u32)> {
    // Using JSON output
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=height:format=duration")
        .arg("-of")
        .arg("json")
        .arg(input)
        .output()
        .await
        .context("failed to run ffprobe")?;

    if !output.status.success() {
        anyhow::bail!("ffprobe failed");
    }

    let json_str = String::from_utf8(output.stdout)?;
    let v: serde_json::Value = serde_json::from_str(&json_str)?;

    let height = v["streams"][0]["height"].as_u64().context("no height found")? as u32;
    let duration_str = v["format"]["duration"].as_str().context("no duration found")?;
    let duration: f64 = duration_str.parse()?;

    Ok((height, duration.round() as u32))
}

pub async fn get_video_height(input: &PathBuf) -> Result<u32> {
    // Keep for backward compatibility or individual usage
    let (h, _) = get_video_metadata(input).await?;
    Ok(h)
}

pub async fn get_video_duration(input: &PathBuf) -> Result<u32> {
     // Keep for backward compatibility or individual usage
    let (_, d) = get_video_metadata(input).await?;
    Ok(d)
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

#[derive(Debug, Clone)]
enum EncoderType {
    Nvenc,
    Vaapi,
    Qsv,
    Cpu,
}

impl EncoderType {
    fn from_string(s: &str) -> Self {
        if s.contains("nvenc") {
            EncoderType::Nvenc
        } else if s.contains("vaapi") {
            EncoderType::Vaapi
        } else if s.contains("qsv") {
            EncoderType::Qsv
        } else {
            EncoderType::Cpu
        }
    }
}

pub async fn encode_to_hls(
    input: &PathBuf,
    out_dir: &PathBuf,
    progress: &ProgressMap,
    upload_id: &str,
    semaphore: Arc<Semaphore>,
    encoder: &str,
) -> Result<()> {
    fs::create_dir_all(out_dir).await?;

    // Get original video height to determine appropriate variants
    // Use the combined metadata fetch for performance
    let (original_height, _) = get_video_metadata(input).await?;
    let variants = get_variants_for_height(original_height);

    if variants.is_empty() {
        anyhow::bail!("No suitable variants for video height {}", original_height);
    }

    let video_codec = encoder.to_string();
    let encoder_type = EncoderType::from_string(&video_codec);

    // GOP size - use 48 for 24fps content (2 seconds), adjust for HLS segment alignment
    let gop = 48;

    let input = Arc::new(input.clone());
    let out_dir = Arc::new(out_dir.clone());
    let video_codec = Arc::new(video_codec);
    let progress = Arc::new(progress.clone());
    let upload_id = upload_id.to_string();

    let mut encode_tasks = Vec::new();
    let total_variants = variants.len() as u32;

    for (index, variant) in variants.clone().iter().enumerate() {
        let input = Arc::clone(&input);
        let out_dir = Arc::clone(&out_dir);
        let video_codec = Arc::clone(&video_codec);
        let semaphore = Arc::clone(&semaphore);
        let progress = Arc::clone(&progress);
        let upload_id = upload_id.clone();
        let variant = variant.clone();
        let encoder_type = encoder_type.clone();

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

            // Update progress before starting this variant
            let current_chunk = (index + 1) as u32;
            let percentage = (((current_chunk as f32) / (total_variants as f32)) * 100.0) as u32;
            // Preserve video_name from existing progress
            let existing_video_name = {
                let progress_map = progress.read().await;
                progress_map
                    .get(&upload_id)
                    .and_then(|p| p.video_name.clone())
            };
            let start_progress = ProgressUpdate {
                stage: "FFmpeg processing".to_string(),
                current_chunk,
                total_chunks: total_variants,
                percentage,
                details: Some(format!(
                    "Encoding variant: {} ({}p)",
                    variant.label, variant.height
                )),
                status: "processing".to_string(),
                result: None,
                error: None,
                video_name: existing_video_name.clone(),
            };
            progress
                .write()
                .await
                .insert(upload_id.clone(), start_progress);

            let mut cmd = Command::new("ffmpeg");
            cmd.stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .arg("-loglevel")
                .arg("error")
                .arg("-y");

            // Hardware acceleration setup
            match encoder_type {
                EncoderType::Nvenc => {
                    cmd.arg("-hwaccel")
                        .arg("cuda")
                        .arg("-hwaccel_output_format")
                        .arg("cuda");
                }
                EncoderType::Vaapi => {
                    cmd.arg("-hwaccel")
                        .arg("vaapi")
                        .arg("-hwaccel_output_format")
                        .arg("vaapi")
                        .arg("-vaapi_device")
                        .arg("/dev/dri/renderD128");
                }
                EncoderType::Qsv => {
                    cmd.arg("-hwaccel")
                        .arg("qsv")
                        .arg("-hwaccel_output_format")
                        .arg("qsv");
                }
                EncoderType::Cpu => {}
            }

            cmd.arg("-i").arg(input.as_ref());

            // Scaling filter
            let scale_filter = match encoder_type {
                EncoderType::Nvenc => format!("scale_cuda=-2:{}", variant.height),
                EncoderType::Vaapi => format!("scale_vaapi=-2:{}", variant.height),
                EncoderType::Qsv => format!("vpp_qsv=w=-2:h={}", variant.height),
                EncoderType::Cpu => format!("scale=-2:{}", variant.height),
            };

            cmd.arg("-c:v").arg(video_codec.as_ref());

            // Encoder specific settings
            match encoder_type {
                EncoderType::Nvenc => {
                    cmd.arg("-preset")
                        .arg("p3") // Faster than p4 (medium), still good quality
                        .arg("-profile:v")
                        .arg("main")
                        .arg("-level:v")
                        .arg("4.1")
                        .arg("-rc:v")
                        .arg("vbr")
                        .arg("-rc-lookahead")
                        .arg("20")
                        .arg("-bf")
                        .arg("3")
                        .arg("-spatial-aq")
                        .arg("1")
                        .arg("-temporal-aq")
                        .arg("1")
                        .arg("-aq-strength")
                        .arg("8")
                        .arg("-surfaces")
                        .arg("8")
                        .arg("-weighted_pred")
                        .arg("1");
                }
                EncoderType::Vaapi => {
                    cmd.arg("-compression_level")
                        .arg("20") // Balance quality/speed
                        .arg("-rc_mode")
                        .arg("VBR")
                        .arg("-profile:v")
                        .arg("main");
                }
                EncoderType::Qsv => {
                    cmd.arg("-preset")
                        .arg("faster") // changed from medium to faster
                        .arg("-profile:v")
                        .arg("main")
                        .arg("-look_ahead")
                        .arg("1")
                        .arg("-look_ahead_depth")
                        .arg("40");
                }
                EncoderType::Cpu => {
                    cmd.arg("-preset")
                        .arg("veryfast")
                        .arg("-profile:v")
                        .arg("main")
                        .arg("-level:v")
                        .arg("4.0");
                }
            }

            cmd.arg("-b:v")
                .arg(&variant.bitrate)
                // Set max bitrate to 1.5x target for VBR headroom
                .arg("-maxrate")
                .arg(format!(
                    "{}k",
                    variant
                        .bitrate
                        .trim_end_matches('k')
                        .parse::<u32>()
                        .unwrap_or(1000)
                        * 3
                        / 2
                ))
                // Buffer size = 2x target bitrate for smooth streaming
                .arg("-bufsize")
                .arg(format!(
                    "{}k",
                    variant
                        .bitrate
                        .trim_end_matches('k')
                        .parse::<u32>()
                        .unwrap_or(1000)
                        * 2
                ))
                .arg("-vf")
                .arg(&scale_filter);

            // Pixel format
            match encoder_type {
                EncoderType::Nvenc => {
                    cmd.arg("-pix_fmt").arg("cuda");
                }
                EncoderType::Vaapi => {
                    cmd.arg("-pix_fmt").arg("vaapi");
                }
                EncoderType::Qsv => {
                    cmd.arg("-pix_fmt").arg("qsv");
                }
                EncoderType::Cpu => {
                    cmd.arg("-pix_fmt").arg("yuv420p");
                }
            }

            cmd.arg("-g")
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
            cmd.arg("-c:s").arg("webvtt");

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

            let output = cmd.output().await.context("failed to run ffmpeg")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                error!("FFmpeg failed for variant {}: {}", variant.label, stderr);
                anyhow::bail!(
                    "ffmpeg exited with status: {} for variant {}",
                    output.status,
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
                details: Some(format!("Encoded variant: {}", variant.label)),
                status: "processing".to_string(),
                result: None,
                error: None,
                video_name: existing_video_name,
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
    let video_codec_thumb = Arc::clone(&video_codec);
    let thumb_task = tokio::task::spawn(async move {
        let thumb_path = out_dir_thumb.join("thumbnail.jpg");
        info!("Generating thumbnail: {:?}", thumb_path);

        let encoder_type = EncoderType::from_string(&video_codec_thumb);

        let mut thumb_cmd = Command::new("ffmpeg");
        thumb_cmd
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .arg("-loglevel")
            .arg("error")
            .arg("-y");

        // Hardware acceleration for thumbnail
        match encoder_type {
            EncoderType::Nvenc => {
                thumb_cmd
                    .arg("-hwaccel")
                    .arg("cuda")
                    .arg("-hwaccel_output_format")
                    .arg("cuda");
            }
            EncoderType::Vaapi => {
                thumb_cmd
                    .arg("-hwaccel")
                    .arg("vaapi")
                    .arg("-hwaccel_output_format")
                    .arg("vaapi")
                    .arg("-vaapi_device")
                    .arg("/dev/dri/renderD128");
            }
            EncoderType::Qsv => {
                thumb_cmd
                    .arg("-hwaccel")
                    .arg("qsv")
                    .arg("-hwaccel_output_format")
                    .arg("qsv");
            }
            EncoderType::Cpu => {}
        }

        thumb_cmd
            .arg("-ss")
            .arg("0")
            .arg("-i")
            .arg(input_thumb.as_ref())
            .arg("-vframes")
            .arg("1");

        // Download back to CPU for JPEG encoding if needed
        match encoder_type {
            EncoderType::Nvenc => {
                thumb_cmd.arg("-vf").arg("hwdownload,format=nv12");
            }
            EncoderType::Vaapi => {
                thumb_cmd.arg("-vf").arg("hwdownload,format=nv12");
            }
            EncoderType::Qsv => {
                thumb_cmd.arg("-vf").arg("hwdownload,format=nv12");
            }
            EncoderType::Cpu => {}
        }

        thumb_cmd.arg("-q:v").arg("20").arg(&thumb_path);

        let thumb_output = thumb_cmd
            .output()
            .await
            .context("failed to generate thumbnail")?;

        if !thumb_output.status.success() {
            let stderr = String::from_utf8_lossy(&thumb_output.stderr);
            error!("Thumbnail generation failed: {}", stderr);
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
